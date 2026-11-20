//! Real calls (A2, D-0152): app-core's signalling and keys, `songbird-calls-media`'s room.
//!
//! - **Starting.** A call in a real chat asks app-core for a ticket (and, 1:1, a fresh call
//!   secret it sends sealed in the offer), joins the room with keys from the key schedule,
//!   publishes the microphone and says who this device is ([`HELLO_TOPIC`], signed with the
//!   device key).
//! - **Ringing here.** An offer from a contact (or a group) rings with the call's own deadline;
//!   answering asks for a ticket and joins; declining tells the caller. An offer while another
//!   call goes on answers "busy".
//! - **In the call.** The room reports who joins and leaves, whose frames decrypt, who mutes and
//!   who speaks; hellos are checked against each sender's signed device list before a name
//!   shows. Raised hands travel on the encrypted data channel. A group's commit during the call
//!   installs the new epoch's keys and switches sending a second later (§8.2).
//! - **Ending.** Whoever ends it, the room is left and the camera and screen stop; a 1:1 call
//!   tells the other side when it is this device that ends it.
//!
//! The media crate's handles are `Send`; they live here on the UI thread and their async work
//! runs on the runtime's workers (`background`).

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt as _;
use nsh_core::{prelude::*, time::unix_ms};

/// Milliseconds since the epoch, as offers count them.
fn now_ms() -> u64 {
    // The clock is past 1970 and within u64 milliseconds.
    unix_ms().max(0.0) as u64
}
use songbird_app_core::calls::{self as core_calls, Signal, Ticket};
use songbird_calls_media::{Call, Event, KeySource, ScreenShare, VideoSender};
use songbird_content::call::EpochSecret;
use zeroize::Zeroizing;

use super::chat::{account_of, core, group_of, ui_id};
use crate::features::calls::{self as ui, EndReason, Phase};
use crate::store::ChatId;
use crate::ui::toast;

/// Who a participant is, signed (app-core's hello).
pub use core_calls::HELLO_TOPIC;
/// A raised hand: one byte, 1 raised, 0 lowered, on the encrypted data channel.
const HAND_TOPIC: &str = "songbird.v1/call-hand";
/// Remote video frames shown per second at most (the tiles are small; the encoder sends more).
const VIDEO_FPS: u64 = 15;
/// The camera's capture size asked of the encoder (it scales what the camera gives).
const CAMERA: (u32, u32) = (640, 480);

/// The key schedule of the call going on.
enum Keys {
    OneToOne {
        call: [u8; 16],
        secret: Zeroizing<[u8; 32]>,
    },
    Group {
        secret: Zeroizing<[u8; 32]>,
        epoch: u64,
    },
}

impl Keys {
    fn epoch(&self) -> EpochSecret {
        match self {
            Self::OneToOne { call, secret } => EpochSecret::one_to_one(call, secret)
                .unwrap_or_else(|_| EpochSecret::group([0; 32], 0)),
            Self::Group { secret, epoch } => EpochSecret::group(**secret, *epoch),
        }
    }

    fn code(&self) -> [u8; 4] {
        self.epoch().verification_code()
    }
}

/// Every participant's key from the call's current epoch.
fn key_source(keys: &Arc<Mutex<Keys>>) -> KeySource {
    let keys = keys.clone();
    Arc::new(move |identity: &str| {
        let e = keys.lock().ok()?.epoch();
        Some((e.key_index(), e.sender_key(identity.as_bytes()).ok()?))
    })
}

/// The call going on (or ringing) on this device.
struct Live {
    /// The UI's call ([`crate::features::calls`]'s `ActiveCall::id`).
    ui: u64,
    chat: ChatId,
    call: [u8; 16],
    /// The other person of a 1:1 call.
    peer: Option<[u8; 16]>,
    /// The group of a group call.
    group: Option<[u8; 16]>,
    /// An offer's secret, until answered (1:1).
    offered: Option<Zeroizing<Vec<u8>>>,
    keys: Option<Arc<Mutex<Keys>>>,
    media: Option<Arc<Call>>,
    camera: Option<VideoSender>,
    /// The camera's frames, until the camera is published.
    frames: Option<songbird_camera::mailbox::Receiver<songbird_camera::Frame>>,
    screen: Option<Arc<ScreenShare>>,
    /// Participant identities whose hello checked out, and the account behind each.
    known: HashMap<String, [u8; 16]>,
}

