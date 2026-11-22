//! Devices (Prompt 3, M4, D-0052; `docs/PROTOCOL.md` §4.6): QR provisioning, linking, the signed
//! device list, removal.
//!
//! | Method and path | Who | What |
//! | --- | --- | --- |
//! | `POST /v1/provisioning` | anyone (per IP range) | Opens a provisioning channel: `{channel}` |
//! | `PUT /v1/provisioning/<channel>` | a device | Its one sealed message (at most 4 KiB) |
//! | `GET /v1/provisioning/<channel>` | the channel's holder | Waits up to 25 s for the message; it is handed out once |
//! | `POST /v1/devices/link-token` | the primary | A single-use link token reserving a device ID |
//! | `POST /v1/account/recover/token` | anyone (throttled) | A recovery token for an account (Prompt 10) |
//! | `POST /v1/account/handover` | the primary | A hand-over token: its replacement needs no second factor |
//! | `POST /v1/account/recover` | the new device (unsigned) | Becomes the primary: the account identity key's signature, and the PIN or a passkey for a recovery token |
//! | `POST /v1/devices/link` | the new device (per IP range) | Links with the token, both keys proven |
//! | `GET /v1/devices` | a device | Its account's devices, the pending ones with their confirmation tags |
//! | `PUT /v1/devices/list` | a device | A list signed by the account identity key |
//! | `POST /v1/devices/list/fetch` | a device | An account's signed list |
//! | `POST /v1/devices/identity` | a device | An account's identity key (D-0138) |
//! | `DELETE /v1/devices/self` | a linked device | Removes itself |
//!
//! # Provisioning channel
//!
//! A channel is a 128-bit random ID (the QR code's bearer secret) naming a record in
//! `valkey-hot` under a keyed hash, so the store cannot be read back for live channel IDs. It holds
//! at most one message, written once and read once, and it expires after ten minutes. The
//! server sees only the sealed bytes (`songbird_keys::provisioning`): the new device's QR key never
//! leaves it.
//!
//! # Linking and the list
//!
//! The primary asks for a link token (hash stored, single use, ten minutes) that reserves a
//! device ID and counts toward the limit until it expires. The new device links with it, proving
//! both its keys over a transcript binding the token, the device ID and the server. It is
//! **pending**: it authenticates, but no sender is asked to encrypt to it and its keys are not
//! served, until the primary has checked its confirmation tag and signed a list that names it. A
//! list is accepted only if it verifies under the account identity key, names the requester's
//! account, has a higher version than the stored one, and names exactly the account's devices it
//! keeps with their stored keys; devices it leaves out are disabled and deleted by the worker.
//! The account's other devices get a server-signed `DeviceListChanged` notice.

use std::net::IpAddr;
use std::time::Duration;

use aws_lc_rs::{digest, hkdf, hmac};
use bytes::Bytes;
use songbird_accounts::devices::{ListEntry, PinRefusal};
use songbird_accounts::{Registry, RegistryError};
use songbird_jobs::{Job, Work};
use songbird_proto::devices::{
    self as wire, DeviceList, Factor, IDENTITY_LEN, LINK_AUTH_CONTEXT, LINK_IDENTITY_CONTEXT,
    LinkRequest,
};
use songbird_proto::system::{DeviceListChanged, Kind};
use songbird_send::system::SystemSigner;
use songbird_types::{AccountId, DeviceId};
use songbird_valkey::ValkeyConn;

use crate::config::{DeviceSettings, KeyThrottle};
use crate::registration::{Registration, range_of};

/// Channel ID length.
pub const CHANNEL_LEN: usize = 16;

/// Writes a channel's message once: only into a channel that exists and is still empty,
/// keeping its expiry. 1 if written.
const PUT_ONCE: &str = "if redis.call('GET', KEYS[1]) == '' then \
     redis.call('SET', KEYS[1], ARGV[1], 'KEEPTTL') return 1 end return 0";
/// Takes a channel's message: the message (and the channel is gone), `''` while it is empty,
/// nil if the channel does not exist.
const TAKE: &str = "local v = redis.call('GET', KEYS[1]) \
     if v and v ~= '' then redis.call('DEL', KEYS[1]) end return v";

/// Counters (aggregate only; names avoid the redaction rules' sensitive words).
#[derive(Debug, Clone, Default)]
pub struct DeviceMetrics {
    /// Devices linked.
    pub linked: songbird_service::telemetry::sharded::ShardedCounter,
    /// Signed lists stored.
    pub lists: songbird_service::telemetry::sharded::ShardedCounter,
    /// Devices removed (by a list or by themselves).
    pub removed: songbird_service::telemetry::sharded::ShardedCounter,
    /// Requests refused (malformed, bad proof or signature, rollback, mismatch, limit).
    pub refused: songbird_service::telemetry::sharded::ShardedCounter,
    /// Requests refused by a throttle.
    pub throttled: songbird_service::telemetry::sharded::ShardedCounter,
    /// Accounts recovered onto a new primary (Prompt 10).
    pub recovered: songbird_service::telemetry::sharded::ShardedCounter,
}

impl DeviceMetrics {
    /// Registers every metric.
    ///
    /// # Errors
    /// A refused name.
    pub fn register(
        &self,
        m: &mut songbird_service::telemetry::Metrics,
    ) -> Result<(), songbird_service::telemetry::TelemetryError> {
        for (name, help, c) in [
            ("api_devices_linked", "Devices linked", &self.linked),
            (
                "api_device_lists",
                "Signed device lists stored",
                &self.lists,
            ),
            ("api_devices_removed", "Devices removed", &self.removed),
            (
                "api_accounts_recovered",
                "Accounts recovered onto a new primary",
                &self.recovered,
            ),
            (
                "api_device_refused",
                "Device requests refused",
                &self.refused,
            ),
            (
                "api_device_throttled",
                "Device requests refused by a throttle",
                &self.throttled,
            ),
        ] {
            m.register(name, help, c.clone())?;
        }
        Ok(())
    }
}

