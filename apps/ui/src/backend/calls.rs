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
use songbird_app_core::call_links::{
    CARD_TOPIC, Card, KEY_TOPIC, LinkJoin, open_link_card, open_link_data, seal_link_data,
};
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
    /// A call link's: each participant's own random key, as it arrived (this device's too).
    Link {
        keys: HashMap<String, (u32, Zeroizing<[u8; 32]>)>,
    },
}

impl Keys {
    fn epoch(&self) -> Option<EpochSecret> {
        match self {
            Self::OneToOne { call, secret } => EpochSecret::one_to_one(call, secret).ok(),
            Self::Group { secret, epoch } => Some(EpochSecret::group(**secret, *epoch)),
            Self::Link { .. } => None,
        }
    }

    fn code(&self) -> Option<[u8; 4]> {
        self.epoch().map(|e| e.verification_code())
    }

    fn key_of(&self, identity: &str) -> Option<(u32, Zeroizing<[u8; 32]>)> {
        match self {
            Self::Link { keys } => keys.get(identity).map(|(i, k)| (*i, k.clone())),
            _ => {
                let e = self.epoch()?;
                Some((e.key_index(), e.sender_key(identity.as_bytes()).ok()?))
            }
        }
    }
}

/// Every participant's key: from the call's current epoch, or (a link call) as it arrived.
fn key_source(keys: &Arc<Mutex<Keys>>) -> KeySource {
    let keys = keys.clone();
    Arc::new(move |identity: &str| keys.lock().ok()?.key_of(identity))
}

/// A key that came before its sender's card: (account, device, index, key).
type EarlyKey = ([u8; 16], [u8; 16], u32, Zeroizing<[u8; 32]>);

/// A link as the UI first sees it: its root, and (this device's own) its name and lobby.
pub type LinkInfo = ([u8; 32], Option<(String, bool)>);

