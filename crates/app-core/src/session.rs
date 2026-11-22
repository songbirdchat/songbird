//! Sessions with remote devices (D-0136): this device's identity and prekeys, one ratchet session
//! per remote device, and whole envelopes in and out.
//!
//! - **Prekeys** are generated here and uploaded in the server's format (PROTOCOL §4.4); one-time
//!   keys leave this store when a message uses them, the signed and last-resort keys are kept
//!   with their predecessor so messages begun against a rotated key still open.
//! - **Sessions**: sending uses the newest session with a device; receiving tries the newest,
//!   then up to [`ARCHIVED`] earlier ones (simultaneous starts and re-starts converge without
//!   losing messages). An initiator repeats its PQXDH initiation on every message until the peer
//!   answers, as libsignal does, so the first message lost does not strand the session.
//! - **Who sent**: [`Sessions::open`] returns the sealed layer's claim; the caller resolves the
//!   claimed device's identity key from the sender's signed device list and passes it to
//!   [`Sessions::decrypt`], which refuses a prekey message from any other identity and a session
//!   message that does not authenticate under the session's keys.

use std::collections::{BTreeMap, HashMap};

use zeroize::Zeroizing;

use crate::crypto::envelope::{self, Inner, Kind};
use crate::crypto::pqxdh::{self, Bundle, Initiation};
use crate::crypto::ratchet::State;
use crate::crypto::{Kem, KemPair, Reader, X25519Pair, serialize_x25519};
use crate::{Error, Result};

/// Earlier sessions kept per device for decryption.
pub const ARCHIVED: usize = 3;
/// One-time keys of each kind the server holds at most (PROTOCOL §4.4).
pub const MAX_ONE_TIME: usize = 200;
/// One-time keys per upload at most.
pub const MAX_PER_UPLOAD: usize = 100;

/// This device.
#[derive(Debug, Clone)]
pub struct Local {
    /// The account.
    pub account: [u8; 16],
    /// This device.
    pub device: [u8; 16],
    /// The device identity key (PQXDH's `IK`).
    pub identity: X25519Pair,
}

/// This device's prekeys.
#[derive(Debug, Default, Clone)]
pub struct Prekeys {
    next_id: u32,
    signed: Vec<(u32, X25519Pair)>,
    kem: Vec<(u32, KemPair)>,
    one_time: BTreeMap<u32, X25519Pair>,
    one_time_kem: BTreeMap<u32, KemPair>,
    uploaded_identity: bool,
}

impl Prekeys {
    fn id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    /// A new upload (PROTOCOL §4.4): the identity on the first one, a new signed and last-resort
    /// KEM prekey when `rotate` (always on the first), and `ec`/`kem` one-time keys (each at most
    /// [`MAX_PER_UPLOAD`]). The new private keys are kept; call it again after a failed upload.
    pub fn upload(
        &mut self,
        local: &Local,
        rotate: bool,
        ec: usize,
        kem: usize,
    ) -> Result<Vec<u8>> {
        let first = !self.uploaded_identity;
        let rotate = rotate || first || self.signed.is_empty();
        let ec = ec
            .min(MAX_PER_UPLOAD)
            .min(MAX_ONE_TIME.saturating_sub(self.one_time.len()));
        let kem = kem
            .min(MAX_PER_UPLOAD)
            .min(MAX_ONE_TIME.saturating_sub(self.one_time_kem.len()));
        let mut flags = 0u8;
        let mut o = vec![1, 0];
        if first {
            flags |= 1;
            o.extend_from_slice(&serialize_x25519(&local.identity.public()));
        }
        if rotate {
            flags |= 2 | 4;
            let id = self.id();
            let k = X25519Pair::generate()?;
            let ser = serialize_x25519(&k.public());
            o.extend_from_slice(&id.to_be_bytes());
            o.extend_from_slice(&ser);
            o.extend_from_slice(&local.identity.sign(&ser)?);
            self.signed.insert(0, (id, k));
            self.signed.truncate(2);
            let id = self.id();
            let k = KemPair::generate(Kem::MlKem1024)?;
            let ser = kem_serialized(&k);
            o.extend_from_slice(&id.to_be_bytes());
            o.extend_from_slice(&ser);
            o.extend_from_slice(&local.identity.sign(&ser)?);
            self.kem.insert(0, (id, k));
            self.kem.truncate(2);
        }
        if let Some(f) = o.get_mut(1) {
            *f = flags;
        }
        o.extend_from_slice(
            &u16::try_from(ec)
                .map_err(|_| Error::Malformed)?
                .to_be_bytes(),
        );
        for _ in 0..ec {
            let id = self.id();
            let k = X25519Pair::generate()?;
            o.extend_from_slice(&id.to_be_bytes());
            o.extend_from_slice(&serialize_x25519(&k.public()));
            self.one_time.insert(id, k);
        }
        o.extend_from_slice(
            &u16::try_from(kem)
                .map_err(|_| Error::Malformed)?
                .to_be_bytes(),
        );
        for _ in 0..kem {
            let id = self.id();
            let k = KemPair::generate(Kem::MlKem1024)?;
            let ser = kem_serialized(&k);
            o.extend_from_slice(&id.to_be_bytes());
            o.extend_from_slice(&ser);
            o.extend_from_slice(&local.identity.sign(&ser)?);
            self.one_time_kem.insert(id, k);
        }
        Ok(o)
    }