/// What a device endpoint answers (the HTTP layer maps it).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 204.
    Done,
    /// 200 with JSON.
    Json(serde_json::Value),
    /// 202 with JSON: accepted, not done yet (a call link's lobby, Prompt 7).
    Accepted(serde_json::Value),
    /// 200 with bytes.
    Blob(Bytes),
    /// 400 with a reason.
    Malformed(&'static str),
    /// 404.
    NotFound,
    /// 409 with a reason.
    Conflict(&'static str),
    /// 409 `stale` with the epoch the group is at (an MLS commit another one beat; Prompt 5).
    Stale(u64),
    /// 403 with a reason.
    Forbidden(&'static str),
    /// 403 `locked`: the PIN is locked for this many seconds (`None`: until a new PIN).
    Locked(Option<u64>),
    /// 429: wait this long (microseconds).
    Throttled(u64),
    /// 503.
    Unavailable,
}

/// `POST /v1/devices/list/fetch` body.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListFetchBody {
    /// The account, 32 hex characters.
    pub account: String,
}

/// Device endpoints' shared state.
pub struct Devices {
    /// Settings.
    pub settings: DeviceSettings,
    /// IP-range prefixes (the key directory's).
    prefixes: (u8, u8),
    record_key: hmac::Key,
    signer: Option<SystemSigner>,
    pepper: zeroize::Zeroizing<[u8; 32]>,
    pin_checks: std::sync::Arc<tokio::sync::Semaphore>,
    /// Counters.
    pub metrics: DeviceMetrics,
}

impl std::fmt::Debug for Devices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Devices")
            .field("notices", &self.signer.is_some())
            .finish_non_exhaustive()
    }
}

fn parse16(h: &str) -> Option<[u8; 16]> {
    let mut b = [0u8; 16];
    hex::decode_to_slice(h, &mut b).ok().map(|()| b)
}

/// What a link token is stored under: `SHA-256("songbird.v1/link-token\0" || token)`. A token is
/// 256 random bits, so an unkeyed hash suffices (as for invite codes).
#[must_use]
pub fn token_hash(token: &[u8; wire::TOKEN_LEN]) -> [u8; 32] {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"songbird.v1/link-token\0");
    ctx.update(token);
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

/// Checks a link request's two proofs (no registry): the authentication key is an honest
/// Ed25519 key that signed the transcript; the identity key an honest X25519 key whose XEdDSA
/// signature over its transcript verifies.
///
/// # Errors
/// The reason.
pub fn verify_link(r: &LinkRequest, server_name: &str) -> Result<(), &'static str> {
    let identity_u = r
        .identity_key
        .split_first()
        .filter(|(t, _)| **t == songbird_proto::keys::TYPE_X25519)
        .and_then(|(_, u)| <[u8; 32]>::try_from(u).ok())
        .filter(songbird_keys::xeddsa::x25519_public_key_ok)
        .ok_or("keys")?;
    if !songbird_keys::ed25519_public_key_ok(&r.auth_key) {
        return Err("keys");
    }
    let t = |c| {
        wire::link_transcript(
            c,
            server_name,
            &r.token,
            &r.device,
            &r.auth_key,
            &r.identity_key,
        )
        .map_err(|_| "request")
    };
    if !songbird_keys::ed25519_verify(&r.auth_key, &t(LINK_AUTH_CONTEXT)?, &r.auth_sig) {
        return Err("signature");
    }
    if !songbird_keys::xeddsa::verify(&identity_u, &t(LINK_IDENTITY_CONTEXT)?, &r.identity_sig) {
        return Err("signature");
    }
    Ok(())
}

/// Checks a list's shape and signature against the account identity key (no registry).
///
/// # Errors
/// The reason.
pub fn verify_list(
    l: &DeviceList,
    identity: &[u8; 32],
    max_devices: u32,
) -> Result<(), &'static str> {
    if usize::try_from(max_devices).is_ok_and(|m| l.devices.len() > m) {
        return Err("limit");
    }
    if !songbird_keys::ed25519_verify(identity, &l.signed_bytes(), &l.signature) {
        return Err("signature");
    }
    Ok(())
}

