//! The app's side of the camera (D-0110): the provider, sessions that run only while they are
//! seen, and the views of every state (not asked, asking, denied, restricted, unavailable,
//! running).
//!
//! The platform camera is `songbird_camera::platform()`. Tests install a
//! [`songbird_camera::fake::FakeCamera`]. Where a platform has no camera module yet, a preview
//! build installs a demo fake that "sees" a code after a moment (`// mock:`), and a release
//! build says there is no camera.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use nsh_core::{platform::Lifecycle, prelude::*};
use songbird_camera::{
    Access, Backend, Capture, CaptureError, Config, Event, Frame, Lens, Purpose, Sink, Still,
    Unavailable, mailbox,
};

use crate::ui::*;

/// The camera the root installed.
#[derive(Clone)]
pub struct Camera {
    backend: Option<Arc<dyn Backend>>,
    /// Whether the platform draws a live preview (`songbird/camera`); a demo or test camera
    /// does not.
    native_preview: bool,
    /// The access the app last read; written back after a prompt or a return to the app.
    access: Signal<Access>,
}

thread_local! {
    static ROOT: RefCell<Option<Camera>> = const { RefCell::new(None) };
}

/// Makes `backend` the app's camera (the root, once; tests per app).
pub fn install(backend: Option<Arc<dyn Backend>>, native_preview: bool) {
    let access = signal(backend.as_ref().map_or(Access::Restricted, |b| b.access()));
    let cam = Camera {
        backend,
        native_preview,
        access,
    };
    ROOT.with(|r| *r.borrow_mut() = Some(cam.clone()));
    provide_context(cam);
}

/// The camera for the running app: the platform's, else (preview builds only) the demo.
pub fn install_for_app() {
    match songbird_camera::platform() {
        Some(b) => install(Some(b), true),
        None if crate::PREVIEW_BUILD => install(Some(Arc::new(demo())), false),
        None => install(None, false),
    }
}

/// mock: where a platform has no camera module yet, preview builds show a scanner that reads a
/// code after a moment, as the mocked flows did before the camera existed.
fn demo() -> songbird_camera::fake::FakeCamera {
    let fake = songbird_camera::fake::FakeCamera::new();
    fake.photo_gives(Ok(Still {
        bytes: Vec::new(),
        mime: "image/jpeg",
        width: 3,
        height: 4,
    }));
    DEMO.with(|d| *d.borrow_mut() = Some(fake.clone()));
    fake
}

thread_local! {
    static DEMO: RefCell<Option<songbird_camera::fake::FakeCamera>> = const { RefCell::new(None) };
}

/// mock: in a preview build without a platform camera, the demo camera "sees" `text` after a
/// moment (the code the scanning screen's mock expects). Does nothing elsewhere.
pub fn demo_shows(text: String) {
    if let Some(fake) = DEMO.with(|d| d.borrow().clone()) {
        set_timeout(Duration::from_millis(2200), move || fake.show_code(&text));
    }
}

fn current() -> Option<Camera> {
    use_context::<Camera>().or_else(|| ROOT.with(|r| r.borrow().clone()))
}

/// What a camera view shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// This device has no camera Songbird can use.
    NoCamera,
    /// The person has not been asked: explain first.
    NotAsked,
    /// The system prompt is up.
    Asking,
    /// The person said no; only Settings can change it.
    Denied,
    /// Device management or parental controls turned the camera off.
    Restricted,
    /// Starting.
    Starting,
    /// Running with this lens.
    Running(String),
    /// It cannot run now.
    Unavailable(Unavailable),
}

/// A camera session of one screen. It runs while `active` holds, the app is not in the
/// background and access is granted; it stops when any of those ends and when the screen goes.
#[derive(Clone)]
pub struct Session {
    /// What the view shows.
    pub shown: Signal<Shown>,
    /// The preview the native component shows (its `session` prop), while running.
    pub preview: Signal<Option<u64>>,
    purpose: Purpose,
    capture: Rc<RefCell<Option<Box<dyn Capture>>>>,
    frames: Rc<RefCell<Option<mailbox::Receiver<Frame>>>>,
    retries: Signal<u32>,
    /// The lens the person flipped to, kept across restarts of the session.
    chosen: Rc<RefCell<Option<String>>>,
    /// Where the system prompt's answer arrives, from any thread.
    answers: mailbox::Sender<Access>,
    /// Where a photo arrives, from any thread, and who waits for it.
    photos: mailbox::Sender<Result<Still, CaptureError>>,
    photo_done: Rc<RefCell<Option<PhotoDone>>>,
}

