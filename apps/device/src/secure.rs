//! The vault key: 32 random bytes the system's secure store keeps for this app on this device
//! (D-0172). The app encrypts its device secrets (identity seeds, the message store's key, the
//! recovery key, …) under it, so app storage alone opens nothing.
//!
//! - **Apple**: a Keychain item, this device only, readable after the first unlock (never in
//!   backups or iCloud Keychain).
//! - **Android**: wrapped by a non-exportable AES key in the Android Keystore (hardware-backed
//!   where the device has it); the wrapped bytes are the app's to keep.
//! - **Windows**: wrapped by DPAPI for this user; the wrapped bytes are the app's to keep.
//! - **Linux**: an item in the Secret Service (GNOME Keyring, KWallet).

#![forbid(unsafe_code)]

use zeroize::Zeroizing;

/// Why the vault key can't be had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// This system has no secure store the app can use (no keyring service, an unsupported
    /// platform): the app says so and keeps secrets as before.
    Unavailable,
    /// The store has a key but refused it (locked, damaged, another device's).
    Refused,
}

/// Where a platform that wraps the key (Android, Windows) keeps the wrapped bytes: the app's
/// own storage.
pub trait Keep {
    /// The wrapped key kept, if any.
    fn load(&self) -> Option<Vec<u8>>;
    /// Keeps the wrapped key.
    fn save(&self, wrapped: &[u8]);
}

/// The vault key: the one the store keeps, or a new one made and kept now.
///
/// # Errors
/// [`Error`].
#[allow(unused_variables, reason = "only the wrapping platforms use `keep`")]
pub fn vault_key(keep: &dyn Keep) -> Result<Zeroizing<[u8; 32]>, Error> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return crate::apple::vault_key();
    #[cfg(target_os = "android")]
    return crate::android::vault_key(keep);
    #[cfg(target_os = "windows")]
    return crate::windows::vault_key(keep);
    #[cfg(target_os = "linux")]
    return crate::linux::vault_key();
    #[allow(unreachable_code, reason = "other platforms")]
    Err(Error::Unavailable)
}

/// A new random key.
pub(crate) fn fresh() -> Result<Zeroizing<[u8; 32]>, Error> {
    let mut k = Zeroizing::new([0u8; 32]);
    crate::random(k.as_mut())?;
    Ok(k)
}
