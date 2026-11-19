//! The Double Ratchet (Signal's specification, revision 4), implemented from the specification
//! (D-0047), with a post-quantum step on every DH ratchet step (D-0136).
//!
//! # Classical part (as specified)
//!
//! - `KDF_RK(rk, in) = HKDF(salt = rk, ikm = in, info)` → 64 bytes: the new root key, a chain key.
//! - `KDF_CK(ck)`: message key `HMAC(ck, 0x01)`, next chain key `HMAC(ck, 0x02)`.
//! - Message encryption: `HKDF(salt = 0^32, ikm = mk, info = "Songbird_MessageKeys")` → an
//!   AES-256-GCM key and nonce (one key per message, so a fixed derivation is a unique nonce); the
//!   associated data is the session's `AD` (PQXDH) followed by the encoded header.
//! - Skipped message keys are kept, at most [`MAX_SKIP`] per chain step and [`MAX_STORED_SKIPPED`]
//!   per session, oldest dropped first.
//! - Initialisation from PQXDH: the responder's signed prekey is its first ratchet key.
//!
//! # Post-quantum step (Songbird's, D-0136)
//!
//! Every sending chain carries a fresh ML-KEM-768 encapsulation key of its sender, and, from the
//! responder's first chain on, a ciphertext encapsulated to the newest key the peer advertised.
//! The KEM secret is mixed into the root step with the DH output:
//! `KDF_RK(rk, DH || SS)` (info `"Songbird_Ratchet_PQ"`), so recovery after a compromise needs
//! both X25519 and ML-KEM broken (the same aim as Apple's PQ3 and Signal's SPQR; this is a
//! simpler construction, and it is recorded for formal modelling before release, D-0136). The
//! KEM fields ride on every message of a chain, as a chain ends when the peer replies, so a lost
//! first message never strands the chain. A header without a ciphertext is accepted only for the
//! initiator's first chain (the one PQXDH's own KEM already protects); stripping one later fails
//! decryption, since the header is authenticated.

use std::collections::VecDeque;

use zeroize::{Zeroize, Zeroizing};

use super::{Kem, KemPair, Reader, X25519Pair, encapsulate, hkdf, hmac, open, put_bytes32, seal};
use crate::{Error, Result};

/// Most message keys skipped in one chain step.
pub const MAX_SKIP: u32 = 1000;
/// Most skipped message keys kept per session.
pub const MAX_STORED_SKIPPED: usize = 2000;

const INFO_RK: &[u8] = b"Songbird_Ratchet";
const INFO_RK_PQ: &[u8] = b"Songbird_Ratchet_PQ";
const INFO_MK: &[u8] = b"Songbird_MessageKeys";
const KEM: Kem = Kem::MlKem768;

/// A message header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The sender's current ratchet key.
    pub dh: [u8; 32],
    /// Messages in the sender's previous sending chain.
    pub pn: u32,
    /// This message's number in its chain.
    pub n: u32,
    /// The sender's KEM key for this chain.
    pub kem_public: Vec<u8>,
    /// A KEM ciphertext to the peer's newest key, absent only in the initiator's first chain.
    pub kem_ct: Option<Vec<u8>>,
}

impl Header {
    /// `dh [32] | pn u32 | n u32 | flags u8 (1 ciphertext) | kem_public [1184] | [kem_ct [1088]]`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.dh);
        out.extend_from_slice(&self.pn.to_be_bytes());
        out.extend_from_slice(&self.n.to_be_bytes());
        out.push(u8::from(self.kem_ct.is_some()));
        out.extend_from_slice(&self.kem_public);
        if let Some(ct) = &self.kem_ct {
            out.extend_from_slice(ct);
        }
    }

    /// Decodes a header from the front of `r`.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let dh = r.array()?;
        let pn = r.u32()?;
        let n = r.u32()?;
        let flags = r.u8()?;
        if flags > 1 {
            return Err(Error::Malformed);
        }
        let kem_public = r.take(KEM.public_len())?.to_vec();
        let kem_ct = if flags == 1 {
            Some(r.take(KEM.ciphertext_len())?.to_vec())
        } else {
            None
        };
        Ok(Self {
            dh,
            pn,
            n,
            kem_public,
            kem_ct,
        })
    }
}