/// What a link call adds (D-0153).
struct LinkState {
    root: Zeroizing<[u8; 32]>,
    /// This device administers the link.
    admin: bool,
    /// The link has a lobby.
    lobby: bool,
    /// This device's sender key and its index.
    own: (u32, Zeroizing<[u8; 32]>),
    /// Participants' opened cards, by participant identity (each checked against its sender).
    cards: HashMap<String, Card>,
    /// Keys that arrived before their sender's card: (account, device, index, key).
    pending: HashMap<String, EarlyKey>,
    /// Identities whose key came from the device their card names (over a Signal session).
    keyed: std::collections::HashSet<String>,
    /// Lobby places by the UI's waiting id.
    waiting: HashMap<u64, [u8; 16]>,
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
    /// A call link's state.
    link: Option<LinkState>,
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
                link: None,
            });
        });
        ui::live::update(ui_call, |c| {
            c.code = code;
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
    // A link call's data channel carries only what the app sealed itself (D-0153).
    let encrypt_data = !LIVE.with_borrow(|l| l.as_ref().is_some_and(|l| l.link.is_some()));
    let joined =
        background(
            async move { Call::join_with(&url, &token, &identity, source, encrypt_data).await },
        )
        .await;
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
    if !encrypt_data {
        link_in_room(ui_call, &call);
    }
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
                    link: None,
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
        // A link call's keys travel on the room's data channel (`link_data`); one that came
        // through the server is not for any call here.
        Signal::SenderKey { .. } => {}
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
        ui::live::update(ui_call, |c| c.code = code);
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
        Event::Joined(identity) => {
            joined(ui_call, identity.clone());
            if in_link() {
                link_joined(ui_call, &identity);
            }
        }
        Event::Left(identity) => {
            ui::live::update(ui_call, |c| {
                c.participants.retain(|p| p.identity != identity);
            });
            ui::live::frame(&identity, None);
            if in_link() {
                link_left(ui_call, &identity);
            }
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
            if in_link() {
                link_data(ui_call, identity, &topic, payload);
            } else if topic == HELLO_TOPIC {
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
    // Tell the newcomer who this device is. Not in a link call: its data channel is not
    // LiveKit-encrypted, and its sealed card says it there instead.
    if in_link() {
        return;
    }
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
    let Some(m) = media_of(ui_call) else { return };
    // In a link call, sealed under the link key (its data channel is not LiveKit-encrypted).
    let payload = match LIVE.with_borrow(|l| {
        l.as_ref()
            .and_then(|l| l.link.as_ref().map(|k| k.root.clone()))
    }) {
        Some(root) => match seal_link_data(&root, &[u8::from(raised)]) {
            Ok(p) => p,
            Err(_) => return,
        },
        None => vec![u8::from(raised)],
    };
    drop(background(async move {
        let _ = m.send_data(HAND_TOPIC, payload, None).await;
    }));
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

// ------------------------------------------------------------------ call links

/// How often a lobby place, or an admin's view of the lobby, is asked again.
const LOBBY_POLL: Duration = Duration::from_secs(3);

thread_local! {
    /// Links a call is about to join, by the UI's call.
    static PREPARED: RefCell<HashMap<u64, Zeroizing<[u8; 32]>>> = RefCell::new(HashMap::new());
}

fn in_link() -> bool {
    LIVE.with_borrow(|l| l.as_ref().is_some_and(|l| l.link.is_some()))
}

fn fresh_key() -> Option<Zeroizing<[u8; 32]>> {
    let mut k = Zeroizing::new([0u8; 32]);
    aws_lc_rs::rand::fill(&mut *k).ok()?;
    Some(k)
}

fn my_name() -> String {
    crate::store::try_store()
        .map(|s| s.me.with_untracked(|m| m.name.clone()))
        .unwrap_or_default()
}

/// A link's UI ID (its list row): from its root, never meeting a chat's.
fn link_id(root: &[u8; 32]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&root[..8]);
    // Top bits `001`: neither a contact's (`1…`) nor a call participant's (`01…`).
    (u64::from_be_bytes(b) >> 3) | (1 << 61)
}

fn to_ui(l: &songbird_app_core::store::StoredCallLink) -> crate::store::CallLink {
    crate::store::CallLink {
        id: link_id(&l.root),
        name: l.name.clone(),
        url: songbird_proto::call_link::CallLink { root: *l.root }.to_link(),
        lobby: l.lobby,
    }
}

/// Shows the links this device made (called at start, with the core held).
pub fn show_links(m: &super::chat::Core) {
    if let (Ok(links), Some(s)) = (m.call_links(), crate::store::try_store()) {
        s.call_links.set(links.iter().rev().map(to_ui).collect());
    }
}

/// Makes a call link (named `name`), then `done` with it as the Calls tab lists it.
pub fn create_link(
    name: String,
    video: bool,
    lobby: bool,
    done: impl FnOnce(crate::store::CallLink) + 'static,
) {
    super::run(async move {
        let made = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            m.create_call_link(&name, video, lobby).await
        };
        let link = made.map_err(|_| super::problem("The link couldn't be made. Try again."))?;
        let ui_link = to_ui(&link);
        if let Some(s) = crate::store::try_store() {
            s.call_links.update(|l| {
                l.retain(|x| x.id != ui_link.id);
                l.insert(0, ui_link.clone());
            });
        }
        done(ui_link);
        Ok(())
    });
}

/// Revokes a link this device made: nobody can join it again, and its call ends for everyone.
pub fn revoke_link(url: &str) {
    let Some(root) = songbird_proto::call_link::CallLink::parse(url).map(|l| l.root) else {
        return;
    };
    super::run(async move {
        let done = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            m.revoke_call_link(&root).await
        };
        done.map_err(|_| super::problem("The link couldn't be revoked. Try again."))?;
        if let Some(s) = crate::store::try_store() {
            let id = link_id(&root);
            s.call_links.update(|l| l.retain(|x| x.id != id));
        }
        toast("Link revoked");
        Ok(())
    });
}

/// What the UI needs to show a link before joining: its name (this device's own links) and
/// whether this device administers it. `None` for something that is not a call link.
pub fn describe_link(url: &str) -> Option<LinkInfo> {
    let root = songbird_proto::call_link::CallLink::parse(url.trim())?.root;
    let id = link_id(&root);
    let own = crate::store::try_store().and_then(|s| {
        s.call_links.with_untracked(|l| {
            l.iter()
                .find(|x| x.id == id)
                .map(|x| (x.name.clone(), x.lobby))
        })
    });
    Some((root, own))
}

/// Remembers the link the UI's call `ui_call` is about to join (see [`join_link`]).
pub fn prepare_link(ui_call: u64, root: [u8; 32]) {
    PREPARED.with_borrow_mut(|p| p.insert(ui_call, Zeroizing::new(root)));
}

/// Joins the prepared link of `ui_call`: straight in (its admin, or no lobby), or into the
/// lobby to wait.
pub fn join_link(ui_call: u64) {
    let Some(root) = PREPARED.with_borrow_mut(|p| p.remove(&ui_call)) else {
        return;
    };
    let name = my_name();
    super::run(async move {
        let asked = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                drop(guard);
                failed(ui_call, "Not connected yet. Try again in a moment.");
                return Ok(());
            };
            m.join_call_link(&root, &name)
                .await
                .map(|j| (j, m.is_link_admin(&root), m.call_links().ok()))
        };
        match asked {
            Ok((LinkJoin::Ticket(t), admin, links)) => {
                let lobby = links
                    .into_iter()
                    .flatten()
                    .any(|l| *l.root == *root && l.lobby);
                enter_link(ui_call, root, t, admin, lobby).await;
            }
            Ok((LinkJoin::Waiting(place), ..)) => {
                ui::live::update(ui_call, |c| c.set_phase(Phase::Lobby));
                poll_place(ui_call, root, place);
            }
            Err(songbird_app_core::Error::Server(404)) => {
                failed(ui_call, "This link doesn't work any more.");
            }
            Err(songbird_app_core::Error::Server(409)) => {
                failed(ui_call, "The lobby is full. Try again later.");
            }
            Err(_) => failed(ui_call, "The call couldn't be joined. Try again."),
        }
        Ok(())
    });
}

