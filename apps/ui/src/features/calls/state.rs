//! The call state machine, in plain Rust (no NSH), so it is unit tested.
//!
//! A **real** call (a chat app-core owns, D-0152) moves through its phases on what signalling
//! and the media room report (`backend::calls`): Calling → Ringing → Connecting → Connected →
//! Ended for an outgoing call, Incoming → Connecting → Connected → Ended for one that rings
//! here. The 250 ms tick only counts: connected time, how long a ring has left, the ended
//! state's seconds.
//!
//! The demo fixtures (screenshots, previews) walk the same phases on timers instead: there is
//! no one at the other end of a demo chat.

use crate::store::{CallDirection, ChatId};

/// One tick, in milliseconds.
pub const TICK_MS: u64 = 250;
/// Ticks per second.
pub const PER_SEC: u32 = (1000 / TICK_MS) as u32;

/// Outgoing: reaching their devices before their phone rings.
pub const CALLING_TICKS: u32 = 4;
/// Outgoing: how long they take to answer.
pub const RINGING_TICKS: u32 = 12;
/// Connecting: short on purpose ("connect fast").
pub const CONNECTING_TICKS: u32 = 3;
/// Incoming: rings 30 seconds before it counts as missed.
pub const INCOMING_TICKS: u32 = 30 * PER_SEC;
/// A lobby: an admin lets the user in after this long.
pub const LOBBY_TICKS: u32 = 16;
/// The "Call ended · 2:31" state stays this long.
pub const ENDED_TICKS: u32 = 10;
/// The demo of a call answered on another device.
pub const ELSEWHERE_TICKS: u32 = 12;

/// Where a call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Desktop, first call: the microphone explainer.
    Permission,
    /// Outgoing: reaching the other person's devices.
    Calling,
    /// Outgoing: their phone is ringing.
    Ringing,
    /// Incoming: ringing here.
    Incoming,
    /// A call link with a lobby: waiting for an admin.
    Lobby,
    Connecting,
    Connected,
    Ended(EndReason),
}

/// Why a call ended (the ended state's line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Someone hung up.
    Hangup,
    /// The user declined an incoming call.
    Declined,
    /// An incoming call rang out.
    Missed,
    /// Another of the user's devices answered.
    AnsweredElsewhere,
    /// The user left a lobby before being let in.
    LeftLobby,
    /// The user chose not to allow the microphone.
    NoMicrophone,
    /// The other person declined.
    TheyDeclined,
    /// The other person was in another call.
    Busy,
    /// Nobody answered before the ring ran out.
    NoAnswer,
    /// The call could not be set up, or the connection was lost.
    Failed,
    /// A call link's admin turned the user away from the lobby (or the link is gone).
    NotAdmitted,
}

/// Grid of equal tiles or one big speaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Grid,
    Speaker,
}

/// What the user is sharing (desktop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Share {
    EntireScreen,
    Window,
}

/// Someone else in the call.
#[derive(Debug, Clone, PartialEq)]
pub struct Participant {
    pub id: u64,
    pub name: String,
    pub seed: u64,
    pub muted: bool,
    pub hand: bool,
    pub video: bool,
    /// Connected ticks after which their tile arrives (demo group calls; 0 in real calls).
    pub joins_at: u32,
    /// Their participant identity in the room (real calls).
    pub identity: String,
    /// Whether their frames decrypt (false: shown as "can't decrypt", never played).
    pub decrypting: bool,
    /// Speaking now.
    pub speaking: bool,
}

/// Someone waiting in a call link's lobby (the admin's side).
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub id: u64,
    pub name: String,
    pub seed: u64,
}

