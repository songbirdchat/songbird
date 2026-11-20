//! Songbird's camera (D-0110): one interface for scanning codes, taking photos and videos, and
//! the frames of video calls, over each platform's own camera framework.
//!
//! - [`Backend`] is what a platform implements: access, lenses, and opening a [`Capture`].
//! - A capture reports through a [`Sink`]: state changes and decoded codes as [`Event`]s, and
//!   frames for calls into a newest-wins [`mailbox`] that never blocks the camera.
//! - [`fake::FakeCamera`] is a scripted backend for headless tests.
//! - [`qr`] decodes QR codes on the device from grey frames (`rqrr`); [`jpeg`] strips a still's
//!   metadata before it is sent.
//!
//! The camera knows nothing of Songbird's formats: what a scanned code means is decided by the
//! app's strict parsers.
//!
//! `unsafe` is allowed in the platform modules only (D-0110 §5); everything here is
//! `#![forbid(unsafe_code)]` per module.

#[cfg(target_os = "android")]
pub mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple;
#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub mod desktop;
pub mod fake;
pub mod frame;
pub mod jpeg;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod mailbox;
pub mod qr;
pub mod video;
#[cfg(target_os = "windows")]
pub mod windows;

use std::sync::{Arc, Mutex};

pub use frame::{Frame, Pixels, Rotation};

/// What a session is for. Platforms configure their outputs from it (a metadata output for
/// codes, a photo output, a movie output, a data output for calls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Reading a QR code: a link code or a contact's safety-number code.
    ScanCode,
    /// A still to send in a chat.
    Photo,
    /// A video clip to send in a chat.
    Video,
    /// The camera feed of a video call.
    Call,
}

/// Whether the app may use the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    /// The person has not been asked yet: explain, then ask at the moment of use.
    NotAsked,
    /// Allowed.
    Granted,
    /// Refused by the person; only the system's settings can change it.
    Denied,
    /// Turned off by device management or parental controls; the person cannot change it.
    Restricted,
}

/// Why a camera cannot run although access may be granted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Unavailable {
    /// No camera is attached (or the iOS Simulator).
    NoCamera,
    /// Another app holds the camera.
    InUse,
    /// The system took the camera away for now: a phone call, Split View, a covered window.
    Interrupted,
    /// The system has the camera turned off (Windows privacy switch, a portal with no camera).
    SystemOff,
    /// The platform failed; the text is for logs and reports, never shown raw.
    Failed(String),
}

/// Which way a lens looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facing {
    /// Towards the person (selfies, calls).
    Front,
    /// Away from the person (scanning, photos).
    Back,
    /// A separate camera: a webcam, Continuity Camera.
    External,
}

/// One camera the device has.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Lens {
    /// The platform's stable identifier, opaque.
    pub id: String,
    /// What the system calls it ("`FaceTime` HD Camera"); shown on desktops.
    pub name: String,
    /// Which way it looks.
    pub facing: Facing,
}

impl Lens {
    /// The lens a purpose starts with: the back camera for codes, photos and videos, the front
    /// one for calls; else the first there is.
    #[must_use]
    pub fn preferred(lenses: &[Lens], purpose: Purpose) -> Option<&Lens> {
        let want = match purpose {
            Purpose::Call => Facing::Front,
            Purpose::ScanCode | Purpose::Photo | Purpose::Video => Facing::Back,
        };
        lenses
            .iter()
            .find(|l| l.facing == want)
            .or_else(|| lenses.first())
    }

    /// Lenses from lines of `id \t facing \t name` (facing 0 front, 1 back, else external),
    /// as Android's host lists them; at most 16, ids of at most 64 bytes.
    #[must_use]
    pub fn parse_lines(text: &str) -> Vec<Lens> {
        text.lines()
            .take(16)
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let id = parts.next()?.trim();
                let facing = match parts.next()? {
                    "0" => Facing::Front,
                    "1" => Facing::Back,
                    _ => Facing::External,
                };
                let name = parts.next().unwrap_or("Camera").to_owned();
                (!id.is_empty() && id.len() <= 64).then(|| Lens {
                    id: id.to_owned(),
                    name,
                    facing,
                })
            })
            .collect()
    }
}

/// What a running capture reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The camera is running with this lens.
    Running {
        /// The lens's [`Lens::id`].
        lens: String,
    },
    /// The camera stopped or never started.
    Unavailable(Unavailable),
    /// A QR code was read; each distinct text once per capture.
    Code(String),
}

