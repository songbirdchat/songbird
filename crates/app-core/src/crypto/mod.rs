//! Primitives on aws-lc-rs, and the protocols built on them.
//!
//! Every secret is held in a type that zeroes it on drop. X25519 uses aws-lc-rs's `agreement`
//! (which refuses an all-zero shared secret, the small-order case), ML-KEM its `kem` module,
//! HKDF-SHA256, HMAC-SHA256 and AES-256-GCM their modules. XEdDSA comes from `songbird-keys`, the
//! same code the server verifies with.

pub mod envelope;
pub mod pqxdh;
pub mod ratchet;
pub mod safety;

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, PrivateKey, UnparsedPublicKey, X25519};
use aws_lc_rs::kem::{Ciphertext, DecapsulationKey, EncapsulationKey, ML_KEM_768, ML_KEM_1024};
use aws_lc_rs::{hkdf, hmac};
use zeroize::{Zeroize, Zeroizing};

use crate::{Error, Result};

/// X25519 public key length.
pub const X25519_LEN: usize = 32;
/// AES-GCM tag length.
pub const TAG_LEN: usize = 16;
/// libsignal's type byte for an X25519 key (`0x05 || u`, PROTOCOL §4.4).
pub const TYPE_X25519: u8 = 0x05;

/// `N` bytes from the system's random source.
pub fn random<const N: usize>() -> Result<Zeroizing<[u8; N]>> {
    let mut b = Zeroizing::new([0u8; N]);
    aws_lc_rs::rand::fill(b.as_mut()).map_err(|_| Error::Random)?;
    Ok(b)
}

/// An X25519 key pair; the secret is zeroed on drop.
#[derive(Clone)]
pub struct X25519Pair {
    secret: [u8; 32],
    public: [u8; X25519_LEN],
}

impl Drop for X25519Pair {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

impl Drop for KemPair {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for X25519Pair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X25519Pair").finish_non_exhaustive()
    }
}

impl X25519Pair {
    /// A fresh key pair.
    pub fn generate() -> Result<Self> {
        Self::from_secret(&*random::<32>()?)
    }

    /// From 32 secret bytes (clamped by X25519 itself).
    pub fn from_secret(secret: &[u8; 32]) -> Result<Self> {
        let k = PrivateKey::from_private_key(&X25519, secret).map_err(|_| Error::Malformed)?;
        let public = k.compute_public_key().map_err(|_| Error::Malformed)?;
        let public: [u8; 32] = public.as_ref().try_into().map_err(|_| Error::Malformed)?;
        Ok(Self {
            secret: *secret,
            public,
        })
    }

    /// The public key.
    #[must_use]
    pub fn public(&self) -> [u8; X25519_LEN] {
        self.public
    }

    /// The secret bytes (for the encrypted store only).
    #[must_use]
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    /// X25519 with `peer`. Refuses dishonest peer keys (non-canonical, small or mixed order) as
    /// the server does, and an all-zero result.
    pub fn dh(&self, peer: &[u8; X25519_LEN]) -> Result<Zeroizing<[u8; 32]>> {
        if !songbird_keys::xeddsa::x25519_public_key_ok(peer) {
            return Err(Error::Verify);
        }
        let k =
            PrivateKey::from_private_key(&X25519, &self.secret).map_err(|_| Error::Malformed)?;
        agreement::agree(
            &k,
            UnparsedPublicKey::new(&X25519, peer),
            Error::Verify,
            |s| {
                let mut out = Zeroizing::new([0u8; 32]);
                out.copy_from_slice(s.get(..32).ok_or(Error::Verify)?);
                Ok(out)
            },
        )
    }

    /// An XEdDSA signature by this key (PROTOCOL §4.4: prekeys are signed by the device identity).
    pub fn sign(&self, message: &[u8]) -> Result<[u8; 64]> {
        let r = random::<64>()?;
        Ok(songbird_keys::xeddsa::Signer::new(&self.secret).sign(message, &r))
    }
}

/// `0x05 || u`.
#[must_use]
pub fn serialize_x25519(public: &[u8; X25519_LEN]) -> [u8; 33] {
    let mut out = [0u8; 33];
    out[0] = TYPE_X25519;
    out[1..].copy_from_slice(public);
    out
}

/// The `u` of `0x05 || u`.
pub fn parse_x25519(bytes: &[u8]) -> Result<[u8; X25519_LEN]> {
    match bytes.split_first() {
        Some((&TYPE_X25519, rest)) => rest.try_into().map_err(|_| Error::Malformed),
        _ => Err(Error::Malformed),
    }
}

