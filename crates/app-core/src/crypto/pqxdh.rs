//! PQXDH (Signal's "The PQXDH Key Agreement Protocol", revision 3), implemented from the
//! specification (D-0047), over the server's key formats (PROTOCOL §4.4).
//!
//! - Curve X25519, hash SHA-256, KEM ML-KEM-1024 (D-0050), info
//!   `"Songbird_PQXDH_X25519_SHA-256_ML-KEM-1024"`.
//! - `SK = HKDF(salt = 0^32, ikm = 0xFF^32 || DH1 || DH2 || DH3 || [DH4] || SS, info)`, with
//!   `DH1 = DH(IK_A, SPK_B)`, `DH2 = DH(EK_A, IK_B)`, `DH3 = DH(EK_A, SPK_B)`,
//!   `DH4 = DH(EK_A, OPK_B)` and `(CT, SS) = ML-KEM.Enc(PQPK_B)`.
//! - The initiator verifies both prekey signatures (XEdDSA by `IK_B` over the serialized key)
//!   before anything else, as the specification requires.
//! - The associated data is `Encode(IK_A) || Encode(IK_B) || SHA-256(PQPK_B || CT)`: the
//!   specification's two identities, plus a binding of the KEM public key and ciphertext (the
//!   re-encapsulation concern the specification's revision 2 discusses; harmless if redundant).

use zeroize::Zeroizing;

use super::{Kem, KemPair, X25519Pair, encapsulate, hkdf, parse_x25519, serialize_x25519, sha256};
use crate::{Error, Result};

const INFO: &[u8] = b"Songbird_PQXDH_X25519_SHA-256_ML-KEM-1024";

/// One device's bundle as `POST /v1/keys/fetch` answers it (PROTOCOL §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The device.
    pub device: [u8; 16],
    /// Its identity key (`u`).
    pub identity: [u8; 32],
    /// Signed prekey: id, key, signature.
    pub signed_id: u32,
    /// The signed prekey (`u`).
    pub signed: [u8; 32],
    /// Its XEdDSA signature over `0x05 || u`.
    pub signed_sig: [u8; 64],
    /// A one-time X25519 prekey, if one was left.
    pub one_time: Option<(u32, [u8; 32])>,
    /// The KEM prekey id.
    pub kem_id: u32,
    /// `0x0A || key` (1,569 bytes).
    pub kem: Vec<u8>,
    /// Its signature over those bytes.
    pub kem_sig: [u8; 64],
    /// Whether the KEM key is one-time (else last-resort).
    pub kem_one_time: bool,
}

impl Bundle {
    /// Parses a fetch answer: `version u8 (1) | n u8 | n × bundle`.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<Self>> {
        let mut r = super::Reader::new(bytes);
        if r.u8()? != 1 {
            return Err(Error::Malformed);
        }
        let n = r.u8()?;
        let mut out = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            let device = r.array()?;
            let identity = parse_x25519(r.take(33)?)?;
            let signed_id = r.u32()?;
            let signed = parse_x25519(r.take(33)?)?;
            let signed_sig = r.array()?;
            let flags = r.u8()?;
            if flags & !3 != 0 {
                return Err(Error::Malformed);
            }
            let one_time = if flags & 1 == 1 {
                Some((r.u32()?, parse_x25519(r.take(33)?)?))
            } else {
                None
            };
            let kem_id = r.u32()?;
            let kem = r.take(songbird_keys::kem::SERIALIZED_LEN)?.to_vec();
            let kem_sig = r.array()?;
            out.push(Self {
                device,
                identity,
                signed_id,
                signed,
                signed_sig,
                one_time,
                kem_id,
                kem,
                kem_sig,
                kem_one_time: flags & 2 == 2,
            });
        }
        r.finish()?;
        Ok(out)
    }

    /// Both prekey signatures verify under the identity key, and the KEM key is ML-KEM-1024.
    pub fn verify(&self) -> Result<()> {
        let ok = songbird_keys::xeddsa::verify(
            &self.identity,
            &serialize_x25519(&self.signed),
            &self.signed_sig,
        ) && songbird_keys::xeddsa::verify(&self.identity, &self.kem, &self.kem_sig)
            && self.kem.first() == Some(&songbird_keys::kem::TYPE_ML_KEM_1024);
        if ok { Ok(()) } else { Err(Error::Verify) }
    }
}

/// What the initiator sends in its first message so the responder can agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initiation {
    /// The initiator's identity key (`u`).
    pub identity: [u8; 32],
    /// Its ephemeral key (`u`).
    pub base: [u8; 32],
    /// Which signed prekey.
    pub signed_id: u32,
    /// Which KEM prekey.
    pub kem_id: u32,
    /// The KEM ciphertext (1,568 bytes).
    pub kem_ct: Vec<u8>,
    /// Which one-time prekey, if any.
    pub one_time_id: Option<u32>,
}

