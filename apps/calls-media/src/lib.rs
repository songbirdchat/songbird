//! Call media for the Songbird apps (D-0151): a LiveKit room joined with a ticket from app-core
//! (`songbird_app_core::calls::Ticket`), every audio and video frame and every data packet
//! encrypted end to end by LiveKit's frame cryptor (AES-GCM) under the per-sender keys of
//! PROTOCOL §8.2. No key ever leaves the device: the server gave the room, the token and the
//! identity, nothing more.
//!
//! - Keys come from a [`KeySource`]: for each participant identity, its (key index, sender key)
//!   at the current epoch (app-core's `one_to_one_key` or `Messenger::group_call_key`). The room
//!   installs the local participant's key before connecting and every remote participant's key
//!   the moment it connects, before any of its tracks is subscribed.
//! - Group epochs (§8.2): [`Call::install_epoch`] puts the new keys for every identity in the
//!   ring at once; [`Call::send_with`] switches the local senders' index, which the app calls
//!   one second later.
//! - Frames that do not decrypt are reported ([`Event::Decryption`]) and never played in the
//!   clear: the frame cryptor drops them.
//! - Media: the microphone and speaker through WebRTC's platform audio device module
//!   ([`Call::publish_microphone`]); camera frames pushed by the app ([`VideoSender`]); remote
//!   audio and video as streams of decoded frames ([`RemoteAudio`], [`RemoteVideo`]).
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt as _;
use livekit::DisconnectReason;
use livekit::e2ee::key_provider::{KeyProvider, KeyProviderOptions};
use livekit::e2ee::{E2eeOptions, EncryptionType};
use livekit::options::{TrackPublishOptions, VideoCodec};
use livekit::prelude::{
    ConnectionState, LocalAudioTrack, LocalTrack, LocalVideoTrack, Participant,
    ParticipantIdentity, RemoteTrack, Room, RoomEvent, RoomOptions, TrackSid, TrackSource,
};
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::native::frame_cryptor::EncryptionState;
use livekit::webrtc::prelude::{
    AudioFrame, I420Buffer, RtcVideoSource, VideoFormatType, VideoFrame, VideoResolution,
    VideoRotation,
};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

pub use livekit::PlatformAudio;

/// Frames that fail to decrypt before a participant is reported as "can't decrypt".
const FAILURE_TOLERANCE: i32 = 25;

/// Key ring size: key indices are `epoch mod 16` (PROTOCOL §8.1).
pub const KEY_RING: u32 = 16;

/// For a participant identity, its (key index, sender key) at the current epoch, or `None` if it
/// is not someone the call knows (its frames then never decrypt).
pub type KeySource =
    Arc<dyn Fn(&str) -> Option<(u32, Zeroizing<[u8; 32]>)> + Send + Sync + 'static>;

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The room could not be joined or a track published (LiveKit's message, which never carries
    /// key material).
    #[error("call media: {0}")]
    Room(String),
    /// The key source has no key for this device's own identity.
    #[error("no key for this device")]
    NoOwnKey,
    /// A key index outside the ring.
    #[error("key index out of range")]
    KeyIndex,
    /// The platform's audio devices could not be opened.
    #[error("audio device: {0}")]
    Audio(String),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Something that happened in the call.
#[derive(Debug)]
pub enum Event {
    /// A participant joined (its keys are installed already).
    Joined(String),
    /// A participant left.
    Left(String),
    /// Whether a participant's frames decrypt (`false`: shown as "can't decrypt").
    Decryption { identity: String, ok: bool },
    /// A participant's audio, decoded.
    Audio {
        identity: String,
        audio: RemoteAudio,
    },
    /// A participant's video, decoded.
    Video {
        identity: String,
        video: RemoteVideo,
    },
    /// A participant muted or unmuted a track.
    Muted {
        identity: String,
        video: bool,
        muted: bool,
    },
    /// A participant sent data on the room's encrypted data channel.
    Data {
        identity: String,
        topic: String,
        payload: Vec<u8>,
    },
    /// The participants now speaking (this device included), loudest first, with their audio
    /// level (0 to 1).
    Speaking(Vec<(String, f32)>),
    /// The connection is being re-established.
    Reconnecting,
    /// The connection is back.
    Reconnected,
    /// The call is over for this device (left, removed by the server, or the network gave up).
    Ended { removed: bool },
}

/// A participant's audio track.
pub struct RemoteAudio(livekit::track::RemoteAudioTrack);

impl std::fmt::Debug for RemoteAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteAudio")
    }
}

