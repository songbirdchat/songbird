//! Songbird's OpenMLS crypto provider for devices, on aws-lc-rs (D-0145; the owner chose it over
//! shipping `OpenMLS`'s `RustCrypto` provider, 2026-11-19).
//!
//! The ciphersuite is the one every Songbird group uses (D-0065 §3):
//! `MLS_128_MLKEM768X25519_AES128GCM_SHA256_Ed25519`. Built from:
//!
//! - **X-Wing** (draft-connolly-cfrg-xwing-kem-06): the decapsulation key is a 32-byte seed,
//!   expanded with SHAKE256 to an ML-KEM-768 seed `d ‖ z` and an X25519 key; the shared secret is
//!   `SHA3-256(ss_M ‖ ss_X ‖ ct_X ‖ pk_X ‖ label)`. ML-KEM, X25519 and SHA3 are aws-lc-rs's;
//!   seeded ML-KEM keygen and SHAKE256 come from aws-lc itself (`songbird-aws-lc-ext`, the FFI
//!   exception).
//! - **HPKE** (RFC 9180) base mode over X-Wing, HKDF-SHA256 and AES-128-GCM, single shot, with
//!   the exporter.
//! - Ed25519 (signing keys are 32-byte seeds), HKDF, HMAC, SHA-2 and AEAD from aws-lc-rs.
//!
//! Correctness against an independent implementation (libcrux and `hpke-rs`, through OpenMLS's
//! `RustCrypto` provider) is checked in `tests/groups`: the same derived keys, and ciphertexts
//! each side opens from the other.

use aws_lc_rs::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, PrivateKey, UnparsedPublicKey, X25519};
use aws_lc_rs::kem::{Ciphertext, DecapsulationKey, EncapsulationKey, ML_KEM_768};
use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey as SigKey};
use aws_lc_rs::{digest, hkdf, hmac};
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::random::OpenMlsRand;
use openmls_traits::types::{
    AeadType, Ciphersuite, CryptoError, ExporterSecret, HashType, HpkeAeadType, HpkeCiphertext,
    HpkeConfig, HpkeKdfType, HpkeKemType, HpkeKeyPair, KemOutput, SignatureScheme,
};
use tls_codec::SecretVLBytes;
use zeroize::Zeroizing;

/// The suite every Songbird group uses.
pub const GROUP_CIPHERSUITE: Ciphersuite =
    Ciphersuite::MLS_128_MLKEM768X25519_AES128GCM_SHA256_Ed25519;

const XWING_LABEL: &[u8; 6] = b"\\.//^\\";
const XWING_PK: usize = 1184 + 32;
const XWING_CT: usize = 1088 + 32;

/// The device's crypto provider. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeviceCrypto;

fn digest_alg(h: HashType) -> &'static digest::Algorithm {
    match h {
        HashType::Sha2_256 => &digest::SHA256,
        HashType::Sha2_384 => &digest::SHA384,
        HashType::Sha2_512 => &digest::SHA512,
    }
}

fn hmac_alg(h: HashType) -> hmac::Algorithm {
    match h {
        HashType::Sha2_256 => hmac::HMAC_SHA256,
        HashType::Sha2_384 => hmac::HMAC_SHA384,
        HashType::Sha2_512 => hmac::HMAC_SHA512,
    }
}

fn hkdf_alg(h: HashType) -> hkdf::Algorithm {
    match h {
        HashType::Sha2_256 => hkdf::HKDF_SHA256,
        HashType::Sha2_384 => hkdf::HKDF_SHA384,
        HashType::Sha2_512 => hkdf::HKDF_SHA512,
    }
}

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(
        hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data)
            .as_ref()
            .to_vec(),
    )
}

fn expand_sha256(prk: &[u8], info: &[u8], len: usize) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let prk = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, prk);
    let info = [info];
    let okm = prk
        .expand(&info, Len(len))
        .map_err(|_| CryptoError::HkdfOutputLengthInvalid)?;
    let mut out = Zeroizing::new(vec![0u8; len]);
    okm.fill(&mut out)
        .map_err(|_| CryptoError::CryptoLibraryError)?;
    Ok(out)
}