    /// The server stored the last upload (the identity is set once).
    pub fn uploaded(&mut self) {
        self.uploaded_identity = true;
    }

    /// One-time keys of each kind held.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.one_time.len(), self.one_time_kem.len())
    }

    fn signed(&self, id: u32) -> Option<&X25519Pair> {
        self.signed.iter().find(|(i, _)| *i == id).map(|(_, k)| k)
    }

    fn kem(&self, id: u32) -> Option<&KemPair> {
        self.kem
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, k)| k)
            .or_else(|| self.one_time_kem.get(&id))
    }

    /// Serialized for the encrypted store.
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        let mut o = Zeroizing::new(vec![1]);
        o.extend_from_slice(&self.next_id.to_be_bytes());
        o.push(u8::from(self.uploaded_identity));
        let put_ec =
            |o: &mut Vec<u8>, list: &mut dyn Iterator<Item = (&u32, &X25519Pair)>, n: usize| {
                o.extend_from_slice(&u32::try_from(n).unwrap_or(0).to_be_bytes());
                for (id, k) in list {
                    o.extend_from_slice(&id.to_be_bytes());
                    o.extend_from_slice(k.secret());
                }
            };
        put_ec(
            &mut o,
            &mut self.signed.iter().map(|(i, k)| (i, k)),
            self.signed.len(),
        );
        put_ec(&mut o, &mut self.one_time.iter(), self.one_time.len());
        for list in [
            self.kem.iter().map(|(i, k)| (*i, k)).collect::<Vec<_>>(),
            self.one_time_kem.iter().map(|(i, k)| (*i, k)).collect(),
        ] {
            o.extend_from_slice(
                &u32::try_from(list.len())
                    .map_err(|_| Error::Malformed)?
                    .to_be_bytes(),
            );
            for (id, k) in list {
                o.extend_from_slice(&id.to_be_bytes());
                crate::crypto::put_bytes32(&mut o, k.secret())?;
                crate::crypto::put_bytes32(&mut o, k.public())?;
            }
        }
        Ok(o)
    }

    /// Reads what [`Self::encode`] wrote.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != 1 {
            return Err(Error::Malformed);
        }
        let next_id = r.u32()?;
        let uploaded_identity = r.u8()? == 1;
        let ec = |r: &mut Reader<'_>, max: usize| -> Result<Vec<(u32, X25519Pair)>> {
            let n = usize::try_from(r.u32()?).map_err(|_| Error::Malformed)?;
            if n > max {
                return Err(Error::Malformed);
            }
            (0..n)
                .map(|_| Ok((r.u32()?, X25519Pair::from_secret(&r.array()?)?)))
                .collect()
        };
        let signed = ec(&mut r, 2)?;
        let one_time = ec(&mut r, MAX_ONE_TIME)?.into_iter().collect();
        let kems = |r: &mut Reader<'_>, max: usize| -> Result<Vec<(u32, KemPair)>> {
            let n = usize::try_from(r.u32()?).map_err(|_| Error::Malformed)?;
            if n > max {
                return Err(Error::Malformed);
            }
            (0..n)
                .map(|_| {
                    let id = r.u32()?;
                    let s = r.bytes32(4096)?.to_vec();
                    let p = r.bytes32(Kem::MlKem1024.public_len())?.to_vec();
                    Ok((id, KemPair::from_parts(Kem::MlKem1024, s, p)?))
                })
                .collect()
        };
        let kem = kems(&mut r, 2)?;
        let one_time_kem = kems(&mut r, MAX_ONE_TIME)?.into_iter().collect();
        r.finish()?;
        Ok(Self {
            next_id,
            signed,
            kem,
            one_time,
            one_time_kem,
            uploaded_identity,
        })
    }
}