/// One session's ratchet state.
#[derive(Clone)]
pub struct State {
    rk: Zeroizing<[u8; 32]>,
    cks: Option<Zeroizing<[u8; 32]>>,
    ckr: Option<Zeroizing<[u8; 32]>>,
    dhs: X25519Pair,
    dhr: Option<[u8; 32]>,
    ns: u32,
    nr: u32,
    pn: u32,
    /// Our KEM key for the current sending chain, and the previous one (a peer may answer the
    /// older one if our newer chain has not reached it yet).
    kem_own: KemPair,
    kem_prev: Option<KemPair>,
    /// The ciphertext for the peer that our current sending chain carries.
    kem_ct: Option<Vec<u8>>,
    /// Whether the next receiving chain must carry a KEM ciphertext.
    expect_kem: bool,
    ad: Vec<u8>,
    skipped: VecDeque<([u8; 32], u32, Zeroizing<[u8; 32]>)>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("ns", &self.ns)
            .field("nr", &self.nr)
            .finish_non_exhaustive()
    }
}

/// Two 32-byte secrets.
type KeyPair32 = (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>);

fn split64(out: &[u8; 64]) -> KeyPair32 {
    let (x, y) = out.split_at(32);
    let mut a = Zeroizing::new([0u8; 32]);
    let mut b = Zeroizing::new([0u8; 32]);
    a.copy_from_slice(x);
    b.copy_from_slice(y);
    (a, b)
}

fn kdf_rk(rk: &[u8; 32], dh: &[u8; 32], ss: Option<&[u8; 32]>) -> Result<KeyPair32> {
    let mut ikm = Zeroizing::new(Vec::with_capacity(64));
    ikm.extend_from_slice(dh);
    let info = if let Some(s) = ss {
        ikm.extend_from_slice(s);
        INFO_RK_PQ
    } else {
        INFO_RK
    };
    let mut out = Zeroizing::new([0u8; 64]);
    hkdf(rk, &ikm, &[info], out.as_mut())?;
    Ok(split64(&out))
}

fn kdf_ck(ck: &[u8; 32]) -> KeyPair32 {
    (hmac(ck, &[&[2]]), hmac(ck, &[&[1]]))
}

fn message_key(mk: &[u8; 32]) -> Result<(Zeroizing<[u8; 32]>, [u8; 12])> {
    let mut out = Zeroizing::new([0u8; 44]);
    hkdf(&[0u8; 32], mk, &[INFO_MK], out.as_mut())?;
    let (k, n) = out.split_at(32);
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(k);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(n);
    Ok((key, nonce))
}

impl State {
    /// The initiator, after PQXDH: `their_ratchet` is the responder's signed prekey.
    pub fn initiator(sk: &[u8; 32], ad: Vec<u8>, their_ratchet: [u8; 32]) -> Result<Self> {
        let dhs = X25519Pair::generate()?;
        let (rk, cks) = kdf_rk(sk, &*dhs.dh(&their_ratchet)?, None)?;
        Ok(Self {
            rk,
            cks: Some(cks),
            ckr: None,
            dhs,
            dhr: Some(their_ratchet),
            ns: 0,
            nr: 0,
            pn: 0,
            kem_own: KemPair::generate(KEM)?,
            kem_prev: None,
            kem_ct: None,
            expect_kem: true,
            ad,
            skipped: VecDeque::new(),
        })
    }

    /// The responder, after PQXDH: `signed` is its signed prekey pair.
    pub fn responder(sk: &[u8; 32], ad: Vec<u8>, signed: X25519Pair) -> Result<Self> {
        let mut rk = Zeroizing::new([0u8; 32]);
        rk.copy_from_slice(sk);
        Ok(Self {
            rk,
            cks: None,
            ckr: None,
            dhs: signed,
            dhr: None,
            ns: 0,
            nr: 0,
            pn: 0,
            kem_own: KemPair::generate(KEM)?,
            kem_prev: None,
            kem_ct: None,
            expect_kem: false,
            ad,
            skipped: VecDeque::new(),
        })
    }

    /// Whether this side can send yet (a responder can only after the first message).
    #[must_use]
    pub fn can_send(&self) -> bool {
        self.cks.is_some()
    }

    /// Whether the peer has ever answered in this session (a chain of theirs arrived after ours
    /// began). Until then an initiator keeps sending its PQXDH initiation with each message.
    #[must_use]
    pub fn peer_answered(&self) -> bool {
        self.ckr.is_some()
    }