thread_local! {
    static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
}

fn with_live<T>(f: impl FnOnce(&mut Live) -> T) -> Option<T> {
    LIVE.with_borrow_mut(|l| l.as_mut().map(f))
}

fn is_live(ui_call: u64) -> bool {
    LIVE.with_borrow(|l| l.as_ref().is_some_and(|l| l.ui == ui_call))
}

fn media_of(ui_call: u64) -> Option<Arc<Call>> {
    LIVE.with_borrow(|l| {
        l.as_ref()
            .filter(|l| l.ui == ui_call)
            .and_then(|l| l.media.clone())
    })
}

/// A participant's UI ID before their hello is checked: from their identity, never meeting a
/// contact's (top bits `01`).
fn stranger_id(identity: &str) -> u64 {
    let n = identity
        .get(..15)
        .and_then(|h| u64::from_str_radix(h, 16).ok())
        .unwrap_or_default();
    (n & !(0b11 << 62)) | (1 << 62)
}

/// Ends the UI's call for a reason this device did not choose, and closes everything.
fn ended_remotely(ui_call: u64, reason: EndReason) {
    ui::live::end(ui_call, reason);
    closed(ui_call, reason, false);
}

fn failed(ui_call: u64, words: &'static str) {
    toast(words);
    ended_remotely(ui_call, EndReason::Failed);
}

// ------------------------------------------------------------------ starting

/// Starts a real call in `chat` for the UI's call `ui_call` (already shown as Calling).
pub fn start(ui_call: u64, chat: ChatId, video: bool) {
    let group = group_of(chat);
    let peer = if group.is_none() {
        account_of(chat)
    } else {
        None
    };
    if group.is_none() && peer.is_none() {
        failed(ui_call, "This chat can't be called.");
        return;
    }
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let Some(m) = guard.as_mut() else {
            drop(guard);
            failed(ui_call, "Not connected yet. Try again in a moment.");
            return Ok(());
        };
        let set_up = if let Some(gid) = group {
            match m.start_group_call(&gid, video).await {
                Ok(t) => m
                    .group_call_secret(&gid, &t.call)
                    .map(|(secret, epoch)| (t, Keys::Group { secret, epoch })),
                Err(e) => Err(e),
            }
        } else if let Some(peer) = peer {
            m.start_call(&peer, video).await.map(|(t, secret)| {
                let call = t.call;
                (t, Keys::OneToOne { call, secret })
            })
        } else {
            return Ok(());
        };
        let (ticket, keys) = match set_up {
            Ok(x) => x,
            Err(e) => {
                drop(guard);
                failed(ui_call, start_failure(&e));
                return Ok(());
            }
        };
        let hello = m.call_hello(&ticket.call, &ticket.identity).ok();
        drop(guard);
        if !ui::live::exists(ui_call) {
            // Hung up while the ticket was on its way: tell the callee to stop ringing.
            if let Some(peer) = peer {
                hang_up_signal(peer, ticket.call);
            }
            return Ok(());
        }
        let code = keys.code();
        let keys = Arc::new(Mutex::new(keys));
        LIVE.with_borrow_mut(|l| {
            *l = Some(Live {
                ui: ui_call,
                chat,
                call: ticket.call,
                peer,
                group,
                offered: None,
                keys: Some(keys.clone()),
                media: None,
                camera: None,
                frames: None,
                screen: None,
                known: HashMap::new(),
            });
        });
        ui::live::update(ui_call, |c| {
            c.code = Some(code);
            // A 1:1 call rings the callee; a group call is open as soon as the room is.
            if peer.is_some() {
                c.set_phase(Phase::Ringing);
            } else {
                c.set_phase(Phase::Connecting);
            }
        });
        join(ui_call, ticket, &keys, hello, peer.is_none()).await;
        Ok(())
    });
}

