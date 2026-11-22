//! Registration proof of work (`docs/PROTOCOL.md` §4.1, D-0048): Argon2id, memory-hard, issued as
//! a stateless challenge the server authenticates with a MAC.
//!
//! # Challenge (51 bytes)
//!
//! ```text
//! version u8 (1) | issued_ms u64 | memory_kib u32 | iterations u32 | bits u8 | solutions u8
//!   | nonce [16] | tag [16]
//! ```
//!
//! Integers are big-endian. `tag` is the first 16 bytes of
//! `HMAC-SHA256(challenge key, "songbird.v1/pow-challenge\0" || the 35 bytes before it)`: any api
//! node can check a challenge another issued, and the server keeps no state until a solution
//! arrives. The server then records the nonce as used (single use) for the challenge's lifetime.
//!
//! # Solution
//!
//! `solutions` counters `c`, strictly increasing `u32`s, each such that
//!
//! ```text
//! Argon2id(v1.3, m = memory_kib, t = iterations, p = 1, 32-byte output)(
//!     password = "songbird.v1/pow\0" || challenge[0..35] || c (u32),
//!     salt = nonce)
//! ```
//!
//! starts with `bits` zero bits. Asking for several solutions at a lower difficulty instead of one
//! at a higher one keeps the work's spread small (the standard deviation of `k` independent
//! searches is `1/sqrt(k)` of their mean), so a phone's time to solve is predictable.
//!
//! The server verifies at most `solutions` Argon2id evaluations and stops at the first failure;
//! the client spends `solutions × 2^bits` on average. Verification runs only after every cheap
//! check (MAC, age, signatures) and after the nonce is burnt, so a forged or replayed solution
//! costs the server at most one evaluation per challenge it handed out.
#![forbid(unsafe_code)]

use aws_lc_rs::{constant_time, hkdf, hmac};

/// Challenge length.
pub const CHALLENGE_LEN: usize = 51;
/// Length of the authenticated part (everything before the tag).
pub const SIGNED_LEN: usize = 35;
/// Nonce length.
pub const NONCE_LEN: usize = 16;
/// Tag length.
pub const TAG_LEN: usize = 16;
/// Challenge format version.
pub const VERSION: u8 = 1;
/// Domain separation of the tag.
pub const TAG_CONTEXT: &[u8] = b"songbird.v1/pow-challenge\0";
/// Domain separation of the Argon2id input.
pub const POW_CONTEXT: &[u8] = b"songbird.v1/pow\0";

/// Bounds on parameters, enforced on both sides (a client refuses a challenge outside them).
pub mod bounds {
    use std::ops::RangeInclusive;
    /// Argon2id memory in KiB (8 KiB is Argon2's minimum for one lane; 256 MiB is beyond any
    /// phone's comfort).
    pub const MEMORY_KIB: RangeInclusive<u32> = 8..=262_144;
    /// Argon2id passes.
    pub const ITERATIONS: RangeInclusive<u32> = 1..=16;
    /// Leading zero bits per solution.
    pub const BITS: RangeInclusive<u8> = 0..=24;
    /// Solutions per challenge.
    pub const SOLUTIONS: RangeInclusive<u8> = 1..=64;
}

/// Argon2id work parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Passes over memory.
    pub iterations: u32,
    /// Leading zero bits each solution needs.
    pub bits: u8,
    /// Number of solutions.
    pub solutions: u8,
}

impl Params {
    /// Whether every parameter is within [`bounds`].
    #[must_use]
    pub fn in_bounds(&self) -> bool {
        bounds::MEMORY_KIB.contains(&self.memory_kib)
            && bounds::ITERATIONS.contains(&self.iterations)
            && bounds::BITS.contains(&self.bits)
            && bounds::SOLUTIONS.contains(&self.solutions)
    }

    /// Expected Argon2id evaluations to solve: `solutions × 2^bits`.
    #[must_use]
    pub fn expected_evaluations(&self) -> u64 {
        u64::from(self.solutions) << self.bits
    }
}

/// A decoded challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Challenge {
    /// When it was issued (server clock, Unix milliseconds).
    pub issued_ms: u64,
    /// The work asked for.
    pub params: Params,
    /// Random, single use.
    pub nonce: [u8; NONCE_LEN],
    /// The wire bytes (tag included).
    pub bytes: [u8; CHALLENGE_LEN],
}