/// The call going on.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveCall {
    pub id: u64,
    /// The chat it belongs to; `None` for a call link.
    pub chat: Option<ChatId>,
    pub title: String,
    pub seed: u64,
    pub group: bool,
    pub incoming: bool,
    pub phase: Phase,
    /// The phase to go to after the permission explainer.
    pub after_permission: Phase,
    /// Ticks in the current phase.
    pub ticks: u32,
    /// Ticks spent connected.
    pub connected: u32,
    pub video: bool,
    pub muted: bool,
    pub speaker: bool,
    pub front_camera: bool,
    pub hand: bool,
    pub sharing: Option<Share>,
    pub layout: Layout,
    pub minimised: bool,
    pub participants: Vec<Participant>,
    pub waiting: Vec<Waiting>,
    /// Indices into the device lists (desktop).
    pub mic: usize,
    pub camera: usize,
    pub output: usize,
    /// Demo: another device answers after [`ELSEWHERE_TICKS`].
    pub answered_elsewhere: bool,
    /// A real call: signalling and media move it, not the demo's timers.
    pub real: bool,
    /// Ticks a ring has left (real: Incoming, and Calling/Ringing for an outgoing call).
    pub ring_left: u32,
    /// The verification code from the call's key (four indices into [`SYMBOLS`]).
    pub code: Option<[u8; 4]>,
    /// The microphone's level now (0 to 1), from the room's speaker reports.
    pub level: f32,
    /// A call link's call (real): keys per participant, cards, maybe a lobby.
    pub link: bool,
    /// This device administers the link (sees and admits who waits).
    pub admin: bool,
}

impl ActiveCall {
    /// A call in its first phase.
    pub fn new(id: u64, title: String, seed: u64, phase: Phase) -> Self {
        Self {
            id,
            chat: None,
            title,
            seed,
            group: false,
            incoming: phase == Phase::Incoming,
            phase,
            after_permission: phase,
            ticks: 0,
            connected: 0,
            video: false,
            muted: false,
            speaker: false,
            front_camera: true,
            hand: false,
            sharing: None,
            layout: Layout::Grid,
            minimised: false,
            participants: Vec::new(),
            waiting: Vec::new(),
            mic: 0,
            camera: 0,
            output: 0,
            answered_elsewhere: false,
            real: false,
            ring_left: 0,
            code: None,
            level: 0.0,
            link: false,
            admin: false,
        }
    }

    /// Moves a real call to `phase` (from signalling or the room).
    pub fn set_phase(&mut self, phase: Phase) {
        if !self.ended() && self.phase != phase {
            self.go(phase);
        }
    }

    /// Seconds connected.
    pub fn secs(&self) -> u32 {
        self.connected / PER_SEC
    }

    pub fn ended(&self) -> bool {
        matches!(self.phase, Phase::Ended(_))
    }

    /// Ringing, connecting or in the call: the call uses the microphone.
    pub fn live(&self) -> bool {
        !matches!(self.phase, Phase::Ended(_) | Phase::Permission)
    }

    /// The participants whose tiles have arrived.
    pub fn joined(&self) -> Vec<Participant> {
        if self.phase != Phase::Connected {
            return Vec::new();
        }
        self.participants
            .iter()
            .filter(|p| p.joins_at <= self.connected)
            .cloned()
            .collect()
    }

    fn go(&mut self, phase: Phase) {
        self.phase = phase;
        self.ticks = 0;
    }

    /// What ending now writes to the call history, if anything.
    fn history(&self, reason: EndReason) -> Option<(CallDirection, u32)> {
        let reached = self.connected > 0;
        match reason {
            EndReason::Missed => Some((CallDirection::Missed, 0)),
            // The device that answered logs it; history syncs.
            EndReason::AnsweredElsewhere | EndReason::NoMicrophone => None,
            EndReason::LeftLobby | EndReason::NotAdmitted => None,
            EndReason::Declined => Some((CallDirection::Incoming, 0)),
            EndReason::TheyDeclined | EndReason::Busy | EndReason::NoAnswer => {
                Some((CallDirection::Outgoing, 0))
            }
            EndReason::Failed if self.incoming => Some((CallDirection::Incoming, self.secs())),
            EndReason::Failed => Some((CallDirection::Outgoing, self.secs())),
            EndReason::Hangup if self.incoming => Some((CallDirection::Incoming, self.secs())),
            EndReason::Hangup => Some((
                CallDirection::Outgoing,
                if reached { self.secs() } else { 0 },
            )),
        }
    }

