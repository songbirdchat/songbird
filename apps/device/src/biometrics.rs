//! Biometrics (D-0172): the system's own prompt, which proves the device's owner is present.
//! The app learns only yes or no; the biometric data never leaves the system.

#![forbid(unsafe_code)]

/// Which biometric the device offers, for the words on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Face ID (iPhone, iPad).
    FaceId,
    /// Touch ID (iPhone, iPad, Mac).
    TouchId,
    /// Optic ID (Vision Pro).
    OpticId,
    /// A fingerprint, face or iris on Android (the system's strong class).
    Android,
    /// Windows Hello (face, fingerprint or the device's PIN).
    WindowsHello,
}

impl Kind {
    /// Its name, as the system calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FaceId => "Face ID",
            Self::TouchId => "Touch ID",
            Self::OpticId => "Optic ID",
            Self::Android => "fingerprint or face",
            Self::WindowsHello => "Windows Hello",
        }
    }
}

/// How a prompt ended without a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The person cancelled, or chose the passcode instead.
    Cancelled,
    /// Not recognised (too many tries: the system locks biometrics for a while).
    NotRecognised,
    /// Biometrics are off, not set up, or locked out right now.
    Unavailable,
}

/// The answer to a prompt, on a thread of the system's choosing.
pub type Done = Box<dyn FnOnce(Result<(), Failure>) + Send>;

/// The biometric the device offers and the app may use, if any.
#[must_use]
pub fn available() -> Option<Kind> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::biometrics();
    #[cfg(target_os = "android")]
    return crate::android::biometrics();
    #[cfg(target_os = "windows")]
    return crate::windows::biometrics();
    #[allow(unreachable_code, reason = "other platforms")]
    None
}

/// Shows the system's prompt with `reason` (why the app asks).
pub fn authenticate(reason: &str, done: Done) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::authenticate(reason, done);
    #[cfg(target_os = "android")]
    return crate::android::authenticate(reason, done);
    #[cfg(target_os = "windows")]
    return crate::windows::authenticate(reason, done);
    #[allow(unreachable_code, reason = "other platforms")]
    {
        let _ = reason;
        done(Err(Failure::Unavailable));
    }
}