fn kem_serialized(k: &KemPair) -> Vec<u8> {
    let mut v = vec![songbird_keys::kem::TYPE_ML_KEM_1024];
    v.extend_from_slice(k.public());
    v
}

/// A session with one remote device.
#[derive(Debug, Clone)]
pub struct Session {
    /// The remote device's identity key.
    pub remote_identity: [u8; 32],
    state: State,
    /// Our initiation, repeated until the peer answers.
    pending: Option<Initiation>,
    /// The initiation that created this session on the responder's side (to recognise repeats).
    started_by: Option<[u8; 32]>,
}

impl Session {
    /// Serialized for the encrypted store.
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        let mut o = Zeroizing::new(vec![1]);
        o.extend_from_slice(&self.remote_identity);
        match &self.pending {
            Some(i) => {
                let mut b = Vec::new();
                i.encode(&mut b);
                o.push(1);
                crate::crypto::put_bytes32(&mut o, &b)?;
            }
            None => o.push(0),
        }
        match &self.started_by {
            Some(b) => {
                o.push(1);
                o.extend_from_slice(b);
            }
            None => o.push(0),
        }
        crate::crypto::put_bytes32(&mut o, &self.state.encode()?)?;
        Ok(o)
    }

    /// Reads what [`Self::encode`] wrote.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != 1 {
            return Err(Error::Malformed);
        }
        let remote_identity = r.array()?;
        let pending = match r.u8()? {
            0 => None,
            1 => {
                let b = r.bytes32(4096)?;
                let mut ir = Reader::new(b);
                let i = Initiation::decode(&mut ir)?;
                ir.finish()?;
                Some(i)
            }
            _ => return Err(Error::Malformed),
        };
        let started_by = match r.u8()? {
            0 => None,
            1 => Some(r.array()?),
            _ => return Err(Error::Malformed),
        };
        let state = State::decode(r.bytes32(1 << 20)?)?;
        r.finish()?;
        Ok(Self {
            remote_identity,
            state,
            pending,
            started_by,
        })
    }
}

/// An address: (account, device).
pub type Address = ([u8; 16], [u8; 16]);

/// What a received envelope turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub struct Decrypted {
    /// Sender account.
    pub account: [u8; 16],
    /// Sender device.
    pub device: [u8; 16],
    /// The padded plaintext (PROTOCOL §5.1).
    pub plaintext: Zeroizing<Vec<u8>>,
    /// Whether this message started a new session.
    pub new_session: bool,
}

/// Every session of this device, newest first per remote device.
#[derive(Debug, Default, Clone)]
pub struct Sessions {
    map: HashMap<Address, Vec<Session>>,
}

impl Sessions {
    /// Whether there is a session with `to` we can send on.
    #[must_use]
    pub fn has(&self, to: &Address) -> bool {
        self.map
            .get(to)
            .and_then(|v| v.first())
            .is_some_and(|s| s.state.can_send())
    }

    /// The remote identity of the current session with `to`.
    #[must_use]
    pub fn identity(&self, to: &Address) -> Option<[u8; 32]> {
        self.map
            .get(to)
            .and_then(|v| v.first())
            .map(|s| s.remote_identity)
    }