/// A photo, in memory: never a file in a shared folder.
#[derive(Clone, PartialEq, Eq)]
pub struct Still {
    /// The encoded image, metadata already stripped ([`jpeg::strip`]).
    pub bytes: Vec<u8>,
    /// Its media type (`image/jpeg`).
    pub mime: &'static str,
    /// Its width in pixels, after orientation.
    pub width: u32,
    /// Its height in pixels, after orientation.
    pub height: u32,
}

impl std::fmt::Debug for Still {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the bytes: they are the person's photo.
        f.debug_struct("Still")
            .field("len", &self.bytes.len())
            .field("mime", &self.mime)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Why a photo or video could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// The session is not for this ([`Purpose`]).
    WrongPurpose,
    /// The camera is not running.
    NotRunning,
    /// The platform failed; for logs, never shown raw.
    Failed(String),
}

/// What a session asks of the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// What it is for.
    pub purpose: Purpose,
    /// The lens to start with ([`Lens::id`]).
    pub lens: String,
}

/// Called once with the person's answer.
pub type AccessCallback = Box<dyn FnOnce(Access) + Send>;
/// Called once with a photo, or why there is none.
pub type StillCallback = Box<dyn FnOnce(Result<Still, CaptureError>) + Send>;

/// A platform's camera.
pub trait Backend: Send + Sync + 'static {
    /// Whether the app may use the camera now. Cheap; no prompt.
    fn access(&self) -> Access;

    /// Shows the system's prompt (where there is one) and calls `done` with the answer, on any
    /// thread. Only ever called on a person's press.
    fn request_access(&self, done: AccessCallback);

    /// The cameras the device has now.
    fn lenses(&self) -> Vec<Lens>;

    /// Starts a camera. It runs until the returned capture is dropped, reporting into `sink`.
    ///
    /// # Errors
    ///
    /// Why it cannot start; the same can come later as [`Event::Unavailable`].
    fn open(&self, config: Config, sink: Sink) -> Result<Box<dyn Capture>, Unavailable>;

    /// Where the system's camera settings open (for "Open Settings" after a refusal).
    fn settings_url(&self) -> Option<&'static str>;
}

/// A running camera; dropping it stops the camera.
pub trait Capture: Send {
    /// Switches to another lens ([`Lens::id`]).
    fn set_lens(&mut self, lens: &str);

    /// Turns the torch on or off; `false` where the lens has none.
    fn set_torch(&mut self, on: bool) -> bool;

    /// Takes a photo ([`Purpose::Photo`] sessions) and calls `done` on any thread.
    fn capture_photo(&mut self, done: StillCallback);

    /// What the native preview component is told to show (its `session` prop).
    fn preview_id(&self) -> u64;
}

/// Where a capture reports: events and frames, from any thread. Cheap to clone.
#[derive(Clone)]
pub struct Sink {
    events: mailbox::Sender<Event>,
    frames: mailbox::Sender<Frame>,
    seen: Arc<Mutex<SeenCodes>>,
}

/// The most events waiting for the app; a stalled app loses the oldest, never blocks the camera.
pub const EVENT_CAPACITY: usize = 32;
/// The most frames waiting (for a call's encoder); newest wins.
pub const FRAME_CAPACITY: usize = 2;
/// The longest code text passed on, in bytes; longer ones are ignored.
pub const MAX_CODE_LEN: usize = 1024;

impl Sink {
    /// A sink and the two receivers the app reads.
    #[must_use]
    pub fn new() -> (Sink, mailbox::Receiver<Event>, mailbox::Receiver<Frame>) {
        let (events, events_rx) = mailbox::channel(EVENT_CAPACITY);
        let (frames, frames_rx) = mailbox::channel(FRAME_CAPACITY);
        let sink = Sink {
            events,
            frames,
            seen: Arc::new(Mutex::new(SeenCodes::default())),
        };
        (sink, events_rx, frames_rx)
    }

    /// The camera runs with `lens`.
    pub fn running(&self, lens: &str) {
        self.events.send(Event::Running {
            lens: lens.to_owned(),
        });
    }

    /// The camera stopped or cannot run.
    pub fn unavailable(&self, why: Unavailable) {
        self.events.send(Event::Unavailable(why));
    }

    /// A code was read. Passed on once per distinct text, and only if it is at most
    /// [`MAX_CODE_LEN`] bytes and has no control characters other than line breaks.
    pub fn code(&self, text: &str) {
        if text.is_empty()
            || text.len() > MAX_CODE_LEN
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\r')
        {
            return;
        }
        let fresh = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(text);
        if fresh {
            self.events.send(Event::Code(text.to_owned()));
        }
    }

