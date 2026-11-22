//! The support channel (Prompt 11, M6; `docs/PROTOCOL.md` §14.8; D-0132).
//!
//! A ticket is opened by the app, signed or not. **Signed**, the device's account is attached
//! (needed for an appeal; answered first on Plus). **Unsigned**, nobody is attached: someone
//! locked out, or who would rather not say who they are, can still write. Every message is
//! sealed on the device (`songbird.v1/support`, X25519 + HKDF + ChaCha20-Poly1305) to the
//! support team's public key, whose private half stays with staff; staff seal replies to the
//! ticket's own **reply key**, made by the app for this ticket alone. The server stores only
//! ciphertext.
//!
//! The ticket is found by a random ID and read with a random **bearer**, both drawn here and
//! returned once; the registry keeps `SHA-256("songbird.v1/ticket-bearer\0" || bearer)`. A
//! wrong bearer and a missing ticket answer the same 404. Opening and every later call spend a
//! budget per IP range (the range keyed, never stored). Closing deletes the ticket and every
//! message; untouched for 90 days, the worker deletes it.
//!
//! **Crash reports** (D-0170) are tickets of kind `crash`, only unsigned: scrubbed on the
//! device, sent only when the user turned them on, sealed like any message.
//!
//! | Request | Auth | Body | Answer |
//! | --- | --- | --- | --- |
//! | `GET /v1/support/key` | — | — | `{"key"}` |
//! | `POST /v1/support/tickets` | optional | `{"kind","reply_key","sealed"}` | 201 `{"ticket","bearer","priority"}` |
//! | `GET /v1/support/tickets/<id>` | bearer | — | `{"kind","messages":[{"from","sealed","day"}]}` |
//! | `POST /v1/support/tickets/<id>/messages` | bearer | `{"sealed"}` | 204 |
//! | `DELETE /v1/support/tickets/<id>` | bearer | — | 204 |

use aws_lc_rs::{digest, hkdf, hmac};
use songbird_accounts::{Registry, RegistryError};
use songbird_entitlements::Level;
use songbird_types::AccountId;
use songbird_valkey::ValkeyConn;

use crate::profiles::Outcome;

/// The bearer's header.
pub const BEARER_HEADER: &str = "x-songbird-ticket-bearer";
/// The largest sealed message (an envelope of a message and, with consent, a log excerpt).
pub const MAX_SEALED: usize = 32 * 1024;
/// The largest request body (the sealed message in hex, and the rest).
pub const MAX_BODY: usize = 2 * MAX_SEALED + 1024;
/// Tickets opened per IP range per hour, and the burst.
const OPEN_PER_HOUR: u32 = 6;
const OPEN_BURST: u32 = 3;
/// Other ticket calls per IP range per hour, and the burst.
const CALLS_PER_HOUR: u32 = 240;
const CALLS_BURST: u32 = 60;

/// What the registry stores for a bearer.
#[must_use]
pub fn bearer_hash(bearer: &[u8; 32]) -> [u8; 32] {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"songbird.v1/ticket-bearer\0");
    ctx.update(bearer);
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

/// The support channel's state.
pub struct Support {
    key: [u8; 32],
    throttle: hmac::Key,
}

impl std::fmt::Debug for Support {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Support")
            .field("key", &hex::encode(self.key))
            .finish_non_exhaustive()
    }
}

/// A support request, parsed from the route.
#[derive(Debug)]
pub enum Request {
    /// `GET /v1/support/key`.
    Key,
    /// `POST /v1/support/tickets`: the body, and the account if signed.
    Open(Vec<u8>, Option<AccountId>),
    /// `GET /v1/support/tickets/<id>`.
    Read([u8; 16], Option<[u8; 32]>),
    /// `POST /v1/support/tickets/<id>/messages`.
    Post([u8; 16], Option<[u8; 32]>, Vec<u8>),
    /// `DELETE /v1/support/tickets/<id>`.
    Close([u8; 16], Option<[u8; 32]>),
}

