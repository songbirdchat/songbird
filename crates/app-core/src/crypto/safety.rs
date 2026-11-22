//! Safety numbers (D-0136): what two people compare to know no one sits between them.
//!
//! Signal's numeric fingerprint construction (version 2), over Songbird's account identity keys
//! (Ed25519, the key that signs each account's device list, PROTOCOL §4.6), so one number covers
//! every device of both accounts and changes only when an account identity changes:
//!
//! - per account: `h = SHA-512^5200(0x00 0x02 || key || account)`, iterating
//!   `h = SHA-512(h || key)`; the first 30 bytes as six 5-digit groups (each 5-byte chunk as a
//!   big-endian integer mod 100,000);
//! - the number is both accounts' 30 digits, the smaller first, so both sides show the same;
//! - the QR code carries `version u8 (1) | both keys' 32-byte fingerprints`, compared as bytes.

use aws_lc_rs::digest::{Context, SHA512};

use super::ct_eq;

const ITERATIONS: u32 = 5200;

fn fingerprint(key: &[u8; 32], account: &[u8; 16]) -> [u8; 32] {
    let mut h = {
        let mut c = Context::new(&SHA512);
        c.update(&[0, 2]);
        c.update(key);
        c.update(account);
        c.update(key);
        c.finish()
    };
    for _ in 1..ITERATIONS {
        let mut c = Context::new(&SHA512);
        c.update(h.as_ref());
        c.update(key);
        h = c.finish();
    }
    let mut out = [0u8; 32];
    for (o, b) in out.iter_mut().zip(h.as_ref()) {
        *o = *b;
    }
    out
}

fn digits(fp: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(30);
    for chunk in fp.as_chunks::<5>().0.iter().take(6) {
        let n = chunk.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b)) % 100_000;
        let _ = write!(s, "{n:05}");
    }
    s
}

/// A safety number between two accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyNumber {
    /// 60 digits.
    pub digits: String,
    /// What the QR code shows.
    pub scannable: Vec<u8>,
}

/// The safety number of (`ours`, `theirs`): each an account and its identity key.
#[must_use]
pub fn safety_number(ours: (&[u8; 16], &[u8; 32]), theirs: (&[u8; 16], &[u8; 32])) -> SafetyNumber {
    let a = fingerprint(ours.1, ours.0);
    let b = fingerprint(theirs.1, theirs.0);
    let (da, db) = (digits(&a), digits(&b));
    let digits = if da <= db {
        format!("{da}{db}")
    } else {
        format!("{db}{da}")
    };
    let mut scannable = vec![1];
    scannable.extend_from_slice(&a);
    scannable.extend_from_slice(&b);
    SafetyNumber { digits, scannable }
}

/// Whether a scanned code (from the other side) matches ours: their code lists their
/// fingerprint first.
#[must_use]
pub fn scanned_matches(ours: &SafetyNumber, scanned: &[u8]) -> bool {
    let (Some(mine), Some(theirs)) = (ours.scannable.get(1..33), ours.scannable.get(33..65)) else {
        return false;
    };
    let mut expect = vec![1];
    expect.extend_from_slice(theirs);
    expect.extend_from_slice(mine);
    ct_eq(&expect, scanned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::many_single_char_names, reason = "two accounts and their keys")]
    fn both_sides_see_one_number_and_their_codes_match() {
        let (a, ka, b, kb) = ([1u8; 16], [2u8; 32], [3u8; 16], [4u8; 32]);
        let x = safety_number((&a, &ka), (&b, &kb));
        let y = safety_number((&b, &kb), (&a, &ka));
        assert_eq!(x.digits, y.digits);
        assert_eq!(x.digits.len(), 60);
        assert!(scanned_matches(&x, &y.scannable));
        assert!(!scanned_matches(&x, &x.scannable));
        let z = safety_number((&a, &ka), (&b, &[5; 32]));
        assert_ne!(x.digits, z.digits);
    }
}