    /// Ends the call; returns what to log.
    pub fn end(&mut self, reason: EndReason) -> Option<(CallDirection, u32)> {
        if self.ended() {
            return None;
        }
        let log = self.history(reason);
        self.sharing = None;
        self.minimised = false;
        self.go(Phase::Ended(reason));
        log
    }

    /// Answers an incoming call.
    pub fn accept(&mut self, video: bool) {
        if self.phase == Phase::Incoming {
            self.video = video;
            self.go(Phase::Connecting);
        }
    }

    /// The microphone explainer was answered.
    pub fn permission(&mut self, granted: bool) -> Option<(CallDirection, u32)> {
        if self.phase != Phase::Permission {
            return None;
        }
        if granted {
            self.go(self.after_permission);
            None
        } else {
            self.end(EndReason::NoMicrophone)
        }
    }

    /// Admits or turns away someone in the lobby (the admin's side).
    pub fn admit(&mut self, id: u64, admit: bool) {
        if let Some(i) = self.waiting.iter().position(|w| w.id == id) {
            let w = self.waiting.remove(i);
            if admit {
                self.participants.push(Participant {
                    id: w.id,
                    name: w.name,
                    seed: w.seed,
                    muted: true,
                    hand: false,
                    video: false,
                    joins_at: self.connected,
                    identity: String::new(),
                    decrypting: true,
                    speaking: false,
                });
            }
        }
    }

    /// One tick. Returns what to log, and whether the call is over and should be cleared.
    pub fn tick(&mut self) -> Step {
        self.ticks += 1;
        if self.real {
            return self.tick_real();
        }
        let mut step = Step::default();
        match self.phase {
            Phase::Permission => {}
            Phase::Calling if self.ticks >= CALLING_TICKS => self.go(Phase::Ringing),
            Phase::Ringing if self.ticks >= RINGING_TICKS => self.go(Phase::Connecting),
            Phase::Lobby if self.ticks >= LOBBY_TICKS => self.go(Phase::Connecting),
            Phase::Connecting if self.ticks >= CONNECTING_TICKS => self.go(Phase::Connected),
            Phase::Incoming if self.answered_elsewhere && self.ticks >= ELSEWHERE_TICKS => {
                step.log = self.end(EndReason::AnsweredElsewhere);
            }
            Phase::Incoming if self.ticks >= INCOMING_TICKS => {
                step.log = self.end(EndReason::Missed);
            }
            Phase::Connected => {
                self.connected += 1;
                // demo: someone in a group raises a hand after a while.
                if self.group && self.connected == 10 * PER_SEC {
                    if let Some(p) = self.participants.get_mut(1) {
                        p.hand = true;
                    }
                }
            }
            Phase::Ended(_) if self.ticks >= ENDED_TICKS => step.clear = true,
            _ => {}
        }
        step
    }
}

impl ActiveCall {
    /// A real call's tick: counting only; a ring that runs out ends the call.
    fn tick_real(&mut self) -> Step {
        let mut step = Step::default();
        match self.phase {
            Phase::Connected => self.connected += 1,
            Phase::Incoming | Phase::Calling | Phase::Ringing => {
                self.ring_left = self.ring_left.saturating_sub(1);
                if self.ring_left == 0 {
                    let reason = if self.phase == Phase::Incoming {
                        EndReason::Missed
                    } else {
                        EndReason::NoAnswer
                    };
                    step.log = self.end(reason);
                    step.ended = true;
                }
            }
            // Joining the room should take seconds; a minute means it failed.
            Phase::Connecting if self.ticks >= CONNECT_LIMIT_TICKS => {
                step.log = self.end(EndReason::Failed);
                step.ended = true;
            }
            Phase::Ended(_) if self.ticks >= ENDED_TICKS => step.clear = true,
            _ => {}
        }
        step
    }
}

