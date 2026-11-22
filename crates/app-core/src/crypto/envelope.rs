//! The per-device envelope (D-0136): what one device's entry in a `Send` carries.
//!
//! The server's `Deliver` frame names no sender (PROTOCOL §1.2), and the server must not learn
//! more than the threat model allows, so the sender's account and device travel *inside* a layer
//! sealed to the recipient device's identity key, in the manner of sealed sender's outer layer:
//!
//! ```text
//! version u8 (1) | ephemeral [32] | AES-256-GCM(k, nonce 0, aad = version || ephemeral || recipient) of
//!     sender_account [16] | sender_device [16] | kind u8 | body
//!       kind 1 (prekey):  PQXDH initiation | ratchet message
//!       kind 2 (message): ratchet message
//! k = HKDF-SHA256(salt = "songbird.v1/envelope", ikm = DH(ephemeral, recipient), info = ephemeral || recipient)
//! ```
//!
//! The claimed sender is only a pointer: the ratchet's AEAD (whose associated data binds both
//! identity keys) authenticates it, and a prekey message's identity key must match the one the
//! sender's signed device list names (checked by [`crate::session`]). The version byte is never
//! `0xF5`, the prefix reserved for what the server writes.

use super::{Reader, X25519Pair, hkdf, open, seal};
use crate::{Error, Result};

/// Envelope version.
pub const VERSION: u8 = 1;
const SALT: &[u8] = b"songbird.v1/envelope";
/// Largest envelope the server accepts (PROTOCOL §1.3, `[delivery.limits]` default 64 KiB).
pub const MAX_LEN: usize = 64 * 1024;

/// What the envelope says inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inner {
    /// The sending account.
    pub account: [u8; 16],
    /// The sending device.
    pub device: [u8; 16],
    /// 1 prekey, 2 message.
    pub kind: Kind,
    /// The initiation (prekey) and ratchet message.
    pub body: Vec<u8>,
}

/// The kind of session message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Starts a session: a PQXDH initiation, then a ratchet message.
    Prekey = 1,
    /// A ratchet message in an established session.
    Message = 2,
}

fn key(
    dh: &[u8; 32],
    ephemeral: &[u8; 32],
    recipient: &[u8; 32],
) -> Result<zeroize::Zeroizing<[u8; 32]>> {
    let mut k = zeroize::Zeroizing::new([0u8; 32]);
    hkdf(SALT, dh, &[ephemeral, recipient], k.as_mut())?;
    Ok(k)
}

/// Seals `inner` to the recipient device's identity key.
pub fn seal_to(recipient: &[u8; 32], inner: &Inner) -> Result<Vec<u8>> {
    let e = X25519Pair::generate()?;
    let k = key(&*e.dh(recipient)?, &e.public(), recipient)?;
    let mut aad = vec![VERSION];
    aad.extend_from_slice(&e.public());
    aad.extend_from_slice(recipient);
    let mut pt = zeroize::Zeroizing::new(Vec::with_capacity(33 + inner.body.len()));
    pt.extend_from_slice(&inner.account);
    pt.extend_from_slice(&inner.device);
    pt.push(match inner.kind {
        Kind::Prekey => 1,
        Kind::Message => 2,
    });
    pt.extend_from_slice(&inner.body);
    let mut out = vec![VERSION];
    out.extend_from_slice(&e.public());
    out.extend_from_slice(&seal(&k, &[0; 12], &aad, &pt)?);
    if out.len() > MAX_LEN {
        return Err(Error::Malformed);
    }
    Ok(out)
}

/// Opens an envelope with this device's identity key.
pub fn open_with(identity: &X25519Pair, envelope: &[u8]) -> Result<Inner> {
    if envelope.len() > MAX_LEN {
        return Err(Error::Malformed);
    }
    let mut r = Reader::new(envelope);
    if r.u8()? != VERSION {
        return Err(Error::Malformed);
    }
    let eph: [u8; 32] = r.array()?;
    let me = identity.public();
    let k = key(&*identity.dh(&eph)?, &eph, &me)?;
    let mut aad = vec![VERSION];
    aad.extend_from_slice(&eph);
    aad.extend_from_slice(&me);
    let pt = zeroize::Zeroizing::new(open(&k, &[0; 12], &aad, r.rest())?);
    let mut r = Reader::new(&pt);
    let account = r.array()?;
    let device = r.array()?;
    let kind = match r.u8()? {
        1 => Kind::Prekey,
        2 => Kind::Message,
        _ => return Err(Error::Malformed),
    };
    Ok(Inner {
        account,
        device,
        kind,
        body: r.rest().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;

    #[test]
    fn round_trips_and_only_the_recipient_opens() {
        let b = X25519Pair::generate().unwrap();
        let inner = Inner {
            account: [1; 16],
            device: [2; 16],
            kind: Kind::Message,
            body: b"x".to_vec(),
        };
        let env = seal_to(&b.public(), &inner).unwrap();
        assert_ne!(env[0], 0xF5);
        assert_eq!(open_with(&b, &env).unwrap(), inner);
        assert!(open_with(&X25519Pair::generate().unwrap(), &env).is_err());
        let mut t = env.clone();
        t[40] ^= 1;
        assert!(open_with(&b, &t).is_err());
    }
}
