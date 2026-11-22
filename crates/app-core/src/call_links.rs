//! Call links on the device (PROTOCOL §8.4, §8.7, D-0153).
//!
//! - **The link** is a random 32-byte root in the fragment of `https://songbird.chat/call#…`. The
//!   server gets only the link ID derived from it (and keeps a hash of that); cards are sealed
//!   under the link key, also derived from it, which never leaves the participants.
//! - **The creator** keeps the admin token in the encrypted store: it skips the lobby, sees who
//!   waits (opening their cards) and admits or turns them away, and can revoke the link.
//! - **Joining** asks for a ticket; with a lobby, a sealed card waits for an admin and the device
//!   polls its place until admitted or turned away.
//! - **Keys** (Signal's design): every participant draws its own random sender key and sends it
//!   to each other participant's device over a Signal session (`CallMessage.sender_key`), new on
//!   every join and leave. The session's envelope travels on the room's data channel
//!   ([`KEY_TOPIC`]), not through the server: the message lanes would refuse strangers, and a
//!   link call is mostly strangers. The media crate installs what arrives.
//! - **The data channel** of a link call is not encrypted by LiveKit (its packets would be under
//!   sender keys a newcomer does not have yet); everything sent there is sealed here instead:
//!   cards and other data under the link key ([`seal_link_data`]), keys in Signal sessions.
//!
//! A card in the lobby is made before the device has a participant identity (the ticket names
//! it), so it carries [`LOBBY_IDENTITY`] there; the card sent in the room carries the real one.

use songbird_content::call::{self as content_call, LinkSecrets, open_card, seal_card};
use songbird_content::schema::{CallLinkCard, CallSenderKey, call_message};
use zeroize::Zeroizing;

use crate::calls::Ticket;
use crate::crypto::random;
use crate::messenger::{Messenger, Net, hex, json, now_ms, unhex};
use crate::store::StoredCallLink;
use crate::{Error, Result};

/// The identity a lobby card carries (the device has none until it is let in).
pub const LOBBY_IDENTITY: &str = "lobby";
/// The data-channel topic of a participant's sealed card in a link call's room.
pub const CARD_TOPIC: &str = "songbird.v1/call-card";
/// The data-channel topic of a sender key's Signal envelope in a link call's room.
pub const KEY_TOPIC: &str = "songbird.v1/call-key";
/// The data-channel topic of a raised hand in a link call (sealed, [`seal_link_data`]).
pub const HAND_TOPIC: &str = "songbird.v1/call-hand";
/// Associated data of other sealed link data.
const DATA_AAD: &[u8] = b"songbird.v1/call-link-data\0";
/// How long a new link lasts (the server's default, a day).
pub const LINK_TTL_SECS: u64 = 86_400;

/// What asking to join a link gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkJoin {
    /// In: the room's ticket.
    Ticket(Ticket),
    /// Waiting in the lobby at this place (poll it with [`Messenger::poll_lobby`]).
    Waiting([u8; 16]),
}

/// Someone waiting in a link's lobby, as their card says (unverified: anyone holding the link
/// can write any name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    /// Their place in the lobby.
    pub handle: [u8; 16],
    /// The account their card names.
    pub account: [u8; 16],
    /// The name their card gives (may be empty).
    pub name: String,
}

/// A participant's card, opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub account: [u8; 16],
    pub device: [u8; 16],
    pub identity: String,
    pub name: String,
}

fn secrets(root: &[u8; 32]) -> Result<LinkSecrets> {
    LinkSecrets::from_root(root).map_err(|_| Error::Malformed)
}

/// Opens a participant's sealed card with the link's key.
pub fn open_link_card(root: &[u8; 32], sealed: &[u8]) -> Result<Card> {
    let s = secrets(root)?;
    let c = open_card(&s.key, sealed).map_err(|_| Error::Verify)?;
    Ok(Card {
        account: c
            .account
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed)?,
        device: c
            .device
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed)?,
        identity: c.identity,
        name: c.name,
    })
}

/// Seals data for a link call's room (a raised hand) under the link key:
/// `nonce [12] | ChaCha20-Poly1305(data, aad = "songbird.v1/call-link-data\0")`.
pub fn seal_link_data(root: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
    let s = secrets(root)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, &*s.key).map_err(|_| Error::Malformed)?,
    );
    let nonce = random::<12>()?;
    let mut body = data.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(*nonce),
        Aad::from(DATA_AAD),
        &mut body,
    )
    .map_err(|_| Error::Malformed)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

/// Opens what [`seal_link_data`] sealed.
pub fn open_link_data(root: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>> {
    use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
    let s = secrets(root)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&CHACHA20_POLY1305, &*s.key).map_err(|_| Error::Malformed)?,
    );
    let (nonce, body) = sealed.split_first_chunk::<12>().ok_or(Error::Malformed)?;
    let mut body = body.to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(*nonce),
            Aad::from(DATA_AAD),
            &mut body,
        )
        .map_err(|_| Error::Verify)?;
    Ok(plain.to_vec())
}