impl Initiation {
    /// Encoded: `identity [32] | base [32] | signed_id u32 | kem_id u32 | kem_ct [1568]
    /// | flags u8 (1 one-time) | [one_time_id u32]`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.identity);
        out.extend_from_slice(&self.base);
        out.extend_from_slice(&self.signed_id.to_be_bytes());
        out.extend_from_slice(&self.kem_id.to_be_bytes());
        out.extend_from_slice(&self.kem_ct);
        match self.one_time_id {
            Some(id) => {
                out.push(1);
                out.extend_from_slice(&id.to_be_bytes());
            }
            None => out.push(0),
        }
    }

    /// Decodes what [`Self::encode`] wrote.
    pub fn decode(r: &mut super::Reader<'_>) -> Result<Self> {
        let identity = r.array()?;
        let base = r.array()?;
        let signed_id = r.u32()?;
        let kem_id = r.u32()?;
        let kem_ct = r.take(Kem::MlKem1024.ciphertext_len())?.to_vec();
        let one_time_id = match r.u8()? {
            0 => None,
            1 => Some(r.u32()?),
            _ => return Err(Error::Malformed),
        };
        Ok(Self {
            identity,
            base,
            signed_id,
            kem_id,
            kem_ct,
            one_time_id,
        })
    }
}

/// The shared secret and associated data both sides derive.
#[derive(Debug)]
pub struct Agreement {
    /// `SK`.
    pub secret: Zeroizing<[u8; 32]>,
    /// `AD`.
    pub ad: Vec<u8>,
}

fn derive(dhs: &[&[u8; 32]], ss: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
    let mut ikm = Zeroizing::new(Vec::with_capacity(32 * 6));
    ikm.extend_from_slice(&[0xFF; 32]);
    for d in dhs {
        ikm.extend_from_slice(*d);
    }
    ikm.extend_from_slice(ss);
    let mut sk = Zeroizing::new([0u8; 32]);
    hkdf(&[0u8; 32], &ikm, &[INFO], sk.as_mut())?;
    Ok(sk)
}

fn ad(ik_a: &[u8; 32], ik_b: &[u8; 32], pqpk: &[u8], ct: &[u8]) -> Vec<u8> {
    let mut ad = Vec::with_capacity(33 * 2 + 32);
    ad.extend_from_slice(&serialize_x25519(ik_a));
    ad.extend_from_slice(&serialize_x25519(ik_b));
    ad.extend_from_slice(&sha256(&[pqpk, ct]));
    ad
}

/// The initiator's side. `bundle` must have been [`Bundle::verify`]-ed (this checks again).
/// Returns the agreement, the initiation to send, and the ephemeral key pair is discarded.
pub fn initiate(identity: &X25519Pair, bundle: &Bundle) -> Result<(Agreement, Initiation)> {
    bundle.verify()?;
    let ek = X25519Pair::generate()?;
    let dh1 = identity.dh(&bundle.signed)?;
    let dh2 = ek.dh(&bundle.identity)?;
    let dh3 = ek.dh(&bundle.signed)?;
    let dh4 = bundle.one_time.map(|(_, k)| ek.dh(&k)).transpose()?;
    let pqpk = bundle.kem.get(1..).ok_or(Error::Malformed)?;
    let (ct, ss) = encapsulate(Kem::MlKem1024, pqpk)?;
    let mut dhs: Vec<&[u8; 32]> = vec![&dh1, &dh2, &dh3];
    if let Some(d) = &dh4 {
        dhs.push(d);
    }
    let secret = derive(&dhs, &ss)?;
    let agreement = Agreement {
        secret,
        ad: ad(&identity.public(), &bundle.identity, pqpk, &ct),
    };
    Ok((
        agreement,
        Initiation {
            identity: identity.public(),
            base: ek.public(),
            signed_id: bundle.signed_id,
            kem_id: bundle.kem_id,
            kem_ct: ct,
            one_time_id: bundle.one_time.map(|(id, _)| id),
        },
    ))
}