impl RemoteAudio {
    /// Its decoded PCM, resampled to `sample_rate` and `channels`, in 10 ms frames (with
    /// platform audio, the speaker plays it already; this is for other sinks).
    pub fn frames(&self, sample_rate: u32, channels: u32) -> impl futures_util::Stream<Item = Pcm> {
        NativeAudioStream::new(
            self.0.rtc_track(),
            i32::try_from(sample_rate).unwrap_or(48_000),
            i32::try_from(channels).unwrap_or(1),
        )
        .map(|f: AudioFrame<'static>| Pcm {
            sample_rate: f.sample_rate,
            channels: f.num_channels,
            samples: f.data.into_owned(),
        })
    }
}

/// Interleaved 16-bit PCM.
#[derive(Debug, Clone)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u32,
    pub samples: Vec<i16>,
}

/// A participant's video track.
pub struct RemoteVideo(livekit::track::RemoteVideoTrack);

impl std::fmt::Debug for RemoteVideo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoteVideo")
    }
}

impl RemoteVideo {
    /// Its decoded frames as RGBA, ready for the app's canvas.
    pub fn frames(&self) -> impl futures_util::Stream<Item = Rgba> {
        NativeVideoStream::new(self.0.rtc_track()).map(|f| {
            let (width, height) = (f.buffer.width(), f.buffer.height());
            let stride = width.saturating_mul(4);
            let mut pixels =
                vec![0u8; usize::try_from(u64::from(stride) * u64::from(height)).unwrap_or(0)];
            if let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height))
                && !pixels.is_empty()
            {
                // libyuv names formats by 32-bit word order: its "ABGR" is the bytes R, G, B, A.
                f.buffer
                    .to_argb(VideoFormatType::ABGR, &mut pixels, stride, w, h);
            }
            Rgba {
                width,
                height,
                rotation: match f.rotation {
                    VideoRotation::VideoRotation0 => 0,
                    VideoRotation::VideoRotation90 => 90,
                    VideoRotation::VideoRotation180 => 180,
                    VideoRotation::VideoRotation270 => 270,
                },
                pixels,
            }
        })
    }
}

/// A decoded video frame, RGBA, `width * 4` bytes per row.
#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    /// Clockwise degrees to rotate it by for display.
    pub rotation: u16,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Rgba {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rgba({}x{})", self.width, self.height)
    }
}

/// Pushes the app's camera frames into the call.
#[derive(Clone)]
pub struct VideoSender {
    source: NativeVideoSource,
    track: LocalVideoTrack,
}

impl std::fmt::Debug for VideoSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VideoSender")
    }
}

