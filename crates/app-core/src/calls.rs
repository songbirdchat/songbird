//! Calls (A2, D-0150): tickets, encrypted signalling and the key schedule (PROTOCOL §8). Media
//! runs on LiveKit through the app's media crate; this module decides who rings whom, and with
//! which keys, and never hands a key to any server.
//!
//! - **Tickets** (§8.6): `POST /v1/calls` (1:1, presenting the callee's access key so the server
//!   may wake them), `/v1/calls/group`, `/v1/calls/join`.
//! - **Signalling** (§8.5): `Content.call` sealed per device like any message (1:1, with
//!   `EXPIRES` 60 s so a missed offer never lingers) or as an MLS message (groups).
//! - **Keys** (§8.2): a 1:1 call's epoch secret from the offer's `call_secret`; a group call's from
//!   the MLS exporter at the current epoch; each participant's sender key from it and its
//!   participant identity (`songbird_content::call`).

use openmls::prelude::*;
use songbird_content::call::EpochSecret;
use songbird_content::schema::{
    CallAnswer, CallDecline, CallHangup, CallMessage, CallOffer, Content, call_event, call_hangup,
    call_message, content::Body,
};
use songbird_proto::frame::{SendHints, SendStatus};
use zeroize::Zeroizing;

use crate::crypto::random;
use crate::messenger::{Messenger, Net, hex, json, now_ms, unhex};
use crate::{Error, Result};

/// How long an offer rings (the protocol's maximum, two minutes).
pub const RING_MS: u64 = 120_000;

/// What a device needs to join a call's room.
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket {
    /// The call.
    pub call: [u8; 16],
    /// LiveKit's URL.
    pub url: String,
    /// LiveKit's token (two minutes to connect).
    pub token: String,
    /// This device's participant identity in the room.
    pub identity: String,
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ticket")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// A call signal that arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Someone calls: ring until `ring_until_ms`.
    Offer {
        /// Video (else audio).
        video: bool,
        /// A 1:1 call's secret (empty for a group call).
        secret: Vec<u8>,
        /// A group call's group.
        group: Option<[u8; 16]>,
        /// Ring until (this device's clock).
        ring_until_ms: u64,
    },
    /// Answered (by the callee, or on another of this account's devices: stop ringing).
    Answer,
    /// Declined.
    Decline,
    /// Busy.
    Busy,
    /// Hung up.
    Hangup,
}

fn ticket(v: &serde_json::Value) -> Result<Ticket> {
    let s = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or(Error::Malformed)
    };
    Ok(Ticket {
        call: unhex(&s("call")?)?,
        url: s("url")?,
        token: s("token")?,
        identity: s("identity")?,
    })
}

impl<N: Net> Messenger<N> {
    async fn call_api(&mut self, path: &str, body: serde_json::Value) -> Result<Ticket> {
        let (status, answer) = self
            .net
            .api("POST", path, body.to_string().into_bytes())
            .await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        ticket(&json(&answer)?)
    }

    /// Calls a contact: a ticket for the room (and wake-ups for their devices), then the
    /// encrypted offer. Answers the ticket and the call's secret (for the key schedule).
    pub async fn start_call(
        &mut self,
        callee: &[u8; 16],
        video: bool,
    ) -> Result<(Ticket, Zeroizing<[u8; 32]>)> {
        let access = self
            .store
            .profile_key(callee)?
            .map(|k| hex(&songbird_keys::sealed::access_key(&k)));
        let mut body = serde_json::json!({
            "callee": hex(callee),
            "media": if video { "video" } else { "audio" },
            "ring": access.is_some(),
        });
        if let (Some(a), Some(o)) = (access, body.as_object_mut()) {
            o.insert("access_key".into(), serde_json::Value::String(a));
        }
        let t = self.call_api("/v1/calls", body).await?;
        let secret = random::<32>()?;
        let offer = CallOffer {
            media: if video {
                call_event::Kind::Video
            } else {
                call_event::Kind::Audio
            }
            .into(),
            call_secret: secret.to_vec(),
            group_id: Vec::new(),
            ring_until_ms: now_ms() + RING_MS,
        };
        self.signal(callee, &t.call, call_message::Kind::Offer(offer))
            .await?;
        Ok((t, secret))
    }