/// Asks again, every few seconds, whether an admin let this device in.
fn poll_place(ui_call: u64, root: Zeroizing<[u8; 32]>, place: [u8; 16]) {
    set_timeout(LOBBY_POLL, move || {
        if !ui::live::exists(ui_call) {
            return;
        }
        super::run(async move {
            let polled = {
                let lock = core();
                let mut guard = lock.lock().await;
                let Some(m) = guard.as_mut() else {
                    return Ok(());
                };
                m.poll_lobby(&root, &place).await
            };
            match polled {
                Ok(Some(t)) => {
                    ui::live::update(ui_call, |c| c.set_phase(Phase::Connecting));
                    enter_link(ui_call, root, t, false, true).await;
                }
                Ok(None) => poll_place(ui_call, root, place),
                Err(songbird_app_core::Error::Server(404)) => {
                    ended_remotely(ui_call, EndReason::NotAdmitted);
                }
                // A passing failure: ask again.
                Err(_) => poll_place(ui_call, root, place),
            }
            Ok(())
        });
    });
}

/// In with a ticket: this device's own sender key, then the room.
async fn enter_link(
    ui_call: u64,
    root: Zeroizing<[u8; 32]>,
    ticket: Ticket,
    admin: bool,
    lobby: bool,
) {
    if !ui::live::exists(ui_call) {
        return;
    }
    let Some(own) = fresh_key() else {
        failed(ui_call, "The call couldn't be joined.");
        return;
    };
    let mut keys = HashMap::new();
    keys.insert(ticket.identity.clone(), (0, own.clone()));
    let keys = Arc::new(Mutex::new(Keys::Link { keys }));
    let chat = 0;
    LIVE.with_borrow_mut(|l| {
        *l = Some(Live {
            ui: ui_call,
            chat,
            call: ticket.call,
            peer: None,
            group: None,
            offered: None,
            keys: Some(keys.clone()),
            media: None,
            camera: None,
            frames: None,
            screen: None,
            known: HashMap::new(),
            link: Some(LinkState {
                root,
                admin,
                lobby,
                own: (0, own),
                cards: HashMap::new(),
                pending: HashMap::new(),
                keyed: std::collections::HashSet::new(),
                waiting: HashMap::new(),
            }),
        });
    });
    ui::live::update(ui_call, |c| {
        c.admin = admin;
        c.code = None;
    });
    join(ui_call, ticket, &keys, None, true).await;
}

