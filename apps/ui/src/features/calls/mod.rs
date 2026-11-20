//! A8: calls — the Calls tab, incoming call, in-call controls, group
//! calls, call links and lobby, verification code, desktop call window.
//!
//! One call at a time lives in a global ([`state::CallState`]); every
//! screen reads it and changes it through the functions here. The call
//! layer, mounted once at the root, drives the call's clock while a call
//! exists and draws the call: full screen on phones, a panel that
//! shrinks to a corner player on desktops, the ringing card, the ended
//! state.
//!
//! A call in a real chat is real (D-0152): `backend::calls` rings, answers and hangs up through
//! app-core and carries the media through LiveKit, end-to-end encrypted, and moves the call
//! through [`live`]. The demo fixtures' chats (previews, screenshots) have no one at the other
//! end: their calls walk the same phases on timers ([`state`]).

mod ringing;
mod state;
mod surface;
mod tab;

use std::time::Duration;

use nsh_core::prelude::*;

use crate::{
    store::{ChatId, ChatKind, Store, store},
    ui::*,
};

use state::ActiveCall;
pub use state::{CallState, EndReason, Layout, MicAccess, Participant, Phase, Share, Waiting};
pub use tab::calls_tab;

/// The call global.
fn calls() -> Signal<CallState> {
    global::<CallState>()
}

/// Changes the active call, if there is one.
fn with_call(f: impl FnOnce(&mut ActiveCall)) {
    calls().update(|s| {
        if let Some(c) = s.call.as_mut() {
            f(c);
        }
    });
}

/// A memo of one field of the active call: bindings that read it run only
/// when it changes, not on every tick.
fn field<T: Clone + PartialEq + 'static>(
    f: impl Fn(&ActiveCall) -> T + 'static,
    none: T,
) -> Memo<T> {
    let st = calls();
    computed(move || st.with(|s| s.call.as_ref().map(&f).unwrap_or_else(|| none.clone())))
}

/// The UI's id for a call (the room's own id stays in `backend::calls`).
fn next_id() -> u64 {
    let n = global::<CallSerial>();
    n.update(|x| x.0 += 1);
    n.with_untracked(|x| x.0)
}

#[derive(Default, Clone, PartialEq)]
struct CallSerial(u64);

/// The chat's title, hue, whether it is a group, and who would be in it.
fn describe(s: &Store, chat: ChatId) -> Option<(String, u64, bool, Vec<Participant>)> {
    let c = s.chat(chat)?;
    let group = !matches!(c.kind, ChatKind::Direct(_) | ChatKind::WhatsApp(_));
    let people: Vec<Participant> = if group {
        c.members
            .iter()
            .filter_map(|m| m.contact)
            .filter_map(|id| s.contact(id))
            .take(7)
            .enumerate()
            .map(|(i, p)| Participant {
                id: p.id,
                name: p.name,
                seed: p.id,
                // demo: a few people join muted, some with video.
                muted: i % 3 == 1,
                hand: false,
                video: i % 2 == 0,
                joins_at: i as u32 * 3,
                identity: String::new(),
                decrypting: true,
                speaking: false,
            })
            .collect()
    } else {
        s.peer(chat)
            .map(|p| Participant {
                id: p.id,
                name: p.name,
                seed: p.id,
                muted: false,
                hand: false,
                video: true,
                joins_at: 0,
                identity: String::new(),
                decrypting: true,
                speaking: false,
            })
            .into_iter()
            .collect()
    };
    // A 1:1 call shows the person: their contact id is the portrait's
    // seed everywhere (lists, headers, calls). A group shows its scene.
    let seed = if group {
        c.seed
    } else {
        people.first().map_or(c.seed, |p| p.id)
    };
    Some((c.title, seed, group, people))
}

/// Opens the call in its first phase, behind the microphone explainer
/// on a desktop's first call.
fn begin(mut call: ActiveCall) {
    if calls().with_untracked(|s| s.call.as_ref().is_some_and(|c| !c.ended())) {
        toast("You're already in a call");
        return;
    }
    let mic = global::<MicAccess>();
    if is_desktop() && mic.get_untracked() != MicAccess::Allowed && call.phase != Phase::Incoming {
        call.after_permission = call.phase;
        call.phase = Phase::Permission;
    }
    if call.phase != Phase::Incoming {
        announce_for_accessibility(format!("Calling {}", call.title));
    }
    start_real(&call);
    calls().set(CallState { call: Some(call) });
}

/// Starts a call in `chat`: a voice call, or a video call with `video`.
/// Direct chats ring the other person; groups start a group call.
pub fn start_call(chat: ChatId, video: bool) {
    let s = store();
    start_call_in(&s, chat, video);
}

