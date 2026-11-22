//! Songbird's voice-note recorder (D-0161).
//!
//! - **Capture**: the default microphone through `cpal` (CoreAudio on macOS and iOS, WASAPI on
//!   Windows, ALSA on Linux, AAudio on Android). The system asks for the microphone the first
//!   time (the purpose strings are in `nsh.json`).
//! - **Format**: whatever the device gives (any rate, any channels, `f32`, `i16` or `u16`) is
//!   mixed to mono and brought to 16 kHz by averaging each output sample's span (a box filter,
//!   enough for speech), then kept as 16-bit PCM. A voice note is a WAV file of that: no codec
//!   library, played everywhere. 32 KB a second; at most [`MAX_SECONDS`].
//! - **Level and waveform**: the level of the last 100 ms while recording (for the meter), and
//!   at the end a waveform of at most [`WAVEFORM_LEN`] samples, one byte each, as
//!   `AttachmentPointer.waveform` carries it.
//!
//! Nothing is written to disk: the recording is in memory until the app encrypts and sends it.
//! The capture stream is not `Send` on every platform: a [`Recorder`] stays on the thread that
//! started it (the app's UI thread).

use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The rate voice notes are kept at.
pub const RATE: u32 = 16_000;
/// The longest voice note (10 minutes, about 19 MB).
pub const MAX_SECONDS: u32 = 600;
/// The most waveform samples a voice note carries (PROTOCOL's `AttachmentPointer.waveform`).
pub const WAVEFORM_LEN: usize = 256;
/// Samples per level window (100 ms at [`RATE`]).
const WINDOW: usize = (RATE / 10) as usize;

/// Why recording could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// No microphone (or access to it was refused).
    NoMicrophone,
    /// The platform's audio failed; for logs.
    Failed(String),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMicrophone => f.write_str("no microphone"),
            Self::Failed(e) => write!(f, "audio: {e}"),
        }
    }
}

impl std::error::Error for RecordError {}

/// A finished voice note.
#[derive(Clone, PartialEq, Eq)]
pub struct Recording {
    /// The WAV file (16 kHz, mono, 16-bit).
    pub wav: Vec<u8>,
    /// Its length.
    pub duration_ms: u32,
    /// At most [`WAVEFORM_LEN`] amplitudes, 0 to 255.
    pub waveform: Vec<u8>,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the audio.
        f.debug_struct("Recording")
            .field("duration_ms", &self.duration_ms)
            .field("bytes", &self.wav.len())
            .finish_non_exhaustive()
    }
}

/// Mono samples arriving at the device's rate, brought to [`RATE`].
#[derive(Debug)]
struct Resampler {
    /// Output samples per input sample (`RATE / device rate`).
    step: f64,
    /// How far the current output sample's span is filled (0 to 1).
    filled: f64,
    sum: f64,
    count: u32,
}

impl Resampler {
    fn new(device_rate: u32) -> Self {
        Self {
            step: f64::from(RATE) / f64::from(device_rate.max(1)),
            filled: 0.0,
            sum: 0.0,
            count: 0,
        }
    }

    /// One mono input sample (-1 to 1); pushes finished output samples.
    fn push(&mut self, x: f32, out: &mut Vec<i16>) {
        self.sum += f64::from(x);
        self.count += 1;
        self.filled += self.step;
        // Upsampling (a device slower than 16 kHz) repeats; downsampling averages the span.
        while self.filled >= 1.0 {
            let v = self.sum / f64::from(self.count.max(1));
            out.push(to_i16(v));
            self.filled -= 1.0;
            if self.filled < 1.0 {
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}

fn to_i16(v: f64) -> i16 {
    // Clamped to the 16-bit range first, so the cast cannot wrap.
    #[allow(clippy::cast_possible_truncation, reason = "clamped to i16's range")]
    let s = (v.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
    s
}

#[derive(Debug)]
struct Shared {
    resampler: Resampler,
    pcm: Vec<i16>,
    channels: usize,
    full: bool,
}

impl Shared {
    fn frames(&mut self, interleaved: impl Iterator<Item = f32>) {
        if self.full {
            return;
        }
        let ch = self.channels.max(1);
        let mut acc = 0.0f32;
        for (i, x) in interleaved.enumerate() {
            acc += x;
            if i % ch == ch - 1 {
                #[allow(clippy::cast_precision_loss, reason = "a channel count")]
                let mono = acc / ch as f32;
                self.resampler.push(mono, &mut self.pcm);
                acc = 0.0;
            }
        }
        if self.pcm.len() >= RATE as usize * MAX_SECONDS as usize {
            self.pcm.truncate(RATE as usize * MAX_SECONDS as usize);
            self.full = true;
        }
    }
}

/// A recording in progress. Dropping it discards the recording.
pub struct Recorder {
    stream: cpal::Stream,
    shared: Arc<Mutex<Shared>>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Recorder")
    }
}

impl Recorder {
    /// Starts recording from the default microphone.
    ///
    /// # Errors
    /// [`RecordError::NoMicrophone`] when there is none or access was refused;
    /// [`RecordError::Failed`] when the platform's audio fails.
    pub fn start() -> Result<Self, RecordError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(RecordError::NoMicrophone)?;
        let config = device
            .default_input_config()
            .map_err(|_| RecordError::NoMicrophone)?;
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let shared = Arc::new(Mutex::new(Shared {
            resampler: Resampler::new(config.sample_rate.0),
            pcm: Vec::with_capacity(RATE as usize * 30),
            channels: usize::from(config.channels),
            full: false,
        }));
        let err = |e: cpal::StreamError| log_error(&e);
        let s = shared.clone();
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |d: &[f32], _| lock(&s).frames(d.iter().copied()),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |d: &[i16], _| lock(&s).frames(d.iter().map(|x| f32::from(*x) / 32_768.0)),
                err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config,
                move |d: &[u16], _| {
                    lock(&s).frames(d.iter().map(|x| (f32::from(*x) - 32_768.0) / 32_768.0));
                },
                err,
                None,
            ),
            other => return Err(RecordError::Failed(format!("sample format {other:?}"))),
        }
        .map_err(|e| match e {
            cpal::BuildStreamError::DeviceNotAvailable => RecordError::NoMicrophone,
            e => RecordError::Failed(e.to_string()),
        })?;
        stream
            .play()
            .map_err(|e| RecordError::Failed(e.to_string()))?;
        Ok(Self { stream, shared })
    }