/// In the room: this device's card to everyone, and (an admin of a link with a lobby) the
/// lobby watched.
fn link_in_room(ui_call: u64, call: &Arc<Call>) {
    send_card(ui_call, call, None);
    let watch = LIVE.with_borrow(|l| {
        l.as_ref()
            .and_then(|l| l.link.as_ref())
            .is_some_and(|k| k.admin && k.lobby)
    });
    if watch {
        watch_lobby(ui_call);
    }
}

/// Sends this device's sealed card (to `to`, or everyone).
fn send_card(ui_call: u64, call: &Arc<Call>, to: Option<String>) {
    let Some(root) = LIVE.with_borrow(|l| {
        l.as_ref()
            .filter(|l| l.ui == ui_call)
            .and_then(|l| l.link.as_ref().map(|k| k.root.clone()))
    }) else {
        return;
    };
    let call = call.clone();
    let own = call.identity().to_owned();
    let name = my_name();
    super::run(async move {
        let card = {
            let lock = core();
            let guard = lock.lock().await;
            guard
                .as_ref()
                .and_then(|m| m.link_card(&root, &own, &name).ok())
        };
        if let Some(card) = card {
            drop(background(async move {
                let _ = call.send_data(CARD_TOPIC, card, to.as_deref()).await;
            }));
        }
        Ok(())
    });
}

/// Someone entered: they get this device's card, and everyone present a new key from this
/// device (so the newcomer cannot decrypt what came before).
fn link_joined(ui_call: u64, identity: &str) {
    if let Some(media) = media_of(ui_call) {
        send_card(ui_call, &media, Some(identity.to_owned()));
    }
    rotate(ui_call);
}

/// Someone left: those still present get a new key from this device (so the leaver cannot
/// decrypt what follows).
fn link_left(ui_call: u64, identity: &str) {
    with_live(|l| {
        if let Some(k) = l.link.as_mut() {
            k.cards.remove(identity);
            k.pending.remove(identity);
            k.keyed.remove(identity);
        }
    });
    rotate(ui_call);
}

/// A new sender key for this device at the next index: installed at once, sent to every
/// participant whose card arrived, sent with a second later (§8.4, as §8.2 for epochs).
fn rotate(ui_call: u64) {
    let Some(media) = media_of(ui_call) else {
        return;
    };
    let Some(key) = fresh_key() else { return };
    let own = media.identity().to_owned();
    let Some((index, targets)) = with_live(|l| {
        let k = l.link.as_mut()?;
        let index = (k.own.0 + 1) % songbird_calls_media::KEY_RING;
        k.own = (index, key.clone());
        if let Some(keys) = &l.keys
            && let Ok(mut keys) = keys.lock()
            && let Keys::Link { keys } = &mut *keys
        {
            keys.insert(own.clone(), (index, key.clone()));
        }
        Some((index, k.cards.clone()))
    })
    .flatten() else {
        return;
    };
    if media.set_key(&own, index, &key).is_err() {
        return;
    }
    for (identity, card) in targets {
        send_key(ui_call, identity, card);
    }
    set_timeout(Duration::from_secs(1), move || {
        let _ = media.send_with(index);
    });
}

/// Sends this device's current key to one participant's device, sealed in a Signal session,
/// on the room's data channel.
fn send_key(ui_call: u64, identity: String, card: Card) {
    let Some((call, media, own)) =
        with_live(|l| Some((l.call, l.media.clone()?, l.link.as_ref()?.own.clone()))).flatten()
    else {
        return;
    };
    if !is_live(ui_call) {
        return;
    }
    let me = media.identity().to_owned();
    super::run(async move {
        let envelope = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            m.link_key_envelope(&card.account, &card.device, &call, &me, own.0, &own.1)
                .await
        };
        match envelope {
            Ok(env) => drop(background(async move {
                let _ = media.send_data(KEY_TOPIC, env, Some(&identity)).await;
            })),
            Err(_) => toast("Someone in the call couldn't be sent your key."),
        }
        Ok(())
    });
}