fn x25519_public(sk: &[u8]) -> Result<[u8; 32], CryptoError> {
    let k =
        PrivateKey::from_private_key(&X25519, sk).map_err(|_| CryptoError::CryptoLibraryError)?;
    let p = k
        .compute_public_key()
        .map_err(|_| CryptoError::CryptoLibraryError)?;
    p.as_ref()
        .try_into()
        .map_err(|_| CryptoError::CryptoLibraryError)
}

fn x25519(sk: &[u8], pk: &[u8]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let k =
        PrivateKey::from_private_key(&X25519, sk).map_err(|_| CryptoError::CryptoLibraryError)?;
    agreement::agree(
        &k,
        UnparsedPublicKey::new(&X25519, pk),
        CryptoError::InvalidPublicKey,
        |s| {
            let mut out = Zeroizing::new([0u8; 32]);
            out.copy_from_slice(s.get(..32).ok_or(CryptoError::CryptoLibraryError)?);
            Ok(out)
        },
    )
}

/// X-Wing's expanded keys: (ML-KEM decapsulation key, ML-KEM encapsulation key, X25519 secret,
/// X25519 public).
struct Expanded {
    dk_m: Zeroizing<Vec<u8>>,
    pk_m: Vec<u8>,
    sk_x: Zeroizing<[u8; 32]>,
    pk_x: [u8; 32],
}

fn xwing_expand(sk: &[u8]) -> Result<Expanded, CryptoError> {
    if sk.len() != 32 {
        return Err(CryptoError::CryptoLibraryError);
    }
    let mut e = Zeroizing::new([0u8; 96]);
    songbird_aws_lc_ext::shake256(sk, e.as_mut()).map_err(|_| CryptoError::CryptoLibraryError)?;
    let (head, tail) = e.split_at(64);
    let mut seed = Zeroizing::new([0u8; 64]);
    seed.copy_from_slice(head);
    let (dk, pk_m) = songbird_aws_lc_ext::ml_kem_768_from_seed(&seed)
        .map_err(|_| CryptoError::CryptoLibraryError)?;
    let mut sk_x = Zeroizing::new([0u8; 32]);
    sk_x.copy_from_slice(tail);
    let pk_x = x25519_public(sk_x.as_ref())?;
    Ok(Expanded {
        dk_m: Zeroizing::new(dk),
        pk_m,
        sk_x,
        pk_x,
    })
}

fn combiner(ss_m: &[u8], ss_x: &[u8], ct_x: &[u8], pk_x: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut c = digest::Context::new(&digest::SHA3_256);
    c.update(ss_m);
    c.update(ss_x);
    c.update(ct_x);
    c.update(pk_x);
    c.update(XWING_LABEL);
    Zeroizing::new(c.finish().as_ref().to_vec())
}

/// X-Wing key pair from a 32-byte seed: (sk, pk).
fn xwing_keypair(sk: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
    let e = xwing_expand(sk)?;
    let mut pk = e.pk_m.clone();
    pk.extend_from_slice(&e.pk_x);
    Ok((sk.to_vec(), pk))
}

fn xwing_encaps(pk: &[u8]) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>), CryptoError> {
    if pk.len() != XWING_PK || !songbird_keys::kem::xwing_public_key_ok(pk) {
        return Err(CryptoError::InvalidPublicKey);
    }
    let (pk_m, pk_x) = pk.split_at(1184);
    let ek = EncapsulationKey::new(&ML_KEM_768, pk_m).map_err(|_| CryptoError::InvalidPublicKey)?;
    let (ct_m, ss_m) = ek
        .encapsulate()
        .map_err(|_| CryptoError::CryptoLibraryError)?;
    let mut eph = Zeroizing::new([0u8; 32]);
    aws_lc_rs::rand::fill(eph.as_mut()).map_err(|_| CryptoError::InsufficientRandomness)?;
    let ct_x = x25519_public(eph.as_ref())?;
    let ss_x = x25519(eph.as_ref(), pk_x)?;
    let ss = combiner(ss_m.as_ref(), ss_x.as_ref(), &ct_x, pk_x);
    let mut ct = ct_m.as_ref().to_vec();
    ct.extend_from_slice(&ct_x);
    Ok((ct, ss))
}