    /// How long it has recorded.
    #[must_use]
    pub fn elapsed_ms(&self) -> u32 {
        duration_ms(lock(&self.shared).pcm.len())
    }

    /// The level of the last 100 ms, 0 to 1.
    #[must_use]
    pub fn level(&self) -> f32 {
        let g = lock(&self.shared);
        let tail = &g.pcm[g.pcm.len().saturating_sub(WINDOW)..];
        rms(tail)
    }

    /// Whether it reached [`MAX_SECONDS`] (it records no more).
    #[must_use]
    pub fn is_full(&self) -> bool {
        lock(&self.shared).full
    }

    /// Stops and answers the voice note (`None` for one too short to keep, under half a second).
    #[must_use]
    pub fn finish(self) -> Option<Recording> {
        let _ = self.stream.pause();
        drop(self.stream);
        let pcm = std::mem::take(&mut lock(&self.shared).pcm);
        if pcm.len() < RATE as usize / 2 {
            return None;
        }
        Some(Recording {
            duration_ms: duration_ms(pcm.len()),
            waveform: waveform(&pcm),
            wav: wav(&pcm),
        })
    }
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

fn log_error(e: &cpal::StreamError) {
    // The stream keeps going or stops; the recording so far is kept either way.
    let _ = e;
}

fn duration_ms(samples: usize) -> u32 {
    u32::try_from(samples as u64 * 1000 / u64::from(RATE)).unwrap_or(u32::MAX)
}

fn rms(s: &[i16]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let sum: f64 = s.iter().map(|x| f64::from(*x).powi(2)).sum();
    #[allow(clippy::cast_precision_loss, reason = "a sample count")]
    let mean = sum / s.len() as f64;
    #[allow(clippy::cast_possible_truncation, reason = "0 to 1")]
    let v = (mean.sqrt() / 32_768.0).min(1.0) as f32;
    v
}

/// At most [`WAVEFORM_LEN`] amplitudes (each window's RMS, scaled so the loudest is 255).
#[must_use]
pub fn waveform(pcm: &[i16]) -> Vec<u8> {
    if pcm.is_empty() {
        return Vec::new();
    }
    let n = WAVEFORM_LEN.min(pcm.len());
    let per = pcm.len().div_ceil(n);
    let levels: Vec<f32> = pcm.chunks(per).map(rms).collect();
    let top = levels.iter().copied().fold(0.0f32, f32::max).max(1e-6);
    levels
        .iter()
        .map(|l| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0 to 255"
            )]
            let b = ((l / top) * 255.0).round().clamp(0.0, 255.0) as u8;
            b
        })
        .collect()
}

/// A WAV file of 16 kHz mono 16-bit PCM.
#[must_use]
pub fn wav(pcm: &[i16]) -> Vec<u8> {
    let data = u32::try_from(pcm.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes a second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Whether this device has a microphone to record from (no prompt).
#[must_use]
pub fn available() -> bool {
    cpal::default_host().default_input_device().is_some()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::cast_precision_loss, reason = "tests")]
    use super::*;

    #[test]
    fn a_48_khz_stereo_tone_becomes_16_khz_mono() {
        let mut s = Shared {
            resampler: Resampler::new(48_000),
            pcm: Vec::new(),
            channels: 2,
            full: false,
        };
        // One second of a 440 Hz tone, the same on both channels.
        let tone = (0..48_000).flat_map(|i| {
            let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5;
            [v, v]
        });
        s.frames(tone);
        assert_eq!(s.pcm.len(), 16_000);
        let peak = s.pcm.iter().map(|x| x.unsigned_abs()).max().unwrap();
        assert!(
            (14_000..=16_500).contains(&peak),
            "about half scale: {peak}"
        );
    }

    #[test]
    fn slower_devices_are_brought_up_to_16_khz() {
        let mut r = Resampler::new(8_000);
        let mut out = Vec::new();
        for _ in 0..8_000 {
            r.push(0.25, &mut out);
        }
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|x| (8_000..=8_300).contains(x)));
    }

    #[test]
    fn the_wav_header_says_what_follows() {
        let w = wav(&[1, -1, 2]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(w.len(), 44 + 6);
    }

    #[test]
    fn the_waveform_is_at_most_256_bytes_and_peaks_at_255() {
        let pcm: Vec<i16> = (0..160_000).map(|i| ((i % 1000) as i16) * 10).collect();
        let w = waveform(&pcm);
        assert!(w.len() <= WAVEFORM_LEN);
        assert_eq!(w.iter().copied().max(), Some(255));
        assert_eq!(waveform(&[]), Vec::<u8>::new());
    }

    #[test]
    fn recordings_stop_at_the_longest_allowed() {
        let mut s = Shared {
            resampler: Resampler::new(16_000),
            pcm: Vec::new(),
            channels: 1,
            full: false,
        };
        for _ in 0..(MAX_SECONDS + 2) {
            s.frames(std::iter::repeat_n(0.1, 16_000));
        }
        assert!(s.full);
        assert_eq!(s.pcm.len(), (RATE * MAX_SECONDS) as usize);
    }
}