fn start_failure(e: &songbird_app_core::Error) -> &'static str {
    match e {
        songbird_app_core::Error::Server(403) => "You can't call them.",
        songbird_app_core::Error::Server(429) => "Too many calls right now. Try again soon.",
        songbird_app_core::Error::Network => "No connection. The call couldn't start.",
        _ => "The call couldn't start. Try again.",
    }
}

/// Joins the room of `ticket` with the call's keys, publishes the microphone, says hello, and
/// follows the room's events. `open`: the call counts as connected once in the room (groups).
async fn join(
    ui_call: u64,
    ticket: Ticket,
    keys: &Arc<Mutex<Keys>>,
    hello: Option<Vec<u8>>,
    open: bool,
) {
    let source = key_source(keys);
    let Ticket {
        url,
        token,
        identity,
        ..
    } = ticket;
    let joined = background(async move { Call::join(&url, &token, &identity, source).await }).await;
    let (call, events) = match joined {
        Ok(Ok(x)) => x,
        _ => {
            failed(ui_call, "The call couldn't connect.");
            return;
        }
    };
    let call = Arc::new(call);
    if !is_live(ui_call) {
        drop(background(async move {
            let _ = call.close().await;
        }));
        return;
    }
    with_live(|l| l.media = Some(call.clone()));
    let mic = call.clone();
    let mic_ok = background(async move { mic.publish_microphone().await })
        .await
        .is_ok_and(|r| r.is_ok());
    if !mic_ok {
        toast("Your microphone couldn't be opened. Others can't hear you.");
    }
    let (muted, video, hand) =
        ui::live::with(ui_call, |c| (c.muted, c.video, c.hand)).unwrap_or_default();
    call.set_microphone_muted(muted);
    if let Some(hello) = hello {
        let c = call.clone();
        drop(background(async move {
            let _ = c.send_data(HELLO_TOPIC, hello, None).await;
        }));
    }
    if hand {
        set_hand(ui_call, true);
    }
    if video {
        set_video(ui_call, true);
    }
    if open {
        ui::live::update(ui_call, |c| c.set_phase(Phase::Connected));
    }
    ui::live::devices_changed();
    spawn(follow(ui_call, events));
}

// ------------------------------------------------------------------ ringing here