    /// A frame for the call's encoder; the oldest waiting one is dropped when two wait.
    pub fn frame(&self, frame: Frame) {
        self.frames.send(frame);
    }

    /// Whether the app still listens (it dropped the event receiver when not).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.events.is_open()
    }
}

/// The codes a capture already passed on: a few, so a code held in front of the camera is
/// reported once, and a stream of distinct codes cannot grow memory.
#[derive(Default)]
struct SeenCodes(std::collections::VecDeque<String>);

impl SeenCodes {
    const KEEP: usize = 16;

    fn insert(&mut self, text: &str) -> bool {
        if self.0.iter().any(|t| t == text) {
            return false;
        }
        if self.0.len() == Self::KEEP {
            self.0.pop_front();
        }
        self.0.push_back(text.to_owned());
        true
    }
}

/// The platform's camera, or `None` where Songbird has none yet (the web, a TUI, tests).
#[must_use]
pub fn platform() -> Option<Arc<dyn Backend>> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Some(Arc::new(apple::AppleCamera));
    #[cfg(target_os = "android")]
    return Some(Arc::new(android::AndroidCamera));
    #[cfg(target_os = "windows")]
    return Some(Arc::new(windows::WindowsCamera));
    #[cfg(target_os = "linux")]
    return Some(Arc::new(linux::LinuxCamera));
    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_code_is_reported_once_and_odd_ones_never() {
        let (sink, events, _) = Sink::new();
        sink.code("SONGBIRD:LINK:AB:CD");
        sink.code("SONGBIRD:LINK:AB:CD");
        sink.code("");
        sink.code(&"x".repeat(MAX_CODE_LEN + 1));
        sink.code("bell\u{7}");
        sink.code("two\nlines");
        assert_eq!(
            events.drain(),
            vec![
                Event::Code("SONGBIRD:LINK:AB:CD".into()),
                Event::Code("two\nlines".into())
            ]
        );
    }

    #[test]
    fn the_codes_kept_are_bounded() {
        let (sink, events, _) = Sink::new();
        for i in 0..100 {
            sink.code(&format!("code {i}"));
        }
        // Forgotten after 16 others: reported again, as a code shown anew would be.
        sink.code("code 0");
        let got = events.drain();
        assert_eq!(got.last(), Some(&Event::Code("code 0".into())));
        assert!(sink.seen.lock().unwrap().0.len() <= SeenCodes::KEEP);
    }

    #[test]
    fn the_preferred_lens_follows_the_purpose() {
        let lenses = vec![
            Lens {
                id: "f".into(),
                name: "Front".into(),
                facing: Facing::Front,
            },
            Lens {
                id: "b".into(),
                name: "Back".into(),
                facing: Facing::Back,
            },
        ];
        assert_eq!(Lens::preferred(&lenses, Purpose::ScanCode).unwrap().id, "b");
        assert_eq!(Lens::preferred(&lenses, Purpose::Call).unwrap().id, "f");
        let webcam = vec![Lens {
            id: "w".into(),
            name: "USB".into(),
            facing: Facing::External,
        }];
        assert_eq!(Lens::preferred(&webcam, Purpose::Call).unwrap().id, "w");
        assert!(Lens::preferred(&[], Purpose::Photo).is_none());
    }

    #[test]
    fn lens_lines_parse_and_bound() {
        let l = Lens::parse_lines("0\t1\tBack camera\n1\t0\tFront camera\n\t1\tno id\n5\t9\n");
        assert_eq!(l.len(), 3);
        assert_eq!(
            (l[0].facing, l[1].facing, l[2].facing),
            (Facing::Back, Facing::Front, Facing::External)
        );
        assert_eq!(l[2].name, "Camera");
        let many = (0..40).fold(String::new(), |mut s, i| {
            use std::fmt::Write;
            let _ = writeln!(s, "{i}\t1\tx");
            s
        });
        assert_eq!(Lens::parse_lines(&many).len(), 16);
        assert_eq!(
            Lens::parse_lines(&format!("{}\t1\tx", "9".repeat(65))),
            Vec::new()
        );
    }

    #[test]
    fn a_still_never_prints_its_bytes() {
        let s = Still {
            bytes: vec![0xff, 0xd8, 0x42],
            mime: "image/jpeg",
            width: 1,
            height: 1,
        };
        let printed = format!("{s:?}");
        assert!(
            printed.contains("len: 3") && !printed.contains("255"),
            "{printed}"
        );
    }
}
