//! Biometric unlock (D-0172): the system's prompt through `songbird_device::biometrics`. The
//! app learns yes or no; the passcode stays the way in when biometrics fail or change.

use songbird_device::biometrics::{self, Failure, Kind};

/// The biometric this device offers, if any.
#[must_use]
pub fn kind() -> Option<Kind> {
    if cfg!(test) {
        return None;
    }
    biometrics::available()
}

/// Shows the system's prompt; answers on the UI thread.
///
/// # Errors
/// How the prompt ended without a match.
pub async fn verify(reason: &str) -> Result<(), Failure> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    biometrics::authenticate(
        reason,
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    rx.await.unwrap_or(Err(Failure::Unavailable))
}
