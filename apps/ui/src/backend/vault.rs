//! Device secrets at rest under the system's secure store (D-0172).
//!
//! [`Storage`] is NSH's app storage with the same three calls, except that the values of
//! [`SECRET`] keys (identity seeds, the message store's key, the recovery key, the records'
//! replica, the backup destination's sign-in, …) are encrypted under the **vault key**, which
//! the Keychain, the Android Keystore, DPAPI or the Secret Service keeps
//! (`songbird_device::secure`). App storage alone then opens nothing.
//!
//! A value is `v1:` and hex of `nonce (12) | AES-256-GCM(vault key, nonce, value,
//! aad = "songbird.v1/vault\0" | key name)`, so a value can't be moved to another name. A value
//! written before (plaintext) is encrypted the first time it is read. Where the system has no
//! secure store (a Linux without a keyring), values stay as they were and [`protected`] says so.

use std::cell::RefCell;

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use nsh_core::storage::{Storage as AppStorage, StorageFull};
use zeroize::Zeroizing;

use super::wire;

/// The keys whose values are secrets.
pub const SECRET: &[&str] = &[
    "sb.identity_seed",
    "sb.device_seed",
    "sb.device_identity_seed",
    "sb.core.db_key",
    "sb.core.device_identity",
    "sb.recovery_secret",
    "sb.profile_key",
    "sb.records",
    "sb.app_lock",
    "sb.subscriber",
    "sb.family",
    "sb.family_seat",
    "sb.tickets",
    "sb.next_token",
    "sb.vault_passes",
    "sb.my_invites",
    "sb.username_link",
    "sb.supervision_parent",
    "sb.supervision_child",
    "sb.backup_config",
];

#[cfg_attr(test, allow(dead_code, reason = "tests use a fixed key"))]
const WRAPPED: &str = "sb.vault_wrapped";
const PREFIX: &str = "v1:";

/// The vault key's state, found once.
enum Key {
    Ready(LessSafeKey),
    /// No secure store here: values stay as written.
    Absent,
    /// The store has a key it won't give (damaged, another device's): encrypted values can't
    /// be read, and nothing is written in the clear over them.
    Refused,
}

thread_local! {
    static KEY: RefCell<Option<std::rc::Rc<Key>>> = const { RefCell::new(None) };
}

#[cfg_attr(test, allow(dead_code, reason = "tests use a fixed key"))]
struct Keep;

impl songbird_device::secure::Keep for Keep {
    fn load(&self) -> Option<Vec<u8>> {
        AppStorage::get_item(WRAPPED).and_then(|h| wire::unhex(&h))
    }
    fn save(&self, wrapped: &[u8]) {
        let _ = AppStorage::set_item(WRAPPED, wire::hex(wrapped));
    }
}

fn load() -> Key {
    #[cfg(test)]
    let got: Result<Zeroizing<[u8; 32]>, songbird_device::secure::Error> =
        Ok(Zeroizing::new([0x5a; 32]));
    #[cfg(not(test))]
    let got = songbird_device::secure::vault_key(&Keep);
    match got {
        Ok(k) => UnboundKey::new(&AES_256_GCM, &k[..])
            .map(|k| Key::Ready(LessSafeKey::new(k)))
            .unwrap_or(Key::Refused),
        Err(songbird_device::secure::Error::Unavailable) => Key::Absent,
        Err(songbird_device::secure::Error::Refused) => Key::Refused,
    }
}

fn key() -> std::rc::Rc<Key> {
    KEY.with_borrow_mut(|k| k.get_or_insert_with(|| std::rc::Rc::new(load())).clone())
}

/// Whether device secrets are kept under the system's secure store.
#[must_use]
pub fn protected() -> bool {
    matches!(*key(), Key::Ready(_))
}

fn aad(name: &str) -> Vec<u8> {
    let mut a = b"songbird.v1/vault\0".to_vec();
    a.extend_from_slice(name.as_bytes());
    a
}

fn seal(k: &LessSafeKey, name: &str, value: &str) -> Option<String> {
    let nonce: [u8; 12] = wire::random();
    let mut buf = value.as_bytes().to_vec();
    k.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(aad(name)),
        &mut buf,
    )
    .ok()?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&buf);
    Some(format!("{PREFIX}{}", wire::hex(&out)))
}

fn open(k: &LessSafeKey, name: &str, sealed: &str) -> Option<String> {
    let bytes = Zeroizing::new(wire::unhex(sealed)?);
    let (nonce, ct) = bytes.split_at_checked(12)?;
    let mut buf = Zeroizing::new(ct.to_vec());
    let plain = k
        .open_in_place(
            Nonce::try_assume_unique_for_key(nonce).ok()?,
            Aad::from(aad(name)),
            &mut buf,
        )
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

/// App storage, with secrets encrypted under the vault key (the same calls as NSH's).
pub struct Storage;

impl Storage {
    /// The value of `key`, `None` when it has none (or a secret that can't be opened).
    #[must_use]
    pub fn get_item(name: &str) -> Option<String> {
        let raw = AppStorage::get_item(name)?;
        if !SECRET.contains(&name) {
            return Some(raw);
        }
        let k = key();
        match (&*k, raw.strip_prefix(PREFIX)) {
            (Key::Ready(k), Some(sealed)) => open(k, name, sealed),
            (_, Some(_)) => None,
            (Key::Ready(k), None) => {
                // Written before the vault: encrypted now.
                if let Some(s) = seal(k, name, &raw) {
                    let _ = AppStorage::set_item(name, s);
                }
                Some(raw)
            }
            (_, None) => Some(raw),
        }
    }

    /// Sets `key` to `value` (encrypted when it is a secret).
    ///
    /// # Errors
    /// [`StorageFull`], or a secret that can't be kept safely (the store refused its key).
    pub fn set_item(name: impl Into<String>, value: impl Into<String>) -> Result<(), StorageFull> {
        let (name, value) = (name.into(), value.into());
        if !SECRET.contains(&name.as_str()) {
            return AppStorage::set_item(name, value);
        }
        let k = key();
        match &*k {
            Key::Ready(k) => match seal(k, &name, &value) {
                Some(s) => AppStorage::set_item(name, s),
                None => Err(StorageFull { bytes: 0 }),
            },
            Key::Absent => AppStorage::set_item(name, value),
            Key::Refused => Err(StorageFull { bytes: 0 }),
        }
    }

    /// Removes `key`.
    pub fn remove_item(name: &str) {
        AppStorage::remove_item(name);
    }
}

/// A stored value as the test key opens it (tests that read app storage directly).
#[cfg(test)]
#[must_use]
pub fn reveal(name: &str, raw: &str) -> Option<String> {
    let Key::Ready(k) = load() else { return None };
    open(&k, name, raw.strip_prefix(PREFIX)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_bound_to_their_name_and_open_only_with_the_key() {
        let Key::Ready(k) = load() else {
            panic!("a test key");
        };
        let s = seal(&k, "sb.identity_seed", "00ff").unwrap();
        assert!(s.starts_with("v1:") && !s.contains("00ff"));
        assert_eq!(
            open(&k, "sb.identity_seed", &s[3..]).as_deref(),
            Some("00ff")
        );
        assert_eq!(
            open(&k, "sb.device_seed", &s[3..]),
            None,
            "moved to another name"
        );
        let other = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &[1; 32]).unwrap());
        assert_eq!(open(&other, "sb.identity_seed", &s[3..]), None);
    }
}