/// A call signal app-core received (called inside `chat::receive`, with the core held).
pub async fn signal(
    m: &mut super::chat::Core,
    from: [u8; 16],
    group: Option<[u8; 16]>,
    call: [u8; 16],
    signal: Signal,
) {
    let me = m.local().account;
    let current = LIVE.with_borrow(|l| {
        l.as_ref()
            .map(|l| (l.ui, l.call, l.peer, l.media.is_some()))
    });
    match signal {
        Signal::Offer {
            video,
            secret,
            group: offered_group,
            ring_until_ms,
        } => {
            if from == me {
                return;
            }
            let group = offered_group.or(group);
            let chat = match group {
                Some(gid) => ui_id(&gid),
                None => ui_id(&from),
            };
            // Only chats app-core owns ring: a non-contact's offer never does (§8.5).
            if !super::chat::owns(chat) {
                return;
            }
            if current.is_some() || ui::live::busy() {
                if group.is_none() {
                    let _ = m.busy_call(&from, &call).await;
                }
                return;
            }
            let left = ring_until_ms.saturating_sub(now_ms());
            let Some(ui_call) = ui::live::ring(chat, video, group.is_some(), left) else {
                return;
            };
            LIVE.with_borrow_mut(|l| {
                *l = Some(Live {
                    ui: ui_call,
                    chat,
                    call,
                    peer: group.is_none().then_some(from),
                    group,
                    offered: (!secret.is_empty()).then(|| Zeroizing::new(secret)),
                    keys: None,
                    media: None,
                    camera: None,
                    frames: None,
                    screen: None,
                    known: HashMap::new(),
                });
            });
        }
        Signal::Answer => {
            let Some((ui_call, live_call, peer, _)) = current else {
                return;
            };
            if live_call != call {
                return;
            }
            if from == me {
                // Another of this account's devices answered: stop ringing here.
                if ui::live::with(ui_call, |c| c.phase == Phase::Incoming).unwrap_or(false) {
                    ended_remotely(ui_call, EndReason::AnsweredElsewhere);
                }
            } else if peer == Some(from) {
                ui::live::update(ui_call, |c| c.set_phase(Phase::Connecting));
            }
        }
        Signal::Decline | Signal::Busy | Signal::Hangup => {
            let Some((ui_call, live_call, peer, _)) = current else {
                return;
            };
            if live_call != call {
                return;
            }
            let incoming = ui::live::with(ui_call, |c| c.phase == Phase::Incoming).unwrap_or(false);
            let reason = match signal {
                // Declined on another of this account's devices.
                Signal::Decline if from == me => EndReason::Declined,
                Signal::Decline => EndReason::TheyDeclined,
                Signal::Busy => EndReason::Busy,
                // The caller gave up before anyone answered.
                _ if incoming => EndReason::Missed,
                _ => EndReason::Hangup,
            };
            if from == me || peer == Some(from) {
                ended_remotely(ui_call, reason);
            }
        }
    }
}

/// Answers the call ringing here (the UI moved it to Connecting).
pub fn answer(ui_call: u64) {
    let Some((call, peer, group, offered)) =
        with_live(|l| (l.ui == ui_call).then(|| (l.call, l.peer, l.group, l.offered.take())))
            .flatten()
    else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let Some(m) = guard.as_mut() else {
            drop(guard);
            failed(ui_call, "Not connected yet. The call couldn't be answered.");
            return Ok(());
        };
        let caller = peer.unwrap_or_default();
        let ticket = match m.answer_call(&caller, &call, group.as_ref()).await {
            Ok(t) => t,
            Err(_) => {
                drop(guard);
                failed(ui_call, "The call couldn't be answered. It may have ended.");
                return Ok(());
            }
        };
        let keys = match (group, offered) {
            (Some(gid), _) => m
                .group_call_secret(&gid, &call)
                .ok()
                .map(|(secret, epoch)| Keys::Group { secret, epoch }),
            (None, Some(secret)) => {
                <[u8; 32]>::try_from(secret.as_slice())
                    .ok()
                    .map(|s| Keys::OneToOne {
                        call,
                        secret: Zeroizing::new(s),
                    })
            }
            (None, None) => None,
        };
        let hello = m.call_hello(&call, &ticket.identity).ok();
        drop(guard);
        let Some(keys) = keys else {
            failed(
                ui_call,
                "This call's keys didn't arrive. Ask them to call again.",
            );
            return Ok(());
        };
        let code = keys.code();
        let keys = Arc::new(Mutex::new(keys));
        with_live(|l| l.keys = Some(keys.clone()));
        ui::live::update(ui_call, |c| c.code = Some(code));
        join(ui_call, ticket, &keys, hello, group.is_some()).await;
        Ok(())
    });
}

// ------------------------------------------------------------------ in the call

/// Follows the room's events until the call ends.
async fn follow(ui_call: u64, mut events: tokio::sync::mpsc::UnboundedReceiver<Event>) {
    while let Some(ev) = events.recv().await {
        if !is_live(ui_call) {
            break;
        }
        on_event(ui_call, ev);
    }
}