    /// Encrypts `plaintext`: the encoded header, then the ciphertext.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cks = self.cks.as_ref().ok_or(Error::NoSession)?;
        let (next, mk) = kdf_ck(cks);
        let header = Header {
            dh: self.dhs.public(),
            pn: self.pn,
            n: self.ns,
            kem_public: self.kem_own.public().to_vec(),
            kem_ct: self.kem_ct.clone(),
        };
        let mut out = Vec::with_capacity(32 + 9 + 2272 + plaintext.len() + 16);
        header.encode(&mut out);
        let (key, nonce) = message_key(&mk)?;
        let mut aad = self.ad.clone();
        aad.extend_from_slice(&out);
        out.extend_from_slice(&seal(&key, &nonce, &aad, plaintext)?);
        self.cks = Some(next);
        self.ns = self.ns.checked_add(1).ok_or(Error::TooFarAhead)?;
        Ok(out)
    }

    /// Decrypts a message. The state changes only if decryption succeeds (a forged or replayed
    /// message leaves it as it was).
    pub fn decrypt(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(message);
        let header = Header::decode(&mut r)?;
        let header_len = message.len() - r.rest().len();
        let (header_bytes, body) = message.split_at(header_len);
        let mut aad = self.ad.clone();
        aad.extend_from_slice(header_bytes);

        // A skipped key?
        if let Some(i) = self
            .skipped
            .iter()
            .position(|(dh, n, _)| *dh == header.dh && *n == header.n)
        {
            let mk = self
                .skipped
                .get(i)
                .map(|(_, _, mk)| mk.clone())
                .ok_or(Error::Malformed)?;
            let (key, nonce) = message_key(&mk)?;
            let pt = open(&key, &nonce, &aad, body)?;
            self.skipped.remove(i);
            return Ok(pt);
        }

        let mut next = self.clone();
        if Some(header.dh) != next.dhr {
            next.skip(header.pn)?;
            next.dh_step(&header)?;
        }
        next.skip(header.n)?;
        let ckr = next.ckr.as_ref().ok_or(Error::NoSession)?;
        if header.n < next.nr {
            return Err(Error::Duplicate);
        }
        let (ck, mk) = kdf_ck(ckr);
        let (key, nonce) = message_key(&mk)?;
        let pt = open(&key, &nonce, &aad, body)?;
        next.ckr = Some(ck);
        next.nr = header.n + 1;
        *self = next;
        Ok(pt)
    }

    fn skip(&mut self, until: u32) -> Result<()> {
        let Some(ckr) = self.ckr.clone() else {
            return Ok(());
        };
        if until > self.nr.saturating_add(MAX_SKIP) {
            return Err(Error::TooFarAhead);
        }
        let dhr = self.dhr.ok_or(Error::NoSession)?;
        let mut ck = ckr;
        while self.nr < until {
            let (next, mk) = kdf_ck(&ck);
            self.skipped.push_back((dhr, self.nr, mk));
            if self.skipped.len() > MAX_STORED_SKIPPED {
                self.skipped.pop_front();
            }
            ck = next;
            self.nr += 1;
        }
        self.ckr = Some(ck);
        Ok(())
    }

    fn dh_step(&mut self, header: &Header) -> Result<()> {
        // Receiving chain: our current ratchet key with theirs, plus their KEM ciphertext to us.
        let ss_in = match &header.kem_ct {
            Some(ct) => Some(
                self.kem_own
                    .decapsulate(ct)
                    .or_else(|e| self.kem_prev.as_ref().map_or(Err(e), |k| k.decapsulate(ct)))?,
            ),
            None if self.expect_kem => return Err(Error::Verify),
            None => None,
        };
        self.pn = self.ns;
        self.ns = 0;
        self.nr = 0;
        self.dhr = Some(header.dh);
        let (rk, ckr) = kdf_rk(&self.rk, &*self.dhs.dh(&header.dh)?, ss_in.as_deref())?;
        // Sending chain: a new ratchet key, and a new KEM key of ours; encapsulate to theirs.
        let dhs = X25519Pair::generate()?;
        let (ct, ss_out) = encapsulate(KEM, &header.kem_public)?;
        let (rk, cks) = kdf_rk(&rk, &*dhs.dh(&header.dh)?, Some(&ss_out))?;
        let new_kem = KemPair::generate(KEM)?;
        self.kem_prev = Some(std::mem::replace(&mut self.kem_own, new_kem));
        self.kem_ct = Some(ct);
        self.dhs = dhs;
        self.rk = rk;
        self.ckr = Some(ckr);
        self.cks = Some(cks);
        self.expect_kem = true;
        Ok(())
    }

    /// Serialized for the encrypted store. Format version 1; every field length-checked on read.
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        let mut o = Zeroizing::new(Vec::with_capacity(8192));
        o.push(1);
        o.extend_from_slice(self.rk.as_ref());
        put_opt32(&mut o, self.cks.as_deref());
        put_opt32(&mut o, self.ckr.as_deref());
        o.extend_from_slice(self.dhs.secret());
        put_opt32(&mut o, self.dhr.as_ref());
        o.extend_from_slice(&self.ns.to_be_bytes());
        o.extend_from_slice(&self.nr.to_be_bytes());
        o.extend_from_slice(&self.pn.to_be_bytes());
        put_bytes32(&mut o, self.kem_own.secret())?;
        put_bytes32(&mut o, self.kem_own.public())?;
        match &self.kem_prev {
            Some(k) => {
                o.push(1);
                put_bytes32(&mut o, k.secret())?;
                put_bytes32(&mut o, k.public())?;
            }
            None => o.push(0),
        }
        match &self.kem_ct {
            Some(ct) => {
                o.push(1);
                put_bytes32(&mut o, ct)?;
            }
            None => o.push(0),
        }
        o.push(u8::from(self.expect_kem));
        put_bytes32(&mut o, &self.ad)?;
        o.extend_from_slice(
            &u32::try_from(self.skipped.len())
                .map_err(|_| Error::Malformed)?
                .to_be_bytes(),
        );
        for (dh, n, mk) in &self.skipped {
            o.extend_from_slice(dh);
            o.extend_from_slice(&n.to_be_bytes());
            o.extend_from_slice(mk.as_ref());
        }
        Ok(o)
    }

    /// Reads what [`Self::encode`] wrote.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != 1 {
            return Err(Error::Malformed);
        }
        let rk = Zeroizing::new(r.array()?);
        let cks = get_opt32(&mut r)?.map(Zeroizing::new);
        let ckr = get_opt32(&mut r)?.map(Zeroizing::new);
        let mut dhs_secret = r.array::<32>()?;
        let dhs = X25519Pair::from_secret(&dhs_secret);
        dhs_secret.zeroize();
        let dhs = dhs?;
        let dhr = get_opt32(&mut r)?;
        let ns = r.u32()?;
        let nr = r.u32()?;
        let pn = r.u32()?;
        let kem_own = read_kem(&mut r)?;
        let kem_prev = match r.u8()? {
            0 => None,
            1 => Some(read_kem(&mut r)?),
            _ => return Err(Error::Malformed),
        };
        let kem_ct = match r.u8()? {
            0 => None,
            1 => Some(r.bytes32(KEM.ciphertext_len())?.to_vec()),
            _ => return Err(Error::Malformed),
        };
        let expect_kem = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Malformed),
        };
        let ad = r.bytes32(256)?.to_vec();
        let n = usize::try_from(r.u32()?).map_err(|_| Error::Malformed)?;
        if n > MAX_STORED_SKIPPED {
            return Err(Error::Malformed);
        }
        let mut skipped = VecDeque::with_capacity(n);
        for _ in 0..n {
            skipped.push_back((r.array()?, r.u32()?, Zeroizing::new(r.array()?)));
        }
        r.finish()?;
        Ok(Self {
            rk,
            cks,
            ckr,
            dhs,
            dhr,
            ns,
            nr,
            pn,
            kem_own,
            kem_prev,
            kem_ct,
            expect_kem,
            ad,
            skipped,
        })
    }
}

