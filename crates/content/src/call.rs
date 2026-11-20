//! Calls (Prompt 7, PROTOCOL §8, D-0086): the media key schedule and call signalling.
//!
//! LiveKit encrypts every frame and data packet end to end with keys its key provider is given
//! per participant identity and key index; no server component ever holds one. This module
//! derives them from Songbird's own sessions:
//!
//! ```text
//! 1:1    epoch secret = HKDF-SHA256(salt = call_id, ikm = call_secret,
//!                                   info = "songbird.v1/call-1to1\0", 32)      call_secret: the offer's
//! group  epoch secret = MLS-Exporter("songbird.v1/call", call_id, 32)          at the group's epoch
//! sender key          = HKDF-Expand(PRK = epoch secret,
//!                                   info = "songbird.v1/call-sender\0" ‖ u8(len) ‖ identity, 32)
//! key index           = epoch mod 16                                          (1:1: epoch 0)
//! ```
//!
//! Each sender's key is bound to its participant identity, so two senders never share a key and
//! LiveKit's per-frame IVs (SSRC, timestamp, counter) cannot collide across senders under one key.
//! A group's commit (any membership change) gives a new epoch, so a new secret no removed device
//! can derive, at the next key index.
//!
//! **Call links** (§8.4, D-0085 §2): the link's fragment holds a 32-byte root; the server gets
//! only `HKDF(root, "…call-link-id")` and stores its SHA-256 ([`link_id_hash`]). Participants
//! seal a [`CallLinkCard`] (who they are) under `HKDF(root, "…call-link-key")`, then send each
//! other their own random sender keys ([`crate::schema::CallSenderKey`]) over Signal sessions,
//! new on every join and leave.

use aws_lc_rs::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::{digest, hkdf};
use prost::Message as _;
use zeroize::Zeroizing;

use crate::schema::{CallLinkCard, CallMessage, call_message::Kind};
use crate::{Error, ID_LEN, KEY_LEN};

/// LiveKit's key ring: key indexes are `0..KEY_RING`.
pub const KEY_RING: u32 = 16;
/// Longest participant identity (the server's are 32 hex characters).
pub const MAX_IDENTITY: usize = 64;
/// Longest display name in a card.
pub const MAX_NAME: usize = 128;
/// Largest sealed card.
pub const MAX_SEALED_CARD: usize = 512;
/// The MLS exporter label of a group call's epoch secret (context: the call ID, length 32).
pub const GROUP_EXPORTER_LABEL: &str = "songbird.v1/call";
/// The MLS exporter's output length.
pub const GROUP_EXPORTER_LEN: usize = 32;
/// Longest ringing an offer may ask for, past its `sent_at_ms`.
pub const MAX_RING_MS: u64 = 120_000;

/// Domain separation of a 1:1 call's epoch secret.
pub const ONE_TO_ONE_CONTEXT: &[u8] = b"songbird.v1/call-1to1\0";
/// Domain separation of a sender key.
pub const SENDER_CONTEXT: &[u8] = b"songbird.v1/call-sender\0";
/// HKDF info of the verification code ([`EpochSecret::verification_code`]).
pub const CODE_CONTEXT: &[u8] = b"songbird.v1/call-code\0";
/// Domain separation of a link's ID.
pub const LINK_ID_CONTEXT: &[u8] = b"songbird.v1/call-link-id\0";
/// Domain separation of a link's card key.
pub const LINK_KEY_CONTEXT: &[u8] = b"songbird.v1/call-link-key\0";
/// Domain separation of the stored hash of a link's ID.
pub const LINK_HASH_CONTEXT: &[u8] = b"songbird.v1/call-link-hash\0";
/// Associated data of a sealed card.
pub const CARD_AAD: &[u8] = b"songbird.v1/call-link-card\0";
/// The sealed card's format version.
pub const CARD_VERSION: u8 = 1;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

struct Len32;

impl hkdf::KeyType for Len32 {
    fn len(&self) -> usize {
        KEY_LEN
    }
}

fn expand(prk: &hkdf::Prk, info: &[&[u8]]) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    prk.expand(info, Len32)
        .and_then(|okm| okm.fill(out.as_mut_slice()))
        .map_err(|_| Error::Invalid("call.hkdf"))?;
    Ok(out)
}

/// One key epoch's secret. Wiped on drop; `Debug` prints nothing of it.
pub struct EpochSecret {
    secret: Zeroizing<[u8; KEY_LEN]>,
    epoch: u64,
}