impl<N: Net> Messenger<N> {
    /// Makes a call link: a fresh root and admin token, registered with the server (which keeps
    /// a hash of the link ID and of the token), and kept in the encrypted store.
    pub async fn create_call_link(
        &mut self,
        name: &str,
        video: bool,
        lobby: bool,
    ) -> Result<StoredCallLink> {
        if name.len() > content_call::MAX_NAME {
            return Err(Error::Malformed);
        }
        let root = Zeroizing::new(*random::<32>()?);
        let admin = Zeroizing::new(*random::<32>()?);
        let s = secrets(&root)?;
        let body = serde_json::json!({
            "link": hex(&*s.id),
            "admin": hex(&*admin),
            "media": if video { "video" } else { "audio" },
            "lobby": lobby,
            "ttl_secs": LINK_TTL_SECS,
        });
        let (status, _) = self
            .net
            .api("POST", "/v1/calls/links", body.to_string().into_bytes())
            .await?;
        if status != 204 {
            return Err(Error::Server(status));
        }
        let link = StoredCallLink {
            root,
            admin,
            name: name.to_owned(),
            video,
            lobby,
            expires_ms: now_ms().saturating_add(LINK_TTL_SECS * 1000),
        };
        self.store.put_call_link(&link)?;
        Ok(link)
    }

    /// The call links this device made and still holds.
    pub fn call_links(&self) -> Result<Vec<StoredCallLink>> {
        self.store.call_links(now_ms())
    }

    /// The admin token of a link this device made.
    fn admin_of(&self, root: &[u8; 32]) -> Result<Option<Zeroizing<[u8; 32]>>> {
        Ok(self
            .call_links()?
            .into_iter()
            .find(|l| *l.root == *root)
            .map(|l| l.admin))
    }

    /// Whether this device made (and administers) the link.
    pub fn is_link_admin(&self, root: &[u8; 32]) -> bool {
        self.admin_of(root).ok().flatten().is_some()
    }

    /// This device's card for a link, sealed: who it is, as `identity`, named `name`.
    pub fn link_card(&self, root: &[u8; 32], identity: &str, name: &str) -> Result<Vec<u8>> {
        let s = secrets(root)?;
        let card = CallLinkCard {
            account: self.local.account.to_vec(),
            device: self.local.device.to_vec(),
            identity: identity.to_owned(),
            name: name.chars().take(64).collect(),
        };
        seal_card(&s.key, &card).map_err(|_| Error::Malformed)
    }

    async fn link_api(&mut self, path: &str, body: serde_json::Value) -> Result<(u16, Vec<u8>)> {
        self.net
            .api("POST", path, body.to_string().into_bytes())
            .await
    }

    /// Asks to join a link's call: a ticket (the admin, or no lobby), or a place in the lobby
    /// with this device's card (`name` is what admins see).
    pub async fn join_call_link(&mut self, root: &[u8; 32], name: &str) -> Result<LinkJoin> {
        let s = secrets(root)?;
        let mut body = serde_json::json!({ "link": hex(&*s.id) });
        if let (Some(admin), Some(o)) = (self.admin_of(root)?, body.as_object_mut()) {
            o.insert("admin".into(), hex(&*admin).into());
        } else if let Some(o) = body.as_object_mut() {
            o.insert(
                "card".into(),
                hex(&self.link_card(root, LOBBY_IDENTITY, name)?).into(),
            );
        }
        let (status, answer) = self.link_api("/v1/calls/links/join", body).await?;
        joined(status, &answer)
    }

    /// Polls a place in a link's lobby: `None` while waiting, a ticket once admitted;
    /// `Error::Server(404)` once turned away (or the link is gone).
    pub async fn poll_lobby(
        &mut self,
        root: &[u8; 32],
        place: &[u8; 16],
    ) -> Result<Option<Ticket>> {
        let s = secrets(root)?;
        let body = serde_json::json!({ "link": hex(&*s.id), "lobby": hex(place) });
        let (status, answer) = self.link_api("/v1/calls/links/join", body).await?;
        match joined(status, &answer)? {
            LinkJoin::Ticket(t) => Ok(Some(t)),
            LinkJoin::Waiting(_) => Ok(None),
        }
    }