    /// Starts (or joins) a group's call and rings the group.
    pub async fn start_group_call(&mut self, gid: &[u8; 16], video: bool) -> Result<Ticket> {
        let t = self
            .call_api(
                "/v1/calls/group",
                serde_json::json!({ "group": hex(gid), "media": if video { "video" } else { "audio" }, "ring": true }),
            )
            .await?;
        let offer = CallOffer {
            media: if video {
                call_event::Kind::Video
            } else {
                call_event::Kind::Audio
            }
            .into(),
            call_secret: Vec::new(),
            group_id: gid.to_vec(),
            ring_until_ms: now_ms() + RING_MS,
        };
        let content = call_content(&t.call, call_message::Kind::Offer(offer))?;
        self.send_group_body(gid, content).await?;
        Ok(t)
    }

    /// Answers a call: a ticket for its room, and the answer to the caller and this account's
    /// other devices (which stop ringing).
    pub async fn answer_call(
        &mut self,
        caller: &[u8; 16],
        call: &[u8; 16],
        group: Option<&[u8; 16]>,
    ) -> Result<Ticket> {
        let mut body = serde_json::json!({ "call": hex(call) });
        if let (Some(g), Some(o)) = (group, body.as_object_mut()) {
            o.insert("group".into(), serde_json::Value::String(hex(g)));
        }
        let t = self.call_api("/v1/calls/join", body).await?;
        if group.is_none() {
            self.signal(caller, call, call_message::Kind::Answer(CallAnswer {}))
                .await?;
            let me = self.local.account;
            let _ = self
                .signal(&me, call, call_message::Kind::Answer(CallAnswer {}))
                .await;
        }
        Ok(t)
    }

    /// Declines a 1:1 call.
    pub async fn decline_call(&mut self, caller: &[u8; 16], call: &[u8; 16]) -> Result<()> {
        self.signal(caller, call, call_message::Kind::Decline(CallDecline {}))
            .await
    }

    /// Hangs up a 1:1 call.
    pub async fn hang_up(&mut self, peer: &[u8; 16], call: &[u8; 16]) -> Result<()> {
        let h = CallHangup {
            reason: call_hangup::Reason::Normal.into(),
        };
        self.signal(peer, call, call_message::Kind::Hangup(h)).await
    }

    /// The sender key of `identity` in a group's call at the group's current epoch.
    pub fn group_call_key(
        &self,
        gid: &[u8; 16],
        call: &[u8; 16],
        identity: &str,
    ) -> Result<(u32, Zeroizing<[u8; 32]>)> {
        let group = self.load_group(gid)?;
        let exported = group
            .export_secret(
                self.mls.crypto(),
                songbird_content::call::GROUP_EXPORTER_LABEL,
                call,
                songbird_content::call::GROUP_EXPORTER_LEN,
            )
            .map_err(|_| Error::Malformed)?;
        let exported: [u8; 32] = exported
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        let e = EpochSecret::group(exported, group.epoch().as_u64());
        Ok((
            e.key_index(),
            e.sender_key(identity.as_bytes())
                .map_err(|_| Error::Malformed)?,
        ))
    }

    /// Sends a call signal to every device of `to` (sealed, kept at most 60 s if undelivered).
    async fn signal(
        &mut self,
        to: &[u8; 16],
        call: &[u8; 16],
        kind: call_message::Kind,
    ) -> Result<()> {
        let content = call_content(call, kind)?;
        let (padded, _) = songbird_content::seal(&content, None).map_err(|_| Error::Malformed)?;
        let mut devices = self.devices_of(to, false).await?;
        if *to == self.local.account {
            devices.retain(|(d, _)| *d != self.local.device);
            if devices.is_empty() {
                return Ok(());
            }
        }
        self.ensure_sessions(to, &devices).await?;
        let mut recipients = Vec::with_capacity(devices.len());
        for (d, _) in &devices {
            recipients.push((*d, self.sessions.encrypt(&self.local, &(*to, *d), &padded)?));
        }
        self.save_sessions_of(to, &devices)?;
        let access = if *to == self.local.account {
            None
        } else {
            self.store
                .profile_key(to)?
                .map(|k| songbird_keys::sealed::access_key(&k))
        };
        let hints = SendHints {
            access,
            expire_after_s: Some(60),
            ..SendHints::default()
        };
        match self.net.send(*random::<16>()?, hints, recipients).await? {
            SendStatus::Accepted => Ok(()),
            _ => Err(Error::Server(0)),
        }
    }
}