impl VideoSender {
    /// Sends one frame of planar I420: `luma` is `width * height` bytes, `blue` and `red` (U
    /// and V) a quarter each, with odd sizes rounded up. Returns `false` if the frame was
    /// malformed or dropped by rate adaptation.
    #[must_use]
    pub fn send_i420(
        &self,
        (width, height): (u32, u32),
        (luma, blue, red): (&[u8], &[u8], &[u8]),
        timestamp_us: i64,
    ) -> bool {
        let size = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
        let (cols, rows) = (size(width), size(height));
        let (half_cols, half_rows) = (size(width.div_ceil(2)), size(height.div_ceil(2)));
        if width == 0
            || height == 0
            || luma.len() < cols.saturating_mul(rows)
            || blue.len() < half_cols.saturating_mul(half_rows)
            || red.len() < half_cols.saturating_mul(half_rows)
        {
            return false;
        }
        let mut buffer = I420Buffer::new(width, height);
        let (luma_stride, blue_stride, red_stride) = buffer.strides();
        let (luma_out, blue_out, red_out) = buffer.data_mut();
        copy_plane(luma_out, size(luma_stride), luma, cols, rows);
        copy_plane(blue_out, size(blue_stride), blue, half_cols, half_rows);
        copy_plane(red_out, size(red_stride), red, half_cols, half_rows);
        let buf = buffer;
        self.source.capture_frame(&VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            timestamp_us,
            frame_metadata: None,
            buffer: buf,
        })
    }

    /// Sends one NV12 frame (what cameras give): a luma plane of `luma_stride` bytes per row and
    /// an interleaved `CbCr` plane of half the rows. Returns `false` if it was malformed or
    /// dropped.
    #[must_use]
    pub fn send_nv12(
        &self,
        (width, height): (u32, u32),
        (luma, luma_stride): (&[u8], usize),
        (chroma, chroma_stride): (&[u8], usize),
        timestamp_us: i64,
    ) -> bool {
        let size = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
        let (cols, rows) = (size(width), size(height));
        let (half_cols, half_rows) = (size(width.div_ceil(2)), size(height.div_ceil(2)));
        if width == 0
            || height == 0
            || luma_stride < cols
            || chroma_stride < half_cols.saturating_mul(2)
            || luma.len() < luma_stride.saturating_mul(rows)
            || chroma.len() < chroma_stride.saturating_mul(half_rows)
        {
            return false;
        }
        let mut packed = Vec::with_capacity(cols * rows);
        for row in luma.chunks(luma_stride).take(rows) {
            packed.extend_from_slice(row.get(..cols).unwrap_or_default());
        }
        let mut blue = Vec::with_capacity(half_cols * half_rows);
        let mut red = Vec::with_capacity(half_cols * half_rows);
        for row in chroma.chunks(chroma_stride).take(half_rows) {
            for [cb, cr] in row
                .get(..half_cols * 2)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
            {
                blue.push(*cb);
                red.push(*cr);
            }
        }
        self.send_i420((width, height), (&packed, &blue, &red), timestamp_us)
    }

    /// Sends one 32-bit BGRA frame (what some cameras give), `stride` bytes per row. Returns
    /// `false` if it was malformed or dropped.
    #[must_use]
    pub fn send_bgra(
        &self,
        (width, height): (u32, u32),
        (data, stride): (&[u8], u32),
        timestamp_us: i64,
    ) -> bool {
        bgra_to_i420(data, stride, width, height).is_some_and(|buffer| {
            self.source.capture_frame(&VideoFrame {
                rotation: VideoRotation::VideoRotation0,
                timestamp_us,
                frame_metadata: None,
                buffer,
            })
        })
    }

    /// Stops sending frames (the others see the camera off) or starts again.
    pub fn set_muted(&self, muted: bool) {
        if muted {
            self.track.mute();
        } else {
            self.track.unmute();
        }
    }
}

/// Copies `rows` rows of `cols` bytes from a packed plane into a strided one.
fn copy_plane(out: &mut [u8], stride: usize, plane: &[u8], cols: usize, rows: usize) {
    for row in 0..rows {
        let (at, from) = (row.saturating_mul(stride), row.saturating_mul(cols));
        if let (Some(o), Some(p)) = (
            out.get_mut(at..at.saturating_add(cols)),
            plane.get(from..from.saturating_add(cols)),
        ) {
            o.copy_from_slice(p);
        }
    }
}

/// Pushes PCM into the call instead of the microphone (tests, and sounds the app mixes in).
#[derive(Clone)]
pub struct AudioSender {
    source: NativeAudioSource,
}

impl std::fmt::Debug for AudioSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioSender")
    }
}

impl AudioSender {
    /// Sends 10 ms of interleaved PCM at the sender's rate and channel count.
    pub async fn send(&self, pcm: &[i16]) -> Result<()> {
        let channels = self.source.num_channels();
        let frame = AudioFrame {
            data: pcm.into(),
            sample_rate: self.source.sample_rate(),
            num_channels: channels,
            samples_per_channel: u32::try_from(pcm.len()).unwrap_or(0) / channels.max(1),
        };
        self.source
            .capture_frame(&frame)
            .await
            .map_err(|e| Error::Room(e.to_string()))
    }
}