fn xwing_decaps(sk: &[u8], ct: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if ct.len() != XWING_CT {
        return Err(CryptoError::HpkeDecryptionError);
    }
    let e = xwing_expand(sk)?;
    let (ct_m, ct_x) = ct.split_at(1088);
    let dk =
        DecapsulationKey::new(&ML_KEM_768, &e.dk_m).map_err(|_| CryptoError::CryptoLibraryError)?;
    let ss_m = dk
        .decapsulate(Ciphertext::from(ct_m))
        .map_err(|_| CryptoError::HpkeDecryptionError)?;
    let ss_x = x25519(e.sk_x.as_ref(), ct_x)?;
    Ok(combiner(ss_m.as_ref(), ss_x.as_ref(), ct_x, &e.pk_x))
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "OpenMLS passes the config by value"
)]
fn check_config(c: HpkeConfig) -> Result<(), CryptoError> {
    match c {
        HpkeConfig(
            HpkeKemType::XWingKemDraft6,
            HpkeKdfType::HkdfSha256,
            HpkeAeadType::AesGcm128,
        ) => Ok(()),
        _ => Err(CryptoError::UnsupportedCiphersuite),
    }
}

/// X-Wing's HPKE KEM code point (draft-06, `0x647a`); OpenMLS's own `HpkeKemType` value
/// (`0x004D`) names the MLS ciphersuite's KEM, not the HPKE one, and HPKE's suite ID uses this.
const XWING_HPKE_KEM: u16 = 0x647a;

/// RFC 9180 suite ID for X-Wing, HKDF-SHA256, AES-128-GCM.
fn suite_id() -> [u8; 10] {
    let mut s = *b"HPKE\0\0\0\0\0\0";
    s[4..6].copy_from_slice(&XWING_HPKE_KEM.to_be_bytes());
    s[6..8].copy_from_slice(&1u16.to_be_bytes()); // HKDF-SHA256
    s[8..10].copy_from_slice(&1u16.to_be_bytes()); // AES-128-GCM
    s
}

fn labeled_extract(salt: &[u8], label: &[u8], ikm: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut m = Zeroizing::new(Vec::with_capacity(32 + ikm.len()));
    m.extend_from_slice(b"HPKE-v1");
    m.extend_from_slice(&suite_id());
    m.extend_from_slice(label);
    m.extend_from_slice(ikm);
    let salt: &[u8] = if salt.is_empty() { &[0u8; 32] } else { salt };
    hmac_sha256(salt, &m)
}

fn labeled_expand(
    prk: &[u8],
    label: &[u8],
    info: &[u8],
    len: usize,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let mut i = Vec::with_capacity(32 + info.len());
    i.extend_from_slice(
        &u16::try_from(len)
            .map_err(|_| CryptoError::HkdfOutputLengthInvalid)?
            .to_be_bytes(),
    );
    i.extend_from_slice(b"HPKE-v1");
    i.extend_from_slice(&suite_id());
    i.extend_from_slice(label);
    i.extend_from_slice(info);
    expand_sha256(prk, &i, len)
}

/// The base-mode key schedule: (key, base nonce, exporter secret).
type Schedule = (Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>);

fn key_schedule(ss: &[u8], info: &[u8]) -> Result<Schedule, CryptoError> {
    let psk_id_hash = labeled_extract(b"", b"psk_id_hash", b"");
    let info_hash = labeled_extract(b"", b"info_hash", info);
    let mut ctx = vec![0u8]; // mode_base
    ctx.extend_from_slice(&psk_id_hash);
    ctx.extend_from_slice(&info_hash);
    let secret = labeled_extract(ss, b"secret", b"");
    Ok((
        labeled_expand(&secret, b"key", &ctx, 16)?,
        labeled_expand(&secret, b"base_nonce", &ctx, 12)?,
        labeled_expand(&secret, b"exp", &ctx, 32)?,
    ))
}

fn aead_alg(a: AeadType) -> &'static aead::Algorithm {
    match a {
        AeadType::Aes128Gcm => &aead::AES_128_GCM,
        AeadType::Aes256Gcm => &aead::AES_256_GCM,
        AeadType::ChaCha20Poly1305 => &aead::CHACHA20_POLY1305,
    }
}

