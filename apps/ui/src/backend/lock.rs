//! The app lock's passcode (Prompt 9, M5): a device-only setting. The
//! passcode never leaves the device and is not stored: only a salt and
//! PBKDF2-HMAC-SHA256 of it (600,000 rounds, OWASP's figure), checked in
//! constant time. Wrong guesses wait longer and longer (30 s after five,
//! doubling). Face ID and fingerprints come with the native host projects.

use std::num::NonZeroU32;

use super::vault::Storage;
use aws_lc_rs::pbkdf2;
use nsh_core::prelude::*;

use super::wire;

const K_PASSCODE: &str = "sb.app_lock";
/// Wrong guesses since the last right one, and the wait still to serve
/// (`n:left_ms`): saved, so closing the app doesn't reset the wait. The
/// wait is counted down on the monotonic clock while the lock screen
/// shows, never against the wall clock, so moving the phone's clock
/// forward skips nothing (D-0108).
const K_FAILS: &str = "sb.app_lock_fails";
const ROUNDS: u32 = 600_000;
/// Wrong guesses allowed before the first wait.
pub const FREE_TRIES: u32 = 5;

fn rounds() -> NonZeroU32 {
    NonZeroU32::new(ROUNDS).unwrap_or(NonZeroU32::MIN)
}

/// `salt [16] ‖ hash [32]` for a passcode.
fn derive(code: &str, salt: [u8; 16]) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        rounds(),
        &salt,
        code.as_bytes(),
        &mut out,
    );
    out
}

/// What is stored for `code` with `salt` (tests set a passcode with it).
#[cfg(test)]
pub(crate) fn stored_form(code: &str, salt: [u8; 16]) -> String {
    let mut v = salt.to_vec();
    v.extend_from_slice(&derive(code, salt));
    wire::hex(&v)
}

/// Whether a passcode is set.
pub fn is_set() -> bool {
    Storage::get_item(K_PASSCODE).is_some()
}

/// Sets the passcode (6 digits), on a blocking thread.
pub async fn set(code: String) -> bool {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let salt: [u8; 16] = wire::random();
    let Ok(hash) = blocking(move || derive(&code, salt)).await else {
        return false;
    };
    let mut stored = salt.to_vec();
    stored.extend_from_slice(&hash);
    Storage::set_item(K_PASSCODE, wire::hex(&stored)).is_ok()
}

/// Forgets the passcode (the lock is off).
pub fn clear() {
    Storage::remove_item(K_PASSCODE);
    Storage::remove_item(K_FAILS);
}

/// Wrong guesses so far and the wait still to serve (ms).
#[must_use]
pub fn failures() -> (u32, u64) {
    Storage::get_item(K_FAILS)
        .and_then(|v| {
            let (n, left) = v.split_once(':')?;
            Some((n.parse().ok()?, left.parse().ok()?))
        })
        .unwrap_or((0, 0))
}

fn save_failures(n: u32, left_ms: u64) {
    if let Err(e) = Storage::set_item(K_FAILS, format!("{n}:{left_ms}")) {
        log::warn!("app lock storage: {e}");
    }
}

/// Counts a wrong guess; returns the new count and the wait to serve (ms).
pub fn record_failure() -> (u32, u64) {
    let n = failures().0.saturating_add(1);
    let left = u64::try_from(wait_after(n).as_millis()).unwrap_or(u64::MAX);
    save_failures(n, left);
    (n, left)
}

/// The wait served so far: what is left is saved (the lock screen calls
/// this as its monotonic countdown runs).
pub fn wait_left(left_ms: u64) {
    save_failures(failures().0, left_ms);
}

/// A right guess clears the count.
pub fn reset_failures() {
    Storage::remove_item(K_FAILS);
}

/// Checks a passcode, on a blocking thread.
pub async fn check(code: String) -> bool {
    let Some(stored) = Storage::get_item(K_PASSCODE).and_then(|h| wire::unhex(&h)) else {
        return false;
    };
    let Some((salt, hash)) = stored.split_first_chunk::<16>() else {
        return false;
    };
    let (salt, hash) = (*salt, hash.to_vec());
    blocking(move || {
        pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            rounds(),
            &salt,
            code.as_bytes(),
            &hash,
        )
        .is_ok()
    })
    .await
    .unwrap_or(false)
}

/// How long to wait before the next guess after `wrong` wrong ones.
#[must_use]
pub fn wait_after(wrong: u32) -> std::time::Duration {
    if wrong < FREE_TRIES {
        return std::time::Duration::ZERO;
    }
    let doublings = (wrong - FREE_TRIES).min(10);
    std::time::Duration::from_secs(30u64 << doublings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passcode_is_kept_only_as_a_salted_slow_hash() {
        let salt: [u8; 16] = wire::random();
        let other: [u8; 16] = wire::random();
        let h = derive("123456", salt);
        assert_ne!(h, derive("123457", salt));
        assert_ne!(h, derive("123456", other));
        assert!(pbkdf2::verify(pbkdf2::PBKDF2_HMAC_SHA256, rounds(), &salt, b"123456", &h).is_ok());
    }

    #[test]
    fn wrong_guesses_wait_longer_and_longer() {
        assert_eq!(wait_after(4).as_secs(), 0);
        assert_eq!(wait_after(5).as_secs(), 30);
        assert_eq!(wait_after(6).as_secs(), 60);
        assert_eq!(wait_after(100).as_secs(), 30 << 10);
    }
}