/// A joined call.
pub struct Call {
    room: Arc<Room>,
    keys: KeyProvider,
    own: ParticipantIdentity,
    send_index: Arc<Mutex<i32>>,
    microphone: Mutex<Option<(PlatformAudio, LocalAudioTrack)>>,
}

impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call").finish_non_exhaustive()
    }
}

/// Makes aws-lc-rs rustls's process-wide provider, which LiveKit's HTTP client (reqwest without
/// a provider of its own) needs. If the app installed one already, it was this one: aws-lc-rs is
/// the only provider compiled in.
fn tls() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn key_provider() -> KeyProvider {
    KeyProvider::new(KeyProviderOptions {
        // Keys change only with the key schedule's epochs; LiveKit's own ratchet (deriving the
        // next key from the last) would make keys no epoch secret produced.
        ratchet_window_size: 0,
        // With a negative tolerance LiveKit never reports a frame that fails to decrypt, and a
        // participant must be shown as "can't decrypt" (§8.3). After this many failed frames
        // (half a second of audio) it reports it and marks the key invalid; the room then
        // installs the participant's key again ([`Call::join`]'s event task), so a burst of bad
        // frames never leaves a good key unused.
        failure_tolerance: FAILURE_TOLERANCE,
        // Frame keys derived from the sender key as on every LiveKit platform (PBKDF2 with
        // LiveKit's salt), so the web app decrypts what the native apps send and back.
        ..KeyProviderOptions::default()
    })
}

fn install(
    keys: &KeyProvider,
    identity: &ParticipantIdentity,
    index: u32,
    key: &[u8; 32],
) -> Result<()> {
    if index >= KEY_RING {
        return Err(Error::KeyIndex);
    }
    let index = i32::try_from(index).map_err(|_| Error::KeyIndex)?;
    if keys.set_key(identity, index, key.to_vec()) {
        Ok(())
    } else {
        Err(Error::KeyIndex)
    }
}

impl Call {
    /// Joins the room of a ticket (`url`, `token`, this device's `identity`) with end-to-end
    /// encryption on, keys from `source`.
    pub async fn join(
        url: &str,
        token: &str,
        identity: &str,
        source: KeySource,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Event>)> {
        Self::join_with(url, token, identity, source, true).await
    }

