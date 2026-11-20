//! Key transparency on `akd` (Prompt 3, D-0057; `docs/PROTOCOL.md` §4.9): signed epoch heads,
//! lookup, history and audit proofs in akd's own protobuf encoding, served from akd's read-only
//! directory (the worker is the only writer).
//!
//! | Method and path | What |
//! | --- | --- |
//! | `GET /v1/kt/keys` | The VRF and log public keys (apps pin both) |
//! | `GET /v1/kt/head` | The current epoch's signed head |
//! | `POST /v1/kt/lookup` | `{"account", "kind"}` → the salt, the signed head, a lookup proof |
//! | `POST /v1/kt/history` | `{"account", "kind"}` → the salt, the signed head, a history proof (newest 64) |
//! | `POST /v1/kt/audit` | `{"from", "to"}` → an append-only proof between two epochs (at most 1,000 apart) |
//! | `POST /v1/kt/roots` | `{"from", "to"}` → every epoch's root from `from` to `to`, for the audit (D-0171) |

use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use aws_lc_rs::{hkdf, hmac};
use songbird_accounts::Registry;
use songbird_akd::{Kind, Reader, SecretVrf, SignedHead, SurrealStore, label, proofs};
use songbird_types::AccountId;
use songbird_valkey::ValkeyConn;

use crate::config::KtSettings;
use crate::registration::Registration;

/// History entries a proof carries (the newest).
pub const HISTORY: usize = 64;
/// Most epochs one audit proof spans.
pub const MAX_AUDIT_SPAN: u64 = 1_000;

