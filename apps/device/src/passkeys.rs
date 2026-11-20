//! The platform's passkeys (D-0177, D-0181): WebAuthn credentials kept by the system (iCloud
//! Keychain, the Android credential provider, Windows Hello or a security key or phone), for the account's second factor (PROTOCOL §4.7). Songbird
//! makes one on the main device and later asks any device holding it for an assertion when a
//! new device links.
//!
//! The system binds a passkey to Songbird's domain (`songbird.chat`): the app must be associated
//! with it (Apple's `webcredentials:` entitlement and `apple-app-site-association`, Android's
//! `assetlinks.json`); until then the system refuses with [`Failure::NotAllowed`].

#![forbid(unsafe_code)]

/// A new passkey, as the server takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// The credential's ID.
    pub credential_id: Vec<u8>,
    /// `clientDataJSON`.
    pub client_data_json: Vec<u8>,
    /// The CBOR attestation object.
    pub attestation_object: Vec<u8>,
}

/// An assertion, as the link request carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    /// The credential used.
    pub credential_id: Vec<u8>,
    /// `authenticatorData`.
    pub authenticator_data: Vec<u8>,
    /// `clientDataJSON`.
    pub client_data_json: Vec<u8>,
    /// The signature.
    pub signature: Vec<u8>,
}

/// Why there is no passkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The person cancelled.
    Cancelled,
    /// The system refused: the app isn't associated with the domain, or no passkey matches.
    NotAllowed,
    /// No passkeys on this platform or version.
    Unavailable,
}

/// Answers, on a thread of the system's choosing.
pub type CreateDone = Box<dyn FnOnce(Result<Created, Failure>) + Send>;
/// Answers, on a thread of the system's choosing.
pub type AssertDone = Box<dyn FnOnce(Result<Assertion, Failure>) + Send>;

/// Whether this platform has passkeys the app can use.
#[must_use]
pub fn available() -> bool {
    #[cfg(target_os = "android")]
    let yes = crate::android::passkeys_available();
    #[cfg(target_os = "windows")]
    let yes = crate::windows::passkeys::available();
    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    let yes = cfg!(any(target_os = "macos", target_os = "ios"));
    yes
}

/// Makes a passkey for `rp_id`, for the user `user_id` called `name`, over `challenge`. Call it
/// on the UI thread.
#[allow(unused_variables, reason = "platforms without passkeys")]
pub fn create(rp_id: &str, user_id: &[u8], name: &str, challenge: &[u8], done: CreateDone) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::passkeys::create(rp_id, user_id, name, challenge, done);
    #[cfg(target_os = "android")]
    return crate::android::passkey_create(rp_id, user_id, name, challenge, done);
    #[cfg(target_os = "windows")]
    return crate::windows::passkeys::create(rp_id, user_id, name, challenge, done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(Err(Failure::Unavailable));
}

/// Asks for an assertion over `challenge` from one of `allow` (any of the account's passkeys
/// when empty). Call it on the UI thread.
#[allow(unused_variables, reason = "platforms without passkeys")]
pub fn assert(rp_id: &str, challenge: &[u8], allow: &[Vec<u8>], done: AssertDone) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::passkeys::assert(rp_id, challenge, allow, done);
    #[cfg(target_os = "android")]
    return crate::android::passkey_assert(rp_id, challenge, allow, done);
    #[cfg(target_os = "windows")]
    return crate::windows::passkeys::assert(rp_id, challenge, allow, done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(Err(Failure::Unavailable));
}