    /// [`Self::join`], choosing whether LiveKit also encrypts the data channel. A call link's
    /// room turns it off (D-0153): its packets would be under sender keys a newcomer does not
    /// hold yet, so everything the app sends there is sealed by the app instead.
    pub async fn join_with(
        url: &str,
        token: &str,
        identity: &str,
        source: KeySource,
        encrypt_data: bool,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Event>)> {
        tls();
        let keys = key_provider();
        let own = ParticipantIdentity(identity.to_owned());
        let (index, key) = source(identity).ok_or(Error::NoOwnKey)?;
        install(&keys, &own, index, &key)?;
        drop(key);
        let send_index = Arc::new(Mutex::new(
            i32::try_from(index).map_err(|_| Error::KeyIndex)?,
        ));

        let mut options = RoomOptions::default();
        options.auto_subscribe = true;
        let e2ee = E2eeOptions {
            encryption_type: EncryptionType::Gcm,
            key_provider: keys.clone(),
        };
        if encrypt_data {
            options.encryption = Some(e2ee);
        } else {
            // Media only: LiveKit encrypts data packets only when given `encryption`.
            #[allow(
                deprecated,
                reason = "the field that keeps the data channel unencrypted"
            )]
            {
                options.e2ee = Some(e2ee);
            }
        }
        let (room, mut room_events) = Room::connect(url, token, options)
            .await
            .map_err(|e| Error::Room(e.to_string()))?;
        let room = Arc::new(room);

        // Everyone already in the room.
        for identity in room.remote_participants().keys() {
            if let Some((i, k)) = source(identity.as_str()) {
                install(&keys, identity, i, &k)?;
            }
        }

        let (tx, rx) = mpsc::unbounded_channel();
        for identity in room.remote_participants().keys() {
            let _ = tx.send(Event::Joined(identity.to_string()));
        }
        let mut pump = Pump {
            source,
            keys: keys.clone(),
            room: Arc::downgrade(&room),
            own: own.clone(),
            send_index: send_index.clone(),
            decrypting: HashMap::new(),
        };
        tokio::spawn(async move {
            while let Some(ev) = room_events.recv().await {
                if let Some(out) = pump.translate(ev)
                    && tx.send(out).is_err()
                {
                    break;
                }
            }
        });

        Ok((
            Self {
                room,
                keys,
                own,
                send_index,
                microphone: Mutex::new(None),
            },
            rx,
        ))
    }

    /// This device's participant identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        self.own.as_str()
    }

    /// The identities in the room besides this device.
    #[must_use]
    pub fn participants(&self) -> Vec<String> {
        self.room
            .remote_participants()
            .keys()
            .map(ToString::to_string)
            .collect()
    }

    /// Installs a new epoch's keys (§8.2): for this device and every participant, the key
    /// `source` gives now, at its index. Old keys stay in the ring for late frames; this device
    /// keeps sending with the old index until [`Self::send_with`].
    pub fn install_epoch(&self, source: &KeySource) -> Result<u32> {
        let (index, key) = source(self.own.as_str()).ok_or(Error::NoOwnKey)?;
        install(&self.keys, &self.own, index, &key)?;
        for identity in self.room.remote_participants().keys() {
            if let Some((i, k)) = source(identity.as_str()) {
                install(&self.keys, identity, i, &k)?;
            }
        }
        Ok(index)
    }

    /// Installs one participant's key (a link call's sender key, as it arrives).
    pub fn set_key(&self, identity: &str, index: u32, key: &[u8; 32]) -> Result<()> {
        install(
            &self.keys,
            &ParticipantIdentity(identity.to_owned()),
            index,
            key,
        )
    }

    /// Sends with key `index` from now on (§8.2: one second after installing an epoch).
    pub fn send_with(&self, index: u32) -> Result<()> {
        if index >= KEY_RING {
            return Err(Error::KeyIndex);
        }
        let index = i32::try_from(index).map_err(|_| Error::KeyIndex)?;
        if let Ok(mut g) = self.send_index.lock() {
            *g = index;
        }
        set_sender_index(&self.room, &self.own, index);
        Ok(())
    }

    /// Publishes the microphone through the platform's audio device module (which also plays
    /// every participant's audio on the speaker).
    pub async fn publish_microphone(&self) -> Result<()> {
        let audio = PlatformAudio::new().map_err(|e| Error::Audio(e.to_string()))?;
        let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
        self.publish(LocalTrack::Audio(track.clone()), TrackSource::Microphone)
            .await?;
        if let Ok(mut m) = self.microphone.lock() {
            *m = Some((audio, track));
        }
        Ok(())
    }

    /// Mutes or unmutes the microphone.
    pub fn set_microphone_muted(&self, muted: bool) {
        if let Ok(m) = self.microphone.lock()
            && let Some((_, track)) = m.as_ref()
        {
            if muted {
                track.mute();
            } else {
                track.unmute();
            }
        }
    }

    /// The platform's audio devices (to pick a microphone or speaker), while the microphone is
    /// published.
    pub fn with_audio_devices<T>(&self, f: impl FnOnce(&PlatformAudio) -> T) -> Option<T> {
        let m = self.microphone.lock().ok()?;
        m.as_ref().map(|(a, _)| f(a))
    }

    /// Publishes an audio track the app feeds itself, `sample_rate` Hz, `channels` channels.
    pub async fn publish_audio(&self, sample_rate: u32, channels: u32) -> Result<AudioSender> {
        let source =
            NativeAudioSource::new(AudioSourceOptions::default(), sample_rate, channels, 0);
        let track = LocalAudioTrack::create_audio_track(
            "microphone",
            RtcAudioSource::Native(source.clone()),
        );
        self.publish(LocalTrack::Audio(track), TrackSource::Microphone)
            .await?;
        Ok(AudioSender { source })
    }

    /// Publishes the camera at `width` x `height`; the app sends the frames.
    pub async fn publish_camera(&self, width: u32, height: u32) -> Result<VideoSender> {
        let source = NativeVideoSource::new(VideoResolution { width, height }, false);
        let track =
            LocalVideoTrack::create_video_track("camera", RtcVideoSource::Native(source.clone()));
        self.publish(LocalTrack::Video(track.clone()), TrackSource::Camera)
            .await?;
        Ok(VideoSender { source, track })
    }

    async fn publish(&self, track: LocalTrack, source: TrackSource) -> Result<TrackSid> {
        let options = TrackPublishOptions {
            source,
            // VP8 decodes everywhere LiveKit runs, with E2EE on every platform.
            video_codec: VideoCodec::VP8,
            dtx: true,
            red: true,
            simulcast: matches!(source, TrackSource::Camera),
            ..TrackPublishOptions::default()
        };
        let publication = self
            .room
            .local_participant()
            .publish_track(track, options)
            .await
            .map_err(|e| Error::Room(e.to_string()))?;
        let i = self.send_index.lock().map_or(0, |g| *g);
        set_sender_index(&self.room, &self.own, i);
        Ok(publication.sid())
    }

    /// Shares the screen (or one window, with `window`): on macOS the system's picker asks
    /// which; elsewhere the first screen (or window) the platform lists. Frames go out as a
    /// screen-share track (encrypted like the camera) until [`ScreenShare::stop`].
    pub async fn share_screen(&self, window: bool) -> Result<ScreenShare> {
        let source = NativeVideoSource::new(
            VideoResolution {
                width: 1920,
                height: 1080,
            },
            true,
        );
        let track =
            LocalVideoTrack::create_video_track("screen", RtcVideoSource::Native(source.clone()));
        let sid = self
            .publish(LocalTrack::Video(track), TrackSource::Screenshare)
            .await?;
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("songbird-screen".into())
            .spawn(move || screen_thread(window, &source, &thread_stop, started_tx))
            .map_err(|e| Error::Room(e.to_string()))?;
        let share = ScreenShare {
            stop,
            sid,
            room: self.room.clone(),
        };
        if started_rx.await.unwrap_or(false) {
            Ok(share)
        } else {
            share.stop().await;
            Err(Error::Room("screen capture is not available".into()))
        }
    }

    /// Sends `payload` to everyone in the room (or to `to` only) on the data channel, reliably,
    /// encrypted end to end like the media.
    pub async fn send_data(&self, topic: &str, payload: Vec<u8>, to: Option<&str>) -> Result<()> {
        let packet = livekit::DataPacket {
            payload,
            topic: Some(topic.to_owned()),
            reliable: true,
            destination_identities: to
                .map(|t| vec![ParticipantIdentity(t.to_owned())])
                .unwrap_or_default(),
        };
        self.room
            .local_participant()
            .publish_data(packet)
            .await
            .map_err(|e| Error::Room(e.to_string()))
    }

    /// Whether the room is connected.
    #[must_use]
    pub fn connected(&self) -> bool {
        self.room.connection_state() == ConnectionState::Connected
    }

    /// Leaves the call, from any handle to it (the microphone and speaker are released).
    pub async fn close(&self) -> Result<()> {
        if let Ok(mut m) = self.microphone.lock() {
            m.take();
        }
        self.room
            .close()
            .await
            .map_err(|e| Error::Room(e.to_string()))
    }

    /// Leaves the call.
    pub async fn leave(self) -> Result<()> {
        if let Ok(mut m) = self.microphone.lock() {
            m.take();
        }
        self.room
            .close()
            .await
            .map_err(|e| Error::Room(e.to_string()))
    }
}