fn seal(
    alg: &'static aead::Algorithm,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    pt: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let k =
        LessSafeKey::new(UnboundKey::new(alg, key).map_err(|_| CryptoError::CryptoLibraryError)?);
    let n = Nonce::try_assume_unique_for_key(nonce).map_err(|_| CryptoError::InvalidLength)?;
    let mut buf = pt.to_vec();
    k.seal_in_place_append_tag(n, Aad::from(aad), &mut buf)
        .map_err(|_| CryptoError::CryptoLibraryError)?;
    Ok(buf)
}

fn open(
    alg: &'static aead::Algorithm,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let k =
        LessSafeKey::new(UnboundKey::new(alg, key).map_err(|_| CryptoError::CryptoLibraryError)?);
    let n = Nonce::try_assume_unique_for_key(nonce).map_err(|_| CryptoError::InvalidLength)?;
    let mut buf = ct.to_vec();
    let len = k
        .open_in_place(n, Aad::from(aad), &mut buf)
        .map_err(|_| CryptoError::AeadDecryptionError)?
        .len();
    buf.truncate(len);
    Ok(buf)
}

impl OpenMlsCrypto for DeviceCrypto {
    fn supports(&self, ciphersuite: Ciphersuite) -> Result<(), CryptoError> {
        if ciphersuite == GROUP_CIPHERSUITE {
            Ok(())
        } else {
            Err(CryptoError::UnsupportedCiphersuite)
        }
    }

    fn supported_ciphersuites(&self) -> Vec<Ciphersuite> {
        vec![GROUP_CIPHERSUITE]
    }

    fn hkdf_extract(
        &self,
        hash_type: HashType,
        salt: &[u8],
        ikm: &[u8],
    ) -> Result<SecretVLBytes, CryptoError> {
        self.hmac(hash_type, salt, ikm)
    }

    fn hmac(
        &self,
        hash_type: HashType,
        key: &[u8],
        message: &[u8],
    ) -> Result<SecretVLBytes, CryptoError> {
        let key = hmac::Key::new(hmac_alg(hash_type), key);
        Ok(SecretVLBytes::new(
            hmac::sign(&key, message).as_ref().to_vec(),
        ))
    }

    fn hkdf_expand(
        &self,
        hash_type: HashType,
        prk: &[u8],
        info: &[u8],
        okm_len: usize,
    ) -> Result<SecretVLBytes, CryptoError> {
        if okm_len == 0 || okm_len > 255 * hash_type.size() {
            return Err(CryptoError::HkdfOutputLengthInvalid);
        }
        let prk = hkdf::Prk::new_less_safe(hkdf_alg(hash_type), prk);
        let info = [info];
        let okm = prk
            .expand(&info, Len(okm_len))
            .map_err(|_| CryptoError::HkdfOutputLengthInvalid)?;
        let mut out = vec![0u8; okm_len];
        okm.fill(&mut out)
            .map_err(|_| CryptoError::CryptoLibraryError)?;
        Ok(SecretVLBytes::new(out))
    }

    fn hash(&self, hash_type: HashType, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Ok(digest::digest(digest_alg(hash_type), data)
            .as_ref()
            .to_vec())
    }

    fn aead_encrypt(
        &self,
        alg: AeadType,
        key: &[u8],
        data: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        seal(aead_alg(alg), key, nonce, aad, data)
    }

    fn aead_decrypt(
        &self,
        alg: AeadType,
        key: &[u8],
        ct_tag: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        open(aead_alg(alg), key, nonce, aad, ct_tag)
    }

    fn signature_key_gen(&self, alg: SignatureScheme) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        if alg != SignatureScheme::ED25519 {
            return Err(CryptoError::UnsupportedSignatureScheme);
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        aws_lc_rs::rand::fill(seed.as_mut()).map_err(|_| CryptoError::InsufficientRandomness)?;
        let k = Ed25519KeyPair::from_seed_unchecked(seed.as_ref())
            .map_err(|_| CryptoError::CryptoLibraryError)?;
        Ok((seed.to_vec(), k.public_key().as_ref().to_vec()))
    }