fn start_call_in(s: &Store, chat: ChatId, video: bool) {
    let Some((title, seed, group, people)) = describe(s, chat) else {
        return;
    };
    let mut call = ActiveCall::new(next_id(), title, seed, Phase::Calling);
    call.chat = Some(chat);
    call.group = group;
    call.video = video;
    if crate::backend::chat::owns(chat) {
        // Real: the room fills the tiles as people join.
        call.real = true;
        call.ring_left = ring_ticks(songbird_app_core::calls::RING_MS);
    } else {
        call.participants = people;
    }
    begin(call);
}

/// A ring of `ms` milliseconds in ticks.
fn ring_ticks(ms: u64) -> u32 {
    u32::try_from(ms / state::TICK_MS).unwrap_or(u32::MAX)
}

/// Asks the backend to set up a real call that just left its first phase (or the permission
/// explainer).
fn start_real(call: &ActiveCall) {
    if call.real && call.link && call.phase == Phase::Connecting {
        crate::backend::calls::join_link(call.id);
    } else if call.real
        && !call.incoming
        && call.phase == Phase::Calling
        && let Some(chat) = call.chat
    {
        crate::backend::calls::start(call.id, chat, call.video);
    }
}

/// Mock: makes `chat` call the user, as a push would (the Calls tab's
/// demo rows use it). The real ring comes from app-core's push handler.
pub fn simulate_incoming(chat: ChatId, video: bool) {
    let s = store();
    simulate_in(&s, chat, video, false);
}

fn simulate_in(s: &Store, chat: ChatId, video: bool, elsewhere: bool) {
    let Some((title, seed, group, people)) = describe(s, chat) else {
        return;
    };
    let mut call = ActiveCall::new(next_id(), title, seed, Phase::Incoming);
    call.chat = Some(chat);
    call.group = group;
    call.video = video;
    call.participants = people;
    call.answered_elsewhere = elsewhere;
    let name = call.title.clone();
    begin(call);
    if !elsewhere {
        s.arrive(crate::store::Arrival::Call { chat, video });
    }
    announce_for_accessibility(format!("{name} is calling"));
}

/// Opens a call link (pasted or tapped): the user's own links by their name, anyone else's as
/// "Call link"; in through the lobby where it has one. `false` if `url` is not a call link.
pub fn open_link(url: &str) -> bool {
    let Some((root, own)) = crate::backend::calls::describe_link(url) else {
        return false;
    };
    let (title, admin) = match own {
        Some((name, _)) => (name, true),
        None => ("Call link".to_string(), false),
    };
    let seed = u64::from_be_bytes(root[..8].try_into().unwrap_or_default()) >> 3;
    let mut call = ActiveCall::new(next_id(), title, seed, Phase::Connecting);
    call.group = true;
    call.real = true;
    call.link = true;
    call.admin = admin;
    crate::backend::calls::prepare_link(call.id, root);
    begin(call);
    true
}

/// Lets someone in from a link's lobby, or turns them away (the admin's side).
fn decide(id: u64, let_in: bool) {
    match real_id() {
        Some(call) => crate::backend::calls::admit(call, id, let_in),
        None => with_call(|c| c.admit(id, let_in)),
    }
}

/// Joins a call link: the user's own links open with the lobby on the
/// admin's side (people waiting); `guest` joins as someone waiting.
fn join_link(name: String, seed: u64, lobby: bool, guest: bool) {
    let phase = if lobby && guest {
        Phase::Lobby
    } else {
        Phase::Connecting
    };
    let mut call = ActiveCall::new(next_id(), name, seed, phase);
    call.group = true;
    // demo: who is already in the call, and who waits.
    let people = [
        ("Ioana Ștefănescu", 3),
        ("Andrei Dumitrescu", 4),
        ("Priya Natarajan", 10),
    ];
    call.participants = people
        .iter()
        .enumerate()
        .map(|(i, (n, id))| Participant {
            id: *id,
            name: (*n).into(),
            seed: *id,
            muted: i == 1,
            hand: false,
            video: i != 1,
            joins_at: i as u32 * 3,
            identity: String::new(),
            decrypting: true,
            speaking: false,
        })
        .collect();
    if lobby && !guest {
        call.waiting = [
            ("Amélie Durand", 8),
            ("Tomás Silva", 11),
            ("Cristina Pavel", 14),
        ]
        .iter()
        .map(|(n, id)| Waiting {
            id: *id,
            name: (*n).into(),
            seed: *id,
        })
        .collect();
    }
    begin(call);
}

// ------------------------------------------------------------------ actions

