//! The device's position, once, when the person asks to share where they are (D-0174): the
//! system asks permission (while using the app only) at that moment. Nothing tracks, nothing
//! runs in the background, and the position goes only where the person sends it.

#![forbid(unsafe_code)]

/// A position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Degrees north.
    pub latitude: f64,
    /// Degrees east.
    pub longitude: f64,
    /// How far off it may be, in metres.
    pub accuracy_m: f64,
}

/// Why there is no position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The person (or device management) said no; only the system's settings change it.
    Denied,
    /// Location services are off, or no fix could be had.
    Unavailable,
}

/// The answer, on a thread of the system's choosing.
pub type Done = Box<dyn FnOnce(Result<Position, Failure>) + Send>;

/// One position (asking permission first if it was never asked). Call it on the UI thread.
pub fn current(done: Done) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::location(done);
    #[cfg(target_os = "android")]
    return crate::android::location(done);
    #[cfg(target_os = "windows")]
    return crate::windows::location(done);
    #[cfg(target_os = "linux")]
    return crate::linux::location(done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(Err(Failure::Unavailable));
}