/// Why a challenge or solution was refused. Deliberately coarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PowError {
    /// Wrong length, version or parameters out of bounds.
    #[error("malformed challenge")]
    Malformed,
    /// The tag does not verify: not issued by this deployment.
    #[error("challenge not authentic")]
    Forged,
    /// Too old, or issued in the future.
    #[error("challenge expired")]
    Expired,
    /// A solution is missing, out of order or does not meet the difficulty.
    #[error("wrong solution")]
    Wrong,
}

/// The key that authenticates challenges, derived from the registration secret.
pub struct ChallengeKey(hmac::Key);

impl std::fmt::Debug for ChallengeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChallengeKey(..)")
    }
}

impl ChallengeKey {
    /// HKDF-SHA256 of the registration secret, info `pow-challenge`.
    ///
    /// # Errors
    /// Only if HKDF refuses its output length, which it does not for 32 bytes.
    pub fn derive(secret: &[u8; 32]) -> Result<Self, aws_lc_rs::error::Unspecified> {
        let mut k = [0u8; 32];
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird-registration-v1")
            .extract(secret)
            .expand(&[b"pow-challenge"], hkdf::HKDF_SHA256)?
            .fill(&mut k)?;
        Ok(Self(hmac::Key::new(hmac::HMAC_SHA256, &k)))
    }

    fn tag(&self, signed: &[u8]) -> [u8; TAG_LEN] {
        let mut ctx = hmac::Context::with_key(&self.0);
        ctx.update(TAG_CONTEXT);
        ctx.update(signed);
        let full = ctx.sign();
        let mut t = [0u8; TAG_LEN];
        t.copy_from_slice(full.as_ref().get(..TAG_LEN).unwrap_or(&[0; TAG_LEN]));
        t
    }

    /// Issues a challenge.
    ///
    /// # Errors
    /// [`PowError::Malformed`] if `params` are out of bounds; the OS random source failing
    /// surfaces as the same error.
    pub fn issue(&self, params: Params, now_ms: u64) -> Result<Challenge, PowError> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| PowError::Malformed)?;
        self.issue_with_nonce(params, now_ms, nonce)
    }

    /// Issues a challenge with a given nonce (test vectors).
    ///
    /// # Errors
    /// [`PowError::Malformed`] if `params` are out of bounds.
    pub fn issue_with_nonce(
        &self,
        params: Params,
        now_ms: u64,
        nonce: [u8; NONCE_LEN],
    ) -> Result<Challenge, PowError> {
        if !params.in_bounds() {
            return Err(PowError::Malformed);
        }
        let mut b = [0u8; CHALLENGE_LEN];
        b[0] = VERSION;
        b[1..9].copy_from_slice(&now_ms.to_be_bytes());
        b[9..13].copy_from_slice(&params.memory_kib.to_be_bytes());
        b[13..17].copy_from_slice(&params.iterations.to_be_bytes());
        b[17] = params.bits;
        b[18] = params.solutions;
        b[19..35].copy_from_slice(&nonce);
        let tag = self.tag(&b[..SIGNED_LEN]);
        b[SIGNED_LEN..].copy_from_slice(&tag);
        Ok(Challenge {
            issued_ms: now_ms,
            params,
            nonce,
            bytes: b,
        })
    }

    /// Checks a challenge's tag (in constant time), format and age.
    ///
    /// # Errors
    /// [`PowError::Malformed`], [`PowError::Forged`] or [`PowError::Expired`].
    pub fn check(&self, bytes: &[u8], now_ms: u64, ttl_ms: u64) -> Result<Challenge, PowError> {
        let c = decode(bytes)?;
        let tag = self.tag(&c.bytes[..SIGNED_LEN]);
        constant_time::verify_slices_are_equal(&tag, &c.bytes[SIGNED_LEN..])
            .map_err(|_| PowError::Forged)?;
        // A little clock skew between api nodes is tolerated in the future direction.
        if c.issued_ms > now_ms.saturating_add(5_000) || now_ms > c.issued_ms.saturating_add(ttl_ms)
        {
            return Err(PowError::Expired);
        }
        Ok(c)
    }
}