fn hex_n<const N: usize>(s: &str) -> Option<[u8; N]> {
    let mut out = [0u8; N];
    hex::decode_to_slice(s, &mut out).ok().map(|()| out)
}

fn sealed(s: &str) -> Option<Vec<u8>> {
    // An envelope: version, ephemeral key, at least a tag.
    let b = hex::decode(s).ok()?;
    (b.len() >= 1 + 32 + 16 && b.len() <= MAX_SEALED).then_some(b)
}

impl Request {
    /// The ticket's ID from `/v1/support/tickets/<id>[/messages]`, and whether it ends in
    /// `/messages`.
    #[must_use]
    pub fn ticket_path(path: &str) -> Option<([u8; 16], bool)> {
        let rest = path.strip_prefix("/v1/support/tickets/")?;
        let (id, messages) = match rest.split_once('/') {
            Some((id, "messages")) => (id, true),
            Some(_) => return None,
            None => (rest, false),
        };
        Some((hex_n(id)?, messages))
    }
}

impl Support {
    /// The channel, sealing to `key` (the support team's X25519 public key), its budgets keyed
    /// from `secret`.
    ///
    /// # Errors
    /// The key derivation failed (never in practice).
    pub fn new(key: [u8; 32], secret: &[u8; 32]) -> Result<Self, aws_lc_rs::error::Unspecified> {
        let mut k = [0u8; 32];
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird.v1/support-throttle")
            .extract(secret)
            .expand(&[b"ip-range"], hkdf::HKDF_SHA256)?
            .fill(&mut k)?;
        Ok(Self {
            key,
            throttle: hmac::Key::new(hmac::HMAC_SHA256, &k),
        })
    }

    async fn spend(
        &self,
        hot: &ValkeyConn,
        kind: &str,
        range: &[u8],
        per_hour: u32,
        burst: u32,
    ) -> Option<Outcome> {
        let tag = hmac::sign(&self.throttle, range);
        let record = format!(
            "t:s{kind}:{}",
            hex::encode(tag.as_ref().get(..16).unwrap_or_default())
        );
        match crate::registration::Registration::gcra(hot, &record, per_hour, burst, false).await {
            Ok(0) => None,
            Ok(wait) => Some(Outcome::Throttled(wait)),
            Err(()) => Some(Outcome::Unavailable),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "the request and its context")]
    async fn open(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        range: &[u8],
        body: &[u8],
        account: Option<AccountId>,
        level: impl Fn(&AccountId) -> Level,
        today: u32,
    ) -> Outcome {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct B {
            kind: String,
            reply_key: String,
            sealed: String,
        }
        let Ok(b) = serde_json::from_slice::<B>(body) else {
            return Outcome::Malformed("body");
        };
        let (Some(reply), Some(sealed)) = (hex_n::<32>(&b.reply_key), sealed(&b.sealed)) else {
            return Outcome::Malformed("body");
        };
        if !songbird_keys::xeddsa::x25519_public_key_ok(&reply) {
            return Outcome::Malformed("reply_key");
        }
        if !matches!(b.kind.as_str(), "help" | "appeal" | "billing" | "crash") {
            return Outcome::Malformed("kind");
        }
        if b.kind == "crash" && account.is_some() {
            // A crash report never names anyone (D-0170): only unsigned.
            return Outcome::Malformed("kind");
        }
        if b.kind == "appeal" && account.is_none() {
            // An appeal is about an account: only its device can open one.
            return Outcome::Unauthorized;
        }
        if let Some(o) = self.spend(hot, "o", range, OPEN_PER_HOUR, OPEN_BURST).await {
            return o;
        }
        let priority = account.as_ref().is_some_and(|a| level(a) >= Level::Plus);
        for _ in 0..4 {
            let (mut id, mut bearer) = ([0u8; 16], [0u8; 32]);
            if getrandom::fill(&mut id).is_err() || getrandom::fill(&mut bearer).is_err() {
                return Outcome::Unavailable;
            }
            match registry
                .ticket_open(
                    &id,
                    &b.kind,
                    &bearer_hash(&bearer),
                    &reply,
                    account.as_ref(),
                    priority,
                    &sealed,
                    today,
                )
                .await
            {
                Ok(()) => {
                    return Outcome::Json(serde_json::json!({
                        "ticket": hex::encode(id),
                        "bearer": hex::encode(bearer),
                        "priority": priority,
                    }));
                }
                Err(RegistryError::Exists) => {}
                Err(_) => return Outcome::Unavailable,
            }
        }
        Outcome::Unavailable
    }