/// Turns LiveKit's room events into [`Event`]s, installing keys as participants come.
struct Pump {
    source: KeySource,
    keys: KeyProvider,
    room: std::sync::Weak<Room>,
    own: ParticipantIdentity,
    send_index: Arc<Mutex<i32>>,
    /// The last decryption state reported per identity, so each change is reported once.
    decrypting: HashMap<String, bool>,
}

impl Pump {
    fn install_for(&self, identity: &ParticipantIdentity) {
        if let Some((i, k)) = (self.source)(identity.as_str()) {
            // A key the source gives is always in range; nothing to report otherwise.
            let _ = install(&self.keys, identity, i, &k);
        }
    }

    fn translate(&mut self, ev: RoomEvent) -> Option<Event> {
        match ev {
            RoomEvent::ParticipantConnected(p) => {
                let id = p.identity();
                self.install_for(&id);
                Some(Event::Joined(id.to_string()))
            }
            RoomEvent::ParticipantDisconnected(p) => {
                let identity = p.identity().to_string();
                self.decrypting.remove(&identity);
                Some(Event::Left(identity))
            }
            RoomEvent::E2eeStateChanged { participant, state } => {
                let id = participant.identity();
                let ok = match state {
                    EncryptionState::Ok => true,
                    EncryptionState::DecryptionFailed
                    | EncryptionState::MissingKey
                    | EncryptionState::InternalError => false,
                    _ => return None,
                };
                if !ok && id != self.own {
                    // LiveKit stopped using the key after too many failures: give it the
                    // current one again (a wrong key keeps failing and stays reported).
                    self.install_for(&id);
                }
                let identity = id.to_string();
                (self.decrypting.insert(identity.clone(), ok) != Some(ok))
                    .then_some(Event::Decryption { identity, ok })
            }
            RoomEvent::TrackSubscribed {
                track, participant, ..
            } => {
                let identity = participant.identity().to_string();
                Some(match track {
                    RemoteTrack::Audio(a) => Event::Audio {
                        identity,
                        audio: RemoteAudio(a),
                    },
                    RemoteTrack::Video(v) => Event::Video {
                        identity,
                        video: RemoteVideo(v),
                    },
                })
            }
            RoomEvent::TrackMuted {
                participant,
                publication,
            } => Some(Event::Muted {
                identity: participant.identity().to_string(),
                video: publication.source() != TrackSource::Microphone,
                muted: true,
            }),
            RoomEvent::TrackUnmuted {
                participant,
                publication,
            } => Some(Event::Muted {
                identity: participant.identity().to_string(),
                video: publication.source() != TrackSource::Microphone,
                muted: false,
            }),
            RoomEvent::DataReceived {
                payload,
                topic,
                participant: Some(p),
                ..
            } => Some(Event::Data {
                identity: p.identity().to_string(),
                topic: topic.unwrap_or_default(),
                payload: payload.as_ref().clone(),
            }),
            RoomEvent::ActiveSpeakersChanged { speakers } => Some(Event::Speaking(
                speakers
                    .iter()
                    .map(|p: &Participant| (p.identity().to_string(), p.audio_level()))
                    .collect(),
            )),
            RoomEvent::LocalTrackPublished { .. } | RoomEvent::LocalTrackRepublished { .. } => {
                // A (re)published sender starts at index 0: put it on the current one.
                if let Some(room) = self.room.upgrade() {
                    let i = self.send_index.lock().map_or(0, |g| *g);
                    set_sender_index(&room, &self.own, i);
                }
                None
            }
            RoomEvent::Reconnecting => Some(Event::Reconnecting),
            RoomEvent::Reconnected => Some(Event::Reconnected),
            RoomEvent::Disconnected { reason } => Some(Event::Ended {
                removed: matches!(
                    reason,
                    DisconnectReason::ParticipantRemoved
                        | DisconnectReason::RoomDeleted
                        | DisconnectReason::DuplicateIdentity
                ),
            }),
            _ => None,
        }
    }
}