fn on_event(ui_call: u64, ev: Event) {
    match ev {
        Event::Joined(identity) => joined(ui_call, identity),
        Event::Left(identity) => {
            ui::live::update(ui_call, |c| {
                c.participants.retain(|p| p.identity != identity);
            });
            ui::live::frame(&identity, None);
            let direct = LIVE.with_borrow(|l| l.as_ref().is_some_and(|l| l.peer.is_some()));
            if direct {
                // The other person left a 1:1 call.
                ended_remotely(ui_call, EndReason::Hangup);
            }
        }
        Event::Decryption { identity, ok } => ui::live::update(ui_call, |c| {
            if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                p.decrypting = ok;
            }
        }),
        Event::Muted {
            identity,
            video,
            muted,
        } => ui::live::update(ui_call, |c| {
            if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                if video {
                    p.video = !muted;
                } else {
                    p.muted = muted;
                }
            }
        }),
        Event::Audio { identity, .. } => ui::live::update(ui_call, |c| {
            if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                p.muted = false;
            }
        }),
        Event::Video { identity, video } => {
            ui::live::update(ui_call, |c| {
                if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                    p.video = true;
                }
            });
            show_video(ui_call, identity, video);
        }
        Event::Speaking(speakers) => {
            let own = media_of(ui_call).map(|m| m.identity().to_owned());
            ui::live::update(ui_call, |c| {
                for p in &mut c.participants {
                    p.speaking = speakers.iter().any(|(i, _)| *i == p.identity);
                }
                c.level = speakers
                    .iter()
                    .find(|(i, _)| Some(i) == own.as_ref())
                    .map_or(0.0, |(_, l)| *l);
            });
        }
        Event::Data {
            identity,
            topic,
            payload,
        } => {
            if topic == HELLO_TOPIC {
                check_hello(ui_call, identity, payload);
            } else if topic == HAND_TOPIC {
                let raised = payload.first() == Some(&1);
                ui::live::update(ui_call, |c| {
                    if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                        p.hand = raised;
                    }
                });
            }
        }
        Event::Reconnecting => toast("Reconnecting…"),
        Event::Reconnected => toast("Reconnected"),
        Event::Ended { removed } => {
            if removed {
                toast("You were removed from the call.");
            }
            ended_remotely(
                ui_call,
                if removed {
                    EndReason::Hangup
                } else {
                    EndReason::Failed
                },
            );
        }
    }
}

/// Someone entered the room: a tile (named once their hello checks out; in a 1:1 call it can
/// only be the person called, the only other holder of the key), and this device's hello.
fn joined(ui_call: u64, identity: String) {
    let (peer, chat) = LIVE
        .with_borrow(|l| l.as_ref().map(|l| (l.peer, l.chat)))
        .unwrap_or_default();
    let (name, id) = match (peer, crate::store::try_store()) {
        (Some(p), Some(s)) => (s.contact_name(Some(ui_id(&p))), ui_id(&p)),
        _ => ("Joining…".to_string(), stranger_id(&identity)),
    };
    let _ = chat;
    ui::live::update(ui_call, |c| {
        if !c.participants.iter().any(|p| p.identity == identity) {
            c.participants.push(ui::Participant {
                id,
                name,
                seed: id,
                muted: true,
                hand: false,
                video: false,
                joins_at: 0,
                identity: identity.clone(),
                decrypting: true,
                speaking: false,
            });
        }
        c.set_phase(Phase::Connected);
    });
    // Tell the newcomer who this device is.
    let Some((call, media)) = with_live(|l| (l.call, l.media.clone())) else {
        return;
    };
    let Some(media) = media else { return };
    let own = media.identity().to_owned();
    super::run(async move {
        let hello = {
            let lock = core();
            let guard = lock.lock().await;
            guard.as_ref().and_then(|m| m.call_hello(&call, &own).ok())
        };
        if let Some(hello) = hello {
            drop(background(async move {
                let _ = media.send_data(HELLO_TOPIC, hello, Some(&identity)).await;
            }));
        }
        Ok(())
    });
}

