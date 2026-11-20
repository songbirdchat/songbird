//! Songbird on the device's own services (D-0172): one interface per service over each
//! platform's framework, so the app holds no platform code.
//!
//! - [`secure`]: the vault key, kept by the system's secure store (the Keychain, the Android
//!   Keystore, DPAPI, the Secret Service). The app encrypts its device secrets under it.
//! - [`biometrics`]: Face ID, Touch ID, fingerprints, Windows Hello.
//!
//! `unsafe` is allowed in the platform modules only, each block saying why it holds.

#[cfg(target_os = "android")]
pub mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
pub mod biometrics;
#[cfg(target_os = "linux")]
mod linux;
pub mod secure;
#[cfg(target_os = "windows")]
mod windows;

/// Fills `out` with random bytes from the system.
///
/// # Errors
/// When the system has no randomness to give (it always has).
pub(crate) fn random(out: &mut [u8]) -> Result<(), secure::Error> {
    getrandom::fill(out).map_err(|_| secure::Error::Unavailable)
}