/// A screen being shared; [`Self::stop`] ends it.
pub struct ScreenShare {
    stop: Arc<std::sync::atomic::AtomicBool>,
    sid: TrackSid,
    room: Arc<Room>,
}

impl std::fmt::Debug for ScreenShare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScreenShare")
    }
}

impl ScreenShare {
    /// Stops capturing and takes the track out of the call.
    pub async fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self
            .room
            .local_participant()
            .unpublish_track(&self.sid)
            .await;
    }
}

/// Screen frames per second.
const SCREEN_FPS: u64 = 15;

/// Captures the screen on its own thread (the capturer is not `Send`) until `stop`, converting
/// each BGRA frame to I420 for the encoder. Says on `started` whether capture began.
fn screen_thread(
    window: bool,
    source: &NativeVideoSource,
    stop: &std::sync::atomic::AtomicBool,
    started: tokio::sync::oneshot::Sender<bool>,
) {
    use livekit::webrtc::desktop_capturer::{
        DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
    };
    let kind = if window {
        DesktopCaptureSourceType::Window
    } else {
        DesktopCaptureSourceType::Screen
    };
    let mut options = DesktopCapturerOptions::new(kind);
    options.set_include_cursor(true);
    let Some(mut capturer) = DesktopCapturer::new(options) else {
        let _ = started.send(false);
        return;
    };
    // macOS asks with its own picker (no source); elsewhere the first the platform lists.
    let chosen = if cfg!(target_os = "macos") {
        None
    } else {
        let first = capturer.get_source_list().into_iter().next();
        if first.is_none() {
            let _ = started.send(false);
            return;
        }
        first
    };
    let sink = source.clone();
    let epoch = std::time::Instant::now();
    capturer.start_capture(chosen, move |frame| {
        let Ok(frame) = frame else { return };
        let (Ok(width), Ok(height)) = (u32::try_from(frame.width()), u32::try_from(frame.height()))
        else {
            return;
        };
        if let Some(buffer) = bgra_to_i420(frame.data(), frame.stride(), width, height) {
            let _ = sink.capture_frame(&VideoFrame {
                rotation: VideoRotation::VideoRotation0,
                timestamp_us: i64::try_from(epoch.elapsed().as_micros()).unwrap_or(i64::MAX),
                frame_metadata: None,
                buffer,
            });
        }
    });
    let _ = started.send(true);
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        capturer.capture_frame();
        std::thread::sleep(std::time::Duration::from_millis(1000 / SCREEN_FPS));
    }
}