/// Checks a participant's hello against their account's signed device list, then names them.
fn check_hello(ui_call: u64, identity: String, hello: Vec<u8>) {
    let Some((call, peer)) = with_live(|l| (l.call, l.peer)) else {
        return;
    };
    super::run(async move {
        let account = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            m.verify_hello(&call, &identity, &hello).await
        };
        let Ok(account) = account else {
            // A hello that does not check out names nobody.
            return Ok(());
        };
        if peer.is_some_and(|p| p != account) {
            return Ok(());
        }
        with_live(|l| l.known.insert(identity.clone(), account));
        let id = ui_id(&account);
        let me = super::chat::me_account() == account;
        let name = crate::store::try_store().map_or_else(String::new, |s| {
            if me {
                format!(
                    "{} (another device)",
                    s.me.with_untracked(|m| m.name.clone())
                )
            } else {
                s.contact_name(Some(id))
            }
        });
        ui::live::update(ui_call, |c| {
            if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                p.id = id;
                p.seed = id;
                p.name = name;
            }
        });
        Ok(())
    });
}

/// Decodes a participant's video into the call's frames, at most [`VIDEO_FPS`] a second.
fn show_video(ui_call: u64, identity: String, video: songbird_calls_media::RemoteVideo) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<songbird_calls_media::Rgba>(2);
    drop(background(async move {
        let mut frames = Box::pin(video.frames());
        let mut last = std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(std::time::Instant::now);
        while let Some(f) = frames.next().await {
            if last.elapsed() < Duration::from_millis(1000 / VIDEO_FPS) {
                continue;
            }
            last = std::time::Instant::now();
            if tx.send(upright(f)).await.is_err() {
                return;
            }
        }
    }));
    spawn(async move {
        while let Some(f) = rx.recv().await {
            if !is_live(ui_call) {
                break;
            }
            if f.width > 0 && f.height > 0 {
                ui::live::frame(
                    &identity,
                    Some(nsh_core::element::ImageSrc::rgba(
                        f.width, f.height, f.pixels,
                    )),
                );
            }
        }
        ui::live::frame(&identity, None);
    });
}

/// A frame turned the way its sender's camera was held.
fn upright(f: songbird_calls_media::Rgba) -> songbird_calls_media::Rgba {
    let (w, h) = (f.width as usize, f.height as usize);
    if f.rotation == 0 || f.pixels.len() != w * h * 4 {
        return f;
    }
    let (nw, nh) = if f.rotation == 180 { (w, h) } else { (h, w) };
    let mut out = vec![0u8; f.pixels.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match f.rotation {
                90 => (h - 1 - y, x),
                180 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let (from, to) = ((y * w + x) * 4, (ny * nw + nx) * 4);
            if let (Some(src), Some(dst)) = (f.pixels.get(from..from + 4), out.get_mut(to..to + 4))
            {
                dst.copy_from_slice(src);
            }
        }
    }
    songbird_calls_media::Rgba {
        width: u32::try_from(nw).unwrap_or(0),
        height: u32::try_from(nh).unwrap_or(0),
        rotation: 0,
        pixels: out,
    }
}

// ------------------------------------------------------------------ the user's controls

/// Mutes or unmutes the microphone.
pub fn set_muted(ui_call: u64, muted: bool) {
    if let Some(m) = media_of(ui_call) {
        m.set_microphone_muted(muted);
    }
}

/// Turns the camera on (published the first time) or off.
pub fn set_video(ui_call: u64, on: bool) {
    let Some(media) = media_of(ui_call) else {
        return;
    };
    if let Some(cam) = with_live(|l| l.camera.clone()).flatten() {
        cam.set_muted(!on);
        return;
    }
    if !on {
        return;
    }
    spawn(async move {
        let published =
            background(async move { media.publish_camera(CAMERA.0, CAMERA.1).await }).await;
        match published {
            Ok(Ok(sender)) => {
                let on_now = ui::live::with(ui_call, |c| c.video).unwrap_or(false);
                sender.set_muted(!on_now);
                with_live(|l| l.camera = Some(sender));
                pump_camera();
            }
            _ => toast("Your camera couldn't be shared."),
        }
    });
}