impl std::fmt::Debug for EpochSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpochSecret")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl EpochSecret {
    /// A 1:1 call's only epoch (0), from the offer's `call_secret`.
    ///
    /// # Errors
    /// Never in practice (HKDF-SHA256 with a 32-byte output cannot fail); kept for symmetry.
    pub fn one_to_one(call_id: &[u8; ID_LEN], call_secret: &[u8; KEY_LEN]) -> Result<Self, Error> {
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, call_id).extract(call_secret);
        Ok(Self {
            secret: expand(&prk, &[ONE_TO_ONE_CONTEXT])?,
            epoch: 0,
        })
    }

    /// A group call's secret at MLS epoch `epoch`: the app's
    /// `export_secret(GROUP_EXPORTER_LABEL, call_id, GROUP_EXPORTER_LEN)` at that epoch.
    #[must_use]
    pub fn group(exported: [u8; GROUP_EXPORTER_LEN], epoch: u64) -> Self {
        Self {
            secret: Zeroizing::new(exported),
            epoch,
        }
    }

    /// The epoch this secret belongs to.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The key index LiveKit is given for this epoch.
    #[must_use]
    pub fn key_index(&self) -> u32 {
        key_index(self.epoch)
    }

    /// The media key of the participant `identity` (as LiveKit shows it) in this epoch.
    ///
    /// # Errors
    /// [`Error::Invalid`] if the identity is empty or longer than [`MAX_IDENTITY`].
    pub fn sender_key(&self, identity: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
        let n = u8::try_from(identity.len())
            .ok()
            .filter(|n| (1..=MAX_IDENTITY).contains(&usize::from(*n)))
            .ok_or(Error::Invalid("call.identity"))?;
        let prk = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, self.secret.as_slice());
        expand(&prk, &[SENDER_CONTEXT, &[n], identity])
    }

    /// The call's verification code in this epoch: four symbols of 6 bits (indices into the
    /// apps' list of 64), from `HKDF-Expand(PRK = epoch secret, "songbird.v1/call-code\0", 32)`'s
    /// first 24 bits. Both sides show the same four only if they hold the same epoch secret,
    /// that is, if nobody stood between them when the secret was shared.
    #[must_use]
    pub fn verification_code(&self) -> [u8; 4] {
        let prk = hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, self.secret.as_slice());
        let okm = expand(&prk, &[CODE_CONTEXT]).unwrap_or_default();
        let bits = okm
            .iter()
            .take(3)
            .fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
        // Each symbol is 6 bits, so every value fits in a byte.
        std::array::from_fn(|i| u8::try_from((bits >> (18 - 6 * i)) & 63).unwrap_or(0))
    }
}

/// The key index of an epoch: `epoch mod 16`.
#[must_use]
pub fn key_index(epoch: u64) -> u32 {
    // The remainder is below 16, so it always fits.
    u32::try_from(epoch % u64::from(KEY_RING)).unwrap_or(0)
}

/// A call link's two secrets, from the root in its fragment. Wiped on drop.
pub struct LinkSecrets {
    /// Sent to the server, which keeps only [`link_id_hash`] of it.
    pub id: Zeroizing<[u8; KEY_LEN]>,
    /// Seals cards; never leaves the participants.
    pub key: Zeroizing<[u8; KEY_LEN]>,
}

impl std::fmt::Debug for LinkSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LinkSecrets { .. }")
    }
}

impl LinkSecrets {
    /// Derives both from the link's root.
    ///
    /// # Errors
    /// Never in practice (HKDF-SHA256 with a 32-byte output cannot fail).
    pub fn from_root(root: &[u8; KEY_LEN]) -> Result<Self, Error> {
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[]).extract(root);
        Ok(Self {
            id: expand(&prk, &[LINK_ID_CONTEXT])?,
            key: expand(&prk, &[LINK_KEY_CONTEXT])?,
        })
    }
}