/// A real call gives up connecting after this long.
pub const CONNECT_LIMIT_TICKS: u32 = 60 * PER_SEC;

/// What one tick asks the UI to do.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub log: Option<(CallDirection, u32)>,
    pub clear: bool,
    /// A real call ended on this tick (its room and signalling are closed).
    pub ended: bool,
}

/// The global the call layer follows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CallState {
    pub call: Option<ActiveCall>,
}

/// Whether the user allowed the microphone (desktop; asked on first use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MicAccess {
    #[default]
    NotAsked,
    Allowed,
    Denied,
}

// ------------------------------------------------------------------ labels

/// The in-call timer: "0:42", "1:02:03" (one format with the ended
/// state's [`short`], so the number never changes shape at hang-up).
pub fn clock(secs: u32) -> String {
    short(secs)
}

/// A short duration: "2:31", "1:02:03".
pub fn short(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A duration in words, for history: "45 sec", "12 min", "1 hr 3 min".
pub fn words(secs: u32) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    match (h, m) {
        (0, 0) => format!("{secs} sec"),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// The status line under the name.
pub fn status(call: &ActiveCall) -> String {
    match call.phase {
        Phase::Permission => "Microphone needed".into(),
        Phase::Calling => "Calling…".into(),
        Phase::Ringing => "Ringing…".into(),
        Phase::Incoming if call.video => "Songbird video call".into(),
        Phase::Incoming => "Songbird audio call".into(),
        Phase::Lobby => "Waiting for an admin to let you in".into(),
        Phase::Connecting => "Connecting…".into(),
        Phase::Connected => clock(call.secs()),
        Phase::Ended(reason) => ended_line(reason, call.secs()),
    }
}

/// The ended state's line.
pub fn ended_line(reason: EndReason, secs: u32) -> String {
    match reason {
        EndReason::Hangup if secs > 0 => format!("Call ended · {}", short(secs)),
        EndReason::Hangup => "Call ended".into(),
        EndReason::Declined => "Call declined".into(),
        EndReason::Missed => "Missed call".into(),
        EndReason::AnsweredElsewhere => "Answered on your phone".into(),
        EndReason::LeftLobby => "You left the call".into(),
        EndReason::NoMicrophone => "Calls need your microphone".into(),
        EndReason::TheyDeclined => "Call declined".into(),
        EndReason::Busy => "On another call".into(),
        EndReason::NoAnswer => "No answer".into(),
        EndReason::Failed if secs > 0 => format!("Call lost · {}", short(secs)),
        EndReason::Failed => "Call couldn't connect".into(),
        EndReason::NotAdmitted => "You weren't let in".into(),
    }
}

// ------------------------------------------------------------------ verification

/// The 64 symbols of the verification code: easy to tell apart, easy to
/// say aloud, with names for screen readers.
pub const SYMBOLS: [(&str, &str); 64] = [
    ("🐶", "Dog"),
    ("🐱", "Cat"),
    ("🦁", "Lion"),
    ("🐴", "Horse"),
    ("🦄", "Unicorn"),
    ("🐷", "Pig"),
    ("🐘", "Elephant"),
    ("🐰", "Rabbit"),
    ("🐼", "Panda"),
    ("🐓", "Rooster"),
    ("🐧", "Penguin"),
    ("🐢", "Turtle"),
    ("🐟", "Fish"),
    ("🐙", "Octopus"),
    ("🦋", "Butterfly"),
    ("🌷", "Tulip"),
    ("🌳", "Tree"),
    ("🌵", "Cactus"),
    ("🍄", "Mushroom"),
    ("🌏", "Globe"),
    ("🌙", "Moon"),
    ("☁️", "Cloud"),
    ("🔥", "Fire"),
    ("🍌", "Banana"),
    ("🍎", "Apple"),
    ("🍓", "Strawberry"),
    ("🌽", "Corn"),
    ("🍕", "Pizza"),
    ("🎂", "Cake"),
    ("❤️", "Heart"),
    ("😀", "Smiley"),
    ("🤖", "Robot"),
    ("🎩", "Hat"),
    ("👓", "Glasses"),
    ("🔧", "Spanner"),
    ("🎅", "Santa"),
    ("👍", "Thumbs up"),
    ("☂️", "Umbrella"),
    ("⌛", "Hourglass"),
    ("⏰", "Clock"),
    ("🎁", "Gift"),
    ("💡", "Light bulb"),
    ("📕", "Book"),
    ("✏️", "Pencil"),
    ("📎", "Paperclip"),
    ("✂️", "Scissors"),
    ("🔒", "Lock"),
    ("🔑", "Key"),
    ("🔨", "Hammer"),
    ("☎️", "Telephone"),
    ("🏁", "Flag"),
    ("🚂", "Train"),
    ("🚲", "Bicycle"),
    ("✈️", "Aeroplane"),
    ("🚀", "Rocket"),
    ("🏆", "Trophy"),
    ("⚽", "Football"),
    ("🎸", "Guitar"),
    ("🎺", "Trumpet"),
    ("🔔", "Bell"),
    ("⚓", "Anchor"),
    ("🎧", "Headphones"),
    ("📁", "Folder"),
    ("📌", "Pin"),
];

/// FNV-1a, 64 bit.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The four symbols of a code from the call's key (`songbird_content::call`'s
/// `verification_code`): both devices show the same four only when they hold the same key.
pub fn symbols(code: [u8; 4]) -> [(&'static str, &'static str); 4] {
    code.map(|i| SYMBOLS[usize::from(i & 63)])
}

/// The demo's code (no key behind a demo call): 24 bits of a hash of its id, 6 per symbol.
pub fn verification_code(call_id: u64, chat: Option<ChatId>) -> [(&'static str, &'static str); 4] {
    let key = format!("songbird-call:{call_id}:{}", chat.unwrap_or(0));
    let h = fnv(key.as_bytes());
    std::array::from_fn(|i| SYMBOLS[((h >> (i * 6)) & 63) as usize])
}

// ------------------------------------------------------------------ devices

/// The demo's device lists (real calls list the platform's: `backend::calls::devices`).
pub const MICS: [&str; 3] = [
    "Built-in microphone",
    "Headset microphone",
    "USB microphone",
];
pub const CAMERAS: [&str; 2] = ["Built-in camera", "USB camera"];
pub const OUTPUTS: [&str; 2] = ["Built-in speakers", "Headphones"];

#[cfg(test)]
mod tests {
    use super::*;

    fn outgoing() -> ActiveCall {
        ActiveCall::new(1, "Ana".into(), 1, Phase::Calling)
    }

    fn run(call: &mut ActiveCall, ticks: u32) -> Vec<Step> {
        (0..ticks).map(|_| call.tick()).collect()
    }

    #[test]
    fn an_outgoing_call_rings_connects_and_counts() {
        let mut c = outgoing();
        run(&mut c, CALLING_TICKS);
        assert_eq!(c.phase, Phase::Ringing);
        run(&mut c, RINGING_TICKS);
        assert_eq!(c.phase, Phase::Connecting);
        run(&mut c, CONNECTING_TICKS);
        assert_eq!(c.phase, Phase::Connected);
        run(&mut c, 42 * PER_SEC);
        assert_eq!(status(&c), "0:42");
    }

    #[test]
    fn hanging_up_logs_once_and_clears_after_the_ended_state() {
        let mut c = outgoing();
        run(
            &mut c,
            CALLING_TICKS + RINGING_TICKS + CONNECTING_TICKS + 151 * PER_SEC,
        );
        assert_eq!(
            c.end(EndReason::Hangup),
            Some((CallDirection::Outgoing, 151))
        );
        assert_eq!(status(&c), "Call ended · 2:31");
        assert_eq!(c.end(EndReason::Hangup), None, "ending twice logs once");
        let steps = run(&mut c, ENDED_TICKS);
        assert!(steps.last().unwrap().clear);
    }

    #[test]
    fn an_unanswered_incoming_call_is_missed() {
        let mut c = ActiveCall::new(2, "Mihai".into(), 2, Phase::Incoming);
        let logs: Vec<_> = run(&mut c, INCOMING_TICKS)
            .into_iter()
            .filter_map(|s| s.log)
            .collect();
        assert_eq!(logs, vec![(CallDirection::Missed, 0)]);
        assert_eq!(c.phase, Phase::Ended(EndReason::Missed));
    }

    #[test]
    fn accepting_connects_and_logs_incoming() {
        let mut c = ActiveCall::new(2, "Mihai".into(), 2, Phase::Incoming);
        c.accept(true);
        assert!(c.video);
        run(&mut c, CONNECTING_TICKS + 10 * PER_SEC);
        assert_eq!(
            c.end(EndReason::Hangup),
            Some((CallDirection::Incoming, 10))
        );
    }

    #[test]
    fn answered_elsewhere_logs_nothing_here() {
        let mut c = ActiveCall::new(2, "Mihai".into(), 2, Phase::Incoming);
        c.answered_elsewhere = true;
        let logs: Vec<_> = run(&mut c, ELSEWHERE_TICKS)
            .into_iter()
            .filter_map(|s| s.log)
            .collect();
        assert!(logs.is_empty());
        assert_eq!(status(&c), "Answered on your phone");
    }

    #[test]
    fn the_permission_explainer_gates_the_call() {
        let mut c = ActiveCall::new(3, "Ana".into(), 1, Phase::Permission);
        c.after_permission = Phase::Calling;
        run(&mut c, 40);
        assert_eq!(c.phase, Phase::Permission, "it waits for the user");
        c.permission(true);
        assert_eq!(c.phase, Phase::Calling);
        let mut d = ActiveCall::new(4, "Ana".into(), 1, Phase::Permission);
        assert_eq!(d.permission(false), None);
        assert_eq!(d.phase, Phase::Ended(EndReason::NoMicrophone));
    }

    #[test]
    fn a_lobby_lets_the_user_in_and_admins_admit() {
        let mut c = ActiveCall::new(5, "Crit".into(), 3, Phase::Lobby);
        run(&mut c, LOBBY_TICKS + CONNECTING_TICKS);
        assert_eq!(c.phase, Phase::Connected);
        c.waiting = vec![
            Waiting {
                id: 1,
                name: "A".into(),
                seed: 1,
            },
            Waiting {
                id: 2,
                name: "B".into(),
                seed: 2,
            },
        ];
        c.admit(1, true);
        c.admit(2, false);
        assert!(c.waiting.is_empty());
        assert_eq!(c.participants.len(), 1);
    }

    #[test]
    fn group_tiles_arrive_in_turn() {
        let mut c = outgoing();
        c.group = true;
        c.participants = (0..3)
            .map(|i| Participant {
                id: i,
                name: format!("P{i}"),
                seed: i,
                muted: false,
                hand: false,
                video: false,
                joins_at: i as u32 * 4,
                identity: String::new(),
                decrypting: true,
                speaking: false,
            })
            .collect();
        c.phase = Phase::Connected;
        assert_eq!(c.joined().len(), 1);
        run(&mut c, 8);
        assert_eq!(c.joined().len(), 3);
    }

    #[test]
    fn the_code_is_stable_and_differs_between_calls() {
        assert_eq!(verification_code(7, Some(1)), verification_code(7, Some(1)));
        assert_ne!(verification_code(7, Some(1)), verification_code(8, Some(1)));
        let symbols: std::collections::HashSet<_> = SYMBOLS.iter().map(|s| s.0).collect();
        assert_eq!(symbols.len(), 64, "every symbol is distinct");
    }

    #[test]
    fn labels() {
        assert_eq!(clock(3723), "1:02:03");
        assert_eq!(short(151), "2:31");
        assert_eq!(words(45), "45 sec");
        assert_eq!(words(754), "12 min");
        assert_eq!(words(3_780), "1 hr 3 min");
    }
}