/// Feeds the camera's frames (a call session's mailbox) into the call while it lasts.
pub fn camera_feed(
    ui_call: u64,
    frames: songbird_camera::mailbox::Receiver<songbird_camera::Frame>,
) {
    if with_live(|l| {
        if l.ui == ui_call {
            l.frames = Some(frames);
        }
    })
    .is_some()
    {
        pump_camera();
    }
}

/// Starts sending camera frames once both the camera's frames and the published track exist.
fn pump_camera() {
    let Some((cam, frames)) = with_live(|l| match (&l.camera, l.frames.is_some()) {
        (Some(cam), true) => l.frames.take().map(|f| (cam.clone(), f)),
        _ => None,
    })
    .flatten() else {
        return;
    };
    drop(background(async move {
        while let Some(frame) = frames.recv().await {
            send_frame(&cam, &frame);
        }
    }));
}

fn send_frame(cam: &VideoSender, frame: &songbird_camera::Frame) {
    let ts = i64::try_from(frame.timestamp_us).unwrap_or(i64::MAX);
    let size = (frame.width, frame.height);
    match &frame.pixels {
        songbird_camera::Pixels::Nv12 {
            y,
            y_stride,
            uv,
            uv_stride,
        } => {
            let _ = cam.send_nv12(size, (y, *y_stride), (uv, *uv_stride), ts);
        }
        songbird_camera::Pixels::Bgra { data, stride } => {
            let _ = cam.send_bgra(size, (data, u32::try_from(*stride).unwrap_or(0)), ts);
        }
        songbird_camera::Pixels::Grey { .. } => {}
    }
}

/// Raises or lowers the hand, for everyone in the call.
pub fn set_hand(ui_call: u64, raised: bool) {
    if let Some(m) = media_of(ui_call) {
        drop(background(async move {
            let _ = m.send_data(HAND_TOPIC, vec![u8::from(raised)], None).await;
        }));
    }
}

/// Starts sharing the screen (or a window); `false` with a toast when it can't.
pub fn share_screen(ui_call: u64, window: bool) {
    let Some(media) = media_of(ui_call) else {
        return;
    };
    spawn(async move {
        match background(async move { media.share_screen(window).await }).await {
            Ok(Ok(share)) => {
                with_live(|l| l.screen = Some(Arc::new(share)));
            }
            _ => {
                ui::live::update(ui_call, |c| c.sharing = None);
                toast("Screen sharing couldn't start. Check that Songbird may record the screen.");
            }
        }
    });
}

/// Stops sharing the screen.
pub fn stop_sharing(ui_call: u64) {
    if let Some(share) =
        with_live(|l| (l.ui == ui_call).then(|| l.screen.take()).flatten()).flatten()
    {
        drop(background(async move { share.stop().await }));
    }
}

/// The platform's microphones and speakers (empty until the call's audio is open).
pub fn audio_devices() -> (Vec<String>, Vec<String>) {
    LIVE.with_borrow(|l| {
        l.as_ref()
            .and_then(|l| l.media.as_ref())
            .and_then(|m| {
                m.with_audio_devices(|a| {
                    (
                        a.recording_devices().map(|d| d.name).collect(),
                        a.playout_devices().map(|d| d.name).collect(),
                    )
                })
            })
            .unwrap_or_default()
    })
}

/// Picks the microphone (`output`: the speakers) at `index` in [`audio_devices`]' order.
pub fn select_audio(ui_call: u64, output: bool, index: usize) -> bool {
    media_of(ui_call)
        .and_then(|m| {
            m.with_audio_devices(|a| {
                if output {
                    a.set_playout_device_by_index(index).is_ok()
                } else {
                    a.set_recording_device_by_index(index).is_ok()
                }
            })
        })
        .unwrap_or(false)
}

// ------------------------------------------------------------------ groups changing