/// Decodes a challenge without authenticating it (clients; the server uses
/// [`ChallengeKey::check`]).
///
/// # Errors
/// [`PowError::Malformed`].
pub fn decode(bytes: &[u8]) -> Result<Challenge, PowError> {
    let b: [u8; CHALLENGE_LEN] = bytes.try_into().map_err(|_| PowError::Malformed)?;
    if b[0] != VERSION {
        return Err(PowError::Malformed);
    }
    let issued: [u8; 8] = b[1..9].try_into().map_err(|_| PowError::Malformed)?;
    let params = Params {
        memory_kib: u32::from_be_bytes([b[9], b[10], b[11], b[12]]),
        iterations: u32::from_be_bytes([b[13], b[14], b[15], b[16]]),
        bits: b[17],
        solutions: b[18],
    };
    if !params.in_bounds() {
        return Err(PowError::Malformed);
    }
    let nonce: [u8; NONCE_LEN] = b[19..35].try_into().map_err(|_| PowError::Malformed)?;
    Ok(Challenge {
        issued_ms: u64::from_be_bytes(issued),
        params,
        nonce,
        bytes: b,
    })
}

/// One Argon2id evaluation, reusing `memory` (sized to `memory_kib` blocks).
struct Evaluator {
    argon: argon2::Argon2<'static>,
    memory: Vec<argon2::Block>,
    input: Vec<u8>,
    salt: [u8; NONCE_LEN],
}

impl Evaluator {
    fn new(c: &Challenge) -> Result<Self, PowError> {
        let p = argon2::Params::new(c.params.memory_kib, c.params.iterations, 1, Some(32))
            .map_err(|_| PowError::Malformed)?;
        let blocks = p.block_count();
        let mut input = Vec::with_capacity(POW_CONTEXT.len() + SIGNED_LEN + 4);
        input.extend_from_slice(POW_CONTEXT);
        input.extend_from_slice(&c.bytes[..SIGNED_LEN]);
        input.extend_from_slice(&[0; 4]);
        Ok(Self {
            argon: argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p),
            memory: vec![argon2::Block::new(); blocks],
            input,
            salt: c.nonce,
        })
    }

    fn hash(&mut self, counter: u32) -> Result<[u8; 32], PowError> {
        let n = self.input.len();
        if let Some(tail) = self.input.get_mut(n - 4..) {
            tail.copy_from_slice(&counter.to_be_bytes());
        }
        let mut out = [0u8; 32];
        self.argon
            .hash_password_into_with_memory(&self.input, &self.salt, &mut out, &mut self.memory)
            .map_err(|_| PowError::Malformed)?;
        Ok(out)
    }
}

/// Whether `hash` starts with `bits` zero bits.
#[must_use]
pub fn meets(hash: &[u8; 32], bits: u8) -> bool {
    let mut left = u32::from(bits);
    for &byte in hash {
        if left == 0 {
            return true;
        }
        if left >= 8 {
            if byte != 0 {
                return false;
            }
            left -= 8;
        } else {
            return byte.leading_zeros() >= left;
        }
    }
    left == 0
}

/// Finds the solutions (clients and tests). Returns `None` if the counter space ran out, which
/// for parameters within bounds happens with negligible probability.
///
/// # Errors
/// [`PowError::Malformed`] for parameters Argon2 refuses.
pub fn solve(c: &Challenge) -> Result<Option<Vec<u32>>, PowError> {
    let mut e = Evaluator::new(c)?;
    let want = usize::from(c.params.solutions);
    let mut found = Vec::with_capacity(want);
    let mut counter = 0u32;
    loop {
        if meets(&e.hash(counter)?, c.params.bits) {
            found.push(counter);
            if found.len() == want {
                return Ok(Some(found));
            }
        }
        let Some(next) = counter.checked_add(1) else {
            return Ok(None);
        };
        counter = next;
    }
}

/// Verifies solutions: exactly `solutions` strictly increasing counters, each meeting the
/// difficulty. Stops at the first failure (so a forged solution costs at most one evaluation).
///
/// # Errors
/// [`PowError::Wrong`] (or [`PowError::Malformed`] for parameters Argon2 refuses).
pub fn verify(c: &Challenge, solutions: &[u32]) -> Result<(), PowError> {
    if solutions.len() != usize::from(c.params.solutions)
        || !solutions.windows(2).all(|w| matches!(w, [a, b] if a < b))
    {
        return Err(PowError::Wrong);
    }
    let mut e = Evaluator::new(c)?;
    for &s in solutions {
        if !meets(&e.hash(s)?, c.params.bits) {
            return Err(PowError::Wrong);
        }
    }
    Ok(())
}