impl Devices {
    /// Derives the record key from the key directory's secret; loads the system key for notices.
    ///
    /// # Errors
    /// HKDF (never for 32 bytes) or a malformed system key.
    pub fn new(
        settings: DeviceSettings,
        throttle: &KeyThrottle,
        secret: &[u8; 32],
        system_seed: Option<&[u8; 32]>,
        metrics: DeviceMetrics,
    ) -> Result<Self, &'static str> {
        let mut k = [0u8; 32];
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird-keys-v1")
            .extract(secret)
            .expand(&[b"device-records"], hkdf::HKDF_SHA256)
            .and_then(|o| o.fill(&mut k))
            .map_err(|_| "hkdf")?;
        let signer = system_seed
            .map(SystemSigner::from_seed)
            .transpose()
            .map_err(|_| "system key")?;
        let mut pepper = zeroize::Zeroizing::new([0u8; 32]);
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird-keys-v1")
            .extract(secret)
            .expand(&[b"pin-pepper"], hkdf::HKDF_SHA256)
            .and_then(|o| o.fill(pepper.as_mut()))
            .map_err(|_| "hkdf")?;
        let permits = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        Ok(Self {
            settings,
            prefixes: (throttle.ipv4_prefix, throttle.ipv6_prefix),
            record_key: hmac::Key::new(hmac::HMAC_SHA256, &k),
            signer,
            pepper,
            pin_checks: std::sync::Arc::new(tokio::sync::Semaphore::new(permits)),
            metrics,
        })
    }

    fn record(&self, kind: &str, subject: &[u8]) -> String {
        let mut ctx = hmac::Context::with_key(&self.record_key);
        ctx.update(kind.as_bytes());
        ctx.update(&[0]);
        ctx.update(subject);
        let tag = ctx.sign();
        let mut k = String::with_capacity(40);
        k.push_str("d:");
        k.push_str(kind);
        k.push(':');
        k.push_str(&hex::encode(tag.as_ref().get(..16).unwrap_or_default()));
        k
    }

    async fn throttle(
        &self,
        hot: &ValkeyConn,
        kind: &str,
        subject: &[u8],
        per_hour: u32,
        burst: u32,
    ) -> Result<(), Outcome> {
        match Registration::gcra(hot, &self.record(kind, subject), per_hour, burst, false).await {
            Ok(0) => Ok(()),
            Ok(wait) => {
                self.metrics.throttled.inc();
                Err(Outcome::Throttled(wait))
            }
            Err(()) => Err(Outcome::Unavailable),
        }
    }

    fn range(&self, peer: IpAddr) -> [u8; 17] {
        range_of(peer, self.prefixes.0, self.prefixes.1)
    }

    fn refused(&self, o: Outcome) -> Outcome {
        if matches!(
            o,
            Outcome::Malformed(_)
                | Outcome::Conflict(_)
                | Outcome::Forbidden(_)
                | Outcome::Locked(_)
        ) {
            self.metrics.refused.inc();
        }
        o
    }

    // ----- provisioning -----

    /// `POST /v1/provisioning`.
    pub async fn open_channel(&self, hot: &ValkeyConn, peer: IpAddr) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(
                hot,
                "c",
                &self.range(peer),
                s.channels_per_hour,
                s.channel_burst,
            )
            .await
        {
            return o;
        }
        let mut id = [0u8; CHANNEL_LEN];
        if getrandom::fill(&mut id).is_err() {
            return Outcome::Unavailable;
        }
        let r = hot
            .query::<redis::Value>(
                redis::cmd("SET")
                    .arg(self.record("p", &id))
                    .arg("")
                    .arg("NX")
                    .arg("EX")
                    .arg(s.channel_ttl_secs),
            )
            .await;
        match r {
            Ok(redis::Value::Okay) => Outcome::Json(serde_json::json!({
                "channel": hex::encode(id),
                "expires_in": s.channel_ttl_secs,
            })),
            _ => Outcome::Unavailable,
        }
    }

    /// `PUT /v1/provisioning/<channel>` by an authenticated device.
    pub async fn put_message(
        &self,
        hot: &ValkeyConn,
        account: &AccountId,
        channel: &str,
        body: &[u8],
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(
                hot,
                "t",
                account.as_bytes(),
                s.tokens_per_hour,
                s.token_burst,
            )
            .await
        {
            return o;
        }
        let Some(id) = parse16(channel) else {
            return Outcome::NotFound;
        };
        if body.is_empty() || body.len() > wire::MAX_PROVISIONING_LEN {
            return self.refused(Outcome::Malformed("size"));
        }
        match hot
            .query::<i64>(
                redis::cmd("EVAL")
                    .arg(PUT_ONCE)
                    .arg(1)
                    .arg(self.record("p", &id))
                    .arg(body),
            )
            .await
        {
            Ok(1) => Outcome::Done,
            // Unknown, expired or already written: one answer.
            Ok(_) => Outcome::NotFound,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `GET /v1/provisioning/<channel>`: waits up to `long_poll_secs`; 204 when nothing came
    /// (ask again), 404 for an unknown, expired or already read channel.
    pub async fn take_message(&self, hot: &ValkeyConn, peer: IpAddr, channel: &str) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(hot, "r", &self.range(peer), s.reads_per_hour, s.read_burst)
            .await
        {
            return o;
        }
        let Some(id) = parse16(channel) else {
            return Outcome::NotFound;
        };
        let record = self.record("p", &id);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(s.long_poll_secs);
        loop {
            match hot
                .query::<Option<Vec<u8>>>(redis::cmd("EVAL").arg(TAKE).arg(1).arg(&record))
                .await
            {
                Ok(None) => return Outcome::NotFound,
                Ok(Some(v)) if !v.is_empty() => return Outcome::Blob(Bytes::from(v)),
                Ok(Some(_)) => {}
                Err(_) => return Outcome::Unavailable,
            }
            if tokio::time::Instant::now() >= deadline {
                return Outcome::Done;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    // ----- linking -----

    /// `POST /v1/devices/link-token` by the primary.
    pub async fn link_token(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        plans: &crate::plans::Plans,
        account: &AccountId,
        primary: &DeviceId,
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(
                hot,
                "t",
                account.as_bytes(),
                s.tokens_per_hour,
                s.token_burst,
            )
            .await
        {
            return o;
        }
        for _ in 0..4 {
            let mut token = [0u8; wire::TOKEN_LEN];
            let mut device = [0u8; 16];
            if getrandom::fill(&mut token).is_err() || getrandom::fill(&mut device).is_err() {
                return Outcome::Unavailable;
            }
            let device = DeviceId::from_bytes(device);
            match registry
                .create_link_token(
                    primary,
                    &token_hash(&token),
                    &device,
                    Duration::from_secs(s.link_token_ttl_secs),
                    // The plan's limit (D-0127): a downgraded account over it keeps its devices
                    // and links no new one until it is back under.
                    plans.devices(account, s.max_devices),
                )
                .await
            {
                Ok(_) => {
                    return Outcome::Json(serde_json::json!({
                        "token": hex::encode(token),
                        "device": device.to_hex_unredacted(),
                        "expires_in": s.link_token_ttl_secs,
                    }));
                }
                Err(RegistryError::Exists) => {}
                Err(RegistryError::NotPrimary) => {
                    return self.refused(Outcome::Conflict("primary"));
                }
                Err(RegistryError::DeviceLimit) => return self.refused(Outcome::Conflict("limit")),
                Err(_) => return Outcome::Unavailable,
            }
        }
        Outcome::Unavailable
    }

    /// `POST /v1/devices/link` by the new device (unsigned: it has no account yet).
    #[allow(
        clippy::too_many_arguments,
        reason = "the stores, the plans and the request; a struct would only rename them"
    )]
    pub async fn link(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        plans: &crate::plans::Plans,
        messages: Option<&crate::messages::Messages>,
        server_name: &str,
        peer: IpAddr,
        body: Bytes,
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(hot, "l", &self.range(peer), s.links_per_hour, s.link_burst)
            .await
        {
            return o;
        }
        let name = server_name.to_owned();
        // Two signature checks: off the async runtime.
        let checked = tokio::task::spawn_blocking(move || {
            let r = LinkRequest::decode(&body).map_err(|_| "request")?;
            verify_link(&r, &name).map(|()| r)
        })
        .await;
        let r = match checked {
            Ok(Ok(r)) => r,
            Ok(Err(why)) => return self.refused(Outcome::Malformed(why)),
            Err(_) => return Outcome::Unavailable,
        };
        // A second factor, when the account has one (M4b, D-0053), before the token is spent.
        let hash = token_hash(&r.token);
        let owner = match registry.link_token_account(&hash).await {
            Ok(Some(a)) => a,
            Ok(None) => return self.refused(Outcome::Conflict("token")),
            Err(_) => return Outcome::Unavailable,
        };
        if let Err(o) = self.second_factor(registry, &owner, &r, server_name).await {
            return self.refused(o);
        }
        let account = match registry
            .link_device(
                &hash,
                &r.device,
                &r.auth_key,
                &r.identity_key,
                &r.confirm,
                plans.devices(&owner, s.max_devices),
            )
            .await
        {
            Ok(a) => a,
            Err(RegistryError::LinkToken) => return self.refused(Outcome::Conflict("token")),
            Err(RegistryError::DeviceLimit) => return self.refused(Outcome::Conflict("limit")),
            Err(_) => return Outcome::Unavailable,
        };
        self.metrics.linked.inc();
        // The account's other devices: the primary checks the new one and signs a new list.
        if let Ok(devs) = registry.devices_of(&account).await {
            let others: Vec<DeviceId> = devs
                .iter()
                .map(|d| d.device)
                .filter(|d| *d != r.device)
                .collect();
            self.notify(messages, &account, &others).await;
        }
        Outcome::Json(serde_json::json!({
            "account": account.to_hex_unredacted(),
            "device": r.device.to_hex_unredacted(),
        }))
    }

    // ----- second factors (Prompt 3, M4b, D-0053) -----

    /// Argon2id(pin, salt) under the pepper, off the async runtime, bounded.
    async fn pin_hash(&self, pin: String, salt: [u8; 16]) -> Option<[u8; 32]> {
        let pin = zeroize::Zeroizing::new(pin);
        let permit = tokio::time::timeout(
            Duration::from_secs(2),
            std::sync::Arc::clone(&self.pin_checks).acquire_owned(),
        )
        .await
        .ok()?
        .ok()?;
        let pepper = self.pepper.clone();
        let cost = self.settings.pin_cost;
        tokio::task::spawn_blocking(move || {
            let params = argon2::Params::new(cost.memory_kib, cost.iterations, 1, Some(32)).ok()?;
            let argon = argon2::Argon2::new_with_secret(
                pepper.as_slice(),
                argon2::Algorithm::Argon2id,
                argon2::Version::V0x13,
                params,
            )
            .ok()?;
            let mut out = [0u8; 32];
            argon
                .hash_password_into(pin.as_bytes(), &salt, &mut out)
                .ok()?;
            drop(permit);
            Some(out)
        })
        .await
        .ok()
        .flatten()
    }

    fn rp(&self) -> Option<songbird_keys::webauthn::RelyingParty<'_>> {
        Some(songbird_keys::webauthn::RelyingParty {
            id: self.settings.rp_id.as_deref()?,
            origins: &self.settings.origins,
            require_uv: self.settings.require_uv,
        })
    }

    /// The link's second factor: required when the account has a PIN or a passkey.
    async fn second_factor(
        &self,
        registry: &Registry,
        account: &AccountId,
        r: &LinkRequest,
        server_name: &str,
    ) -> Result<(), Outcome> {
        let factors = registry
            .factors(account)
            .await
            .map_err(|_| Outcome::Unavailable)?;
        if !factors.any() {
            return Ok(());
        }
        match &r.factor {
            Factor::Pin(pin) if factors.pin => {
                let s = &self.settings;
                let verifier = match registry
                    .spend_pin_attempt(
                        account,
                        s.pin_free_attempts,
                        Duration::from_secs(s.pin_lock_base_secs),
                        s.pin_max_attempts,
                    )
                    .await
                {
                    Ok(Ok(v)) => v,
                    Ok(Err(PinRefusal::NoPin)) => return Err(Outcome::Forbidden("factor")),
                    Ok(Err(PinRefusal::Locked(left))) => return Err(Outcome::Locked(left)),
                    Err(_) => return Err(Outcome::Unavailable),
                };
                let (Some(salt), Some(stored)) = (
                    verifier
                        .get(..16)
                        .and_then(|x| <[u8; 16]>::try_from(x).ok()),
                    verifier.get(16..),
                ) else {
                    return Err(Outcome::Unavailable);
                };
                let got = self
                    .pin_hash(pin.clone(), salt)
                    .await
                    .ok_or(Outcome::Unavailable)?;
                if aws_lc_rs::constant_time::verify_slices_are_equal(&got, stored).is_err() {
                    return Err(Outcome::Forbidden("factor"));
                }
                registry
                    .pin_ok(account)
                    .await
                    .map_err(|_| Outcome::Unavailable)?;
                Ok(())
            }
            Factor::Passkey {
                credential_id,
                authenticator_data,
                client_data_json,
                signature,
            } => {
                let rp = self.rp().ok_or(Outcome::Forbidden("factor"))?;
                let stored = registry
                    .passkey(account, credential_id)
                    .await
                    .map_err(|_| Outcome::Unavailable)?
                    .ok_or(Outcome::Forbidden("factor"))?;
                let key = songbird_keys::webauthn::CredentialKey::from_bytes(&stored.key)
                    .map_err(|_| Outcome::Unavailable)?;
                let t = wire::link_transcript(
                    LINK_IDENTITY_CONTEXT,
                    server_name,
                    &r.token,
                    &r.device,
                    &r.auth_key,
                    &r.identity_key,
                )
                .map_err(|_| Outcome::Malformed("request"))?;
                let challenge = songbird_keys::webauthn::link_challenge(&t);
                let count = songbird_keys::webauthn::check_assertion(
                    &key,
                    stored.count,
                    authenticator_data,
                    client_data_json,
                    signature,
                    &challenge,
                    &rp,
                )
                .map_err(|_| Outcome::Forbidden("factor"))?;
                if !registry
                    .bump_passkey(account, credential_id, stored.count, count)
                    .await
                    .map_err(|_| Outcome::Unavailable)?
                {
                    return Err(Outcome::Forbidden("factor"));
                }
                Ok(())
            }
            _ => Err(Outcome::Forbidden("factor")),
        }
    }

    /// `PUT /v1/account/pin` by the primary: `{"pin": "6 to 12 digits"}`.
    pub async fn set_pin(&self, registry: &Registry, account: &AccountId, body: &[u8]) -> Outcome {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            pin: String,
        }
        let Ok(b) = serde_json::from_slice::<Body>(body) else {
            return self.refused(Outcome::Malformed("request"));
        };
        let pin = zeroize::Zeroizing::new(b.pin);
        if !wire::pin_ok(&pin) {
            return self.refused(Outcome::Malformed("pin"));
        }
        let mut salt = [0u8; 16];
        if getrandom::fill(&mut salt).is_err() {
            return Outcome::Unavailable;
        }
        let Some(h) = self.pin_hash(pin.to_string(), salt).await else {
            return Outcome::Unavailable;
        };
        let mut verifier = salt.to_vec();
        verifier.extend_from_slice(&h);
        match registry.set_pin(account, Some(&verifier)).await {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `DELETE /v1/account/pin` by the primary.
    pub async fn clear_pin(&self, registry: &Registry, account: &AccountId) -> Outcome {
        match registry.set_pin(account, None).await {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/account/passkeys/challenge` by the primary: a registration challenge, five
    /// minutes, single use.
    pub async fn passkey_challenge(&self, hot: &ValkeyConn, account: &AccountId) -> Outcome {
        if self.rp().is_none() {
            return Outcome::Unavailable;
        }
        let mut c = [0u8; 32];
        if getrandom::fill(&mut c).is_err() {
            return Outcome::Unavailable;
        }
        match hot
            .query::<redis::Value>(
                redis::cmd("SET")
                    .arg(self.record("w", account.as_bytes()))
                    .arg(&c[..])
                    .arg("EX")
                    .arg(300),
            )
            .await
        {
            Ok(redis::Value::Okay) => Outcome::Json(serde_json::json!({
                "challenge": hex::encode(c),
                "rp_id": self.settings.rp_id,
            })),
            _ => Outcome::Unavailable,
        }
    }

    /// `PUT /v1/account/passkeys` by the primary:
    /// `{"client_data_json": hex, "attestation_object": hex}`.
    pub async fn add_passkey(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        account: &AccountId,
        body: &[u8],
    ) -> Outcome {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            client_data_json: String,
            attestation_object: String,
        }
        let Some(rp) = self.rp() else {
            return Outcome::Unavailable;
        };
        let Ok(b) = serde_json::from_slice::<Body>(body) else {
            return self.refused(Outcome::Malformed("request"));
        };
        let (Ok(cd), Ok(ao)) = (
            hex::decode(&b.client_data_json),
            hex::decode(&b.attestation_object),
        ) else {
            return self.refused(Outcome::Malformed("request"));
        };
        let challenge = match hot
            .query::<Option<Vec<u8>>>(
                redis::cmd("GETDEL").arg(self.record("w", account.as_bytes())),
            )
            .await
        {
            Ok(Some(c)) => c,
            Ok(None) => return self.refused(Outcome::Conflict("challenge")),
            Err(_) => return Outcome::Unavailable,
        };
        let Ok(cred) = songbird_keys::webauthn::check_registration(&cd, &ao, &challenge, &rp)
        else {
            return self.refused(Outcome::Malformed("passkey"));
        };
        let stored = songbird_accounts::devices::StoredPasskey {
            id: cred.id.clone(),
            key: cred.key.to_bytes(),
            count: cred.sign_count,
        };
        match registry
            .add_passkey(account, &stored, self.settings.max_passkeys)
            .await
        {
            Ok(()) => Outcome::Json(serde_json::json!({ "id": hex::encode(&cred.id) })),
            Err(RegistryError::Full) => self.refused(Outcome::Conflict("limit")),
            Err(RegistryError::Duplicate) => self.refused(Outcome::Conflict("duplicate")),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `DELETE /v1/account/passkeys` by the primary: `{"id": hex}`.
    pub async fn delete_passkey(
        &self,
        registry: &Registry,
        account: &AccountId,
        body: &[u8],
    ) -> Outcome {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            id: String,
        }
        let Some(id) = serde_json::from_slice::<Body>(body)
            .ok()
            .and_then(|b| hex::decode(b.id).ok())
        else {
            return self.refused(Outcome::Malformed("request"));
        };
        match registry.delete_passkey(account, &id).await {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `GET /v1/account/factors`: whether a PIN is set (and locked), the passkeys' IDs.
    pub async fn factors(&self, registry: &Registry, account: &AccountId) -> Outcome {
        match registry.factors(account).await {
            Ok(f) => Outcome::Json(serde_json::json!({
                "pin": f.pin,
                "pin_locked_secs": f.pin_locked,
                "pin_unusable": f.pin && f.pin_tries >= self.settings.pin_max_attempts,
                "passkeys": f.passkeys.iter().map(hex::encode).collect::<Vec<_>>(),
            })),
            Err(_) => Outcome::Unavailable,
        }
    }

    // ----- the list -----

    /// `GET /v1/devices`: the requester's account's enabled devices and the stored list version.
    pub async fn list_own(&self, registry: &Registry, account: &AccountId) -> Outcome {
        let (Ok(devs), Ok(list)) = (
            registry.devices_of(account).await,
            registry.device_list(account).await,
        ) else {
            return Outcome::Unavailable;
        };
        let devices: Vec<serde_json::Value> = devs
            .iter()
            .map(|d| {
                serde_json::json!({
                    "device": d.device.to_hex_unredacted(),
                    "auth_key": hex::encode(d.auth_key),
                    "identity_key": d.identity.map(hex::encode),
                    "primary": d.primary,
                    "pending": d.pending,
                    "confirm": d.confirm.map(hex::encode),
                })
            })
            .collect();
        Outcome::Json(serde_json::json!({
            "devices": devices,
            "list_version": list.map(|(_, v)| v),
        }))
    }

    /// `PUT /v1/devices/list`.
    pub async fn put_list(
        &self,
        registry: &Registry,
        queue: &songbird_jobs::Queue,
        messages: Option<&crate::messages::Messages>,
        requester: (&AccountId, &DeviceId),
        body: Bytes,
    ) -> Outcome {
        let out = self
            .put_list_inner(registry, queue, messages, requester, body)
            .await;
        self.refused(out)
    }

    async fn put_list_inner(
        &self,
        registry: &Registry,
        queue: &songbird_jobs::Queue,
        messages: Option<&crate::messages::Messages>,
        (account, device): (&AccountId, &DeviceId),
        body: Bytes,
    ) -> Outcome {
        let Ok(list) = DeviceList::decode(&body) else {
            return Outcome::Malformed("list");
        };
        if &list.account != account {
            return Outcome::Malformed("account");
        }
        let identity = match registry.identity_key(account).await {
            Ok(Some(k)) => k,
            Ok(None) => return Outcome::Conflict("identity"),
            Err(_) => return Outcome::Unavailable,
        };
        if let Err(why) = verify_list(&list, &identity, self.settings.max_devices) {
            return Outcome::Malformed(why);
        }
        let entries: Vec<ListEntry> = list
            .devices
            .iter()
            .map(|d| ListEntry {
                device: d.device,
                auth_key: d.auth_key,
                identity: d.identity_key,
                primary: d.primary,
            })
            .collect();
        let Ok(job) = Job::new(Work::DeleteDevices { account: *account }) else {
            return Outcome::Unavailable;
        };
        let change = match registry
            .put_device_list(account, list.list_version, &entries, &body, &job)
            .await
        {
            Ok(c) => c,
            Err(RegistryError::Rollback) => return Outcome::Conflict("version"),
            Err(RegistryError::ListMismatch) => return Outcome::Conflict("devices"),
            Err(_) => return Outcome::Unavailable,
        };
        self.metrics.lists.inc();
        if !change.removed.is_empty() {
            self.metrics
                .removed
                .inc_by(u64::try_from(change.removed.len()).unwrap_or(u64::MAX));
            if let Err(e) = songbird_jobs::relay(registry, queue).await {
                tracing::warn!(error = %e, "outbox relay deferred");
            }
        }
        let others: Vec<DeviceId> = change
            .remaining
            .iter()
            .copied()
            .filter(|d| d != device)
            .collect();
        self.notify(messages, account, &others).await;
        Outcome::Done
    }

    /// `POST /v1/devices/list/fetch`: an account's signed list (the client checks it against
    /// the account identity key and the highest version it has seen).
    pub async fn fetch_list(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        requester: &AccountId,
        body: &[u8],
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(
                hot,
                "f",
                requester.as_bytes(),
                s.list_fetches_per_hour,
                s.list_fetch_burst,
            )
            .await
        {
            return o;
        }
        let Ok(req) = serde_json::from_slice::<ListFetchBody>(body) else {
            return Outcome::Malformed("request");
        };
        let Some(account) = parse16(&req.account).map(AccountId::from_bytes) else {
            return Outcome::Malformed("account");
        };
        match registry.device_list(&account).await {
            Ok(Some((b, _))) => Outcome::Blob(b),
            Ok(None) => Outcome::NotFound,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/devices/identity`: an account's identity key (A2, D-0138), the key that signs
    /// its device list. A public key the server already holds (threat model §1); the client
    /// pins it on first sight and proves it with a key-transparency lookup (§4.9). Spends the
    /// list-fetch budget, as the list and its key are fetched together.
    pub async fn fetch_identity(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        requester: &AccountId,
        body: &[u8],
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(
                hot,
                "f",
                requester.as_bytes(),
                s.list_fetches_per_hour,
                s.list_fetch_burst,
            )
            .await
        {
            return o;
        }
        let Ok(req) = serde_json::from_slice::<ListFetchBody>(body) else {
            return Outcome::Malformed("request");
        };
        let Some(account) = parse16(&req.account).map(AccountId::from_bytes) else {
            return Outcome::Malformed("account");
        };
        match registry.account_identity(&account).await {
            Ok(Some(k)) => Outcome::Json(serde_json::json!({ "identity_key": hex::encode(k) })),
            Ok(None) => Outcome::NotFound,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `DELETE /v1/devices/self` by a linked device.
    pub async fn remove_self(
        &self,
        registry: &Registry,
        queue: &songbird_jobs::Queue,
        messages: Option<&crate::messages::Messages>,
        device: &DeviceId,
    ) -> Outcome {
        let Ok(row) = registry.device(device).await else {
            return Outcome::Unavailable;
        };
        let Some(row) = row else {
            return Outcome::Unavailable;
        };
        let Ok(job) = Job::new(Work::DeleteDevices {
            account: row.account,
        }) else {
            return Outcome::Unavailable;
        };
        let (account, remaining) = match registry.remove_device(device, &job).await {
            Ok(x) => x,
            Err(RegistryError::NotPrimary) => return self.refused(Outcome::Conflict("primary")),
            Err(_) => return Outcome::Unavailable,
        };
        self.metrics.removed.inc();
        if let Err(e) = songbird_jobs::relay(registry, queue).await {
            tracing::warn!(error = %e, "outbox relay deferred");
        }
        self.notify(messages, &account, &remaining).await;
        Outcome::Done
    }

    // ----- account recovery and the hand-over (Prompt 10, M4, D-0123) -----

    /// Draws a recovery token (`issuer` `None`, unsigned, anyone) or a hand-over token (`issuer`
    /// the signing primary) for `account`.
    async fn recovery_token(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        account: &AccountId,
        issuer: Option<&DeviceId>,
        peer: Option<IpAddr>,
    ) -> Outcome {
        let s = &self.settings;
        if let Some(p) = peer
            && let Err(o) = self
                .throttle(hot, "r", &self.range(p), s.links_per_hour, s.link_burst)
                .await
        {
            return o;
        }
        // Strangers asking for recovery tokens are throttled per account; the primary's
        // hand-overs on their own budget, keyed by the primary, so strangers can't use them up
        // (D-0125).
        let (kind, subject) = match issuer {
            None => ("rt", account.as_bytes().as_slice()),
            Some(p) => ("ht", p.as_bytes().as_slice()),
        };
        if let Err(o) = self
            .throttle(hot, kind, subject, s.tokens_per_hour, s.token_burst)
            .await
        {
            return o;
        }
        for _ in 0..4 {
            let mut token = [0u8; wire::TOKEN_LEN];
            let mut device = [0u8; 16];
            if getrandom::fill(&mut token).is_err() || getrandom::fill(&mut device).is_err() {
                return Outcome::Unavailable;
            }
            let device = DeviceId::from_bytes(device);
            match registry
                .create_recovery_token(
                    account,
                    issuer,
                    &token_hash(&token),
                    &device,
                    Duration::from_secs(s.link_token_ttl_secs),
                )
                .await
            {
                Ok(()) => {
                    return Outcome::Json(serde_json::json!({
                        "token": hex::encode(token),
                        "device": device.to_hex_unredacted(),
                        "expires_in": s.link_token_ttl_secs,
                    }));
                }
                Err(RegistryError::Exists) => {}
                // A stranger learns nothing about whether the account exists: a token that is
                // never stored, refused later as any wrong token is (D-0125).
                Err(RegistryError::NotFound) if issuer.is_none() => {
                    return Outcome::Json(serde_json::json!({
                        "token": hex::encode(token),
                        "device": device.to_hex_unredacted(),
                        "expires_in": s.link_token_ttl_secs,
                    }));
                }
                Err(RegistryError::NotFound) => return self.refused(Outcome::NotFound),
                Err(RegistryError::NotPrimary) => {
                    return self.refused(Outcome::Conflict("primary"));
                }
                Err(_) => return Outcome::Unavailable,
            }
        }
        Outcome::Unavailable
    }

    /// `POST /v1/account/recover/token` (unsigned): `{"account": hex}`.
    pub async fn recover_token(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        peer: IpAddr,
        body: &[u8],
    ) -> Outcome {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            account: String,
        }
        let Some(account) = serde_json::from_slice::<Body>(body)
            .ok()
            .and_then(|b| AccountId::parse_hex(&b.account).ok())
        else {
            return self.refused(Outcome::Malformed("request"));
        };
        self.recovery_token(registry, hot, &account, None, Some(peer))
            .await
    }

    /// `POST /v1/account/handover` by the primary: a token its replacement recovers with, no
    /// second factor needed (the primary vouches).
    pub async fn handover_token(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        account: &AccountId,
        primary: &DeviceId,
    ) -> Outcome {
        self.recovery_token(registry, hot, account, Some(primary), None)
            .await
    }

    /// `POST /v1/account/recover` (unsigned): the new device becomes the account's primary.
    ///
    /// Checked before anything changes: the throttle per range, the new keys' two proofs, the
    /// account identity key's signature over the same transcript, the token (unexpired, this
    /// account, this device), and for a recovery token the account's PIN or passkey (counted
    /// and locked as for linking).
    pub async fn recover(
        &self,
        registry: &Registry,
        hot: &ValkeyConn,
        server_name: &str,
        peer: IpAddr,
        body: Bytes,
    ) -> Outcome {
        let s = &self.settings;
        if let Err(o) = self
            .throttle(hot, "l", &self.range(peer), s.links_per_hour, s.link_burst)
            .await
        {
            return o;
        }
        let Ok(r) = wire::RecoverRequest::decode(&body) else {
            return self.refused(Outcome::Malformed("request"));
        };
        let identity = match registry.account_identity(&r.account).await {
            Ok(Some(k)) => k,
            // One answer for an unknown account and a refused signature.
            Ok(None) => return self.refused(Outcome::Malformed("signature")),
            Err(_) => return Outcome::Unavailable,
        };
        let name = server_name.to_owned();
        let link = r.link.clone();
        let account_sig = r.account_sig;
        let checked = tokio::task::spawn_blocking(move || {
            verify_link(&link, &name)?;
            let t = wire::link_transcript(
                wire::RECOVER_CONTEXT,
                &name,
                &link.token,
                &link.device,
                &link.auth_key,
                &link.identity_key,
            )
            .map_err(|_| "request")?;
            if songbird_keys::ed25519_verify(&identity, &t, &account_sig) {
                Ok(())
            } else {
                Err("signature")
            }
        })
        .await;
        match checked {
            Ok(Ok(())) => {}
            Ok(Err(why)) => return self.refused(Outcome::Malformed(why)),
            Err(_) => return Outcome::Unavailable,
        }
        let hash = token_hash(&r.link.token);
        let kind = match registry.recovery_token(&hash).await {
            Ok(Some((a, k))) if a == r.account => k,
            Ok(_) => return self.refused(Outcome::Conflict("token")),
            Err(_) => return Outcome::Unavailable,
        };
        if kind == songbird_accounts::recovery::TokenKind::Recovery
            && let Err(o) = self
                .second_factor(registry, &r.account, &r.link, server_name)
                .await
        {
            return self.refused(o);
        }
        let Ok(job) = Job::new(Work::DeleteDevices { account: r.account }) else {
            return Outcome::Unavailable;
        };
        match registry
            .recover_account(
                &hash,
                &r.account,
                &r.link.device,
                &r.link.auth_key,
                &r.link.identity_key,
                &job,
            )
            .await
        {
            Ok(removed) => {
                self.metrics.recovered.inc();
                Outcome::Json(serde_json::json!({
                    "account": r.account.to_hex_unredacted(),
                    "device": r.link.device.to_hex_unredacted(),
                    "removed": removed.len(),
                }))
            }
            Err(RegistryError::LinkToken) => self.refused(Outcome::Conflict("token")),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// A `DeviceListChanged` notice to `devices` (best effort: clients also see the change at
    /// their next list fetch and in the send path's device check).
    async fn notify(
        &self,
        messages: Option<&crate::messages::Messages>,
        account: &AccountId,
        devices: &[DeviceId],
    ) {
        let (Some(m), Some(signer)) = (messages, &self.signer) else {
            return;
        };
        if devices.is_empty() {
            return;
        }
        let mut id = [0u8; songbird_proto::system::NOTICE_ID_LEN];
        if getrandom::fill(&mut id).is_err() {
            return;
        }
        let body = DeviceListChanged { account: *account }.encode();
        if let Err(e) = m
            .path
            .system(signer, devices, Kind::DeviceListChanged, &body, id)
            .await
        {
            tracing::warn!(error = %e, "device-list notice not sent");
        }
    }
}

const _: () = assert!(IDENTITY_LEN == songbird_proto::keys::EC_LEN);

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
    use songbird_proto::devices::ListedDevice;

    use super::*;

    const SERVER: &str = "chat.example";

    fn ed() -> Ed25519KeyPair {
        Ed25519KeyPair::generate().unwrap()
    }

    fn pk(k: &Ed25519KeyPair) -> [u8; 32] {
        k.public_key().as_ref().try_into().unwrap()
    }

    fn link_request(auth: &Ed25519KeyPair, ident: &songbird_keys::xeddsa::Signer) -> LinkRequest {
        let mut r = LinkRequest {
            token: [7; 32],
            device: DeviceId::from_bytes([8; 16]),
            auth_key: pk(auth),
            identity_key: [0; IDENTITY_LEN],
            auth_sig: [0; 64],
            identity_sig: [0; 64],
            confirm: [0; 32],
            factor: Factor::None,
        };
        r.identity_key[0] = 5;
        r.identity_key[1..].copy_from_slice(&ident.public());
        let t = |c| {
            wire::link_transcript(c, SERVER, &r.token, &r.device, &r.auth_key, &r.identity_key)
                .unwrap()
        };
        r.auth_sig
            .copy_from_slice(auth.sign(&t(LINK_AUTH_CONTEXT)).as_ref());
        r.identity_sig = ident.sign(&t(LINK_IDENTITY_CONTEXT), &[9; 64]);
        r
    }

    #[test]
    fn link_requests_need_both_proofs_over_this_server() {
        let auth = ed();
        let ident = songbird_keys::xeddsa::Signer::new(&[4; 32]);
        let r = link_request(&auth, &ident);
        assert_eq!(verify_link(&r, SERVER), Ok(()));
        // Another server's transcript.
        assert_eq!(verify_link(&r, "other.example"), Err("signature"));
        // Each signature byte, the token, the device, either key.
        let mut t = r.clone();
        t.auth_sig[0] ^= 1;
        assert_eq!(verify_link(&t, SERVER), Err("signature"));
        let mut t = r.clone();
        t.identity_sig[5] ^= 1;
        assert_eq!(verify_link(&t, SERVER), Err("signature"));
        let mut t = r.clone();
        t.token[0] ^= 1;
        assert_eq!(verify_link(&t, SERVER), Err("signature"));
        let mut t = r.clone();
        t.device = DeviceId::from_bytes([9; 16]);
        assert_eq!(verify_link(&t, SERVER), Err("signature"));
        let mut t = r.clone();
        t.auth_key = pk(&ed());
        assert_eq!(verify_link(&t, SERVER), Err("signature"));
        // Dishonest keys: the Ed25519 identity point, a wrong type byte, a small-order X25519 key.
        let mut t = r.clone();
        t.auth_key = [0; 32];
        t.auth_key[0] = 1;
        assert_eq!(verify_link(&t, SERVER), Err("keys"));
        let mut t = r.clone();
        t.identity_key[0] = 6;
        assert_eq!(verify_link(&t, SERVER), Err("keys"));
        let mut t = r;
        t.identity_key[1..].copy_from_slice(&[0; 32]);
        assert_eq!(verify_link(&t, SERVER), Err("keys"));
    }

    #[test]
    fn lists_need_the_account_identity_key_and_the_limit() {
        let account_key = ed();
        let mut l = DeviceList {
            account: AccountId::from_bytes([1; 16]),
            list_version: 1,
            issued_ms: 0,
            devices: vec![ListedDevice {
                device: DeviceId::from_bytes([2; 16]),
                auth_key: [3; 32],
                identity_key: [5; IDENTITY_LEN],
                primary: true,
            }],
            signature: [0; 64],
        };
        let sig = account_key.sign(&l.signed_bytes());
        l.signature.copy_from_slice(sig.as_ref());
        assert_eq!(verify_list(&l, &pk(&account_key), 6), Ok(()));
        // Another key (a device's own), a changed version, the limit.
        assert_eq!(verify_list(&l, &pk(&ed()), 6), Err("signature"));
        let mut t = l.clone();
        t.list_version = 2;
        assert_eq!(verify_list(&t, &pk(&account_key), 6), Err("signature"));
        assert_eq!(verify_list(&l, &pk(&account_key), 0), Err("limit"));
    }

    #[test]
    fn token_hashes_are_domain_separated() {
        assert_ne!(token_hash(&[1; 32]), token_hash(&[2; 32]));
        let mut plain = [0u8; 32];
        plain.copy_from_slice(digest::digest(&digest::SHA256, &[1; 32]).as_ref());
        assert_ne!(token_hash(&[1; 32]), plain);
    }
}