/// A group changed (a commit): a call in it moves to the new epoch's keys, sending with them a
/// second later (§8.2). Called with the core held.
pub fn group_changed(m: &super::chat::Core, gid: &[u8; 16]) {
    let Some((ui_call, call, keys, media)) = LIVE.with_borrow(|l| {
        l.as_ref()
            .filter(|l| l.group == Some(*gid))
            .and_then(|l| Some((l.ui, l.call, l.keys.clone()?, l.media.clone()?)))
    }) else {
        return;
    };
    let Ok((secret, epoch)) = m.group_call_secret(gid, &call) else {
        return;
    };
    let changed = keys.lock().is_ok_and(|mut k| match &mut *k {
        Keys::Group {
            secret: s,
            epoch: e,
        } if *e != epoch => {
            *s = secret;
            *e = epoch;
            true
        }
        _ => false,
    });
    if !changed {
        return;
    }
    let code = keys.lock().map(|k| k.code()).ok();
    ui::live::update(ui_call, |c| c.code = code);
    let source = key_source(&keys);
    if let Ok(index) = media.install_epoch(&source) {
        set_timeout(Duration::from_secs(1), move || {
            let _ = media.send_with(index);
        });
    }
}

/// This device left `gid` (or was removed): a call in it ends.
pub fn group_left(gid: &[u8; 16]) {
    if let Some(ui_call) =
        LIVE.with_borrow(|l| l.as_ref().filter(|l| l.group == Some(*gid)).map(|l| l.ui))
    {
        ended_remotely(ui_call, EndReason::Hangup);
    }
}

// ------------------------------------------------------------------ ending

/// The UI's call `ui_call` ended (`reason`); `by_user`: this device ended it, so a 1:1 call's
/// other side is told (a hang-up, or a decline while ringing here). Leaves the room and stops
/// the camera and screen. Idempotent.
pub fn closed(ui_call: u64, reason: EndReason, by_user: bool) {
    let Some(live) = LIVE.with_borrow_mut(|l| {
        if l.as_ref().is_some_and(|x| x.ui == ui_call) {
            l.take()
        } else {
            None
        }
    }) else {
        return;
    };
    ui::live::clear_frames();
    if by_user && let Some(peer) = live.peer {
        match reason {
            EndReason::Declined => {
                let call = live.call;
                super::run(async move {
                    let lock = core();
                    let mut guard = lock.lock().await;
                    if let Some(m) = guard.as_mut() {
                        let _ = m.decline_call(&peer, &call).await;
                    }
                    Ok(())
                });
            }
            EndReason::Hangup | EndReason::NoAnswer => hang_up_signal(peer, live.call),
            _ => {}
        }
    }
    let (media, screen) = (live.media, live.screen);
    drop(background(async move {
        if let Some(share) = screen {
            share.stop().await;
        }
        if let Some(media) = media {
            let _ = media.close().await;
        }
    }));
}

fn hang_up_signal(peer: [u8; 16], call: [u8; 16]) {
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            let _ = m.hang_up(&peer, &call).await;
        }
        Ok(())
    });
}

/// Ends everything at sign-out.
pub fn stop() {
    if let Some(ui_call) = LIVE.with_borrow(|l| l.as_ref().map(|l| l.ui)) {
        closed(ui_call, EndReason::Hangup, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strangers_never_take_a_contacts_id() {
        let id = stranger_id("0123456789abcdef0123456789abcdef");
        assert_eq!(id >> 62, 1, "top bits 01; contacts have the top bit set");
        assert_eq!(stranger_id("zz"), 1 << 62);
    }

    #[test]
    fn frames_turn_upright() {
        // 2 x 1, red then green, sent turned a quarter.
        let f = songbird_calls_media::Rgba {
            width: 2,
            height: 1,
            rotation: 90,
            pixels: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        let u = upright(f);
        assert_eq!((u.width, u.height), (1, 2));
        assert_eq!(u.pixels, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }
}