/// What a transparency endpoint answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 200 with JSON.
    Json(serde_json::Value),
    /// 200 with bytes.
    Blob(bytes::Bytes),
    /// 400.
    Malformed(&'static str),
    /// 404.
    NotFound,
    /// 429 (microseconds).
    Throttled(u64),
    /// 503.
    Unavailable,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelBody {
    account: String,
    kind: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditBody {
    from: u64,
    to: u64,
}

/// Transparency state.
pub struct Transparency {
    /// Settings.
    pub settings: KtSettings,
    key: Ed25519KeyPair,
    vrf: SecretVrf,
    store: SurrealStore,
    reader: tokio::sync::OnceCell<Reader>,
    record_key: hmac::Key,
}

impl std::fmt::Debug for Transparency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transparency").finish_non_exhaustive()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn kind(s: &str) -> Option<Kind> {
    match s {
        "identity" => Some(Kind::Identity),
        "device_list" => Some(Kind::DeviceList),
        "username" => Some(Kind::Username),
        _ => None,
    }
}

impl Transparency {
    /// Loads the log and VRF keys; derives the throttle key.
    ///
    /// # Errors
    /// A malformed key, HKDF (never).
    pub fn new(
        settings: KtSettings,
        secret: &[u8; 32],
        log_seed: &[u8; 32],
        vrf_seed: &[u8; 32],
        store: SurrealStore,
    ) -> Result<Self, &'static str> {
        let key = Ed25519KeyPair::from_seed_unchecked(log_seed).map_err(|_| "log key")?;
        let mut k = [0u8; 32];
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird-kt-v1")
            .extract(secret)
            .expand(&[b"kt-records"], hkdf::HKDF_SHA256)
            .and_then(|o| o.fill(&mut k))
            .map_err(|_| "hkdf")?;
        Ok(Self {
            settings,
            key,
            vrf: SecretVrf::new(vrf_seed),
            store,
            reader: tokio::sync::OnceCell::new(),
            record_key: hmac::Key::new(hmac::HMAC_SHA256, &k),
        })
    }

    /// The log's public key.
    #[must_use]
    pub fn log_public_key(&self) -> [u8; 32] {
        self.key.public_key().as_ref().try_into().unwrap_or([0; 32])
    }

    /// The directory reader, opened once the worker has created the directory.
    async fn reader(&self) -> Result<&Reader, Outcome> {
        self.reader
            .get_or_try_init(|| songbird_akd::reader(self.store.clone(), self.vrf.clone()))
            .await
            .map_err(|_| Outcome::Unavailable)
    }

    async fn throttle(&self, hot: &ValkeyConn, requester: &AccountId) -> Result<(), Outcome> {
        let mut ctx = hmac::Context::with_key(&self.record_key);
        ctx.update(b"f\0");
        ctx.update(requester.as_bytes());
        let record = format!(
            "kt:f:{}",
            hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
        );
        match Registration::gcra(
            hot,
            &record,
            self.settings.fetches_per_hour,
            self.settings.fetch_burst,
            false,
        )
        .await
        {
            Ok(0) => Ok(()),
            Ok(w) => Err(Outcome::Throttled(w)),
            Err(()) => Err(Outcome::Unavailable),
        }
    }

    fn sign(&self, h: &akd::helper_structs::EpochHash) -> SignedHead {
        SignedHead::sign(&self.key, h.epoch(), h.hash(), now_ms())
    }

    /// `GET /v1/kt/keys`.
    pub async fn keys(&self) -> Outcome {
        let Ok(r) = self.reader().await else {
            return Outcome::Unavailable;
        };
        match r.get_public_key().await {
            Ok(vrf) => Outcome::Json(serde_json::json!({
                "vrf": hex::encode(vrf.as_bytes()),
                "log": hex::encode(self.log_public_key()),
            })),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `GET /v1/kt/head`.
    pub async fn head(&self) -> Outcome {
        let r = match self.reader().await {
            Ok(r) => r,
            Err(o) => return o,
        };
        match r.get_epoch_hash().await {
            Ok(h) => Outcome::Blob(bytes::Bytes::from(self.sign(&h).encode())),
            Err(_) => Outcome::Unavailable,
        }
    }

    async fn subject_label(
        &self,
        registry: &Registry,
        body: &[u8],
    ) -> Result<([u8; 32], akd::AkdLabel), Outcome> {
        let b =
            serde_json::from_slice::<LabelBody>(body).map_err(|_| Outcome::Malformed("request"))?;
        let mut a = [0u8; 16];
        hex::decode_to_slice(&b.account, &mut a).map_err(|_| Outcome::Malformed("account"))?;
        let k = kind(&b.kind).ok_or(Outcome::Malformed("kind"))?;
        let salt = registry
            .kt_salt(&AccountId::from_bytes(a))
            .await
            .map_err(|_| Outcome::Unavailable)?
            .ok_or(Outcome::NotFound)?;
        Ok((salt, label(k, &a)))
    }

    /// `POST /v1/kt/lookup`.
    pub async fn lookup(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        requester: &AccountId,
        body: &[u8],
    ) -> Outcome {
        if let Err(o) = self.throttle(hot, requester).await {
            return o;
        }
        let (salt, l) = match self.subject_label(registry, body).await {
            Ok(x) => x,
            Err(o) => return o,
        };
        let r = match self.reader().await {
            Ok(r) => r,
            Err(o) => return o,
        };
        match r.lookup(l).await {
            Ok((proof, h)) => match proofs::encode_lookup(&proof) {
                Ok(p) => Outcome::Json(serde_json::json!({
                    "salt": hex::encode(salt),
                    "head": hex::encode(self.sign(&h).encode()),
                    "proof": hex::encode(p),
                })),
                Err(_) => Outcome::Unavailable,
            },
            // Never published under this label.
            Err(_) => Outcome::NotFound,
        }
    }

    /// `POST /v1/kt/history`.
    pub async fn history(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        requester: &AccountId,
        body: &[u8],
    ) -> Outcome {
        if let Err(o) = self.throttle(hot, requester).await {
            return o;
        }
        let (salt, l) = match self.subject_label(registry, body).await {
            Ok(x) => x,
            Err(o) => return o,
        };
        let r = match self.reader().await {
            Ok(r) => r,
            Err(o) => return o,
        };
        match r
            .key_history(&l, akd::HistoryParams::MostRecent(HISTORY))
            .await
        {
            Ok((proof, h)) => match proofs::encode_history(&proof) {
                Ok(p) => Outcome::Json(serde_json::json!({
                    "salt": hex::encode(salt),
                    "head": hex::encode(self.sign(&h).encode()),
                    "most_recent": HISTORY,
                    "proof": hex::encode(p),
                })),
                Err(_) => Outcome::Unavailable,
            },
            Err(_) => Outcome::NotFound,
        }
    }

    /// `POST /v1/kt/roots`: the roots of epochs `from..=to` as the publisher recorded them, so a
    /// device can check an audit proof between two signed heads (D-0171). Unsigned: the proof
    /// ties each root to the next, and the two ends to signed heads.
    pub async fn roots(&self, hot: &ValkeyConn, requester: &AccountId, body: &[u8]) -> Outcome {
        if let Err(o) = self.throttle(hot, requester).await {
            return o;
        }
        let Ok(b) = serde_json::from_slice::<AuditBody>(body) else {
            return Outcome::Malformed("request");
        };
        if b.from == 0 || b.from >= b.to || b.to - b.from > MAX_AUDIT_SPAN {
            return Outcome::Malformed("epochs");
        }
        match self.store.roots(b.from, b.to).await {
            Ok(Some(r)) => Outcome::Json(serde_json::json!({
                "roots": r.iter().map(hex::encode).collect::<Vec<_>>(),
            })),
            Ok(None) => Outcome::NotFound,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/kt/audit`.
    pub async fn audit(&self, hot: &ValkeyConn, requester: &AccountId, body: &[u8]) -> Outcome {
        if let Err(o) = self.throttle(hot, requester).await {
            return o;
        }
        let Ok(b) = serde_json::from_slice::<AuditBody>(body) else {
            return Outcome::Malformed("request");
        };
        let r = match self.reader().await {
            Ok(r) => r,
            Err(o) => return o,
        };
        let Ok(current) = r.get_epoch_hash().await else {
            return Outcome::Unavailable;
        };
        if b.from == 0 || b.from >= b.to || b.to > current.epoch() || b.to - b.from > MAX_AUDIT_SPAN
        {
            return Outcome::Malformed("epochs");
        }
        match r.audit(b.from, b.to).await {
            Ok(p) => match proofs::encode_audit(&p) {
                Ok(bytes) => Outcome::Json(serde_json::json!({ "proof": hex::encode(bytes) })),
                Err(_) => Outcome::Unavailable,
            },
            Err(_) => Outcome::Unavailable,
        }
    }
}