type PhotoDone = Box<dyn FnOnce(Result<Still, CaptureError>)>;

/// A session for `purpose` on the current screen. `on_code` gets each distinct code text a
/// [`Purpose::ScanCode`] session reads.
pub fn use_session(
    purpose: Purpose,
    active: impl Fn() -> bool + 'static,
    on_code: impl Fn(String) + 'static,
) -> Session {
    let cam = current().expect("the root installs the camera");
    let (answers, answered) = mailbox::channel::<Access>(1);
    let retries = signal(0u32);
    {
        // The prompt's answer, back on the engine thread; ends with the screen. The view is
        // worked out again even when the answer is what access already was (a dismissed
        // prompt), so it never stays on "Asking".
        let access = cam.access;
        spawn(async move {
            while let Some(answer) = answered.recv().await {
                let _ = access.try_set(answer);
                if let Some(n) = retries.try_get() {
                    let _ = retries.try_set(n.wrapping_add(1));
                }
            }
        });
    }
    let (photos, taken) = mailbox::channel::<Result<Still, CaptureError>>(1);
    let photo_done: Rc<RefCell<Option<PhotoDone>>> = Rc::new(RefCell::new(None));
    {
        // A photo, back on the engine thread, to whoever asked for it.
        let photo_done = photo_done.clone();
        spawn(async move {
            while let Some(result) = taken.recv().await {
                if let Some(done) = photo_done.borrow_mut().take() {
                    done(result);
                }
            }
        });
    }
    let session = Session {
        shown: signal(Shown::Starting),
        preview: signal(None),
        purpose,
        capture: Rc::new(RefCell::new(None)),
        frames: Rc::new(RefCell::new(None)),
        retries,
        chosen: Rc::new(RefCell::new(None)),
        answers,
        photos,
        photo_done,
    };
    let on_code: Rc<dyn Fn(String)> = Rc::new(on_code);

    // Back in the app: the person may have changed access in Settings.
    {
        let cam = cam.clone();
        effect(move || {
            if lifecycle() == Lifecycle::Active
                && let Some(b) = &cam.backend
            {
                let now = b.access();
                untracked(|| cam.access.set(now));
            }
        });
    }

    {
        let s = session.clone();
        let cam = cam.clone();
        effect(move || {
            let shown_here = active();
            // A call keeps its camera on a desktop whose window is minimised.
            let backgrounded =
                lifecycle() == Lifecycle::Background && !(purpose == Purpose::Call && is_desktop());
            let access = cam.access.get();
            let _ = s.retries.get();
            untracked(|| {
                s.stop();
                let Some(backend) = cam.backend.clone() else {
                    s.shown.set(Shown::NoCamera);
                    return;
                };
                match access {
                    Access::Granted => {}
                    Access::NotAsked => return s.shown.set(Shown::NotAsked),
                    Access::Denied => return s.shown.set(Shown::Denied),
                    Access::Restricted => return s.shown.set(Shown::Restricted),
                }
                if !shown_here || backgrounded {
                    return;
                }
                s.start(&backend, on_code.clone());
            });
        });
    }
    {
        let s = session.clone();
        cleanup(move || s.stop());
    }
    session
}