/// Which ML-KEM parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kem {
    /// PQXDH's prekeys (PROTOCOL §4.4, D-0050).
    MlKem1024,
    /// The ratchet's periodic re-keying (D-0136): smaller, still NIST level 3.
    MlKem768,
}

impl Kem {
    fn alg(self) -> &'static aws_lc_rs::kem::Algorithm<aws_lc_rs::kem::AlgorithmId> {
        match self {
            Self::MlKem1024 => &ML_KEM_1024,
            Self::MlKem768 => &ML_KEM_768,
        }
    }

    /// Encapsulation key length.
    #[must_use]
    pub const fn public_len(self) -> usize {
        match self {
            Self::MlKem1024 => 1568,
            Self::MlKem768 => 1184,
        }
    }

    /// Ciphertext length.
    #[must_use]
    pub const fn ciphertext_len(self) -> usize {
        match self {
            Self::MlKem1024 => 1568,
            Self::MlKem768 => 1088,
        }
    }
}

/// An ML-KEM key pair: the decapsulation key's bytes (zeroed on drop) and the public key.
#[derive(Clone)]
pub struct KemPair {
    kem: Kem,
    secret: Vec<u8>,
    public: Vec<u8>,
}

impl std::fmt::Debug for KemPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KemPair")
            .field("kem", &self.kem)
            .finish_non_exhaustive()
    }
}

impl KemPair {
    /// A fresh key pair.
    pub fn generate(kem: Kem) -> Result<Self> {
        let dk = DecapsulationKey::generate(kem.alg()).map_err(|_| Error::Random)?;
        let public = dk
            .encapsulation_key()
            .and_then(|e| e.key_bytes())
            .map_err(|_| Error::Random)?
            .as_ref()
            .to_vec();
        let secret = dk.key_bytes().map_err(|_| Error::Random)?.as_ref().to_vec();
        Ok(Self {
            kem,
            secret,
            public,
        })
    }

    /// From stored parts.
    pub fn from_parts(kem: Kem, secret: Vec<u8>, public: Vec<u8>) -> Result<Self> {
        DecapsulationKey::new(kem.alg(), &secret).map_err(|_| Error::Malformed)?;
        EncapsulationKey::new(kem.alg(), &public).map_err(|_| Error::Malformed)?;
        Ok(Self {
            kem,
            secret,
            public,
        })
    }

    /// The parameter set.
    #[must_use]
    pub fn kem(&self) -> Kem {
        self.kem
    }

    /// The encapsulation key.
    #[must_use]
    pub fn public(&self) -> &[u8] {
        &self.public
    }

    /// The decapsulation key bytes (for the encrypted store only).
    #[must_use]
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }

    /// Decapsulates `ciphertext`.
    pub fn decapsulate(&self, ciphertext: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        if ciphertext.len() != self.kem.ciphertext_len() {
            return Err(Error::Malformed);
        }
        let dk =
            DecapsulationKey::new(self.kem.alg(), &self.secret).map_err(|_| Error::Malformed)?;
        let ss = dk
            .decapsulate(Ciphertext::from(ciphertext))
            .map_err(|_| Error::Verify)?;
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(ss.as_ref().get(..32).ok_or(Error::Verify)?);
        Ok(out)
    }
}

/// Encapsulates to `public` (FIPS 203's modulus check first, as the server does).
pub fn encapsulate(kem: Kem, public: &[u8]) -> Result<(Vec<u8>, Zeroizing<[u8; 32]>)> {
    if public.len() != kem.public_len() {
        return Err(Error::Malformed);
    }
    if kem == Kem::MlKem1024 {
        let key: &[u8; 1568] = public.try_into().map_err(|_| Error::Malformed)?;
        if !songbird_keys::kem::modulus_check(key) {
            return Err(Error::Verify);
        }
    }
    let ek = EncapsulationKey::new(kem.alg(), public).map_err(|_| Error::Verify)?;
    let (ct, ss) = ek.encapsulate().map_err(|_| Error::Random)?;
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(ss.as_ref().get(..32).ok_or(Error::Verify)?);
    Ok((ct.as_ref().to_vec(), out))
}

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

/// HKDF-SHA256 into `out`.
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[&[u8]], out: &mut [u8]) -> Result<()> {
    hkdf::Salt::new(hkdf::HKDF_SHA256, salt)
        .extract(ikm)
        .expand(info, Len(out.len()))
        .and_then(|o| o.fill(out))
        .map_err(|_| Error::Malformed)
}