/// Data on a link call's channel: a card, a key (a Signal envelope), a raised hand.
fn link_data(ui_call: u64, identity: String, topic: &str, payload: Vec<u8>) {
    let Some(root) = LIVE.with_borrow(|l| {
        l.as_ref()
            .and_then(|l| l.link.as_ref().map(|k| k.root.clone()))
    }) else {
        return;
    };
    match topic {
        t if t == CARD_TOPIC => {
            // A card counts only for the identity that sent it.
            let Ok(card) = open_link_card(&root, &payload) else {
                return;
            };
            if card.identity != identity {
                return;
            }
            let fresh = with_live(|l| {
                l.link
                    .as_mut()
                    .map(|k| k.cards.insert(identity.clone(), card.clone()).is_none())
            })
            .flatten()
            .unwrap_or(false);
            name_participant(ui_call, &identity);
            if fresh {
                send_key(ui_call, identity.clone(), card);
            }
            // A key that came before the card.
            let early =
                with_live(|l| l.link.as_mut().and_then(|k| k.pending.remove(&identity))).flatten();
            if let Some((account, device, index, key)) = early {
                use_key(ui_call, &identity, account, device, index, &key);
            }
        }
        t if t == KEY_TOPIC => {
            let call = LIVE
                .with_borrow(|l| l.as_ref().map(|l| l.call))
                .unwrap_or_default();
            super::run(async move {
                let opened = {
                    let lock = core();
                    let mut guard = lock.lock().await;
                    let Some(m) = guard.as_mut() else {
                        return Ok(());
                    };
                    m.receive(&payload).await
                };
                if let Ok(songbird_app_core::messenger::Received::Call {
                    account,
                    call: c,
                    signal:
                        Signal::SenderKey {
                            device,
                            identity: kid,
                            index,
                            key,
                        },
                    ..
                }) = opened
                    && c == call
                    && kid == identity
                {
                    use_key(ui_call, &identity, account, device, index, &key);
                }
                Ok(())
            });
        }
        t if t == HAND_TOPIC => {
            if let Ok(b) = open_link_data(&root, &payload) {
                let raised = b.first() == Some(&1);
                ui::live::update(ui_call, |c| {
                    if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
                        p.hand = raised;
                    }
                });
            }
        }
        _ => {}
    }
}

/// Installs a participant's key if it came from the device their card names; one that came
/// before the card waits for it.
fn use_key(
    ui_call: u64,
    identity: &str,
    account: [u8; 16],
    device: [u8; 16],
    index: u32,
    key: &Zeroizing<[u8; 32]>,
) {
    let matches = with_live(|l| {
        let k = l.link.as_mut()?;
        match k.cards.get(identity) {
            Some(card) => Some(card.account == account && card.device == device),
            None => {
                k.pending
                    .insert(identity.to_owned(), (account, device, index, key.clone()));
                None
            }
        }
    })
    .flatten();
    if matches != Some(true) {
        return;
    }
    let Some(media) = media_of(ui_call) else {
        return;
    };
    if media.set_key(identity, index, key).is_err() {
        return;
    }
    with_live(|l| {
        if let Some(keys) = &l.keys
            && let Ok(mut keys) = keys.lock()
            && let Keys::Link { keys } = &mut *keys
        {
            keys.insert(identity.to_owned(), (index, key.clone()));
        }
        if let Some(k) = l.link.as_mut() {
            k.keyed.insert(identity.to_owned());
        }
    });
    name_participant(ui_call, identity);
}