/// The sender key of `identity` in a 1:1 call (epoch 0), from the offer's secret:
/// (LiveKit key index, key).
pub fn one_to_one_key(
    call: &[u8; 16],
    secret: &[u8],
    identity: &str,
) -> Result<(u32, Zeroizing<[u8; 32]>)> {
    let secret: [u8; 32] = secret.try_into().map_err(|_| Error::Malformed)?;
    let e = EpochSecret::one_to_one(call, &secret).map_err(|_| Error::Malformed)?;
    Ok((
        e.key_index(),
        e.sender_key(identity.as_bytes())
            .map_err(|_| Error::Malformed)?,
    ))
}

fn call_content(call: &[u8; 16], kind: call_message::Kind) -> Result<Content> {
    let msg = CallMessage {
        call_id: call.to_vec(),
        kind: Some(kind),
    };
    songbird_content::call::validate(&msg).map_err(|_| Error::Malformed)?;
    Ok(Content {
        version: songbird_content::VERSION,
        required_version: 1,
        message_id: random::<16>()?.to_vec(),
        sent_at_ms: now_ms(),
        body: Some(Body::Call(msg)),
    })
}

/// A received `CallMessage` as a [`Signal`]; `None` for anything else (a link call's sender key
/// is the media crate's).
#[must_use]
pub fn signal_of(msg: &CallMessage, sent_at_ms: u64, now: u64) -> Option<([u8; 16], Signal)> {
    let call: [u8; 16] = msg.call_id.as_slice().try_into().ok()?;
    let s = match msg.kind.as_ref()? {
        call_message::Kind::Offer(o) => {
            let left = songbird_content::call::ring_left_ms(sent_at_ms, o.ring_until_ms, now);
            if left == 0 {
                return None;
            }
            Signal::Offer {
                video: o.media == i32::from(call_event::Kind::Video),
                secret: o.call_secret.clone(),
                group: o.group_id.as_slice().try_into().ok(),
                ring_until_ms: now + left,
            }
        }
        call_message::Kind::Answer(_) => Signal::Answer,
        call_message::Kind::Decline(_) => Signal::Decline,
        call_message::Kind::Busy(_) => Signal::Busy,
        call_message::Kind::Hangup(_) => Signal::Hangup,
        call_message::Kind::SenderKey(_) => return None,
    };
    Some((call, s))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn both_sides_of_a_one_to_one_call_derive_the_same_keys() {
        let (call, secret) = ([1u8; 16], [2u8; 32]);
        let a = one_to_one_key(&call, &secret, "aaaa").unwrap();
        let b = one_to_one_key(&call, &secret, "aaaa").unwrap();
        assert_eq!((a.0, &*a.1), (b.0, &*b.1));
        let c = one_to_one_key(&call, &secret, "bbbb").unwrap();
        assert_ne!(&*a.1, &*c.1, "each sender its own key");
    }

    #[test]
    fn stale_offers_never_ring() {
        let offer = CallMessage {
            call_id: vec![1; 16],
            kind: Some(call_message::Kind::Offer(CallOffer {
                media: call_event::Kind::Audio.into(),
                call_secret: vec![2; 32],
                group_id: Vec::new(),
                ring_until_ms: 1_000 + RING_MS,
            })),
        };
        assert!(signal_of(&offer, 1_000, 2_000).is_some());
        assert!(signal_of(&offer, 1_000, 1_000 + RING_MS + 1).is_none());
    }
}
