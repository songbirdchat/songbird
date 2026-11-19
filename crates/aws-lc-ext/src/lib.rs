//! aws-lc operations aws-lc-rs does not expose (D-0145), for app-core's OpenMLS provider:
//!
//! - [`ml_kem_768_from_seed`]: FIPS 203 `ML-KEM.KeyGen_internal(d, z)` from the 64-byte seed
//!   `d ‖ z` (`EVP_PKEY_keygen_deterministic`), which X-Wing's key expansion and OpenMLS's
//!   `derive_hpke_keypair` need: a node's key pair must follow from its path secret.
//! - [`shake256`]: SHAKE256 with any output length (X-Wing expands its 32-byte key with it).
//!
//! This is a workspace `unsafe` exception (constitution rule 5): every `unsafe` block is an FFI
//! call into the aws-lc build aws-lc-rs already links, with buffers this module owns and sizes it
//! checks; every allocated aws-lc object is freed on every path by a guard. The results are
//! cross-checked against an independent implementation (libcrux through `hpke-rs`) in
//! `tests/groups`.
#![deny(unsafe_code)]

use std::ptr;

/// ML-KEM-768 decapsulation key (expanded) length.
pub const ML_KEM_768_SK: usize = 2400;
/// ML-KEM-768 encapsulation key length.
pub const ML_KEM_768_PK: usize = 1184;

/// The operation failed inside aws-lc (no detail: it carries key material).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failed;

#[allow(unsafe_code, reason = "FFI into aws-lc; D-0145")]
mod ffi {
    use aws_lc_sys as sys;

    use super::{Failed, ML_KEM_768_PK, ML_KEM_768_SK, ptr};

    /// Frees an `EVP_PKEY_CTX` on drop.
    struct Ctx(*mut sys::EVP_PKEY_CTX);
    impl Drop for Ctx {
        fn drop(&mut self) {
            // SAFETY: the pointer came from EVP_PKEY_CTX_new_id and is freed once, here.
            unsafe { sys::EVP_PKEY_CTX_free(self.0) }
        }
    }

    /// Frees an `EVP_PKEY` on drop.
    struct Key(*mut sys::EVP_PKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the pointer came from keygen (or is null, which EVP_PKEY_free accepts).
            unsafe { sys::EVP_PKEY_free(self.0) }
        }
    }

    /// Frees an `EVP_MD_CTX` on drop.
    struct Md(*mut sys::EVP_MD_CTX);
    impl Drop for Md {
        fn drop(&mut self) {
            // SAFETY: the pointer came from EVP_MD_CTX_new and is freed once, here.
            unsafe { sys::EVP_MD_CTX_free(self.0) }
        }
    }

    pub(super) fn ml_kem_768(seed: &[u8; 64]) -> Result<(Vec<u8>, Vec<u8>), Failed> {
        // SAFETY: plain constructor call; the result is checked for null.
        let ctx = Ctx(unsafe { sys::EVP_PKEY_CTX_new_id(sys::EVP_PKEY_KEM, ptr::null_mut()) });
        if ctx.0.is_null() {
            return Err(Failed);
        }
        // SAFETY: ctx is a live KEM context.
        if unsafe { sys::EVP_PKEY_CTX_kem_set_params(ctx.0, sys::NID_MLKEM768) } != 1 {
            return Err(Failed);
        }
        // SAFETY: ctx is a live KEM context with its parameters set.
        if unsafe { sys::EVP_PKEY_keygen_init(ctx.0) } != 1 {
            return Err(Failed);
        }
        let mut key = Key(ptr::null_mut());
        let mut seed_len: usize = seed.len();
        // SAFETY: seed points to 64 readable bytes and seed_len says so; out_pkey is a valid
        // pointer to a null EVP_PKEY*, which aws-lc fills with a new key (freed by `Key`).
        if unsafe {
            sys::EVP_PKEY_keygen_deterministic(
                ctx.0,
                &raw mut key.0,
                seed.as_ptr(),
                &raw mut seed_len,
            )
        } != 1
            || key.0.is_null()
        {
            return Err(Failed);
        }
        let mut sk = vec![0u8; ML_KEM_768_SK];
        let mut sk_len = sk.len();
        // SAFETY: sk has sk_len writable bytes; aws-lc writes at most that many and updates it.
        if unsafe { sys::EVP_PKEY_get_raw_private_key(key.0, sk.as_mut_ptr(), &raw mut sk_len) }
            != 1
            || sk_len != ML_KEM_768_SK
        {
            return Err(Failed);
        }
        let mut pk = vec![0u8; ML_KEM_768_PK];
        let mut pk_len = pk.len();
        // SAFETY: pk has pk_len writable bytes; aws-lc writes at most that many and updates it.
        if unsafe { sys::EVP_PKEY_get_raw_public_key(key.0, pk.as_mut_ptr(), &raw mut pk_len) } != 1
            || pk_len != ML_KEM_768_PK
        {
            return Err(Failed);
        }
        Ok((sk, pk))
    }

    pub(super) fn shake256(input: &[u8], out: &mut [u8]) -> Result<(), Failed> {
        // SAFETY: plain constructor call; checked for null.
        let md = Md(unsafe { sys::EVP_MD_CTX_new() });
        if md.0.is_null() {
            return Err(Failed);
        }
        // SAFETY: md is live; EVP_shake256 returns a static method table.
        if unsafe { sys::EVP_DigestInit_ex(md.0, sys::EVP_shake256(), ptr::null_mut()) } != 1 {
            return Err(Failed);
        }
        // SAFETY: input points to input.len() readable bytes.
        if unsafe { sys::EVP_DigestUpdate(md.0, input.as_ptr().cast(), input.len()) } != 1 {
            return Err(Failed);
        }
        // SAFETY: out points to out.len() writable bytes.
        if unsafe { sys::EVP_DigestFinalXOF(md.0, out.as_mut_ptr(), out.len()) } != 1 {
            return Err(Failed);
        }
        Ok(())
    }
}

/// `ML-KEM-768.KeyGen_internal(d, z)` from `seed = d ‖ z`: (decapsulation key in aws-lc's
/// expanded form, which `aws_lc_rs::kem::DecapsulationKey::new` takes; encapsulation key).
pub fn ml_kem_768_from_seed(seed: &[u8; 64]) -> Result<(Vec<u8>, Vec<u8>), Failed> {
    ffi::ml_kem_768(seed)
}

/// SHAKE256 of `input`, `out.len()` bytes of output.
pub fn shake256(input: &[u8], out: &mut [u8]) -> Result<(), Failed> {
    ffi::shake256(input, out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
    use super::*;

    #[test]
    fn shake256_matches_fips_202_empty_input() {
        let mut out = [0u8; 32];
        shake256(b"", &mut out).unwrap();
        // SHAKE256("", 256) from FIPS 202's examples.
        let want = "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f";
        let got = out.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
        assert_eq!(got, want);
    }

    #[test]
    fn seeded_ml_kem_is_deterministic_and_usable() {
        let (sk1, pk1) = ml_kem_768_from_seed(&[7; 64]).unwrap();
        let (sk2, pk2) = ml_kem_768_from_seed(&[7; 64]).unwrap();
        assert_eq!((&sk1, &pk1), (&sk2, &pk2));
        let (_, pk3) = ml_kem_768_from_seed(&[8; 64]).unwrap();
        assert_ne!(pk1, pk3);
        // FIPS 203: the expanded key carries the encapsulation key at offset 1152.
        assert_eq!(&sk1[1152..1152 + ML_KEM_768_PK], &pk1[..]);
    }
}