    fn verify_signature(
        &self,
        alg: SignatureScheme,
        data: &[u8],
        pk: &[u8],
        signature: &[u8],
    ) -> Result<(), CryptoError> {
        if alg != SignatureScheme::ED25519 {
            return Err(CryptoError::UnsupportedSignatureScheme);
        }
        let key: &[u8; 32] = pk.try_into().map_err(|_| CryptoError::InvalidPublicKey)?;
        if !songbird_keys::ed25519_public_key_ok(key) {
            return Err(CryptoError::InvalidPublicKey);
        }
        SigKey::new(&ED25519, pk)
            .verify(data, signature)
            .map_err(|_| CryptoError::InvalidSignature)
    }

    fn sign(&self, alg: SignatureScheme, data: &[u8], key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if alg != SignatureScheme::ED25519 {
            return Err(CryptoError::UnsupportedSignatureScheme);
        }
        let k = Ed25519KeyPair::from_seed_unchecked(key).map_err(|_| CryptoError::SigningError)?;
        Ok(k.sign(data).as_ref().to_vec())
    }

    fn hpke_seal(
        &self,
        config: HpkeConfig,
        pk_r: &[u8],
        info: &[u8],
        aad: &[u8],
        ptxt: &[u8],
    ) -> Result<HpkeCiphertext, CryptoError> {
        check_config(config)?;
        let (enc, ss) = xwing_encaps(pk_r).map_err(|_| CryptoError::HpkeEncryptionError)?;
        let (key, nonce, _) = key_schedule(&ss, info)?;
        let ct = seal(&aead::AES_128_GCM, &key, &nonce, aad, ptxt)?;
        Ok(HpkeCiphertext {
            kem_output: enc.into(),
            ciphertext: ct.into(),
        })
    }

    fn hpke_open(
        &self,
        config: HpkeConfig,
        input: &HpkeCiphertext,
        sk_r: &[u8],
        info: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        check_config(config)?;
        let ss = xwing_decaps(sk_r, input.kem_output.as_slice())
            .map_err(|_| CryptoError::HpkeDecryptionError)?;
        let (key, nonce, _) = key_schedule(&ss, info)?;
        open(
            &aead::AES_128_GCM,
            &key,
            &nonce,
            aad,
            input.ciphertext.as_slice(),
        )
        .map_err(|_| CryptoError::HpkeDecryptionError)
    }

    fn hpke_setup_sender_and_export(
        &self,
        config: HpkeConfig,
        pk_r: &[u8],
        info: &[u8],
        exporter_context: &[u8],
        exporter_length: usize,
    ) -> Result<(KemOutput, ExporterSecret), CryptoError> {
        check_config(config)?;
        let (enc, ss) = xwing_encaps(pk_r).map_err(|_| CryptoError::SenderSetupError)?;
        let (_, _, exp) = key_schedule(&ss, info)?;
        let out = labeled_expand(&exp, b"sec", exporter_context, exporter_length)?;
        Ok((enc, ExporterSecret::from(out.to_vec())))
    }

    fn hpke_setup_receiver_and_export(
        &self,
        config: HpkeConfig,
        enc: &[u8],
        sk_r: &[u8],
        info: &[u8],
        exporter_context: &[u8],
        exporter_length: usize,
    ) -> Result<ExporterSecret, CryptoError> {
        check_config(config)?;
        let ss = xwing_decaps(sk_r, enc).map_err(|_| CryptoError::ReceiverSetupError)?;
        let (_, _, exp) = key_schedule(&ss, info)?;
        let out = labeled_expand(&exp, b"sec", exporter_context, exporter_length)?;
        Ok(ExporterSecret::from(out.to_vec()))
    }

    fn derive_hpke_keypair(
        &self,
        config: HpkeConfig,
        ikm: &[u8],
    ) -> Result<HpkeKeyPair, CryptoError> {
        check_config(config)?;
        // X-Wing's DeriveKeyPair: the seed is SHAKE256(ikm, 32) (draft-06 §5.3).
        let mut seed = Zeroizing::new([0u8; 32]);
        songbird_aws_lc_ext::shake256(ikm, seed.as_mut())
            .map_err(|_| CryptoError::CryptoLibraryError)?;
        let (sk, pk) = xwing_keypair(seed.as_ref())?;
        Ok(HpkeKeyPair {
            private: sk.into(),
            public: pk,
        })
    }
}

/// Random bytes from aws-lc-rs.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeviceRand;

impl OpenMlsRand for DeviceRand {
    type Error = CryptoError;