impl Session {
    fn start(&self, backend: &Arc<dyn Backend>, on_code: Rc<dyn Fn(String)>) {
        let lenses = backend.lenses();
        let lens = self
            .chosen
            .borrow()
            .clone()
            .filter(|id| lenses.iter().any(|l| &l.id == id))
            .or_else(|| Lens::preferred(&lenses, self.purpose).map(|l| l.id.clone()));
        let Some(lens) = lens else {
            self.shown.set(Shown::Unavailable(Unavailable::NoCamera));
            return;
        };
        self.shown.set(Shown::Starting);
        let (sink, events, frames) = Sink::new();
        match backend.open(
            Config {
                purpose: self.purpose,
                lens,
            },
            sink,
        ) {
            Ok(capture) => {
                self.preview.set(Some(capture.preview_id()));
                *self.capture.borrow_mut() = Some(capture);
                if self.purpose == Purpose::Call {
                    *self.frames.borrow_mut() = Some(frames);
                }
            }
            Err(why) => {
                self.shown.set(Shown::Unavailable(why));
                return;
            }
        }
        let shown = self.shown;
        let scanning = self.purpose == Purpose::ScanCode;
        // Ends when the capture is dropped (its senders go with it).
        spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    Event::Running { lens } => {
                        let _ = shown.try_set(Shown::Running(lens));
                    }
                    Event::Unavailable(why) => {
                        let _ = shown.try_set(Shown::Unavailable(why));
                    }
                    Event::Code(text) if scanning => on_code(text),
                    Event::Code(_) => {}
                }
            }
        });
    }

    /// Stops the camera (the view stays; it starts again when its conditions hold again).
    fn stop(&self) {
        let taken = self.capture.borrow_mut().take();
        if taken.is_some() {
            drop(taken);
            let _ = self.preview.try_set(None);
        }
        self.frames.borrow_mut().take();
    }

    /// Shows the system's prompt (on a press only).
    pub fn ask(&self) {
        let Some(backend) = current().and_then(|c| c.backend) else {
            return;
        };
        self.shown.set(Shown::Asking);
        let tx = Mutex::new(Some(self.answers.clone()));
        backend.request_access(Box::new(move |answer| {
            if let Some(tx) = tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                tx.send(answer);
            }
        }));
    }

    /// Takes a photo ([`Purpose::Photo`] sessions); `done` runs on the engine thread. A photo
    /// asked for while one is on its way replaces the earlier callback.
    pub fn take_photo(&self, done: impl FnOnce(Result<Still, CaptureError>) + 'static) {
        let mut capture = self.capture.borrow_mut();
        let Some(c) = capture.as_mut() else {
            drop(capture);
            return done(Err(CaptureError::NotRunning));
        };
        *self.photo_done.borrow_mut() = Some(Box::new(done));
        let tx = Mutex::new(Some(self.photos.clone()));
        c.capture_photo(Box::new(move |r| {
            if let Some(tx) = tx
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                tx.send(r);
            }
        }));
    }

    /// Tries again after a failure.
    pub fn retry(&self) {
        self.retries.update(|n| *n += 1);
    }

    /// Switches to the next lens (front and back).
    pub fn flip(&self) {
        let Some(backend) = current().and_then(|c| c.backend) else {
            return;
        };
        let lenses = backend.lenses();
        let now = match self.shown.get_untracked() {
            Shown::Running(l) => l,
            _ => return,
        };
        let i = lenses.iter().position(|l| l.id == now).unwrap_or(0);
        if let Some(next) = lenses.get((i + 1) % lenses.len().max(1)) {
            *self.chosen.borrow_mut() = Some(next.id.clone());
            if let Some(c) = self.capture.borrow_mut().as_mut() {
                c.set_lens(&next.id);
            }
        }
    }

    /// Switches to the lens `id` (a call's devices sheet).
    pub fn select(&self, id: &str) {
        *self.chosen.borrow_mut() = Some(id.to_owned());
        if let Some(c) = self.capture.borrow_mut().as_mut() {
            c.set_lens(id);
        }
    }

    /// The frames of a [`Purpose::Call`] session, newest wins (taken once).
    pub fn take_frames(&self) -> Option<mailbox::Receiver<Frame>> {
        self.frames.borrow_mut().take()
    }

    /// Whether the camera runs now.
    pub fn is_running(&self) -> bool {
        self.capture.borrow().is_some()
    }
}

/// Opens the system's camera settings, where there is a page for them.
fn open_settings() {
    if let Some(url) = current()
        .and_then(|c| c.backend)
        .and_then(|b| b.settings_url())
    {
        // Acts at once; whether it opened does not change what the screen shows.
        drop(open_url(url));
    }
}

/// What the camera is for, in the words its screens use.
fn why(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::ScanCode => "to scan the code",
        Purpose::Photo => "to take a photo",
        Purpose::Video => "to record a video",
        Purpose::Call => "for video in calls",
    }
}