/// A link participant's tile name: a contact's name once their key came from the device their
/// card names; anyone else's card name, marked unverified (anyone holding the link can write
/// any name in a card, §8.7).
fn name_participant(ui_call: u64, identity: &str) {
    let Some((card, keyed)) = LIVE.with_borrow(|l| {
        let k = l.as_ref()?.link.as_ref()?;
        Some((k.cards.get(identity)?.clone(), k.keyed.contains(identity)))
    }) else {
        return;
    };
    let id = ui_id(&card.account);
    let me = super::chat::me_account() == card.account;
    let contact = crate::store::try_store()
        .filter(|s| super::chat::owns(id) && s.contact(id).is_some())
        .map(|s| s.contact_name(Some(id)));
    let shown = if card.name.trim().is_empty() {
        "Guest".to_string()
    } else {
        card.name.trim().to_string()
    };
    let name = match (contact, keyed, me) {
        (_, true, true) => format!("{} (another device)", my_name()),
        (Some(n), true, false) => n,
        _ => format!("{shown} (unverified)"),
    };
    ui::live::update(ui_call, |c| {
        if let Some(p) = c.participants.iter_mut().find(|p| p.identity == identity) {
            p.name = name;
            if keyed && !me {
                p.id = id;
                p.seed = id;
            }
        }
    });
}

/// An admin's view of the lobby, asked again every few seconds while the call lasts.
fn watch_lobby(ui_call: u64) {
    let Some(root) = LIVE.with_borrow(|l| {
        l.as_ref()
            .and_then(|l| l.link.as_ref().map(|k| k.root.clone()))
    }) else {
        return;
    };
    super::run(async move {
        let waiting = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            m.lobby_waiting(&root).await
        };
        if let Ok(waiting) = waiting {
            let mut places = HashMap::new();
            let shown: Vec<ui::Waiting> = waiting
                .into_iter()
                .map(|w| {
                    let id = stranger_id(&super::wire::hex(&w.handle));
                    places.insert(id, w.handle);
                    let contact = ui_id(&w.account);
                    let name = crate::store::try_store()
                        .filter(|s| super::chat::owns(contact) && s.contact(contact).is_some())
                        .map_or_else(
                            || {
                                let n = w.name.trim();
                                format!("{} (unverified)", if n.is_empty() { "Guest" } else { n })
                            },
                            |s| s.contact_name(Some(contact)),
                        );
                    ui::Waiting { id, name, seed: id }
                })
                .collect();
            with_live(|l| {
                if let Some(k) = l.link.as_mut() {
                    k.waiting = places;
                }
            });
            ui::live::update(ui_call, |c| c.waiting = shown);
        }
        if is_live(ui_call) {
            set_timeout(LOBBY_POLL, move || watch_lobby(ui_call));
        }
        Ok(())
    });
}

/// Lets someone waiting in the lobby in, or turns them away (the admin's choice).
pub fn admit(ui_call: u64, waiting: u64, let_in: bool) {
    let Some((root, place)) = with_live(|l| {
        let k = l.link.as_ref()?;
        Some((k.root.clone(), *k.waiting.get(&waiting)?))
    })
    .flatten() else {
        return;
    };
    ui::live::update(ui_call, |c| c.waiting.retain(|w| w.id != waiting));
    super::run(async move {
        let done = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            m.admit(&root, &place, let_in).await
        };
        if done.is_err() {
            toast("That didn't go through. They may have left the lobby.");
        }
        Ok(())
    });
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
    let code = keys.lock().ok().and_then(|k| k.code());
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
    PREPARED.with_borrow_mut(|p| p.remove(&ui_call));
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
    let leaving = background(async move {
        if let Some(share) = screen {
            share.stop().await;
        }
        let mut traffic = (0, 0);
        if let Some(media) = media {
            // What WebRTC counted for this call, for Network usage.
            traffic = media.traffic().await;
            let _ = media.close().await;
        }
        traffic
    });
    spawn(async move {
        if let Ok((sent, received)) = leaving.await {
            super::usage::count(
                super::usage::Lane::Calls,
                usize::try_from(sent).unwrap_or(usize::MAX),
                usize::try_from(received).unwrap_or(usize::MAX),
            );
        }
    });
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