    /// Every address with a session.
    pub fn addresses(&self) -> impl Iterator<Item = &Address> {
        self.map.keys()
    }

    /// Forget every session with `to` (a removed device).
    pub fn remove(&mut self, to: &Address) {
        self.map.remove(to);
    }

    /// The sessions with `to`, for the store.
    #[must_use]
    pub fn get(&self, to: &Address) -> Option<&[Session]> {
        self.map.get(to).map(Vec::as_slice)
    }

    /// Restores sessions read from the store.
    pub fn put(&mut self, to: Address, sessions: Vec<Session>) {
        self.map.insert(to, sessions);
    }

    /// Starts a session with a device from its fetched bundle (verifying both signatures).
    pub fn start(&mut self, local: &Local, account: [u8; 16], bundle: &Bundle) -> Result<()> {
        let (agreement, init) = pqxdh::initiate(&local.identity, bundle)?;
        let state = State::initiator(&agreement.secret, agreement.ad, bundle.signed)?;
        self.push(
            (account, bundle.device),
            Session {
                remote_identity: bundle.identity,
                state,
                pending: Some(init),
                started_by: None,
            },
        );
        Ok(())
    }

    fn push(&mut self, to: Address, s: Session) {
        let v = self.map.entry(to).or_default();
        v.insert(0, s);
        v.truncate(1 + ARCHIVED);
    }

    /// Encrypts `plaintext` for `to` (needs a session: [`Self::start`] it from a bundle first).
    pub fn encrypt(&mut self, local: &Local, to: &Address, plaintext: &[u8]) -> Result<Vec<u8>> {
        let s = self
            .map
            .get_mut(to)
            .and_then(|v| v.first_mut())
            .filter(|s| s.state.can_send())
            .ok_or(Error::NoSession)?;
        if s.state.peer_answered() {
            s.pending = None;
        }
        let msg = s.state.encrypt(plaintext)?;
        let (kind, body) = match &s.pending {
            Some(init) => {
                let mut b = Vec::with_capacity(1600 + msg.len());
                init.encode(&mut b);
                b.extend_from_slice(&msg);
                (Kind::Prekey, b)
            }
            None => (Kind::Message, msg),
        };
        envelope::seal_to(
            &s.remote_identity,
            &Inner {
                account: local.account,
                device: local.device,
                kind,
                body,
            },
        )
    }

    /// Opens the sealed layer: who claims to have sent it.
    pub fn open(local: &Local, envelope: &[u8]) -> Result<Inner> {
        envelope::open_with(&local.identity, envelope)
    }