/// Ends the call (hang up, decline, leave the lobby) and logs it.
fn end(s: Store, reason: EndReason) {
    let mut log = None;
    let mut chat = None;
    let mut video = false;
    let mut real = None;
    with_call(|c| {
        log = c.end(reason);
        chat = c.chat;
        video = c.video;
        real = c.real.then_some(c.id);
    });
    if let Some(id) = real {
        crate::backend::calls::closed(id, reason, true);
    }
    if let (Some((direction, secs)), Some(chat)) = (log, chat) {
        s.log_call(chat, video, direction, secs);
    }
    if let Some(line) = calls().with_untracked(|st| st.call.as_ref().map(state::status)) {
        announce_for_accessibility(line);
    }
}

fn hang_up(s: Store) {
    let reason = match calls().with_untracked(|st| st.call.as_ref().map(|c| c.phase)) {
        Some(Phase::Incoming) => EndReason::Declined,
        Some(Phase::Lobby) => EndReason::LeftLobby,
        _ => EndReason::Hangup,
    };
    end(s, reason);
}

fn accept(video: bool) {
    let mut real = None;
    with_call(|c| {
        c.accept(video);
        real = (c.real && c.phase == Phase::Connecting).then_some(c.id);
    });
    if let Some(id) = real {
        crate::backend::calls::answer(id);
    }
}

/// The real call's id, if the call going on is real.
fn real_id() -> Option<u64> {
    calls().with_untracked(|s| s.call.as_ref().filter(|c| c.real).map(|c| c.id))
}

fn toggle_mute() {
    with_call(|c| c.muted = !c.muted);
    let muted = calls().with_untracked(|s| s.call.as_ref().is_some_and(|c| c.muted));
    if let Some(id) = real_id() {
        crate::backend::calls::set_muted(id, muted);
    }
    announce_for_accessibility(if muted {
        "Microphone off"
    } else {
        "Microphone on"
    });
}

fn toggle_video() {
    with_call(|c| c.video = !c.video);
    let on = calls().with_untracked(|s| s.call.as_ref().is_some_and(|c| c.video));
    if let Some(id) = real_id() {
        crate::backend::calls::set_video(id, on);
    }
    announce_for_accessibility(if on { "Camera on" } else { "Camera off" });
}

fn set_minimised(minimised: bool) {
    with_call(|c| {
        if c.live() && !c.ended() {
            c.minimised = minimised;
        }
    });
}

/// Drives the call's clock and applies what each tick asks. Owned by the
/// call layer's branch, so it runs only while a call exists.
fn drive(s: Store) {
    set_interval(Duration::from_millis(state::TICK_MS), move || {
        let mut step = state::Step::default();
        let mut chat = None;
        let mut video = false;
        let mut ended = None;
        let before = calls().with_untracked(|st| st.call.as_ref().map(|c| c.phase));
        with_call(|c| {
            step = c.tick();
            chat = c.chat;
            video = c.video;
            if step.ended
                && let Phase::Ended(reason) = c.phase
            {
                ended = Some((c.id, reason));
            }
        });
        if let Some((id, reason)) = ended {
            // A ring that ran out here: the caller tells the callee to stop ringing.
            crate::backend::calls::closed(id, reason, reason == EndReason::NoAnswer);
        }
        if let (Some((direction, secs)), Some(chat)) = (step.log, chat) {
            s.log_call(chat, video, direction, secs);
        }
        let after = calls().with_untracked(|st| st.call.as_ref().map(|c| c.phase));
        if before != after {
            if let Some(line) = calls().with_untracked(|st| st.call.as_ref().map(state::status)) {
                announce_for_accessibility(line);
            }
        }
        if step.clear {
            calls().set(CallState::default());
        }
    });
}

/// The keys of a call on desktops: M mute, V camera, Escape minimise,
/// Shift+Mod+H hang up. Every one also has a button.
fn keys(s: Store) {
    if !is_desktop() {
        return;
    }
    on_key("shift+mod+h", move |_| hang_up(s));
}

fn panel_keys() {
    if !is_desktop() {
        return;
    }
    on_key("m", |_| toggle_mute());
    on_key("v", |_| toggle_video());
    on_key("escape", |_| set_minimised(true));
}

/// The call layer over everything (incoming ring, active call, mini
/// player). Mounted once at the root; it draws nothing, takes no space
/// and no pointer events while there is no call.
pub fn call_layer() -> AnyElement {
    let s = store();
    let active = field(|_| true, false);
    when(
        move || active.get(),
        move || {
            drive(s);
            keys(s);
            surface::layer(s)
        },
    )
    .into_element()
}

/// Raises or lowers the user's hand (everyone in the call sees it).
fn toggle_hand() {
    with_call(|c| c.hand = !c.hand);
    let raised = calls().with_untracked(|s| s.call.as_ref().is_some_and(|c| c.hand));
    if let Some(id) = real_id() {
        crate::backend::calls::set_hand(id, raised);
    }
    announce_for_accessibility(if raised {
        "Hand raised"
    } else {
        "Hand lowered"
    });
}