/// The Argon2id hash of one counter (benchmarks and vectors).
///
/// # Errors
/// [`PowError::Malformed`] for parameters Argon2 refuses.
pub fn evaluate(c: &Challenge, counter: u32) -> Result<[u8; 32], PowError> {
    Evaluator::new(c)?.hash(counter)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;

    const CHEAP: Params = Params {
        memory_kib: 8,
        iterations: 1,
        bits: 3,
        solutions: 4,
    };

    fn key() -> ChallengeKey {
        ChallengeKey::derive(&[7; 32]).unwrap()
    }

    #[test]
    fn solved_challenges_verify_and_tampering_does_not() {
        let k = key();
        let c = k.issue(CHEAP, 1_000_000).unwrap();
        let checked = k.check(&c.bytes, 1_000_500, 600_000).unwrap();
        assert_eq!(checked, c);
        let s = solve(&c).unwrap().unwrap();
        verify(&c, &s).unwrap();
        // Out of order, repeated, missing, extra, or a counter that does not meet the target.
        let mut swapped = s.clone();
        swapped.swap(0, 1);
        assert_eq!(verify(&c, &swapped), Err(PowError::Wrong));
        let mut repeated = s.clone();
        repeated[1] = repeated[0];
        assert_eq!(verify(&c, &repeated), Err(PowError::Wrong));
        assert_eq!(verify(&c, &s[..3]), Err(PowError::Wrong));
        let mut extra = s.clone();
        extra.push(u32::MAX);
        assert_eq!(verify(&c, &extra), Err(PowError::Wrong));
        // The last counter replaced by a larger one that does not meet the difficulty.
        let mut wrong = s.clone();
        let last = wrong.len() - 1;
        wrong[last] = (wrong[last - 1] + 1..u32::MAX)
            .find(|&n| !meets(&evaluate(&c, n).unwrap(), 3))
            .unwrap();
        assert_eq!(verify(&c, &wrong), Err(PowError::Wrong));
    }

    #[test]
    fn forged_expired_and_malformed_challenges_are_refused() {
        let k = key();
        let c = k.issue(CHEAP, 1_000_000).unwrap();
        // Any changed byte breaks the tag (or the format, checked first).
        for i in 0..CHALLENGE_LEN {
            let mut b = c.bytes;
            b[i] ^= 1;
            assert!(k.check(&b, 1_000_000, 600_000).is_err(), "byte {i}");
        }
        // Another deployment's key.
        let other = ChallengeKey::derive(&[8; 32]).unwrap();
        assert_eq!(
            other.check(&c.bytes, 1_000_000, 600_000),
            Err(PowError::Forged)
        );
        // Age.
        assert_eq!(
            k.check(&c.bytes, 1_600_001, 600_000),
            Err(PowError::Expired)
        );
        assert_eq!(k.check(&c.bytes, 994_999, 600_000), Err(PowError::Expired));
        assert!(k.check(&c.bytes, 995_000, 600_000).is_ok());
        // Length.
        assert_eq!(
            k.check(&c.bytes[..50], 1_000_000, 600_000),
            Err(PowError::Malformed)
        );
        // Parameters out of bounds are never issued.
        for p in [
            Params {
                memory_kib: 4,
                ..CHEAP
            },
            Params {
                iterations: 0,
                ..CHEAP
            },
            Params { bits: 25, ..CHEAP },
            Params {
                solutions: 0,
                ..CHEAP
            },
        ] {
            assert_eq!(k.issue(p, 0), Err(PowError::Malformed));
        }
    }

    #[test]
    fn leading_zero_bits() {
        let mut h = [0xFFu8; 32];
        assert!(meets(&h, 0));
        assert!(!meets(&h, 1));
        h[0] = 0x0F;
        assert!(meets(&h, 4));
        assert!(!meets(&h, 5));
        h[0] = 0;
        h[1] = 0x7F;
        assert!(meets(&h, 9));
        assert!(!meets(&h, 10));
        assert!(meets(&[0; 32], 24));
    }
}