    fn random_array<const N: usize>(&self) -> Result<[u8; N], Self::Error> {
        let mut a = [0u8; N];
        aws_lc_rs::rand::fill(&mut a).map_err(|_| CryptoError::InsufficientRandomness)?;
        Ok(a)
    }

    fn random_vec(&self, len: usize) -> Result<Vec<u8>, Self::Error> {
        let mut v = vec![0u8; len];
        aws_lc_rs::rand::fill(&mut v).map_err(|_| CryptoError::InsufficientRandomness)?;
        Ok(v)
    }
}

/// The OpenMLS signer for this device's group signing key (an Ed25519 seed).
pub struct DeviceSigner {
    seed: Zeroizing<Vec<u8>>,
    public: Vec<u8>,
}

impl std::fmt::Debug for DeviceSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSigner").finish_non_exhaustive()
    }
}

impl DeviceSigner {
    /// A new signing key.
    pub fn generate() -> Result<Self, CryptoError> {
        let (seed, public) = DeviceCrypto.signature_key_gen(SignatureScheme::ED25519)?;
        Ok(Self {
            seed: Zeroizing::new(seed),
            public,
        })
    }

    /// From a stored seed.
    pub fn from_seed(seed: &[u8]) -> Result<Self, CryptoError> {
        let k = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| CryptoError::CryptoLibraryError)?;
        Ok(Self {
            seed: Zeroizing::new(seed.to_vec()),
            public: k.public_key().as_ref().to_vec(),
        })
    }

    /// The seed, for the encrypted store.
    #[must_use]
    pub fn seed(&self) -> &[u8] {
        &self.seed
    }

    /// The public key.
    #[must_use]
    pub fn public(&self) -> &[u8] {
        &self.public
    }
}

impl openmls_traits::signatures::Signer for DeviceSigner {
    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, openmls_traits::signatures::SignerError> {
        DeviceCrypto
            .sign(SignatureScheme::ED25519, payload, &self.seed)
            .map_err(|_| openmls_traits::signatures::SignerError::SigningError)
    }

    fn signature_scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    const CFG: HpkeConfig = HpkeConfig(
        HpkeKemType::XWingKemDraft6,
        HpkeKdfType::HkdfSha256,
        HpkeAeadType::AesGcm128,
    );

    #[test]
    fn hpke_round_trips_and_exports_agree() {
        let c = DeviceCrypto;
        let kp = c.derive_hpke_keypair(CFG, b"path secret").unwrap();
        assert_eq!(kp.public.len(), XWING_PK);
        assert_eq!(
            c.derive_hpke_keypair(CFG, b"path secret").unwrap().public,
            kp.public
        );
        let ct = c
            .hpke_seal(CFG, &kp.public, b"info", b"aad", b"welcome")
            .unwrap();
        assert_eq!(
            c.hpke_open(CFG, &ct, &kp.private, b"info", b"aad").unwrap(),
            b"welcome"
        );
        assert!(c.hpke_open(CFG, &ct, &kp.private, b"info", b"aae").is_err());
        let (enc, a) = c
            .hpke_setup_sender_and_export(CFG, &kp.public, b"i", b"ctx", 32)
            .unwrap();
        let b = c
            .hpke_setup_receiver_and_export(CFG, &enc, &kp.private, b"i", b"ctx", 32)
            .unwrap();
        assert_eq!(&*a, &*b);
    }

    #[test]
    fn signatures_and_aead_work() {
        let c = DeviceCrypto;
        let (sk, pk) = c.signature_key_gen(SignatureScheme::ED25519).unwrap();
        let sig = c.sign(SignatureScheme::ED25519, b"m", &sk).unwrap();
        c.verify_signature(SignatureScheme::ED25519, b"m", &pk, &sig)
            .unwrap();
        assert!(
            c.verify_signature(SignatureScheme::ED25519, b"n", &pk, &sig)
                .is_err()
        );
        let ct = c
            .aead_encrypt(AeadType::Aes128Gcm, &[1; 16], b"x", &[0; 12], b"a")
            .unwrap();
        assert_eq!(
            c.aead_decrypt(AeadType::Aes128Gcm, &[1; 16], &ct, &[0; 12], b"a")
                .unwrap(),
            b"x"
        );
    }
}