/// HMAC-SHA256.
#[must_use]
pub fn hmac(key: &[u8], parts: &[&[u8]]) -> Zeroizing<[u8; 32]> {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut ctx = hmac::Context::with_key(&k);
    for p in parts {
        ctx.update(p);
    }
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(ctx.sign().as_ref());
    out
}

/// SHA-256.
#[must_use]
pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut ctx = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
    for p in parts {
        ctx.update(p);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

/// AES-256-GCM seal of `plaintext` (appends the tag).
pub fn seal(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::Malformed)?);
    let mut buf = plaintext.to_vec();
    k.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(*nonce),
        Aad::from(aad),
        &mut buf,
    )
    .map_err(|_| Error::Malformed)?;
    Ok(buf)
}

/// AES-256-GCM open (`ciphertext` ends with the tag).
pub fn open(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    if ciphertext.len() < TAG_LEN {
        return Err(Error::Malformed);
    }
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::Malformed)?);
    let mut buf = ciphertext.to_vec();
    let n = k
        .open_in_place(
            Nonce::assume_unique_for_key(*nonce),
            Aad::from(aad),
            &mut buf,
        )
        .map_err(|_| Error::Verify)?
        .len();
    buf.truncate(n);
    Ok(buf)
}

/// Constant-time equality.
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && aws_lc_rs::constant_time::verify_slices_are_equal(a, b).is_ok()
}

/// A bounded reader for the hand-written formats: every read checks length first.
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Over `buf`.
    #[must_use]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// `n` bytes.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.buf.len() < n {
            return Err(Error::Malformed);
        }
        let (a, b) = self.buf.split_at(n);
        self.buf = b;
        Ok(a)
    }

    /// A fixed array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| Error::Malformed)
    }

    /// A byte.
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    /// A big-endian u16.
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    /// A big-endian u32.
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    /// A big-endian u64.
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// Bytes prefixed by a u32 length, at most `max`.
    pub fn bytes32(&mut self, max: usize) -> Result<&'a [u8]> {
        let n = usize::try_from(self.u32()?).map_err(|_| Error::Malformed)?;
        if n > max {
            return Err(Error::Malformed);
        }
        self.take(n)
    }

    /// What is left.
    #[must_use]
    pub fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.buf)
    }

    /// Fails unless everything was read (one encoding per value).
    pub fn finish(&self) -> Result<()> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
}

/// Appends bytes prefixed by a u32 length.
pub fn put_bytes32(out: &mut Vec<u8>, b: &[u8]) -> Result<()> {
    out.extend_from_slice(
        &u32::try_from(b.len())
            .map_err(|_| Error::Malformed)?
            .to_be_bytes(),
    );
    out.extend_from_slice(b);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;

    #[test]
    fn x25519_agrees_and_refuses_small_order() {
        let a = X25519Pair::generate().unwrap();
        let b = X25519Pair::generate().unwrap();
        assert_eq!(*a.dh(&b.public()).unwrap(), *b.dh(&a.public()).unwrap());
        assert_eq!(a.dh(&[0; 32]), Err(Error::Verify));
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(a.dh(&one), Err(Error::Verify));
    }

    #[test]
    fn signatures_verify_with_the_servers_code() {
        let a = X25519Pair::generate().unwrap();
        let sig = a.sign(b"m").unwrap();
        assert!(songbird_keys::xeddsa::verify(&a.public(), b"m", &sig));
    }

    #[test]
    fn kem_round_trips_from_stored_parts() {
        for kem in [Kem::MlKem768, Kem::MlKem1024] {
            let k = KemPair::generate(kem).unwrap();
            let k2 = KemPair::from_parts(kem, k.secret().to_vec(), k.public().to_vec()).unwrap();
            let (ct, ss) = encapsulate(kem, k.public()).unwrap();
            assert_eq!(*k2.decapsulate(&ct).unwrap(), *ss);
            assert_eq!(k.decapsulate(&ct[1..]), Err(Error::Malformed));
        }
    }

    #[test]
    fn aead_round_trips_and_refuses_tampering() {
        let c = seal(&[1; 32], &[0; 12], b"ad", b"hello").unwrap();
        assert_eq!(open(&[1; 32], &[0; 12], b"ad", &c).unwrap(), b"hello");
        assert_eq!(open(&[1; 32], &[0; 12], b"ae", &c), Err(Error::Verify));
    }
}