    /// Decrypts an opened envelope. `sender_identity` is the identity key the sender's signed
    /// device list gives the claimed device; `None` when the device list has no such device
    /// (then only an existing session can authenticate it).
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the opened envelope is used up here"
    )]
    pub fn decrypt(
        &mut self,
        local: &Local,
        prekeys: &mut Prekeys,
        inner: Inner,
        sender_identity: Option<[u8; 32]>,
    ) -> Result<Decrypted> {
        let addr = (inner.account, inner.device);
        let (msg, init) = match inner.kind {
            Kind::Message => (inner.body.as_slice(), None),
            Kind::Prekey => {
                let mut r = Reader::new(&inner.body);
                let init = Initiation::decode(&mut r)?;
                (r.rest(), Some(init))
            }
        };
        if let Some(id) = sender_identity
            && init.as_ref().is_some_and(|i| i.identity != id)
        {
            return Err(Error::Verify);
        }
        // Existing sessions first (an initiation is repeated until answered).
        if let Some(list) = self.map.get_mut(&addr) {
            for i in 0..list.len() {
                let Some(s) = list.get_mut(i) else { break };
                if sender_identity.is_some_and(|id| id != s.remote_identity) {
                    continue;
                }
                if let Some(init) = &init
                    && s.started_by != Some(init.base)
                {
                    continue;
                }
                if let Ok(pt) = s.state.decrypt(msg) {
                    if i != 0 {
                        let s = list.remove(i);
                        list.insert(0, s);
                    }
                    return Ok(Decrypted {
                        account: inner.account,
                        device: inner.device,
                        plaintext: Zeroizing::new(pt),
                        new_session: false,
                    });
                }
            }
        }
        let Some(init) = init else {
            return Err(Error::NoSession);
        };
        let Some(identity) = sender_identity else {
            return Err(Error::Verify);
        };
        let signed = prekeys
            .signed(init.signed_id)
            .ok_or(Error::UnknownPrekey)?
            .clone();
        let kem = prekeys
            .kem(init.kem_id)
            .ok_or(Error::UnknownPrekey)?
            .clone();
        let one_time = match init.one_time_id {
            Some(id) => Some(
                prekeys
                    .one_time
                    .get(&id)
                    .ok_or(Error::UnknownPrekey)?
                    .clone(),
            ),
            None => None,
        };
        let agreement = pqxdh::respond(&local.identity, &signed, &kem, one_time.as_ref(), &init)?;
        let mut state = State::responder(&agreement.secret, agreement.ad, signed)?;
        let pt = state.decrypt(msg)?;
        // Only now, with the message authenticated, are one-time keys used up.
        if let Some(id) = init.one_time_id {
            prekeys.one_time.remove(&id);
        }
        prekeys.one_time_kem.remove(&init.kem_id);
        self.push(
            addr,
            Session {
                remote_identity: identity,
                state,
                pending: None,
                started_by: Some(init.base),
            },
        );
        Ok(Decrypted {
            account: inner.account,
            device: inner.device,
            plaintext: Zeroizing::new(pt),
            new_session: true,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;

    struct Dev {
        local: Local,
        prekeys: Prekeys,
        sessions: Sessions,
    }

    impl Dev {
        fn new(account: u8, device: u8) -> Self {
            Self {
                local: Local {
                    account: [account; 16],
                    device: [device; 16],
                    identity: X25519Pair::generate().unwrap(),
                },
                prekeys: Prekeys::default(),
                sessions: Sessions::default(),
            }
        }

        /// What the server would serve from this device's upload (taking one of each one-time key).
        fn bundle(&mut self) -> Bundle {
            let upload = self.prekeys.upload(&self.local, false, 2, 2).unwrap();
            self.prekeys.uploaded();
            let mut r = Reader::new(&upload);
            assert_eq!(r.u8().unwrap(), 1);
            let flags = r.u8().unwrap();
            assert_eq!(flags, 7);
            let identity = crate::crypto::parse_x25519(r.take(33).unwrap()).unwrap();
            let signed_id = r.u32().unwrap();
            let signed = crate::crypto::parse_x25519(r.take(33).unwrap()).unwrap();
            let signed_sig = r.array().unwrap();
            let _kem_id = r.u32().unwrap();
            let _ = r.take(1569 + 64).unwrap();
            assert_eq!(r.u16().unwrap(), 2);
            let ot_id = r.u32().unwrap();
            let ot = crate::crypto::parse_x25519(r.take(33).unwrap()).unwrap();
            let _ = r.take(37).unwrap();
            assert_eq!(r.u16().unwrap(), 2);
            let kem_id = r.u32().unwrap();
            let kem = r.take(1569).unwrap().to_vec();
            let kem_sig = r.array().unwrap();
            Bundle {
                device: self.local.device,
                identity,
                signed_id,
                signed,
                signed_sig,
                one_time: Some((ot_id, ot)),
                kem_id,
                kem,
                kem_sig,
                kem_one_time: true,
            }
        }

        fn send(&mut self, to: &Dev, text: &[u8]) -> Vec<u8> {
            self.sessions
                .encrypt(&self.local, &(to.local.account, to.local.device), text)
                .unwrap()
        }

        fn recv(&mut self, from: &Dev, env: &[u8]) -> Result<Decrypted> {
            let inner = Sessions::open(&self.local, env)?;
            assert_eq!(
                (inner.account, inner.device),
                (from.local.account, from.local.device)
            );
            self.sessions.decrypt(
                &self.local,
                &mut self.prekeys,
                inner,
                Some(from.local.identity.public()),
            )
        }
    }

    #[test]
    fn a_first_message_starts_a_session_and_one_time_keys_are_used_once() {
        let mut a = Dev::new(1, 1);
        let mut b = Dev::new(2, 2);
        let bundle = b.bundle();
        a.sessions
            .start(&a.local, b.local.account, &bundle)
            .unwrap();
        let e1 = a.send(&b, b"hi");
        let e2 = a.send(&b, b"again");
        assert_eq!(b.prekeys.counts(), (2, 2));
        let d = b.recv(&a, &e1).unwrap();
        assert!(d.new_session);
        assert_eq!(&*d.plaintext, b"hi");
        assert_eq!(b.prekeys.counts(), (1, 1));
        // The repeated initiation lands in the same session.
        let d = b.recv(&a, &e2).unwrap();
        assert!(!d.new_session);
        assert_eq!(&*d.plaintext, b"again");
        let r = b.send(&a, b"hello");
        assert_eq!(&*a.recv(&b, &r).unwrap().plaintext, b"hello");
        // a stops repeating its initiation once answered.
        let e3 = a.send(&b, b"3");
        assert_eq!(Sessions::open(&b.local, &e3).unwrap().kind, Kind::Message);
        assert_eq!(&*b.recv(&a, &e3).unwrap().plaintext, b"3");
    }

    #[test]
    fn a_prekey_message_from_another_identity_is_refused() {
        let mut a = Dev::new(1, 1);
        let mut b = Dev::new(2, 2);
        let bundle = b.bundle();
        a.sessions
            .start(&a.local, b.local.account, &bundle)
            .unwrap();
        let e = a.send(&b, b"hi");
        let inner = Sessions::open(&b.local, &e).unwrap();
        let other = X25519Pair::generate().unwrap().public();
        assert_eq!(
            b.sessions
                .decrypt(&b.local, &mut b.prekeys, inner, Some(other))
                .unwrap_err(),
            Error::Verify
        );
        assert_eq!(b.prekeys.counts(), (2, 2));
    }

    #[test]
    #[allow(clippy::many_single_char_names, reason = "two devices")]
    fn simultaneous_starts_converge() {
        let mut a = Dev::new(1, 1);
        let mut b = Dev::new(2, 2);
        let ba = a.bundle();
        let bb = b.bundle();
        a.sessions.start(&a.local, b.local.account, &bb).unwrap();
        b.sessions.start(&b.local, a.local.account, &ba).unwrap();
        let x = a.send(&b, b"from a");
        let y = b.send(&a, b"from b");
        assert_eq!(&*b.recv(&a, &x).unwrap().plaintext, b"from a");
        assert_eq!(&*a.recv(&b, &y).unwrap().plaintext, b"from b");
        for i in 0..3u8 {
            let m = a.send(&b, &[i]);
            assert_eq!(&*b.recv(&a, &m).unwrap().plaintext, &[i]);
            let m = b.send(&a, &[i]);
            assert_eq!(&*a.recv(&b, &m).unwrap().plaintext, &[i]);
        }
    }

    #[test]
    fn everything_survives_the_store() {
        let mut a = Dev::new(1, 1);
        let mut b = Dev::new(2, 2);
        let bundle = b.bundle();
        a.sessions
            .start(&a.local, b.local.account, &bundle)
            .unwrap();
        let e = a.send(&b, b"hi");
        b.prekeys = Prekeys::decode(&b.prekeys.encode().unwrap()).unwrap();
        b.recv(&a, &e).unwrap();
        let addr = (a.local.account, a.local.device);
        let stored: Vec<_> = b
            .sessions
            .get(&addr)
            .unwrap()
            .iter()
            .map(|s| s.encode().unwrap())
            .collect();
        let mut fresh = Sessions::default();
        fresh.put(
            addr,
            stored.iter().map(|s| Session::decode(s).unwrap()).collect(),
        );
        b.sessions = fresh;
        let r = b.send(&a, b"back");
        assert_eq!(&*a.recv(&b, &r).unwrap().plaintext, b"back");
    }
}