/// The responder's side, with the private halves of the prekeys the initiation names.
pub fn respond(
    identity: &X25519Pair,
    signed: &X25519Pair,
    kem: &KemPair,
    one_time: Option<&X25519Pair>,
    init: &Initiation,
) -> Result<Agreement> {
    if kem.kem() != Kem::MlKem1024 || init.one_time_id.is_some() != one_time.is_some() {
        return Err(Error::UnknownPrekey);
    }
    let dh1 = signed.dh(&init.identity)?;
    let dh2 = identity.dh(&init.base)?;
    let dh3 = signed.dh(&init.base)?;
    let dh4 = one_time.map(|k| k.dh(&init.base)).transpose()?;
    let ss = kem.decapsulate(&init.kem_ct)?;
    let mut dhs: Vec<&[u8; 32]> = vec![&dh1, &dh2, &dh3];
    if let Some(d) = &dh4 {
        dhs.push(d);
    }
    Ok(Agreement {
        secret: derive(&dhs, &ss)?,
        ad: ad(
            &init.identity,
            &identity.public(),
            kem.public(),
            &init.kem_ct,
        ),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;

    /// A responder's keys and the bundle the server would serve for them.
    pub(crate) struct Responder {
        pub(crate) identity: X25519Pair,
        pub(crate) signed: X25519Pair,
        pub(crate) kem: KemPair,
        pub(crate) one_time: X25519Pair,
    }

    impl Responder {
        pub(crate) fn new() -> Self {
            Self {
                identity: X25519Pair::generate().unwrap(),
                signed: X25519Pair::generate().unwrap(),
                kem: KemPair::generate(Kem::MlKem1024).unwrap(),
                one_time: X25519Pair::generate().unwrap(),
            }
        }

        pub(crate) fn bundle(&self, with_one_time: bool) -> Bundle {
            let mut kem = vec![songbird_keys::kem::TYPE_ML_KEM_1024];
            kem.extend_from_slice(self.kem.public());
            Bundle {
                device: [7; 16],
                identity: self.identity.public(),
                signed_id: 1,
                signed: self.signed.public(),
                signed_sig: self
                    .identity
                    .sign(&serialize_x25519(&self.signed.public()))
                    .unwrap(),
                one_time: with_one_time.then(|| (9, self.one_time.public())),
                kem_id: 2,
                kem_sig: self.identity.sign(&kem).unwrap(),
                kem,
                kem_one_time: false,
            }
        }
    }

    #[test]
    #[allow(clippy::many_single_char_names, reason = "the protocol's names")]
    fn both_sides_agree_with_and_without_a_one_time_key() {
        for with in [true, false] {
            let b = Responder::new();
            let a = X25519Pair::generate().unwrap();
            let (x, init) = initiate(&a, &b.bundle(with)).unwrap();
            let y = respond(
                &b.identity,
                &b.signed,
                &b.kem,
                with.then_some(&b.one_time),
                &init,
            )
            .unwrap();
            assert_eq!(*x.secret, *y.secret);
            assert_eq!(x.ad, y.ad);
            let mut buf = Vec::new();
            init.encode(&mut buf);
            let mut r = crate::crypto::Reader::new(&buf);
            assert_eq!(Initiation::decode(&mut r).unwrap(), init);
            r.finish().unwrap();
        }
    }

    #[test]
    fn a_forged_prekey_signature_is_refused() {
        let b = Responder::new();
        let mut bundle = b.bundle(true);
        bundle.signed = X25519Pair::generate().unwrap().public();
        assert_eq!(
            initiate(&X25519Pair::generate().unwrap(), &bundle).unwrap_err(),
            Error::Verify
        );
        let mut bundle = b.bundle(true);
        bundle.kem_sig[0] ^= 1;
        assert!(initiate(&X25519Pair::generate().unwrap(), &bundle).is_err());
    }

    #[test]
    fn a_wrong_one_time_key_gives_another_secret() {
        let b = Responder::new();
        let a = X25519Pair::generate().unwrap();
        let (x, init) = initiate(&a, &b.bundle(true)).unwrap();
        let other = X25519Pair::generate().unwrap();
        let y = respond(&b.identity, &b.signed, &b.kem, Some(&other), &init).unwrap();
        assert_ne!(*x.secret, *y.secret);
    }

    #[test]
    fn fetch_answers_parse() {
        let b = Responder::new();
        let bundle = b.bundle(true);
        let mut bytes = vec![1, 1];
        bytes.extend_from_slice(&bundle.device);
        bytes.extend_from_slice(&serialize_x25519(&bundle.identity));
        bytes.extend_from_slice(&bundle.signed_id.to_be_bytes());
        bytes.extend_from_slice(&serialize_x25519(&bundle.signed));
        bytes.extend_from_slice(&bundle.signed_sig);
        bytes.push(1);
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(&serialize_x25519(&b.one_time.public()));
        bytes.extend_from_slice(&bundle.kem_id.to_be_bytes());
        bytes.extend_from_slice(&bundle.kem);
        bytes.extend_from_slice(&bundle.kem_sig);
        let parsed = Bundle::parse_all(&bytes).unwrap();
        assert_eq!(parsed, vec![bundle]);
        parsed[0].verify().unwrap();
        bytes.push(0);
        assert!(Bundle::parse_all(&bytes).is_err());
    }
}