/// Starts sharing the screen, or a window.
fn start_sharing(what: Share) {
    with_call(|c| {
        c.sharing = Some(what);
        c.minimised = false;
    });
    if let Some(id) = real_id() {
        crate::backend::calls::share_screen(id, what == Share::Window);
    }
    announce_for_accessibility("You are sharing your screen");
}

/// Stops sharing the screen.
fn stop_sharing() {
    with_call(|c| c.sharing = None);
    if let Some(id) = real_id() {
        crate::backend::calls::stop_sharing(id);
    }
    announce_for_accessibility("You stopped sharing your screen");
}

/// The participants' decoded video in real calls, by participant identity.
#[derive(Default, Clone)]
pub struct CallFrames(pub std::collections::HashMap<String, nsh_core::element::ImageSrc>);

impl PartialEq for CallFrames {
    fn eq(&self, _: &Self) -> bool {
        // Every set is a new frame.
        false
    }
}

/// The devices a real call can use (the platform's), shown in the devices sheet.
#[derive(Default, Clone, PartialEq)]
pub struct CallDevices {
    pub mics: Vec<String>,
    pub cameras: Vec<String>,
    pub outputs: Vec<String>,
}

/// What `backend::calls` changes: only the call it names, never another.
pub mod live {
    use super::{ActiveCall, CallDevices, CallFrames, CallState, EndReason, Phase, calls};
    use nsh_core::prelude::*;

    /// Changes call `id` if it is the one going on.
    pub fn update(id: u64, f: impl FnOnce(&mut ActiveCall)) {
        calls().update(|s| {
            if let Some(c) = s.call.as_mut().filter(|c| c.id == id) {
                f(c);
            }
        });
    }

    /// Reads call `id`.
    pub fn with<T>(id: u64, f: impl FnOnce(&ActiveCall) -> T) -> Option<T> {
        calls().with_untracked(|s| s.call.as_ref().filter(|c| c.id == id).map(f))
    }

    /// Whether call `id` still goes on.
    pub fn exists(id: u64) -> bool {
        with(id, |c| !c.ended()).unwrap_or(false)
    }

    /// Whether any call goes on (or rings) here.
    pub fn busy() -> bool {
        calls().with_untracked(|s| s.call.as_ref().is_some_and(|c| !c.ended()))
    }

    /// Ends call `id` for a reason this device did not choose, logging it.
    pub fn end(id: u64, reason: EndReason) {
        let mut log = None;
        let mut chat = None;
        let mut video = false;
        update(id, |c| {
            log = c.end(reason);
            chat = c.chat;
            video = c.video;
        });
        if let (Some((direction, secs)), Some(chat), Some(s)) =
            (log, chat, crate::store::try_store())
        {
            s.log_call(chat, video, direction, secs);
        }
        crate::sounds::stop();
        if let Some(line) = with(id, super::state::status) {
            announce_for_accessibility(line);
        }
    }

    /// An offer rings here: the incoming call, ringing `ring_ms` at most. `None` if one goes on.
    pub fn ring(chat: crate::store::ChatId, video: bool, group: bool, ring_ms: u64) -> Option<u64> {
        if busy() || ring_ms == 0 {
            return None;
        }
        let s = crate::store::try_store()?;
        let (title, seed, _, _) = super::describe(&s, chat)?;
        let mut call = ActiveCall::new(super::next_id(), title, seed, Phase::Incoming);
        call.chat = Some(chat);
        call.group = group;
        call.video = video;
        call.real = true;
        call.ring_left = super::ring_ticks(ring_ms);
        let (id, name) = (call.id, call.title.clone());
        calls().set(CallState { call: Some(call) });
        s.arrive(crate::store::Arrival::Call { chat, video });
        announce_for_accessibility(format!("{name} is calling"));
        Some(id)
    }

    /// Shows (or with `None` removes) a participant's latest video frame.
    pub fn frame(identity: &str, src: Option<nsh_core::element::ImageSrc>) {
        global::<CallFrames>().update(|f| match src {
            Some(src) => {
                f.0.insert(identity.to_owned(), src);
            }
            None => {
                f.0.remove(identity);
            }
        });
    }

    /// Forgets every frame (the call ended).
    pub fn clear_frames() {
        global::<CallFrames>().set(CallFrames::default());
    }

    /// Reads the platform's devices again (the call's audio opened, or a device came or went).
    pub fn devices_changed() {
        let (mics, outputs) = crate::backend::calls::audio_devices();
        let cameras = crate::camera::lens_names();
        global::<CallDevices>().set(CallDevices {
            mics,
            cameras,
            outputs,
        });
    }
}