/// What the server stores for a link ID: `SHA-256("songbird.v1/call-link-hash\0" ‖ id)`.
#[must_use]
pub fn link_id_hash(id: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(LINK_HASH_CONTEXT);
    ctx.update(id);
    let mut out = [0u8; KEY_LEN];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

fn card_key(key: &[u8; KEY_LEN]) -> Result<LessSafeKey, Error> {
    UnboundKey::new(&aead::CHACHA20_POLY1305, key)
        .map(LessSafeKey::new)
        .map_err(|_| Error::Invalid("call.card_key"))
}

/// Checks a card's fields.
///
/// # Errors
/// [`Error::Invalid`] naming the field.
pub fn validate_card(c: &CallLinkCard) -> Result<(), Error> {
    check(c.account.len() == ID_LEN, "card.account")?;
    check(c.device.len() == ID_LEN, "card.device")?;
    identity(&c.identity, "card.identity")?;
    check(c.name.len() <= MAX_NAME, "card.name")
}

/// Seals a card under the link's key with the given nonce:
/// `version u8 (1) | nonce [12] | ChaCha20-Poly1305(card, aad = "songbird.v1/call-link-card\0")`.
/// Apps use [`seal_card`], which draws a random nonce; this form exists for test vectors.
///
/// # Errors
/// [`Error::Invalid`] for a card out of bounds; [`Error::TooLarge`] past [`MAX_SEALED_CARD`].
pub fn seal_card_with_nonce(
    key: &[u8; KEY_LEN],
    card: &CallLinkCard,
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, Error> {
    validate_card(card)?;
    let mut body = Zeroizing::new(card.encode_to_vec());
    if 1 + NONCE_LEN + body.len() + TAG_LEN > MAX_SEALED_CARD {
        return Err(Error::TooLarge);
    }
    card_key(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(CARD_AAD),
            &mut *body,
        )
        .map_err(|_| Error::Invalid("call.card_seal"))?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + body.len());
    out.push(CARD_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Seals a card under the link's key with a random nonce.
///
/// # Errors
/// As [`seal_card_with_nonce`]; [`Error::Invalid`] if the system's random source fails.
pub fn seal_card(key: &[u8; KEY_LEN], card: &CallLinkCard) -> Result<Vec<u8>, Error> {
    let nonce = aws_lc_rs::rand::generate::<[u8; NONCE_LEN]>(&aws_lc_rs::rand::SystemRandom::new())
        .map_err(|_| Error::Invalid("call.random"))?
        .expose();
    seal_card_with_nonce(key, card, nonce)
}

/// Opens a sealed card: bounded first, authenticated, decoded, validated.
///
/// # Errors
/// [`Error::TooLarge`], [`Error::Decode`] (wrong version, key or bytes), [`Error::Invalid`].
pub fn open_card(key: &[u8; KEY_LEN], sealed: &[u8]) -> Result<CallLinkCard, Error> {
    if sealed.len() > MAX_SEALED_CARD {
        return Err(Error::TooLarge);
    }
    let Some((&CARD_VERSION, rest)) = sealed.split_first() else {
        return Err(Error::Decode);
    };
    let (nonce, ct) = rest.split_at_checked(NONCE_LEN).ok_or(Error::Decode)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| Error::Decode)?;
    let mut buf = Zeroizing::new(ct.to_vec());
    let plain = card_key(key)?
        .open_in_place(nonce, Aad::from(CARD_AAD), &mut buf)
        .map_err(|_| Error::Decode)?;
    let card = CallLinkCard::decode(&*plain).map_err(|_| Error::Decode)?;
    validate_card(&card)?;
    Ok(card)
}

fn check(ok: bool, field: &'static str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(field))
    }
}

fn identity(s: &str, field: &'static str) -> Result<(), Error> {
    check(!s.is_empty() && s.len() <= MAX_IDENTITY, field)
}

/// Checks a [`CallMessage`] (called by [`crate::validate::content`]). An unknown kind (a newer
/// writer's) is accepted, as everywhere in the schema.
///
/// # Errors
/// [`Error::Invalid`] naming the first field out of bounds.
pub fn validate(c: &CallMessage) -> Result<(), Error> {
    check(c.call_id.len() == ID_LEN, "call.call_id")?;
    match &c.kind {
        None | Some(Kind::Answer(_) | Kind::Decline(_) | Kind::Busy(_)) => Ok(()),
        Some(Kind::Offer(o)) => {
            check(o.media != 0, "call.offer.media")?;
            check(
                (o.call_secret.len() == KEY_LEN && o.group_id.is_empty())
                    || (o.call_secret.is_empty() && o.group_id.len() == ID_LEN),
                "call.offer.secret",
            )?;
            check(o.ring_until_ms != 0, "call.offer.ring_until_ms")
        }
        Some(Kind::Hangup(h)) => check(h.reason != 0, "call.hangup.reason"),
        Some(Kind::SenderKey(k)) => {
            identity(&k.identity, "call.sender_key.identity")?;
            check(k.key_index < KEY_RING, "call.sender_key.key_index")?;
            check(k.key.len() == KEY_LEN, "call.sender_key.key")
        }
    }
}

/// How long an app rings for an offer: until `ring_until_ms`, but never past `sent_at_ms` plus
/// [`MAX_RING_MS`], and not at all once that is before `now_ms`. Returns the milliseconds left.
#[must_use]
pub fn ring_left_ms(sent_at_ms: u64, ring_until_ms: u64, now_ms: u64) -> u64 {
    // A sender clock in the future counts as now (D-0091): it cannot stretch the ringing.
    ring_until_ms
        .min(sent_at_ms.min(now_ms).saturating_add(MAX_RING_MS))
        .saturating_sub(now_ms)
        .min(MAX_RING_MS)
}
