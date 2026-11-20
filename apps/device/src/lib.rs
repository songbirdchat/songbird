//! Songbird on the device's own services (D-0172): one interface per service over each
//! platform's framework, so the app holds no platform code.
//!
//! - [`secure`]: the vault key, kept by the system's secure store (the Keychain, the Android
//!   Keystore, DPAPI, the Secret Service). The app encrypts its device secrets under it.
//! - [`audio`]: a call's audio on a phone: the microphone's permission, the speaker.
//! - [`callkit`]: incoming calls on iOS: PushKit wakes the app, CallKit rings.
//! - [`biometrics`]: Face ID, Touch ID, fingerprints, Windows Hello.
//! - [`passkeys`]: the platform's passkeys (WebAuthn), for the account's second factor.
//! - [`photos`]: saving a photo or video the person chose (or received, with "Save to photos").
//! - [`push`]: the device's push token (APNs, FCM).
//! - [`location`]: one position, when the person asks to share where they are.
//! - [`store`]: buying a plan through the App Store (StoreKit 2) or Google Play.
//! - [`screen`]: keeping the app's windows out of screenshots and screen recording.
//!
//! `unsafe` is allowed in the platform modules only, each block saying why it holds.

#[cfg(target_os = "android")]
pub mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
pub mod audio;
pub mod biometrics;
pub mod callkit;
#[cfg(target_os = "linux")]
mod linux;
pub mod location;
pub mod passkeys;
pub mod photos;
pub mod push;
pub mod screen;
pub mod secure;
pub mod store;
#[cfg(target_os = "windows")]
mod windows;

/// Fills `out` with random bytes from the system.
///
/// # Errors
/// When the system has no randomness to give (it always has).
pub(crate) fn random(out: &mut [u8]) -> Result<(), secure::Error> {
    getrandom::fill(out).map_err(|_| secure::Error::Unavailable)
}