    /// Serves one request. `level` gives a signed account's plan (priority on Plus).
    pub async fn serve(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        range: &[u8],
        req: Request,
        level: impl Fn(&AccountId) -> Level,
        today: u32,
    ) -> Outcome {
        let gone = |e: RegistryError| match e {
            RegistryError::NotFound => Outcome::NotFound,
            RegistryError::Full => Outcome::Conflict("full"),
            _ => Outcome::Unavailable,
        };
        match req {
            Request::Key => Outcome::Json(serde_json::json!({ "key": hex::encode(self.key) })),
            Request::Open(body, account) => {
                self.open(registry, hot, range, &body, account, level, today)
                    .await
            }
            Request::Read(id, Some(bearer)) => {
                if let Some(o) = self
                    .spend(hot, "c", range, CALLS_PER_HOUR, CALLS_BURST)
                    .await
                {
                    return o;
                }
                match registry.ticket_read(&id, Some(&bearer_hash(&bearer))).await {
                    Ok((kind, msgs)) => Outcome::Json(serde_json::json!({
                        "kind": kind,
                        "messages": msgs.iter().map(|m| serde_json::json!({
                            "from": m.from,
                            "sealed": hex::encode(&m.sealed),
                            "day": m.day,
                        })).collect::<Vec<_>>(),
                    })),
                    Err(e) => gone(e),
                }
            }
            Request::Post(id, Some(bearer), body) => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct B {
                    sealed: String,
                }
                let Some(sealed) = serde_json::from_slice::<B>(&body)
                    .ok()
                    .and_then(|b| sealed(&b.sealed))
                else {
                    return Outcome::Malformed("body");
                };
                if let Some(o) = self
                    .spend(hot, "c", range, CALLS_PER_HOUR, CALLS_BURST)
                    .await
                {
                    return o;
                }
                match registry
                    .ticket_post(&id, Some(&bearer_hash(&bearer)), &sealed, today)
                    .await
                {
                    Ok(()) => Outcome::Done,
                    Err(e) => gone(e),
                }
            }
            Request::Close(id, Some(bearer)) => {
                if let Some(o) = self
                    .spend(hot, "c", range, CALLS_PER_HOUR, CALLS_BURST)
                    .await
                {
                    return o;
                }
                match registry
                    .ticket_close(&id, Some(&bearer_hash(&bearer)))
                    .await
                {
                    Ok(()) => Outcome::Done,
                    Err(e) => gone(e),
                }
            }
            // No bearer: the same answer as a wrong one.
            Request::Read(_, None) | Request::Post(_, None, _) | Request::Close(_, None) => {
                Outcome::NotFound
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_paths_are_strict() {
        let id = "00112233445566778899aabbccddeeff";
        assert_eq!(
            Request::ticket_path(&format!("/v1/support/tickets/{id}")).map(|x| x.1),
            Some(false)
        );
        assert_eq!(
            Request::ticket_path(&format!("/v1/support/tickets/{id}/messages")).map(|x| x.1),
            Some(true)
        );
        for bad in [
            "/v1/support/tickets/".to_owned(),
            "/v1/support/tickets/00".to_owned(),
            format!("/v1/support/tickets/{id}/other"),
            format!("/v1/support/tickets/{id}/messages/1"),
        ] {
            assert!(Request::ticket_path(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_bearer_is_hashed_with_its_own_label() {
        let b = [7u8; 32];
        assert_ne!(bearer_hash(&b), b);
        assert_ne!(bearer_hash(&b), songbird_tokens::family::bearer_hash(&b));
    }
}