    /// Who waits in the lobby of a link this device administers. Cards that do not open are
    /// left out (nobody can admit what they cannot read).
    pub async fn lobby_waiting(&mut self, root: &[u8; 32]) -> Result<Vec<Waiting>> {
        let s = secrets(root)?;
        let admin = self.admin_of(root)?.ok_or(Error::Verify)?;
        let body = serde_json::json!({ "link": hex(&*s.id), "admin": hex(&*admin) });
        let (status, answer) = self.link_api("/v1/calls/links/lobby", body).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let v = json(&answer)?;
        let mut out = Vec::new();
        for w in v
            .get("waiting")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let field = |k: &str| {
                w.get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            };
            let Ok(handle) = unhex::<16>(field("handle")) else {
                continue;
            };
            let Ok(sealed) = crate::messenger::unhex_vec(field("card")) else {
                continue;
            };
            if let Ok(card) = open_link_card(root, &sealed) {
                out.push(Waiting {
                    handle,
                    account: card.account,
                    name: card.name,
                });
            }
        }
        Ok(out)
    }

    /// Lets someone in from the lobby, or turns them away.
    pub async fn admit(&mut self, root: &[u8; 32], handle: &[u8; 16], let_in: bool) -> Result<()> {
        let s = secrets(root)?;
        let admin = self.admin_of(root)?.ok_or(Error::Verify)?;
        let body = serde_json::json!({
            "link": hex(&*s.id), "admin": hex(&*admin), "handle": hex(handle), "admit": let_in,
        });
        match self.link_api("/v1/calls/links/admit", body).await?.0 {
            204 => Ok(()),
            s => Err(Error::Server(s)),
        }
    }

    /// Revokes a link this device made: the server deletes it and its lobby and ends its call
    /// for everyone; this device forgets it.
    pub async fn revoke_call_link(&mut self, root: &[u8; 32]) -> Result<()> {
        let s = secrets(root)?;
        let admin = self.admin_of(root)?.ok_or(Error::Verify)?;
        let body = serde_json::json!({ "link": hex(&*s.id), "admin": hex(&*admin) });
        match self.link_api("/v1/calls/links/revoke", body).await?.0 {
            204 | 404 => self.store.delete_call_link(root),
            s => Err(Error::Server(s)),
        }
    }

    /// This device's sender key for a link call, sealed for one participant's device in a
    /// Signal session: the envelope that device's [`Messenger::receive`] opens (as
    /// [`crate::calls::Signal::SenderKey`], naming this device). It travels on the room's data
    /// channel, never through the server's message lanes, so strangers in a link call need no
    /// contact proof to exchange keys.
    pub async fn link_key_envelope(
        &mut self,
        account: &[u8; 16],
        device: &[u8; 16],
        call: &[u8; 16],
        identity: &str,
        index: u32,
        key: &[u8; 32],
    ) -> Result<Vec<u8>> {
        let kind = call_message::Kind::SenderKey(CallSenderKey {
            identity: identity.to_owned(),
            key_index: index,
            key: key.to_vec(),
        });
        let content = crate::calls::call_content(call, kind)?;
        let (padded, _) = songbird_content::seal(&content, None).map_err(|_| Error::Malformed)?;
        let mut devices = self.devices_of(account, false).await?;
        if !devices.iter().any(|(d, _)| d == device) {
            devices = self.devices_of(account, true).await?;
        }
        devices.retain(|(d, _)| d == device);
        if devices.is_empty() {
            return Err(Error::Verify);
        }
        self.ensure_sessions(account, &devices).await?;
        let sealed = self
            .sessions
            .encrypt(&self.local, &(*account, *device), &padded)?;
        self.save_sessions_of(account, &devices)?;
        Ok(sealed)
    }
}

fn joined(status: u16, answer: &[u8]) -> Result<LinkJoin> {
    match status {
        200 => Ok(LinkJoin::Ticket(crate::calls::ticket(&json(answer)?)?)),
        202 => {
            let v = json(answer)?;
            let place = v
                .get("lobby")
                .and_then(serde_json::Value::as_str)
                .ok_or(Error::Malformed)?;
            Ok(LinkJoin::Waiting(unhex(place)?))
        }
        s => Err(Error::Server(s)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn cards_open_only_with_their_links_key() {
        let root = [3u8; 32];
        let s = secrets(&root).unwrap();
        let card = CallLinkCard {
            account: vec![1; 16],
            device: vec![2; 16],
            identity: "abc".into(),
            name: "Ana".into(),
        };
        let sealed = seal_card(&s.key, &card).unwrap();
        let opened = open_link_card(&root, &sealed).unwrap();
        assert_eq!((opened.account, opened.name.as_str()), ([1; 16], "Ana"));
        assert!(open_link_card(&[4; 32], &sealed).is_err());
    }

    #[test]
    fn link_data_opens_only_with_its_key() {
        let sealed = seal_link_data(&[5; 32], &[1]).unwrap();
        assert_eq!(open_link_data(&[5; 32], &sealed).unwrap(), vec![1]);
        assert!(open_link_data(&[6; 32], &sealed).is_err());
        let mut bad = sealed.clone();
        bad[13] ^= 1;
        assert!(open_link_data(&[5; 32], &bad).is_err());
    }

    #[test]
    fn join_answers_are_read_strictly() {
        assert_eq!(
            joined(202, br#"{"lobby":"0102030405060708090a0b0c0d0e0f10"}"#).unwrap(),
            LinkJoin::Waiting([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        );
        assert_eq!(joined(404, b"").unwrap_err(), Error::Server(404));
        assert!(joined(202, b"{}").is_err());
    }
}