/// The camera view: the live preview while running, else what the state needs (with its
/// action). `size` is its side in points; `caption` shows under a running scanner.
pub fn camera_view(session: &Session, size: f32, caption: impl Into<SharedString>) -> View {
    let s = session.clone();
    let caption: SharedString = caption.into();
    let live = current().is_some_and(|c| c.native_preview);
    view().items_center().w_full().child(dynamic(move || {
        let shown = s.shown.get();
        let s = s.clone();
        let caption = caption.clone();
        untracked(move || match shown {
            Shown::Running(_) | Shown::Starting if live => {
                let preview = s.preview;
                view()
                    .items_center()
                    .gap_3()
                    .child(
                        native("songbird/camera")
                            .prop("session", move || preview.get().map_or(serde_json::Value::Null, serde_json::Value::from))
                            .prop("purpose", serde_json::Value::from(format!("{:?}", s.purpose)))
                            .size(px(size))
                            .rounded(move || radius::lg())
                            .overflow_hidden()
                            .bg(move || c::call_bg())
                            .role(AccessibilityRole::Image)
                            .label("Camera"),
                    )
                    .child(muted(caption).text_center())
                    .into_element()
            }
            Shown::Running(_) | Shown::Starting => scanner(size, caption).into_element(),
            Shown::NotAsked => state(
                Icon::Camera,
                "Allow the camera",
                format!(
                    "Songbird uses the camera {}. Nothing it sees leaves your device unless you send it, and what you send is end\u{2011}to\u{2011}end encrypted.",
                    why(s.purpose)
                ),
                Some(btn("Continue", Variant::Primary, BtnSize::Md, move || s.ask()).into_element()),
            ),
            Shown::Asking => state(Icon::Camera, "Allow the camera", "Waiting for your answer…".to_string(), None),
            Shown::Denied => state(
                Icon::VideoOff,
                "Camera access is off",
                format!("To use the camera {}, allow it for Songbird in Settings.", why(s.purpose)),
                Some(btn("Open Settings", Variant::Primary, BtnSize::Md, open_settings).into_element()),
            ),
            Shown::Restricted => state(
                Icon::VideoOff,
                "The camera is turned off",
                "Your organisation or parental controls turned the camera off on this device.".to_string(),
                None,
            ),
            Shown::NoCamera | Shown::Unavailable(Unavailable::NoCamera) => state(
                Icon::VideoOff,
                "No camera found",
                if is_desktop() {
                    "Connect a camera to this computer to use it here.".to_string()
                } else {
                    "This device has no camera Songbird can use.".to_string()
                },
                None,
            ),
            Shown::Unavailable(Unavailable::InUse) => state(
                Icon::VideoOff,
                "The camera is in use",
                "Another app is using the camera. Close it, then try again.".to_string(),
                Some(btn("Try again", Variant::Secondary, BtnSize::Md, move || s.retry()).into_element()),
            ),
            Shown::Unavailable(Unavailable::Interrupted) => state(
                Icon::VideoOff,
                "The camera is paused",
                "It comes back when the system gives it back.".to_string(),
                None,
            ),
            Shown::Unavailable(Unavailable::SystemOff) => state(
                Icon::VideoOff,
                "The camera is turned off",
                "The camera is turned off in your system's privacy settings.".to_string(),
                Some(btn("Open Settings", Variant::Primary, BtnSize::Md, open_settings).into_element()),
            ),
            Shown::Unavailable(Unavailable::Failed(_)) => state(
                Icon::VideoOff,
                "The camera couldn't start",
                "Something went wrong with the camera.".to_string(),
                Some(btn("Try again", Variant::Secondary, BtnSize::Md, move || s.retry()).into_element()),
            ),
        })
    }))
}

/// The cameras this device has (desktops show their names in a call's devices sheet).
pub fn lens_names() -> Vec<String> {
    current()
        .and_then(|c| c.backend)
        .map(|b| b.lenses().into_iter().map(|l| l.name).collect())
        .unwrap_or_default()
}

/// The camera at `index` of [`lens_names`]: its id, to switch a session to it.
pub fn lens_id(index: usize) -> Option<String> {
    current()
        .and_then(|c| c.backend)
        .and_then(|b| b.lenses().into_iter().nth(index).map(|l| l.id))
}

/// Whether the platform draws a live preview (`songbird/camera`); a demo or test camera does
/// not.
pub fn has_live_preview() -> bool {
    current().is_some_and(|c| c.native_preview)
}

/// The live preview filling its parent (a call's self-view), shown while the session runs.
pub fn preview_fill(session: &Session) -> View {
    let preview = session.preview;
    view().absolute().inset_0().child(
        native("songbird/camera")
            .prop("session", move || {
                preview
                    .get()
                    .map_or(serde_json::Value::Null, serde_json::Value::from)
            })
            .prop(
                "purpose",
                serde_json::Value::from(format!("{:?}", session.purpose)),
            )
            .flex_1()
            .role(AccessibilityRole::Image)
            .label("Your camera"),
    )
}

fn state(
    glyph: Icon,
    title: &'static str,
    message: String,
    action: Option<AnyElement>,
) -> AnyElement {
    empty_state(glyph, title, message, action).into_element()
}

#[cfg(test)]
mod tests;
