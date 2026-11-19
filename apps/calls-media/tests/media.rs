//! Call media end to end on the compose LiveKit (D-0151): two and three devices in one room,
//! keys from the real key schedule (PROTOCOL §8.2), tokens minted as `services/calls` mints them.
//!
//! Needs `SONGBIRD_TEST_LIVEKIT`, `SONGBIRD_TEST_LIVEKIT_KEY` and `SONGBIRD_TEST_LIVEKIT_SECRET`
//! (the stack's `.env`); skipped without them unless `SONGBIRD_REQUIRE_STACK` is set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::many_single_char_names,
    clippy::single_match_else,
    clippy::format_collect,
    clippy::doc_markdown,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "tests"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt as _;
use livekit_api::access_token::{AccessToken, VideoGrants};
use songbird_calls_media::{Call, Event, KeySource, RemoteAudio, RemoteVideo};
use songbird_content::call::EpochSecret;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;

struct Stack {
    url: String,
    key: String,
    secret: String,
}

fn stack() -> Option<Stack> {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    match (
        get("SONGBIRD_TEST_LIVEKIT"),
        get("SONGBIRD_TEST_LIVEKIT_KEY"),
        get("SONGBIRD_TEST_LIVEKIT_SECRET"),
    ) {
        (Some(url), Some(key), Some(secret)) => Some(Stack { url, key, secret }),
        _ => {
            assert!(
                std::env::var_os("SONGBIRD_REQUIRE_STACK").is_none(),
                "SONGBIRD_REQUIRE_STACK is set but the LiveKit settings are missing"
            );
            None
        }
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    aws_lc_rs::rand::fill(&mut b).unwrap();
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A room and identities as the server names them (§8.1): hex of 16 random bytes.
fn room() -> ([u8; 16], String) {
    let call = random::<16>();
    let name = hex(&call);
    (call, name)
}

fn identity() -> String {
    hex(&random::<16>())
}

/// Keys of a 1:1-style epoch 0 from `secret`.
fn one_to_one(call: [u8; 16], secret: [u8; 32]) -> KeySource {
    Arc::new(move |identity: &str| {
        let e = EpochSecret::one_to_one(&call, &secret).ok()?;
        Some((e.key_index(), e.sender_key(identity.as_bytes()).ok()?))
    })
}

/// Keys of whatever group epoch `state` holds now.
fn group(state: Arc<Mutex<([u8; 32], u64)>>) -> KeySource {
    Arc::new(move |identity: &str| {
        let (secret, epoch) = *state.lock().unwrap();
        let e = EpochSecret::group(secret, epoch);
        Some((e.key_index(), e.sender_key(identity.as_bytes()).ok()?))
    })
}

/// Creates the room as `services/calls` does (LiveKit's RoomService; rooms are never created
/// on join), over plain HTTP to the local stack.
async fn create_room(s: &Stack, room: &str) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let admin = AccessToken::with_api_key(&s.key, &s.secret)
        .with_ttl(Duration::from_secs(60))
        .with_grants(VideoGrants {
            room_create: true,
            ..VideoGrants::default()
        })
        .to_jwt()
        .unwrap();
    let host = s
        .url
        .trim_start_matches("http://")
        .trim_start_matches("ws://")
        .trim_end_matches('/')
        .to_owned();
    let body = format!(r#"{{"name":"{room}","empty_timeout":60}}"#);
    let req = format!(
        "POST /twirp/livekit.RoomService/CreateRoom HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {admin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut tcp = tokio::net::TcpStream::connect(&host).await.unwrap();
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut answer = String::new();
    tcp.read_to_string(&mut answer).await.unwrap();
    assert!(
        answer.starts_with("HTTP/1.1 200"),
        "CreateRoom: {}",
        answer.lines().next().unwrap_or("")
    );
}

async fn join(
    s: &Stack,
    room: &str,
    identity: &str,
    keys: KeySource,
) -> (Call, UnboundedReceiver<Event>) {
    let grants = VideoGrants {
        room_join: true,
        room: room.to_owned(),
        can_publish: Some(true),
        can_subscribe: Some(true),
        can_publish_data: Some(true),
        can_publish_sources: vec!["microphone".into(), "camera".into()],
        ..VideoGrants::default()
    };
    let token = AccessToken::with_api_key(&s.key, &s.secret)
        .with_ttl(Duration::from_secs(300))
        .with_identity(identity)
        .with_grants(grants)
        .to_jwt()
        .unwrap();
    Call::join(&s.url, &token, identity, keys).await.unwrap()
}

/// Sends a 440 Hz tone, 10 ms at a time, until dropped.
async fn tone(call: &Call) -> tokio::task::JoinHandle<()> {
    let sender = call.publish_audio(48_000, 1).await.unwrap();
    tokio::spawn(async move {
        let mut n: u64 = 0;
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            tick.tick().await;
            let pcm: Vec<i16> = (0..480)
                .map(|i| {
                    let t = (n + i) as f64 / 48_000.0;
                    (8000.0 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16
                })
                .collect();
            n += 480;
            if sender.send(&pcm).await.is_err() {
                return;
            }
        }
    })
}

/// Sends grey frames (luma 200) at 30 fps until dropped.
async fn camera(call: &Call) -> tokio::task::JoinHandle<()> {
    let sender = call.publish_camera(320, 240).await.unwrap();
    tokio::spawn(async move {
        let y = vec![200u8; 320 * 240];
        let uv = vec![128u8; 160 * 120];
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        let mut ts = 0i64;
        loop {
            tick.tick().await;
            ts += 33_000;
            let _ = sender.send_i420((320, 240), (&y, &uv, &uv), ts);
        }
    })
}

/// Waits up to `secs` for an event `f` picks.
async fn until<T>(
    rx: &mut UnboundedReceiver<Event>,
    secs: u64,
    mut f: impl FnMut(Event) -> Option<T>,
) -> T {
    timeout(Duration::from_secs(secs), async {
        loop {
            let ev = rx.recv().await.expect("call events ended");
            if let Some(t) = f(ev) {
                return t;
            }
        }
    })
    .await
    .expect("timed out waiting for a call event")
}

/// Whether `audio` carries the tone (a frame louder than silence or concealment) within `secs`.
async fn hears(audio: &RemoteAudio, secs: u64) -> bool {
    let mut frames = Box::pin(audio.frames(48_000, 1));
    timeout(Duration::from_secs(secs), async {
        // The first second is skipped: the decoder conceals lost frames by repeating the last
        // good audio for a moment, which is not hearing anything new.
        let mut skip = 100;
        while let Some(f) = frames.next().await {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if f.samples.iter().any(|s| s.unsigned_abs() > 4000) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}

async fn sees_grey(video: &RemoteVideo, secs: u64) -> bool {
    let mut frames = Box::pin(video.frames());
    timeout(Duration::from_secs(secs), async {
        while let Some(f) = frames.next().await {
            if f.width == 0 || f.pixels.is_empty() {
                continue;
            }
            let red: u64 = f.pixels.iter().step_by(4).map(|p| u64::from(*p)).sum();
            let mean = red / (u64::from(f.width) * u64::from(f.height));
            if (170..=240).contains(&mean) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn audio_and_video_cross_encrypted() {
    let Some(s) = stack() else { return };
    let (call, name) = room();
    create_room(&s, &name).await;
    let secret = random::<32>();
    let (a_id, b_id) = (identity(), identity());
    let (a, mut a_rx) = join(&s, &name, &a_id, one_to_one(call, secret)).await;
    let (b, mut b_rx) = join(&s, &name, &b_id, one_to_one(call, secret)).await;
    until(&mut a_rx, 20, |e| {
        matches!(e, Event::Joined(ref i) if *i == b_id).then_some(())
    })
    .await;

    let t = tone(&a).await;
    let c = camera(&a).await;

    let mut audio = None;
    let mut video = None;
    let mut decrypting = false;
    until(&mut b_rx, 30, |e| {
        match e {
            Event::Audio { identity, audio: x } if identity == a_id => audio = Some(x),
            Event::Video { identity, video: x } if identity == a_id => video = Some(x),
            Event::Decryption { identity, ok } if identity == a_id => {
                assert!(ok, "the right key must decrypt");
                decrypting = true;
            }
            _ => {}
        }
        (audio.is_some() && video.is_some()).then_some(())
    })
    .await;
    assert!(hears(audio.as_ref().unwrap(), 20).await, "B hears A's tone");
    assert!(
        sees_grey(video.as_ref().unwrap(), 20).await,
        "B sees A's frames"
    );
    if !decrypting {
        let ok = until(&mut b_rx, 10, |e| match e {
            Event::Decryption { identity, ok } if identity == a_id => Some(ok),
            _ => None,
        })
        .await;
        assert!(ok, "the right key must decrypt");
    }
    t.abort();
    c.abort();
    a.leave().await.unwrap();
    until(&mut b_rx, 20, |e| {
        matches!(e, Event::Left(ref i) if *i == a_id).then_some(())
    })
    .await;
    b.leave().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_key_is_never_played() {
    let Some(s) = stack() else { return };
    let (call, name) = room();
    create_room(&s, &name).await;
    let (b_id, c_id) = (identity(), identity());
    let (b, mut b_rx) = join(&s, &name, &b_id, one_to_one(call, random())).await;
    // C holds a different secret: B's key for C is not the one C sends with.
    let (c, _c_rx) = join(&s, &name, &c_id, one_to_one(call, random())).await;
    let t = tone(&c).await;

    let mut audio = None;
    until(&mut b_rx, 30, |e| match e {
        Event::Audio { identity, audio: x } if identity == c_id => {
            audio = Some(x);
            None
        }
        Event::Decryption { identity, ok } if identity == c_id => {
            assert!(!ok, "a wrong key never decrypts");
            Some(())
        }
        _ => None,
    })
    .await;
    if let Some(audio) = audio {
        assert!(
            !hears(&audio, 4).await,
            "undecryptable frames are dropped, never played"
        );
    }
    t.abort();
    c.leave().await.unwrap();
    b.leave().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_epoch_locks_out_old_keys() {
    let Some(s) = stack() else { return };
    let (_, name) = room();
    create_room(&s, &name).await;
    let epoch0 = (random::<32>(), 6u64);
    let state = Arc::new(Mutex::new(epoch0));
    let (a_id, b_id, d_id) = (identity(), identity(), identity());
    let (a, _a_rx) = join(&s, &name, &a_id, group(state.clone())).await;
    let (b, mut b_rx) = join(&s, &name, &b_id, group(state.clone())).await;
    // D was removed at the commit: it keeps epoch 6's keys and never learns epoch 7's.
    let (d, mut d_rx) = join(&s, &name, &d_id, group(Arc::new(Mutex::new(epoch0)))).await;
    let t = tone(&a).await;

    let audio_b = until(&mut b_rx, 30, |e| match e {
        Event::Audio { identity, audio } if identity == a_id => Some(audio),
        _ => None,
    })
    .await;
    assert!(hears(&audio_b, 20).await, "epoch 6: B hears A");
    let audio_d = until(&mut d_rx, 30, |e| match e {
        Event::Audio { identity, audio } if identity == a_id => Some(audio),
        _ => None,
    })
    .await;
    assert!(hears(&audio_d, 20).await, "epoch 6: D hears A");

    // The commit: epoch 7 (key index 7), installed by A and B at once; A sends with it 1 s later.
    *state.lock().unwrap() = (random::<32>(), 7);
    let keys = group(state.clone());
    assert_eq!(a.install_epoch(&keys).unwrap(), 7);
    assert_eq!(b.install_epoch(&keys).unwrap(), 7);
    tokio::time::sleep(Duration::from_secs(1)).await;
    a.send_with(7).unwrap();

    until(&mut d_rx, 20, |e| match e {
        Event::Decryption {
            identity,
            ok: false,
        } if identity == a_id => Some(()),
        _ => None,
    })
    .await;
    assert!(!hears(&audio_d, 4).await, "D cannot decrypt epoch 7");
    assert!(hears(&audio_b, 10).await, "B still hears A at epoch 7");
    t.abort();
    for c in [a, b, d] {
        c.leave().await.unwrap();
    }
}