fn put_opt32(o: &mut Vec<u8>, v: Option<&[u8; 32]>) {
    match v {
        Some(b) => {
            o.push(1);
            o.extend_from_slice(b);
        }
        None => o.push(0),
    }
}

fn get_opt32(r: &mut Reader<'_>) -> Result<Option<[u8; 32]>> {
    match r.u8()? {
        0 => Ok(None),
        1 => Ok(Some(r.array()?)),
        _ => Err(Error::Malformed),
    }
}

fn read_kem(r: &mut Reader<'_>) -> Result<KemPair> {
    let secret = r.bytes32(4096)?.to_vec();
    let public = r.bytes32(KEM.public_len())?.to_vec();
    KemPair::from_parts(KEM, secret, public)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use proptest::prelude::*;

    use super::*;

    fn pair() -> (State, State) {
        let sk = [5u8; 32];
        let spk = X25519Pair::generate().unwrap();
        let a = State::initiator(&sk, b"ad".to_vec(), spk.public()).unwrap();
        let b = State::responder(&sk, b"ad".to_vec(), spk).unwrap();
        (a, b)
    }

    #[test]
    fn a_conversation_round_trips_both_ways() {
        let (mut a, mut b) = pair();
        assert!(!b.can_send());
        for round in 0..5u8 {
            for i in 0..3u8 {
                let c = a.encrypt(&[round, i]).unwrap();
                assert_eq!(b.decrypt(&c).unwrap(), [round, i]);
            }
            let c = b.encrypt(&[round]).unwrap();
            assert_eq!(a.decrypt(&c).unwrap(), [round]);
        }
        assert!(a.peer_answered());
    }

    #[test]
    fn out_of_order_and_lost_messages_decrypt_once() {
        let (mut a, mut b) = pair();
        let m: Vec<_> = (0..6u8).map(|i| a.encrypt(&[i]).unwrap()).collect();
        assert_eq!(b.decrypt(&m[3]).unwrap(), [3]);
        assert_eq!(b.decrypt(&m[0]).unwrap(), [0]);
        assert_eq!(b.decrypt(&m[0]).unwrap_err(), Error::Duplicate);
        let reply = b.encrypt(b"r").unwrap();
        assert_eq!(a.decrypt(&reply).unwrap(), b"r");
        // A message from a's previous chain still opens after b ratcheted.
        assert_eq!(b.decrypt(&m[5]).unwrap(), [5]);
        let next = a.encrypt(b"n").unwrap();
        assert_eq!(b.decrypt(&next).unwrap(), b"n");
        assert_eq!(b.decrypt(&m[1]).unwrap(), [1]);
    }

    #[test]
    fn tampering_leaves_the_state_unchanged() {
        let (mut a, mut b) = pair();
        let c = a.encrypt(b"x").unwrap();
        for i in [0, 40, c.len() - 1] {
            let mut t = c.clone();
            t[i] ^= 1;
            assert!(b.decrypt(&t).is_err());
        }
        assert_eq!(b.decrypt(&c).unwrap(), b"x");
    }

    #[test]
    fn stripping_the_kem_ciphertext_is_refused() {
        let (mut a, mut b) = pair();
        b.decrypt(&a.encrypt(b"1").unwrap()).unwrap();
        let reply = b.encrypt(b"2").unwrap();
        let mut r = Reader::new(&reply);
        let mut h = Header::decode(&mut r).unwrap();
        let body = r.rest();
        assert!(h.kem_ct.is_some());
        h.kem_ct = None;
        let mut forged = Vec::new();
        h.encode(&mut forged);
        forged.extend_from_slice(body);
        assert_eq!(a.decrypt(&forged).unwrap_err(), Error::Verify);
        assert_eq!(a.decrypt(&reply).unwrap(), b"2");
    }

    #[test]
    fn too_far_ahead_is_refused() {
        let (mut a, mut b) = pair();
        b.decrypt(&a.encrypt(b"0").unwrap()).unwrap();
        let mut last = Vec::new();
        for _ in 0..=MAX_SKIP + 1 {
            last = a.encrypt(b"x").unwrap();
        }
        assert_eq!(b.decrypt(&last).unwrap_err(), Error::TooFarAhead);
    }

    #[test]
    fn state_survives_the_store() {
        let (mut a, mut b) = pair();
        let m: Vec<_> = (0..3u8).map(|i| a.encrypt(&[i]).unwrap()).collect();
        b.decrypt(&m[2]).unwrap();
        let mut b = State::decode(&b.encode().unwrap()).unwrap();
        assert_eq!(b.decrypt(&m[0]).unwrap(), [0]);
        let r = b.encrypt(b"r").unwrap();
        let mut a = State::decode(&a.encode().unwrap()).unwrap();
        assert_eq!(a.decrypt(&r).unwrap(), b"r");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        /// Any interleaving of sends, with every message delivered in any order, decrypts exactly.
        #[test]
        fn any_schedule_decrypts(script in proptest::collection::vec((any::<bool>(), any::<u8>()), 1..24), seed in any::<u64>()) {
            let (mut a, mut b) = pair();
            // b cannot send until a's first message arrives.
            let first = a.encrypt(b"start").unwrap();
            prop_assert_eq!(b.decrypt(&first).unwrap(), b"start");
            let mut to_b = Vec::new();
            let mut to_a = Vec::new();
            for (from_a, v) in &script {
                if *from_a { to_b.push((a.encrypt(&[*v]).unwrap(), *v)); }
                else { to_a.push((b.encrypt(&[*v]).unwrap(), *v)); }
            }
            let mut s = seed;
            let mut shuffle = |v: &mut Vec<(Vec<u8>, u8)>| {
                for i in (1..v.len()).rev() {
                    s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    let j = usize::try_from(s >> 33).unwrap() % (i + 1);
                    v.swap(i, j);
                }
            };
            shuffle(&mut to_b);
            shuffle(&mut to_a);
            for (c, v) in to_b { prop_assert_eq!(b.decrypt(&c).unwrap(), vec![v]); }
            for (c, v) in to_a { prop_assert_eq!(a.decrypt(&c).unwrap(), vec![v]); }
        }
    }
}