/// A BGRA frame (libyuv's "ARGB") as I420; `None` if its sizes do not add up.
fn bgra_to_i420(data: &[u8], stride: u32, width: u32, height: u32) -> Option<I420Buffer> {
    let (cols, rows) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
    if width == 0
        || height == 0
        || stride < width.checked_mul(4)?
        || u64::try_from(data.len()).ok()? < u64::from(stride) * u64::from(height)
    {
        return None;
    }
    let mut buffer = I420Buffer::new(width, height);
    let (sy, su, sv) = buffer.strides();
    let (luma, blue, red) = buffer.data_mut();
    livekit::webrtc::native::yuv_helper::argb_to_i420(
        data, stride, luma, sy, blue, su, red, sv, cols, rows,
    );
    Some(buffer)
}

/// Puts every frame cryptor of this device's own tracks on key `index`.
fn set_sender_index(room: &Room, own: &ParticipantIdentity, index: i32) {
    let cryptors: HashMap<_, _> = room.e2ee_manager().frame_cryptors();
    for ((identity, _), cryptor) in cryptors {
        if identity == *own {
            cryptor.set_key_index(index);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    /// The signalling WebSocket and LiveKit's HTTP client build their TLS configurations from
    /// rustls's process default, which is aws-lc-rs (the only provider compiled in).
    #[test]
    fn tls_runs_on_aws_lc_rs() {
        super::tls();
        // reqwest, as livekit-net builds it, and rustls's builder, as tokio-tungstenite calls it.
        reqwest::Client::builder().use_rustls_tls().build().unwrap();
        let provider = rustls::crypto::CryptoProvider::get_default().unwrap();
        let aws = rustls::crypto::aws_lc_rs::default_provider();
        assert_eq!(
            provider
                .kx_groups
                .iter()
                .map(|g| g.name())
                .collect::<Vec<_>>(),
            aws.kx_groups.iter().map(|g| g.name()).collect::<Vec<_>>(),
        );
        let _ = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
    }
}

#[cfg(test)]
mod send_check {
    fn send_sync<T: Send + Sync>() {}
    #[test]
    fn handles_cross_threads() {
        send_sync::<super::Call>();
        send_sync::<super::VideoSender>();
        send_sync::<super::RemoteVideo>();
    }
}
