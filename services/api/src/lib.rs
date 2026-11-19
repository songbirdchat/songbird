//! Songbird `api`: HTTP for everything that is not on the hot path (D-0036).
//!
//! | Method and path | What |
//! | --- | --- |
//! | `POST /v1/registration/challenge` | A proof-of-work challenge ([`registration`]; unsigned) |
//! | `POST /v1/registration` | A new account and its first device ([`registration`]; unsigned, proof of work) |
//! | `POST`, `GET`, `DELETE /v1/invites` | Invite codes: create from the allowance, see it, revoke ([`registration`]) |
//! | `PUT`, `DELETE /v1/username`; `POST /v1/username/lookup` | Claim, release and look up usernames ([`usernames`]) |
//! | `PUT`, `DELETE /v1/username/key`; `PUT`, `DELETE`, `GET /v1/username/link[/<handle>]` | The username key; username links ([`usernames`]) |
//! | `PUT`, `GET /v1/keys`; `POST /v1/keys/fetch` | The key directory: upload, counts, fetch bundles ([`keys`]) |
//! | `PUT`, `GET /v1/keypackages`; `POST /v1/keypackages/fetch` | MLS KeyPackages: upload, counts, fetch ([`keys`]) |
//! | `POST /v1/provisioning`; `PUT`, `GET /v1/provisioning/<channel>` | The QR provisioning channel ([`devices`]; open and read unsigned) |
//! | `POST /v1/devices/link-token`; `POST /v1/devices/link` | Linking a new device ([`devices`]; the link itself unsigned, both keys proven) |
//! | `PUT /v1/profile`; `POST /v1/profile/fetch`; `PUT`, `GET /v1/profile/avatar[/<id>]` | Encrypted profiles and avatars ([`profiles`]) |
//! | `GET /v1/certificate`; `PUT /v1/messages/sealed` | Sealed sender ([`profiles`]; the sealed send unsigned, authorized by access keys) |
//! | `GET /v1/kt/keys`, `GET /v1/kt/head`; `POST /v1/kt/lookup`, `/history`, `/audit` | Key transparency on akd ([`kt`]) |
//! | `GET /v1/devices`; `PUT /v1/devices/list`; `POST /v1/devices/list/fetch`; `DELETE /v1/devices/self` | The signed device list and removal ([`devices`]) |
//! | `PUT /v1/push` | Registers the device's push channel, sealed to the push service's key |
//! | `DELETE /v1/push` | Removes it |
//! | `/v1/attachments…` | Grants, single and resumable uploads, downloads ([`media`]) |
//! | `DELETE /v1/account` | Deletes the device's account from every store (outbox job; the primary only) |
//! | `GET /v1/account/export`; `PUT /v1/account/inactivity` | GDPR export; the inactivity period (the primary only) |
//! | `POST /v1/calls`, `/v1/calls/group`, `/v1/calls/join`, `/v1/calls/links…` | Calls: LiveKit rooms and tokens, call links ([`calls`]) |
//! | `PUT /v1/messages` | A WebSocket `Send` frame, for clients with no open socket; answers its `SendResult` ([`messages`]) |
//!
//! Every request but the two registration steps, opening and reading a provisioning channel,
//! linking and a parent's later policy (`PUT /v1/supervision/update`, authorized by the policy's
//! signature, D-0108) is signed by the device ([`auth`]); there are no
//! tokens or cookies. Registration is paid for with proof of work and throttled per IP range. Bodies are
//! bounded before they are read. Responses carry status codes and small JSON; errors say nothing
//! about whether a device, account or object exists beyond what the caller already proved.
//! Nothing here logs identifiers or object names (invariant 6).
#![forbid(unsafe_code)]

pub mod abuse;
pub mod account;
pub mod admin;
pub mod auth;
pub mod calls;
pub mod client;
pub mod communities;
pub mod config;
pub mod devices;
pub mod gifs;
pub mod groups;
pub mod keys;
pub mod kt;
pub mod media;
pub mod messages;
pub mod packs;
pub mod plans;
pub mod profiles;
pub mod registration;
pub mod reports;
pub mod requests;
pub mod server;
pub mod storage;
pub mod support;
pub mod usernames;
pub mod vault;

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt as _, Empty, Full, Limited, StreamBody};
use hyper::body::Incoming;
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use serde::Deserialize;
use songbird_accounts::push_token::{self, Registration};
use songbird_accounts::{Registry, push_index};
use songbird_jobs::{Job, ObjectKey, Queue, Work};
use songbird_service::telemetry::sharded::ShardedCounter as Counter;
use songbird_types::{AccountId, DeviceId};
use songbird_valkey::ValkeyConn;

pub use config::{ApiConfig, ApiSettings};

/// Largest JSON body accepted.
pub const MAX_JSON: usize = 16 * 1024;

/// Response body type.
pub type Body = UnsyncBoxBody<Bytes, std::io::Error>;

/// Counters (aggregate only).
#[derive(Debug, Clone, Default)]
pub struct ApiMetrics {
    /// Requests answered.
    pub requests: Counter,
    /// Requests refused for authentication (bad, stale, replayed or unknown).
    pub auth_failures: Counter,
    /// Push registrations stored.
    pub push_registrations: Counter,
    /// Uploads completed.
    pub uploads: Counter,
    /// Downloads completed.
    pub downloads: Counter,
    /// Account deletions started.
    pub deletions: Counter,
    /// Requests that failed on a dependency (database, Valkey, object store).
    pub errors: Counter,
    /// The send path's counters (`PUT /v1/messages`), registered as `api_*`.
    pub send: songbird_send::SendMetrics,
    /// Push wake-ups from `PUT /v1/messages`.
    pub push: songbird_send::PushMetrics,
    /// Registration and invites.
    pub registration: registration::RegistrationMetrics,
    /// Usernames.
    pub usernames: usernames::UsernameMetrics,
    /// The key directory.
    pub keys: keys::KeyMetrics,
    /// Devices.
    pub devices: devices::DeviceMetrics,
    /// Profiles and sealed sender.
    pub profiles: profiles::ProfileMetrics,
    /// The account record store (Prompt 9).
    pub storage: storage::StorageMetrics,
    /// The backup vault (Prompt 10).
    pub vault: vault::VaultMetrics,
    /// Blocks and the request policy.
    pub blocks: requests::RequestsMetrics,
    /// Reports.
    pub reports: reports::ReportMetrics,
    /// Media (Prompt 6).
    pub media: media::MediaMetrics,
    /// Calls (Prompt 7).
    pub calls: calls::CallMetrics,
}

impl ApiMetrics {
    /// Registers every metric.
    ///
    /// # Errors
    /// A refused name.
    pub fn register(
        &self,
        m: &mut songbird_service::telemetry::Metrics,
    ) -> Result<(), songbird_service::telemetry::TelemetryError> {
        for (name, help, c) in [
            ("api_requests", "Requests answered", &self.requests),
            (
                "api_auth_failures",
                "Requests refused for authentication",
                &self.auth_failures,
            ),
            (
                "api_push_registrations",
                "Push registrations stored",
                &self.push_registrations,
            ),
            ("api_uploads", "Attachment uploads completed", &self.uploads),
            (
                "api_downloads",
                "Attachment downloads completed",
                &self.downloads,
            ),
            (
                "api_deletions",
                "Account deletions started",
                &self.deletions,
            ),
            (
                "api_errors",
                "Requests failed on a dependency",
                &self.errors,
            ),
            (
                "api_push_jobs",
                "Push jobs enqueued for offline devices",
                &self.push.jobs,
            ),
            (
                "api_push_dropped",
                "Push wake-ups dropped (the message stays in the mailbox)",
                &self.push.dropped,
            ),
        ] {
            m.register(name, help, c.clone())?;
        }
        self.registration.register(m)?;
        self.usernames.register(m)?;
        self.keys.register(m)?;
        self.devices.register(m)?;
        self.profiles.register(m)?;
        self.storage.register(m)?;
        self.vault.register(m)?;
        self.blocks.register(m)?;
        self.reports.register(m)?;
        self.media.register(m)?;
        self.calls.register(m)?;
        self.send.register(m, "api")
    }
}

/// Everything a request needs.
pub struct App {
    /// Host name devices sign.
    pub server_name: String,
    /// Account registry.
    pub registry: Arc<Registry>,
    /// Push registrations, in their own database (D-0095).
    pub push_store: Arc<songbird_accounts::PushStore>,
    /// Attachment store.
    pub s3: songbird_s3::Client,
    /// Job queue.
    pub queue: Arc<Queue>,
    /// `valkey-hot` (nonces).
    pub hot: ValkeyConn,
    /// Push service public key.
    pub push_public: [u8; 32],
    /// Push-token row key.
    pub push_index_secret: [u8; 32],
    /// Attachment limits.
    pub limits: config::AttachmentLimits,
    /// `[api.media]` (Prompt 6): upload budgets; `None` only in development.
    pub media: Option<media::Media>,
    /// Metrics.
    pub metrics: ApiMetrics,
    /// `PUT /v1/messages` (absent: the endpoint answers 503).
    pub messages: Option<messages::Messages>,
    /// Registration and invites (absent: those endpoints answer 503).
    pub registration: Option<registration::Registration>,
    /// Usernames (absent: those endpoints answer 503).
    pub usernames: Option<usernames::Usernames>,
    /// The key directory (absent: those endpoints answer 503).
    pub keys: Option<keys::KeyDirectory>,
    /// Devices: provisioning, linking, the signed list (absent: those endpoints answer 503).
    pub devices: Option<devices::Devices>,
    /// Encrypted profiles and sealed sender (absent: those endpoints answer 503).
    pub profiles: Option<profiles::Profiles>,
    /// The account record store (absent: those endpoints answer 503).
    pub storage: Option<storage::Storage>,
    /// The backup vault (Prompt 10; absent: those endpoints answer 503).
    pub vault: Option<vault::Vault>,
    /// Key transparency (absent: those endpoints answer 503).
    pub kt: Option<kt::Transparency>,
    /// Blocks and the message-request policy (absent: those endpoints answer 503).
    pub requests: Option<requests::Requests>,
    /// Reporting with message franking (absent: `POST /v1/reports` answers 503).
    pub reports: Option<reports::Reports>,
    /// Calls (Prompt 7; absent: `/v1/calls…` answers 503).
    pub calls: Option<calls::CallsApi>,
    /// The support channel (Prompt 11, D-0132; absent: `/v1/support…` answers 503).
    pub support: Option<Box<support::Support>>,
    /// Plans: the catalogue and the live entitlements snapshot (Prompt 11, D-0127). The default
    /// is the owner's catalogue with every account on Free.
    pub plans: plans::Plans,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").finish_non_exhaustive()
    }
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Empty::new().map_err(|e| match e {}).boxed_unsync());
    *r.status_mut() = status;
    r
}

fn json(status: StatusCode, v: &serde_json::Value) -> Response<Body> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let mut r = Response::new(
        Full::new(Bytes::from(body))
            .map_err(|e| match e {})
            .boxed_unsync(),
    );
    *r.status_mut() = status;
    r.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    r
}

/// The push registration a device sends (binary fields in hex).
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
enum PushBody {
    Apns {
        token: String,
        #[serde(default)]
        voip: Option<String>,
    },
    Fcm {
        token: String,
    },
    Webpush {
        endpoint: String,
        p256dh: String,
        auth: String,
    },
}

impl PushBody {
    fn registration(self) -> Option<Registration> {
        let bin = |s: &str, max: usize| (s.len() <= 2 * max).then(|| hex::decode(s).ok()).flatten();
        let r = match self {
            Self::Apns { token, voip } => Registration::Apns {
                token: bin(&token, push_token::MAX_APNS_TOKEN)?,
                voip: match voip {
                    Some(v) => Some(bin(&v, push_token::MAX_APNS_TOKEN)?),
                    None => None,
                },
            },
            Self::Fcm { token } => Registration::Fcm { token },
            Self::Webpush {
                endpoint,
                p256dh,
                auth,
            } => {
                let mut p = [0u8; 65];
                hex::decode_to_slice(p256dh, &mut p).ok()?;
                let mut a = [0u8; 16];
                hex::decode_to_slice(auth, &mut a).ok()?;
                Registration::WebPush {
                    endpoint,
                    p256dh: p,
                    auth: a,
                }
            }
        };
        r.validate().ok()?;
        Some(r)
    }
}

/// Why a request was refused before its handler ran.
enum Refusal {
    Status(StatusCode),
}

impl App {
    /// Handles one request.
    pub async fn handle(self: &Arc<Self>, req: Request<Incoming>) -> Response<Body> {
        self.metrics.requests.inc();
        match self.route(req).await {
            Ok(r) => r,
            Err(Refusal::Status(s)) => {
                if s == StatusCode::UNAUTHORIZED {
                    self.metrics.auth_failures.inc();
                } else if s == StatusCode::SERVICE_UNAVAILABLE {
                    self.metrics.errors.inc();
                }
                empty(s)
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the dispatch table: one arm per endpoint family"
    )]
    async fn route(self: &Arc<Self>, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let path = req.uri().path().to_owned();
        let method = req.method().clone();
        if path == "/v1/attachments" || path.starts_with("/v1/attachments/") {
            return media::route(self, req).await;
        }
        if path.starts_with("/v1/media/") {
            return media::serve(self, req).await;
        }
        if path == "/v1/packs" || path.starts_with("/v1/packs/") {
            return packs::route(self, req).await;
        }
        if path.starts_with("/v1/gifs/") {
            return gifs::route(self, req).await;
        }
        if path == "/v1/family" || path.starts_with("/v1/family/") {
            return self.family(req).await;
        }
        if path.starts_with("/v1/support/") {
            return self.support(req).await;
        }
        match (&method, path.as_str()) {
            (&Method::PUT, "/v1/push") => {
                let (device, body) = self.authenticate_json(req).await?;
                self.put_push(device, &body).await
            }
            (&Method::DELETE, "/v1/push") => {
                let (device, _) = self.authenticate_json(req).await?;
                self.push_store
                    .delete(&[push_index(&self.push_index_secret, &device)])
                    .await
                    .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
                Ok(empty(StatusCode::NO_CONTENT))
            }
            (&Method::GET, "/v1/entitlements") => {
                let (device, _) = self.authenticate_json(req).await?;
                let row = self
                    .registry
                    .device(&device)
                    .await
                    .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?
                    .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
                if row.pending {
                    return Ok(pending_refusal());
                }
                Ok(json(StatusCode::OK, &self.plans.describe(&row.account)))
            }
            (&Method::POST, "/v1/entitlements/redeem") => {
                let r = self
                    .plans
                    .redeemer
                    .clone()
                    .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
                let (device, body) = self.authenticate_json(req).await?;
                let row = self
                    .registry
                    .device(&device)
                    .await
                    .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?
                    .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
                if row.pending {
                    return Ok(pending_refusal());
                }
                Ok(profile_outcome(
                    r.redeem(
                        &self.registry,
                        &self.hot,
                        &row.account,
                        &body,
                        songbird_entitlements::today(),
                    )
                    .await,
                ))
            }
            (&Method::GET, "/v1/tokens/keys") => match &self.plans.redeemer {
                Some(r) => Ok(json(StatusCode::OK, &r.public_keys())),
                None => Ok(empty(StatusCode::SERVICE_UNAVAILABLE)),
            },
            (&Method::DELETE, "/v1/account") => {
                let (device, _) = self.authenticate_json(req).await?;
                self.delete_account(device).await
            }
            (&Method::PUT, "/v1/messages") => self.put_message(req).await,
            (_, p) if p == "/v1/storage" || p.starts_with("/v1/storage/") => {
                self.storage(req).await
            }
            (_, p) if p == "/v1/vault" || p.starts_with("/v1/vault/") => self.vault(req).await,
            (_, p)
                if p == "/v1/messages/sealed"
                    || p == "/v1/ephemeral/sealed"
                    || p == "/v1/certificate"
                    || p == "/v1/profile"
                    || p.starts_with("/v1/profile/") =>
            {
                self.profiles(req).await
            }
            (&Method::POST, "/v1/registration/challenge") => {
                let (r, peer) = self.registration_and_peer(&req)?;
                Ok(outcome(r.challenge(&self.hot, peer).await))
            }
            (&Method::POST, "/v1/registration") => {
                let (r, peer) = self.registration_and_peer(&req)?;
                let body = bounded_body(req, songbird_proto::registration::MAX_LEN).await?;
                Ok(outcome(
                    r.register(&self.registry, &self.hot, &self.server_name, peer, &body)
                        .await,
                ))
            }
            (&Method::POST, "/v1/invites/status") => self.invite_status(req).await,
            (&Method::POST | &Method::GET | &Method::DELETE, "/v1/invites") => {
                self.invites(req).await
            }
            (_, p) if p == "/v1/username" || p.starts_with("/v1/username/") => {
                self.usernames(req).await
            }
            (_, "/v1/keys" | "/v1/keys/fetch" | "/v1/keypackages" | "/v1/keypackages/fetch") => {
                self.keys(req).await
            }
            (_, "/v1/blocks" | "/v1/account/requests") => self.requests(req).await,
            (&Method::POST, "/v1/reports") => self.report(req).await,
            (&Method::POST, p) if p == "/v1/calls" || p.starts_with("/v1/calls/") => {
                self.calls(req).await
            }
            (
                &Method::POST,
                "/v1/groups"
                | "/v1/groups/commit"
                | "/v1/groups/state"
                | "/v1/groups/propose"
                | "/v1/groups/meta"
                | "/v1/groups/log"
                | "/v1/groups/links"
                | "/v1/groups/links/revoke"
                | "/v1/groups/links/open"
                | "/v1/groups/join"
                | "/v1/groups/requests"
                | "/v1/groups/requests/list"
                | "/v1/groups/requests/decline"
                | "/v1/groups/ds"
                | "/v1/communities"
                | "/v1/communities/groups/add"
                | "/v1/communities/groups/remove"
                | "/v1/communities/meta"
                | "/v1/communities/state"
                | "/v1/communities/delete",
            ) => self.groups(req).await,
            (
                _,
                "/v1/account/settings"
                | "/v1/account/discoverable"
                | "/v1/account/groups"
                | "/v1/supervision",
            ) => self.account_settings(req).await,
            (_, "/v1/supervision/update") => self.supervision_update(req).await,
            (&Method::POST, "/v1/account/recover/token" | "/v1/account/recover") => {
                self.recovery(req).await
            }
            (_, p) if p.starts_with("/v1/account/") => self.factors(req).await,
            (_, p) if p.starts_with("/v1/kt/") => self.kt(req).await,
            (_, p) if p.starts_with("/v1/provisioning") || p.starts_with("/v1/devices") => {
                self.devices(req).await
            }
            _ => Err(Refusal::Status(StatusCode::NOT_FOUND)),
        }
    }

    fn registration_and_peer(
        &self,
        req: &Request<Incoming>,
    ) -> Result<(&registration::Registration, std::net::IpAddr), Refusal> {
        let r = self
            .registration
            .as_ref()
            .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        // The listener always sets it; without it there is nothing to throttle by.
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        Ok((r, peer.0))
    }

    /// `/v1/profile…`, `/v1/certificate` and `/v1/messages/sealed` ([`profiles`]).
    async fn profiles(self: &Arc<Self>, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let p = self.profiles.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        // Who may change account-wide state: a listed (not pending) device of the account.
        let listed = |device: DeviceId| async move {
            let row = self
                .registry
                .device(&device)
                .await
                .map_err(|_| unavailable())?
                .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
            Ok::<_, Refusal>((row.account, !row.pending, row.primary))
        };
        let out = match (&method, path.as_str()) {
            (&Method::PUT, "/v1/messages/sealed") => {
                let peer = req
                    .extensions()
                    .get::<registration::Peer>()
                    .map(|x| x.0)
                    .ok_or_else(unavailable)?;
                let key = header_key(&req, "unidentified-access-key");
                let max = self
                    .messages
                    .as_ref()
                    .map_or(0, |m| m.limits.max_send_bytes);
                let body = bounded_body(req, max.max(1)).await?;
                p.sealed_send(
                    &self.registry,
                    &self.hot,
                    self.messages.as_ref(),
                    peer,
                    key,
                    body,
                )
                .await
            }
            (&Method::PUT, "/v1/ephemeral/sealed") => {
                let peer = req
                    .extensions()
                    .get::<registration::Peer>()
                    .map(|x| x.0)
                    .ok_or_else(unavailable)?;
                let key = header_key(&req, "unidentified-access-key");
                let body = bounded_body(req, songbird_proto::ephemeral::MAX_BODY).await?;
                p.sealed_presence(&self.hot, self.messages.as_ref(), peer, key, &body)
                    .await
            }
            (&Method::GET, "/v1/certificate") => {
                let (device, _) = self.authenticate_json(req).await?;
                match listed(device).await? {
                    // A suspended account gets no sender certificate (D-0131): its sealed sends
                    // stop when the one it holds expires.
                    (a, true, _)
                        if self.plans.live.standing(&a)
                            == songbird_entitlements::Standing::Suspended =>
                    {
                        profiles::Outcome::Forbidden("suspended")
                    }
                    (a, true, _) => p.certificate(&self.registry, &a, &device).await,
                    (_, false, _) => return Ok(pending_refusal()),
                }
            }
            (&Method::PUT, "/v1/profile") => {
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::profile::MAX_PROFILE + 64)
                    .await?;
                match listed(device).await? {
                    (a, true, primary) => p.put(&self.registry, &a, primary, &body).await,
                    (_, false, _) => return Ok(pending_refusal()),
                }
            }
            (&Method::POST, "/v1/profile/fetch") => {
                let (device, body) = self.authenticate_json(req).await?;
                let (a, _, _) = listed(device).await?;
                p.fetch(&self.registry, &self.hot, &a, &body).await
            }
            (&Method::PUT, "/v1/profile/avatar") => {
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::profile::MAX_AVATAR)
                    .await?;
                match listed(device).await? {
                    (a, true, _) => p.put_avatar(&self.registry, &self.s3, &a, body).await,
                    (_, false, _) => return Ok(pending_refusal()),
                }
            }
            (&Method::GET, path) if path.starts_with("/v1/profile/avatar/") => {
                let mut id = [0u8; 32];
                hex::decode_to_slice(path.trim_start_matches("/v1/profile/avatar/"), &mut id)
                    .map_err(|_| Refusal::Status(StatusCode::NOT_FOUND))?;
                let (device, _) = self.authenticate_json(req).await?;
                let (a, _, _) = listed(device).await?;
                if let Err(o) = p.avatar_allowed(&self.registry, &self.hot, &a, &id).await {
                    return Ok(profile_outcome(o));
                }
                return self.serve_avatar(&id).await;
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(profile_outcome(out))
    }

    /// `/v1/storage…` ([`storage`]): a listed (not pending) device of the account only.
    async fn storage(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let st = self.storage.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let object = |p: &str| {
            let mut id = [0u8; 32];
            hex::decode_to_slice(p.trim_start_matches("/v1/storage/object/"), &mut id)
                .map(|()| id)
                .map_err(|_| Refusal::Status(StatusCode::NOT_FOUND))
        };
        let (device, body) = match (&method, path.as_str()) {
            (&Method::PUT, "/v1/storage") => {
                self.authenticate_body(req, st.settings.max_write_body())
                    .await?
            }
            (&Method::PUT, "/v1/storage/object") => {
                self.authenticate_body(req, songbird_proto::storage::MAX_OBJECT)
                    .await?
            }
            (&Method::POST, "/v1/storage/read") => {
                self.authenticate_body(
                    req,
                    2 + songbird_proto::storage::MAX_READ * songbird_proto::storage::ID_LEN,
                )
                .await?
            }
            (&Method::GET, "/v1/storage" | "/v1/storage/ids") => {
                self.authenticate_json(req).await?
            }
            (&Method::GET | &Method::DELETE, p) if p.starts_with("/v1/storage/object/") => {
                self.authenticate_json(req).await?
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        if row.pending {
            return Ok(pending_refusal());
        }
        let a = row.account;
        let out = match (&method, path.as_str()) {
            (&Method::GET, "/v1/storage") => st.head(&self.registry, &self.hot, &a).await,
            (&Method::GET, "/v1/storage/ids") => st.ids(&self.registry, &self.hot, &a).await,
            (&Method::POST, "/v1/storage/read") => {
                st.read(&self.registry, &self.hot, &a, &body).await
            }
            (&Method::PUT, "/v1/storage") => st.write(&self.registry, &self.hot, &a, &body).await,
            (&Method::PUT, "/v1/storage/object") => {
                st.put_object(&self.registry, &self.hot, &self.s3, &a, body)
                    .await
            }
            (&Method::GET, p) => {
                let id = object(p)?;
                if let Err(o) = st.object_allowed(&self.registry, &self.hot, &a, &id).await {
                    return Ok(profile_outcome(o));
                }
                return self.serve_object(&storage::object_name(&id)).await;
            }
            (&Method::DELETE, p) => {
                let id = object(p)?;
                st.delete_object(&self.registry, &self.hot, &self.s3, &a, &id)
                    .await
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(profile_outcome(out))
    }

    /// `/v1/vault…` ([`vault`]): reads by the vault's credentials alone; writes also signed by
    /// the primary of the owning account.
    #[allow(
        clippy::too_many_lines,
        reason = "the vault's dispatch table: one arm per endpoint"
    )]
    async fn vault(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let v = self.vault.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .ok_or_else(unavailable)?
            .0;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let query = req.uri().query().map(str::to_owned);
        let today = songbird_entitlements::today();
        // Tokens for the account's primary: signed, and naming no vault (D-0130).
        if method == Method::POST && path == "/v1/vault/tokens" {
            let (device, body) = self.authenticate_json(req).await?;
            let row = self
                .registry
                .device(&device)
                .await
                .map_err(|_| unavailable())?
                .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
            return Ok(profile_outcome(
                self.plans
                    .issue_vault(&self.registry, &self.hot, &row, &v.settings, &body, today)
                    .await,
            ));
        }
        // Without both credentials there is no vault to name: one 404, as for any miss.
        let creds =
            vault::credentials(req.headers()).ok_or(Refusal::Status(StatusCode::NOT_FOUND))?;
        let keys = self.plans.redeemer.as_ref().map(|r| r.keys());
        let open = || async {
            v.open(&self.registry, &self.hot, peer, &creds)
                .await
                .map_err(profile_outcome)
        };
        let writable = |o: vault::Opened| async move {
            v.write_allowed(&self.hot, &o, today)
                .await
                .map(|()| o)
                .map_err(profile_outcome)
        };
        match (&method, path.as_str()) {
            (&Method::POST, "/v1/vault") => {
                let body = bounded_body(req, 4_096).await?;
                return Ok(profile_outcome(
                    v.create(&self.registry, &self.hot, peer, &creds, keys, &body, today)
                        .await,
                ));
            }
            (&Method::POST, "/v1/vault/day") => {
                let body = bounded_body(req, 4_096).await?;
                let o = match open().await {
                    Ok(o) => o,
                    Err(r) => return Ok(r),
                };
                return Ok(profile_outcome(
                    v.pass(&self.registry, &o, keys, &body, today).await,
                ));
            }
            (&Method::DELETE, "/v1/vault") => {
                let o = match open().await {
                    Ok(o) => writable(o).await,
                    Err(r) => return Ok(r),
                };
                return Ok(match o {
                    Ok(o) => profile_outcome(v.delete(&self.registry, &o).await),
                    Err(r) => r,
                });
            }
            (&Method::POST, "/v1/vault/chunks/delete") => {
                let body = bounded_body(req, 2 + songbird_proto::vault::MAX_NAMES * 32).await?;
                let o = match open().await {
                    Ok(o) => writable(o).await,
                    Err(r) => return Ok(r),
                };
                return Ok(match o {
                    Ok(o) => profile_outcome(v.delete_chunks(&self.registry, &o, &body).await),
                    Err(r) => r,
                });
            }
            (&Method::PUT, "/v1/vault/manifest") => {
                let expected = req
                    .headers()
                    .get(songbird_proto::vault::EXPECTED_HEADER)
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.parse::<u64>().ok())
                    .ok_or(Refusal::Status(StatusCode::BAD_REQUEST))?;
                let body = bounded_body(req, v.settings.max_manifest).await?;
                let o = match open().await {
                    Ok(o) => writable(o).await,
                    Err(r) => return Ok(r),
                };
                return Ok(match o {
                    Ok(o) => {
                        let quota = v.quota(&self.plans, o.row.level);
                        profile_outcome(
                            v.put_manifest(&self.registry, &o, quota, expected, body)
                                .await,
                        )
                    }
                    Err(r) => r,
                });
            }
            (&Method::PUT, p) if p.starts_with("/v1/vault/chunks/") => {
                let name = vault::chunk_name(p).ok_or(Refusal::Status(StatusCode::NOT_FOUND))?;
                let max = usize::try_from(songbird_proto::vault::max_chunk_object())
                    .unwrap_or(usize::MAX);
                let body = bounded_body(req, max).await?;
                let o = match open().await {
                    Ok(o) => writable(o).await,
                    Err(r) => return Ok(r),
                };
                return Ok(match o {
                    Ok(o) => {
                        let quota = v.quota(&self.plans, o.row.level);
                        profile_outcome(v.put_chunk(&self.registry, &o, quota, &name, body).await)
                    }
                    Err(r) => r,
                });
            }
            (&Method::GET, "/v1/vault" | "/v1/vault/manifest" | "/v1/vault/chunks") => {}
            (&Method::GET, p) if p.starts_with("/v1/vault/chunks/") => {}
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        }
        let o = match open().await {
            Ok(o) => o,
            Err(r) => return Ok(r),
        };
        if let Err(out) = v.read_budget(&self.hot, &o.vk).await {
            return Ok(profile_outcome(out));
        }
        match path.as_str() {
            "/v1/vault" => Ok(profile_outcome(
                v.stats(&o, v.quota(&self.plans, o.row.level)),
            )),
            "/v1/vault/manifest" => match v.manifest_name(&o) {
                Some(name) => self.serve_from(&v.s3, &name).await,
                None => Ok(empty(StatusCode::NO_CONTENT)),
            },
            "/v1/vault/chunks" => Ok(profile_outcome(match vault::after(query.as_deref()) {
                Ok(after) => v.list(&self.registry, &o, after).await,
                Err(out) => out,
            })),
            p => {
                let name = vault::chunk_name(p).ok_or(Refusal::Status(StatusCode::NOT_FOUND))?;
                match v.chunk_recorded(&self.registry, &o, &name).await {
                    Ok(object) => self.serve_from(&v.s3, &object).await,
                    Err(out) => Ok(profile_outcome(out)),
                }
            }
        }
    }

    /// `/v1/family…` (Prompt 11, D-0130): the family mailbox. No account; the IP range only
    /// for its budget.
    async fn family(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let r = self
            .plans
            .redeemer
            .clone()
            .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?
            .0;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let header = |n: &str| {
            req.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (token, bearer) = (
            header("x-songbird-family-token"),
            header("x-songbird-family-bearer"),
        );
        let body = if matches!(method, Method::POST | Method::PUT) {
            bounded_body(req, 16 * 1024).await?.to_vec()
        } else {
            Vec::new()
        };
        let Some(fr) = plans::FamilyRequest::route(&method, &path, token, bearer, body) else {
            return Err(Refusal::Status(StatusCode::NOT_FOUND));
        };
        let range = registration::range_of(peer, 24, 48);
        Ok(profile_outcome(
            r.family(
                &self.registry,
                &self.hot,
                &range,
                fr,
                songbird_entitlements::today(),
            )
            .await,
        ))
    }

    /// `/v1/support/…` ([`support`]): signed or not; a ticket is read with its bearer.
    async fn support(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let s = self.support.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .ok_or_else(unavailable)?
            .0;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let bearer = req
            .headers()
            .get(support::BEARER_HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                let mut b = [0u8; 32];
                hex::decode_to_slice(v, &mut b).ok().map(|()| b)
            });
        let sr = match (&method, path.as_str()) {
            (&Method::GET, "/v1/support/key") => support::Request::Key,
            (&Method::POST, "/v1/support/tickets") => {
                if req.headers().contains_key(auth::AUTHORIZATION) {
                    // Signed: the account is attached (an appeal, priority on Plus).
                    let (device, body) = self.authenticate_body(req, support::MAX_BODY).await?;
                    let row = self
                        .registry
                        .device(&device)
                        .await
                        .map_err(|_| unavailable())?
                        .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
                    if row.pending {
                        return Ok(pending_refusal());
                    }
                    support::Request::Open(body.to_vec(), Some(row.account))
                } else {
                    support::Request::Open(
                        bounded_body(req, support::MAX_BODY).await?.to_vec(),
                        None,
                    )
                }
            }
            (m, p) => match (m, support::Request::ticket_path(p)) {
                (&Method::GET, Some((id, false))) => support::Request::Read(id, bearer),
                (&Method::DELETE, Some((id, false))) => support::Request::Close(id, bearer),
                (&Method::POST, Some((id, true))) => support::Request::Post(
                    id,
                    bearer,
                    bounded_body(req, support::MAX_BODY).await?.to_vec(),
                ),
                _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
            },
        };
        let opened = matches!(sr, support::Request::Open(..));
        let range = registration::range_of(peer, 24, 48);
        let out = s
            .serve(
                &self.registry,
                &self.hot,
                &range,
                sr,
                |a| self.plans.live.level(a),
                songbird_entitlements::today(),
            )
            .await;
        let mut resp = profile_outcome(out);
        if opened && resp.status() == StatusCode::OK {
            *resp.status_mut() = StatusCode::CREATED;
        }
        Ok(resp)
    }

    /// `POST /v1/invites/status`: which of the caller's invite codes are still live (D-0132).
    /// The body names codes by their hash; the answer says only live or gone, never who used one.
    async fn invite_status(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct B {
            codes: Vec<String>,
        }
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        if row.pending {
            return Ok(pending_refusal());
        }
        // A budget per account (keyed, never the ID): 60 asks an hour, burst 20 (D-0134).
        let record = {
            let k =
                aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, &self.push_index_secret);
            let mut ctx = aws_lc_rs::hmac::Context::with_key(&k);
            ctx.update(b"songbird.v1/invite-status\0");
            ctx.update(row.account.as_bytes());
            format!(
                "t:is:{}",
                hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
            )
        };
        match registration::Registration::gcra(&self.hot, &record, 60, 20, false).await {
            Ok(0) => {}
            Ok(wait) => return Ok(profile_outcome(profiles::Outcome::Throttled(wait))),
            Err(()) => return Err(unavailable()),
        }
        let bad = || {
            json(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": "body" }),
            )
        };
        let Ok(b) = serde_json::from_slice::<B>(&body) else {
            return Ok(bad());
        };
        if b.codes.is_empty() || b.codes.len() > 50 {
            return Ok(bad());
        }
        let mut out = Vec::with_capacity(b.codes.len());
        for c in &b.codes {
            let mut h = [0u8; 32];
            if hex::decode_to_slice(c, &mut h).is_err() {
                return Ok(bad());
            }
            let live = self
                .registry
                .invite_uses(&h)
                .await
                .map_err(|_| unavailable())?
                .is_some_and(|n| n > 0);
            out.push(live);
        }
        Ok(json(StatusCode::OK, &serde_json::json!({ "live": out })))
    }

    /// A device's account's standing (Good when the device is unknown: the request fails later).
    async fn standing_of_device(&self, device: &DeviceId) -> songbird_entitlements::Standing {
        match self.registry.device(device).await {
            Ok(Some(r)) => self.plans.live.standing(&r.account),
            _ => songbird_entitlements::Standing::Good,
        }
    }

    /// `/v1/kt/…` ([`kt`]): signed like every device request.
    async fn kt(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let t = self.kt.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let (device, body) = self.authenticate_json(req).await?;
        let account = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?
            .account;
        let out = match (&method, path.as_str()) {
            (&Method::GET, "/v1/kt/keys") => t.keys().await,
            (&Method::GET, "/v1/kt/head") => t.head().await,
            (&Method::POST, "/v1/kt/lookup") => {
                t.lookup(&self.registry, &self.hot, &account, &body).await
            }
            (&Method::POST, "/v1/kt/history") => {
                t.history(&self.registry, &self.hot, &account, &body).await
            }
            (&Method::POST, "/v1/kt/audit") => t.audit(&self.hot, &account, &body).await,
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(match out {
            kt::Outcome::Json(v) => json(StatusCode::OK, &v),
            kt::Outcome::Blob(b) => profile_outcome(profiles::Outcome::Blob(b)),
            kt::Outcome::Malformed(w) => profile_outcome(profiles::Outcome::Malformed(w)),
            kt::Outcome::NotFound => empty(StatusCode::NOT_FOUND),
            kt::Outcome::Throttled(w) => profile_outcome(profiles::Outcome::Throttled(w)),
            kt::Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
        })
    }

    /// An avatar object, streamed.
    async fn serve_avatar(&self, id: &[u8; 32]) -> Result<Response<Body>, Refusal> {
        self.serve_object(&profiles::avatar_name(id)).await
    }

    /// An object from `RustFS`, streamed.
    async fn serve_object(&self, name: &str) -> Result<Response<Body>, Refusal> {
        self.serve_from(&self.s3, name).await
    }

    /// An object from a bucket, streamed.
    async fn serve_from(
        &self,
        s3: &songbird_s3::Client,
        name: &str,
    ) -> Result<Response<Body>, Refusal> {
        let got = match s3.get_object(name, None).await {
            Ok(g) => g,
            Err(songbird_s3::Error::NotFound) => {
                return Err(Refusal::Status(StatusCode::NOT_FOUND));
            }
            Err(_) => return Err(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE)),
        };
        let mut r = Response::new(StreamBody::new(media::Frames(got.body)).boxed_unsync());
        r.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        r.headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from(got.length));
        Ok(r)
    }

    /// The GDPR export (M7, D-0056) with Prompt 4's stores: the request policy and how many
    /// blocks the account holds (their rows are keyed hashes; the list itself is the user's
    /// encrypted sync).
    async fn export(
        &self,
        account: &AccountId,
    ) -> Result<serde_json::Value, songbird_accounts::RegistryError> {
        let mut v = self
            .registry
            .export(account, Some((&self.push_store, &self.push_index_secret)))
            .await?;
        let off = self.registry.requests_off(account).await?;
        let blocks = match &self.requests {
            Some(r) => Some(self.registry.block_count(&r.owner(account)).await?),
            None => None,
        };
        // Backup vaults name no account (D-0130): nothing of them can be exported here; the
        // export says so in `not_attributable`.
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "message_requests".into(),
                serde_json::json!({
                    "from": if off { "nobody" } else { "everyone" },
                    "blocks": blocks,
                }),
            );
        }
        Ok(v)
    }

    /// `POST /v1/reports` ([`reports`], Prompt 4).
    /// `POST /v1/groups`, `/v1/groups/commit`, `/v1/groups/state` (Prompt 5, `groups.rs`).
    async fn groups(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let m = self.messages.as_ref().ok_or_else(unavailable)?;
        let g = m.groups.as_ref().ok_or_else(unavailable)?;
        let path = req.uri().path().to_owned();
        // Prompt 9 (D-0103): proof that the committer is a contact of every added account that
        // takes group adds from contacts only.
        let proofs = groups::AddProofs {
            add_key: header_key(&req, "group-add-access-key"),
            approvals: groups::AddProofs::approvals_from(
                req.headers()
                    .get("group-add-approval")
                    .and_then(|v| v.to_str().ok()),
            ),
        };
        let (device, body) = self.authenticate_body(req, groups::MAX_BODY).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        if row.pending {
            return Ok(device_outcome(devices::Outcome::Forbidden("pending")));
        }
        let budget = match path.as_str() {
            // Reads rebuild the tree: their own budget (D-0074).
            "/v1/groups/state" | "/v1/groups/log" | "/v1/communities/state" | "/v1/groups/ds" => {
                Some((g.read_record(&device), g.reads_per_sec, g.read_burst))
            }
            // Anything a link opens has its own, smaller budget per account (D-0071), shared by
            // its devices: links are not guessable, but a leaked one should not be tried at speed.
            "/v1/groups/links/open" | "/v1/groups/join" | "/v1/groups/requests" => {
                Some((g.link_record(&row.account), g.links_per_sec, g.link_burst))
            }
            // Validating a commit costs CPU: a per-device budget, shared by creations,
            // proposals, metadata writes and link and request management.
            _ => Some((g.rate_record(&device), g.commits_per_sec, g.commit_burst)),
        };
        if let Some((record, per_sec, burst)) = budget {
            let wait = self
                .gcra(&record, per_sec, burst)
                .await
                .map_err(|()| unavailable())?;
            if wait > 0 {
                return Ok(device_outcome(devices::Outcome::Throttled(wait)));
            }
        }
        let r = &self.registry;
        // A commit may remove devices from a live call (Prompt 7, §8.6): `version | group | …`.
        let committed = (path == "/v1/groups/commit")
            .then(|| body.get(1..17).and_then(|g| <[u8; 16]>::try_from(g).ok()))
            .flatten();
        let outcome = match path.as_str() {
            "/v1/groups" => g.create(r, m, device, body).await,
            "/v1/groups/commit" => g.commit(r, m, device, body, proofs).await,
            "/v1/groups/propose" => g.propose(r, m, device, body).await,
            "/v1/groups/meta" => g.meta(r, device, body).await,
            "/v1/groups/log" => g.log(r, m, device, body).await,
            "/v1/groups/links"
                if self.standing_of_device(&device).await
                    != songbird_entitlements::Standing::Good =>
            {
                devices::Outcome::Forbidden("standing")
            }
            "/v1/groups/links" => g.create_link(r, device, body).await,
            "/v1/groups/links/revoke" => g.revoke_link(r, device, body).await,
            "/v1/groups/links/open" => g.open(r, body).await,
            "/v1/groups/join" => g.join(r, m, device, body, proofs).await,
            "/v1/groups/requests" => g.request(r, m, device, body).await,
            "/v1/groups/requests/list" => g.requests(r, device, body).await,
            "/v1/groups/requests/decline" => g.decline(r, device, body).await,
            "/v1/groups/ds" => g.ds_key(&body),
            "/v1/communities" => g.create_community(r, device, body).await,
            "/v1/communities/groups/add" => g.add_community_group(r, device, body).await,
            "/v1/communities/groups/remove" => g.remove_community_group(r, device, body).await,
            "/v1/communities/meta" => g.community_meta(r, device, body).await,
            "/v1/communities/state" => g.community_state(r, m, device, body).await,
            "/v1/communities/delete" => g.delete_community(r, device, body).await,
            _ => g.state(&self.registry, m, device, body).await,
        };
        if let (Some(group), Some(c), devices::Outcome::Json(_)) =
            (committed, self.calls.as_ref(), &outcome)
        {
            c.reconcile_group(&self.hot, &self.registry, &group).await;
        }
        Ok(device_outcome(outcome))
    }

    /// `POST /v1/calls`, `/v1/calls/group`, `/v1/calls/join` ([`calls`], Prompt 7).
    async fn calls(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let c = self.calls.as_ref().ok_or_else(unavailable)?;
        let path = req.uri().path().to_owned();
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        if row.pending {
            return Ok(device_outcome(devices::Outcome::Forbidden("pending")));
        }
        // A suspended account starts no call and makes no link (D-0131); it may still answer.
        if self.plans.live.standing(&row.account) == songbird_entitlements::Standing::Suspended
            && matches!(
                path.as_str(),
                "/v1/calls" | "/v1/calls/group" | "/v1/calls/links"
            )
        {
            return Ok(device_outcome(devices::Outcome::Forbidden("suspended")));
        }
        let caller = calls::Caller {
            device,
            account: row.account,
        };
        let (hot, r) = (&self.hot, &self.registry);
        let outcome = match (path.as_str(), self.messages.as_ref()) {
            ("/v1/calls/join", _) => c.join(hot, r, caller, &body).await,
            ("/v1/calls/links", _) => c.create_link(hot, r, caller, &body).await,
            ("/v1/calls/links/join", _) => c.join_link(hot, r, caller, &body).await,
            ("/v1/calls/links/lobby", _) => c.admin_link(hot, r, caller, "lobby", &body).await,
            ("/v1/calls/links/admit", _) => c.admin_link(hot, r, caller, "admit", &body).await,
            ("/v1/calls/links/revoke", _) => c.admin_link(hot, r, caller, "revoke", &body).await,
            (_, None) => return Err(unavailable()),
            ("/v1/calls", Some(m)) => c.one_to_one(&self.hot, &self.queue, m, caller, &body).await,
            ("/v1/calls/group", Some(m)) => {
                c.group(&self.hot, &self.queue, m, &self.registry, caller, &body)
                    .await
            }
            _ => devices::Outcome::NotFound,
        };
        Ok(device_outcome(outcome))
    }

    async fn report(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let r = self.reports.as_ref().ok_or_else(unavailable)?;
        let (device, body) = self
            .authenticate_body(req, songbird_proto::report::MAX_LEN)
            .await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Ok(device_outcome(
            r.submit(&self.registry, &self.hot, &row, &body, now).await,
        ))
    }

    /// `/v1/blocks` and `/v1/account/requests` ([`requests`], Prompt 4).
    async fn requests(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let r = self.requests.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let out = match (&method, path.as_str()) {
            (&Method::PUT, "/v1/blocks") => r.block(&self.registry, &self.hot, &row, &body).await,
            (&Method::DELETE, "/v1/blocks") => r.unblock(&self.registry, &row, &body).await,
            (&Method::GET, "/v1/account/requests") => {
                requests::Requests::policy(&self.registry, &row).await
            }
            (&Method::PUT, "/v1/account/requests") => {
                requests::Requests::set_policy(&self.registry, &row, &body).await
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(device_outcome(out))
    }

    /// `/v1/account/…`: second factors ([`devices`], M4b), the export and the inactivity period
    /// (M7); the primary's to change.
    async fn factors(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let d = self.devices.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let primary = row.primary.then_some(row.account);
        let not_primary = || devices::Outcome::Forbidden("primary");
        let out = match (&method, path.as_str()) {
            (&Method::PUT, "/v1/account/pin") => match primary {
                Some(a) => d.set_pin(&self.registry, &a, &body).await,
                None => not_primary(),
            },
            (&Method::DELETE, "/v1/account/pin") => match primary {
                Some(a) => d.clear_pin(&self.registry, &a).await,
                None => not_primary(),
            },
            (&Method::POST, "/v1/account/passkeys/challenge") => match primary {
                Some(a) => d.passkey_challenge(&self.hot, &a).await,
                None => not_primary(),
            },
            (&Method::PUT, "/v1/account/passkeys") => match primary {
                Some(a) => d.add_passkey(&self.registry, &self.hot, &a, &body).await,
                None => not_primary(),
            },
            (&Method::DELETE, "/v1/account/passkeys") => match primary {
                Some(a) => d.delete_passkey(&self.registry, &a, &body).await,
                None => not_primary(),
            },
            (&Method::GET, "/v1/account/factors") => d.factors(&self.registry, &row.account).await,
            // Prompt 10: the primary hands the account over to its replacement.
            (&Method::POST, "/v1/account/handover") => match primary {
                Some(a) => {
                    d.handover_token(&self.registry, &self.hot, &a, &device)
                        .await
                }
                None => not_primary(),
            },
            // GDPR (M7): the export and the inactivity period, the primary's.
            (&Method::GET, "/v1/account/export") => match primary {
                Some(a) => match self.export(&a).await {
                    Ok(v) => devices::Outcome::Json(v),
                    Err(_) => devices::Outcome::Unavailable,
                },
                None => not_primary(),
            },
            (&Method::PUT, "/v1/account/inactivity") => match primary {
                Some(a) => {
                    let months = serde_json::from_slice::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v.get("months").and_then(serde_json::Value::as_u64))
                        .and_then(|m| u32::try_from(m).ok())
                        .filter(|m| songbird_accounts::gdpr::INACTIVITY_CHOICES.contains(m));
                    match months {
                        Some(m) => match self.registry.set_inactivity(&a, m).await {
                            Ok(()) => devices::Outcome::Done,
                            Err(_) => devices::Outcome::Unavailable,
                        },
                        None => devices::Outcome::Malformed("months"),
                    }
                }
                None => not_primary(),
            },
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(device_outcome(out))
    }

    /// Server-enforced settings (Prompt 9, M2, D-0103; [`account`]).
    async fn account_settings(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let out = match (&method, path.as_str()) {
            (&Method::GET, "/v1/account/settings") => account::get(&self.registry, &row).await,
            (&Method::PUT, "/v1/account/discoverable") => {
                account::set_discoverable(&self.registry, &row, &body).await
            }
            (&Method::PUT, "/v1/account/groups") => {
                account::set_groups(&self.registry, &row, &body).await
            }
            (&Method::PUT, "/v1/supervision") => {
                account::put_supervision(&self.registry, &row, &body).await
            }
            (&Method::GET, "/v1/supervision") => {
                account::get_supervision(&self.registry, &row).await
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(device_outcome(out))
    }

    /// `PUT /v1/supervision/update`, unsigned, throttled per IP range ([`account`], D-0108).
    async fn supervision_update(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        if req.method() != Method::PUT {
            return Err(Refusal::Status(StatusCode::METHOD_NOT_ALLOWED));
        }
        let p = self.profiles.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .map(|p| p.0)
            .ok_or_else(unavailable)?;
        if let Err(o) = p.policy_budget(&self.hot, peer).await {
            return Ok(profile_outcome(o));
        }
        let body = bounded_body(req, songbird_proto::supervision::MAX_POLICY_LEN).await?;
        Ok(device_outcome(
            account::update_supervision(&self.registry, &body).await,
        ))
    }

    /// `/v1/provisioning…` and `/v1/devices…` ([`devices`]).
    /// Account recovery (Prompt 10, M4, D-0123): unsigned, the new device has no account yet.
    async fn recovery(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let d = self.devices.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .map(|p| p.0)
            .ok_or_else(unavailable)?;
        let out = if req.uri().path() == "/v1/account/recover/token" {
            let body = bounded_body(req, 128).await?;
            d.recover_token(&self.registry, &self.hot, peer, &body)
                .await
        } else {
            let body = bounded_body(req, songbird_proto::devices::MAX_RECOVER_LEN).await?;
            d.recover(&self.registry, &self.hot, &self.server_name, peer, body)
                .await
        };
        Ok(device_outcome(out))
    }

    async fn devices(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let d = self.devices.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .map(|p| p.0)
            .ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let channel = path.strip_prefix("/v1/provisioning/").map(str::to_owned);
        let account_of = |device: DeviceId| async move {
            self.registry
                .device(&device)
                .await
                .map_err(|_| unavailable())?
                .map(|r| r.account)
                .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))
        };
        let out = match (&method, path.as_str(), channel) {
            (&Method::POST, "/v1/provisioning", None) => d.open_channel(&self.hot, peer).await,
            (&Method::GET, _, Some(c)) => d.take_message(&self.hot, peer, &c).await,
            (&Method::PUT, _, Some(c)) => {
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::devices::MAX_PROVISIONING_LEN)
                    .await?;
                let account = account_of(device).await?;
                d.put_message(&self.hot, &account, &c, &body).await
            }
            (&Method::POST, "/v1/devices/link-token", None) => {
                let (device, _) = self.authenticate_json(req).await?;
                let account = account_of(device).await?;
                d.link_token(&self.registry, &self.hot, &self.plans, &account, &device)
                    .await
            }
            (&Method::POST, "/v1/devices/link", None) => {
                let body = bounded_body(req, songbird_proto::devices::MAX_LINK_LEN).await?;
                d.link(
                    &self.registry,
                    &self.hot,
                    &self.plans,
                    self.messages.as_ref(),
                    &self.server_name,
                    peer,
                    body,
                )
                .await
            }
            (&Method::GET, "/v1/devices", None) => {
                let (device, _) = self.authenticate_json(req).await?;
                let account = account_of(device).await?;
                d.list_own(&self.registry, &account).await
            }
            (&Method::PUT, "/v1/devices/list", None) => {
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::devices::MAX_LIST_LEN)
                    .await?;
                let account = account_of(device).await?;
                d.put_list(
                    &self.registry,
                    &self.queue,
                    self.messages.as_ref(),
                    (&account, &device),
                    body,
                )
                .await
            }
            (&Method::POST, "/v1/devices/list/fetch", None) => {
                let (device, body) = self.authenticate_json(req).await?;
                let account = account_of(device).await?;
                d.fetch_list(&self.registry, &self.hot, &account, &body)
                    .await
            }
            (&Method::POST, "/v1/devices/identity", None) => {
                let (device, body) = self.authenticate_json(req).await?;
                let account = account_of(device).await?;
                d.fetch_identity(&self.registry, &self.hot, &account, &body)
                    .await
            }
            (&Method::DELETE, "/v1/devices/self", None) => {
                let (device, _) = self.authenticate_json(req).await?;
                d.remove_self(&self.registry, &self.queue, self.messages.as_ref(), &device)
                    .await
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(device_outcome(out))
    }

    /// `/v1/keys…` ([`keys`]): signed like every device request.
    async fn keys(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let k = self.keys.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .map(|p| p.0)
            .ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let out = match (&method, path.as_str()) {
            (&Method::PUT, "/v1/keys") => {
                // Large and signed over its digest: authenticated from the headers before the
                // body is read.
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::keys::MAX_UPLOAD_LEN)
                    .await?;
                k.upload(&self.registry, &device, body).await
            }
            (&Method::GET, "/v1/keys") => {
                let (device, _) = self.authenticate_json(req).await?;
                k.counts(&self.registry, &device).await
            }
            (&Method::PUT, "/v1/keypackages") => {
                let (device, body) = self
                    .authenticate_body(req, songbird_proto::keys::MAX_KEY_PACKAGE_UPLOAD_LEN)
                    .await?;
                k.upload_key_packages(&self.registry, &device, body).await
            }
            (&Method::GET, "/v1/keypackages") => {
                let (device, _) = self.authenticate_json(req).await?;
                k.key_package_counts(&self.registry, &device).await
            }
            (&Method::POST, "/v1/keypackages/fetch") => {
                let (device, body) = self.authenticate_json(req).await?;
                let account = self
                    .registry
                    .device(&device)
                    .await
                    .map_err(|_| unavailable())?
                    .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?
                    .account;
                k.fetch_key_packages(
                    &self.registry,
                    &self.hot,
                    self.messages.as_ref(),
                    (&account, &device),
                    peer,
                    &body,
                )
                .await
            }
            (&Method::POST, "/v1/keys/fetch") => {
                let (device, body) = self.authenticate_json(req).await?;
                let account = self
                    .registry
                    .device(&device)
                    .await
                    .map_err(|_| unavailable())?
                    .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?
                    .account;
                k.fetch(
                    &self.registry,
                    &self.hot,
                    self.messages.as_ref(),
                    (&account, &device),
                    peer,
                    &body,
                )
                .await
            }
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(key_outcome(out))
    }

    /// `/v1/username…` ([`usernames`]): signed like every device request.
    async fn usernames(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let u = self.usernames.as_ref().ok_or_else(unavailable)?;
        let peer = req
            .extensions()
            .get::<registration::Peer>()
            .map(|p| p.0)
            .ok_or_else(unavailable)?;
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let handle = path
            .strip_prefix("/v1/username/link/")
            .map(|h| {
                let mut out = [0u8; 16];
                hex::decode_to_slice(h, &mut out)
                    .map(|()| out)
                    .map_err(|_| Refusal::Status(StatusCode::NOT_FOUND))
            })
            .transpose()?;
        let (device, body) = self.authenticate_json(req).await?;
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        // A pending device (linked, not yet vouched for by a signed list) changes nothing
        // account-wide (D-0052); it may look up and read links.
        let changes = !(method == Method::GET || path == "/v1/username/lookup");
        if row.pending && changes {
            return Ok(pending_refusal());
        }
        let account = row.account;
        let (r, hot) = (self.registry.as_ref(), &self.hot);
        let done = |res: Result<(), songbird_accounts::RegistryError>| match res {
            Ok(()) => Ok(empty(StatusCode::NO_CONTENT)),
            Err(_) => Err(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE)),
        };
        let out = match (&method, path.as_str(), handle) {
            (&Method::PUT, "/v1/username", None) => u.claim(r, hot, &account, &body).await,
            (&Method::DELETE, "/v1/username", None) => {
                return done(r.delete_username(&account).await);
            }
            (&Method::POST, "/v1/username/lookup", None) => {
                u.lookup(r, hot, &account, peer, &body).await
            }
            (&Method::PUT, "/v1/username/key", None) => u.set_key(r, &account, &body).await,
            (&Method::DELETE, "/v1/username/key", None) => {
                return done(r.set_username_key(&account, None).await);
            }
            (&Method::PUT, "/v1/username/link", None) => u.put_link(r, &account, &body).await,
            (&Method::DELETE, "/v1/username/link", None) => {
                return done(r.delete_username_link(&account).await);
            }
            (&Method::GET, _, Some(h)) => u.get_link(r, hot, &account, peer, &h).await,
            _ => return Err(Refusal::Status(StatusCode::NOT_FOUND)),
        };
        Ok(username_outcome(out))
    }

    /// `/v1/invites`: signed like every device request.
    async fn invites(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let unavailable = || Refusal::Status(StatusCode::SERVICE_UNAVAILABLE);
        let r = self.registration.as_ref().ok_or_else(unavailable)?;
        let method = req.method().clone();
        let (device, body) = self.authenticate_json(req).await?;
        let parsed: registration::InviteBody = if body.is_empty() {
            registration::InviteBody::default()
        } else {
            serde_json::from_slice(&body).map_err(|_| Refusal::Status(StatusCode::BAD_REQUEST))?
        };
        let row = self
            .registry
            .device(&device)
            .await
            .map_err(|_| unavailable())?
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        // Invites spend the account's allowance: not from a pending device (D-0052).
        if row.pending {
            return Ok(pending_refusal());
        }
        let account = row.account;
        match method {
            Method::POST => match r.create_invite(&self.registry, &account, &parsed).await {
                Ok(Some((code, ttl))) => Ok(json(
                    StatusCode::CREATED,
                    &serde_json::json!({ "code": code, "expires_in_secs": ttl }),
                )),
                Ok(None) => Ok(json(
                    StatusCode::FORBIDDEN,
                    &serde_json::json!({ "error": "allowance" }),
                )),
                Err(_) => Err(unavailable()),
            },
            Method::GET => {
                let left = self
                    .registry
                    .invite_allowance(&account)
                    .await
                    .map_err(|_| unavailable())?;
                Ok(json(
                    StatusCode::OK,
                    &serde_json::json!({
                        "allowance": left,
                        "invite_only": r.settings.invite_only,
                    }),
                ))
            }
            _ => {
                let code = parsed
                    .code
                    .as_deref()
                    .and_then(songbird_proto::registration::invite::parse)
                    .ok_or(Refusal::Status(StatusCode::BAD_REQUEST))?;
                self.registry
                    .revoke_invite(&songbird_accounts::invite_hash(&code))
                    .await
                    .map_err(|_| unavailable())?;
                // The same answer whether or not the code existed.
                Ok(empty(StatusCode::NO_CONTENT))
            }
        }
    }

    /// Checks the signature over the request and its body (read here, bounded); returns the
    /// device and the body.
    async fn authenticate_json(
        &self,
        req: Request<Incoming>,
    ) -> Result<(DeviceId, Bytes), Refusal> {
        let (parts, body) = req.into_parts();
        let declared = parts
            .headers
            .get(auth::CONTENT_SHA256)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                let mut h = [0u8; 32];
                hex::decode_to_slice(v, &mut h).map(|()| h)
            })
            .transpose()
            .map_err(|_| Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let body = Limited::new(body, MAX_JSON)
            .collect()
            .await
            .map_err(|_| Refusal::Status(StatusCode::PAYLOAD_TOO_LARGE))?
            .to_bytes();
        // A body must be signed; no body signs as zeros.
        let hash = match declared {
            Some(h) if h == auth::body_sha256(&body) => h,
            None if body.is_empty() => [0; 32],
            _ => return Err(Refusal::Status(StatusCode::UNAUTHORIZED)),
        };
        let device = self
            .verify(&parts.method, parts.uri.path(), &parts.headers, &hash)
            .await?;
        Ok((device, body))
    }

    /// A signed body read only after its signature verified: the signature covers the declared
    /// digest, so the headers alone authenticate the request (time, nonce, key, not disabled);
    /// then at most the declared `Content-Length` (itself at most `max`) is read and must match
    /// the digest.
    async fn authenticate_body(
        &self,
        req: Request<Incoming>,
        max: usize,
    ) -> Result<(DeviceId, Bytes), Refusal> {
        let (parts, body) = req.into_parts();
        let length = parts
            .headers
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or(Refusal::Status(StatusCode::LENGTH_REQUIRED))?;
        if length == 0 || length > max {
            return Err(Refusal::Status(StatusCode::PAYLOAD_TOO_LARGE));
        }
        let mut declared = [0u8; 32];
        parts
            .headers
            .get(auth::CONTENT_SHA256)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| hex::decode_to_slice(v, &mut declared).ok())
            .ok_or(Refusal::Status(StatusCode::UNAUTHORIZED))?;
        let device = self
            .verify(&parts.method, parts.uri.path(), &parts.headers, &declared)
            .await?;
        let body = Limited::new(body, length)
            .collect()
            .await
            .map_err(|_| Refusal::Status(StatusCode::PAYLOAD_TOO_LARGE))?
            .to_bytes();
        if body.len() != length || auth::body_sha256(&body) != declared {
            return Err(Refusal::Status(StatusCode::UNAUTHORIZED));
        }
        Ok((device, body))
    }

    /// `PUT /v1/messages` ([`messages`]).
    async fn put_message(&self, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
        let received = std::time::Instant::now();
        let m = self
            .messages
            .as_ref()
            .ok_or(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        let (device, body) = self.authenticate_body(req, m.limits.max_send_bytes).await?;
        let frame =
            messages::decode(&body, &m.limits).ok_or(Refusal::Status(StatusCode::BAD_REQUEST))?;
        let req_id = frame.req_id;
        let group = matches!(
            frame.payload,
            songbird_proto::frame::SendPayload::Shared { .. }
        );
        let status = match self.rate(m, &device, group).await {
            Ok(0) => m.path.submit(&device, frame, received).await,
            Ok(wait_us) => {
                m.metrics.sends_rejected.inc();
                messages::rate_limited(wait_us)
            }
            Err(()) => {
                // Fail closed: without the limiter, nothing is sent.
                self.metrics.errors.inc();
                m.metrics.sends_rejected.inc();
                songbird_proto::frame::SendStatus::Rejected {
                    reason: songbird_proto::frame::RejectReason::Unavailable,
                    retry_after_ms: songbird_send::RETRY_MS,
                }
            }
        };
        let mut r = Response::new(
            Full::new(messages::result_frame(req_id, status))
                .map_err(|e| match e {})
                .boxed_unsync(),
        );
        r.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        Ok(r)
    }

    /// The device's send rate (GCRA in `valkey-hot`), and for a group send its group rate too:
    /// 0 if allowed, else microseconds to wait.
    async fn rate(
        &self,
        m: &messages::Messages,
        device: &DeviceId,
        group: bool,
    ) -> Result<u64, ()> {
        let wait = self
            .gcra(
                &messages::rate_record(&m.rate_key, device),
                m.sends_per_sec,
                m.send_burst,
            )
            .await?;
        if wait > 0 || !group {
            return Ok(wait);
        }
        self.gcra(
            &messages::group_rate_record(&m.rate_key, device),
            m.group_sends_per_sec,
            m.group_send_burst,
        )
        .await
    }

    async fn gcra(&self, record: &str, per_sec: u32, burst: u32) -> Result<u64, ()> {
        let interval_us = 1_000_000 / u64::from(per_sec.max(1));
        let tolerance_us = interval_us.saturating_mul(u64::from(burst.max(1)));
        self.hot
            .query::<i64>(
                redis::cmd("EVAL")
                    .arg(messages::GCRA)
                    .arg(1)
                    .arg(record)
                    .arg(interval_us)
                    .arg(tolerance_us),
            )
            .await
            .map(|w| u64::try_from(w).unwrap_or(0))
            .map_err(|_| ())
    }

    async fn verify(
        &self,
        method: &Method,
        path: &str,
        headers: &hyper::HeaderMap,
        hash: &[u8; 32],
    ) -> Result<DeviceId, Refusal> {
        let unauthorized = || Refusal::Status(StatusCode::UNAUTHORIZED);
        let s = headers
            .get(auth::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(auth::Signed::parse)
            .ok_or_else(unauthorized)?;
        if !auth::fresh(s.ts_ms) {
            return Err(unauthorized());
        }
        let row = self
            .registry
            .device(&s.device)
            .await
            .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        let account = row.map(|r| r.account);
        let key = row.filter(|r| !r.disabled).map(|r| r.key);
        let t = auth::transcript(
            &self.server_name,
            method.as_str(),
            path,
            s.ts_ms,
            &s.nonce,
            &s.device,
            hash,
        );
        if !auth::verify(key.as_ref(), &t, &s.sig) {
            return Err(unauthorized());
        }
        // Single use: a replay within the window finds the record.
        let fresh: redis::Value = self
            .hot
            .query(
                redis::cmd("SET")
                    .arg(auth::nonce_key(&s.device, &s.nonce))
                    .arg(1)
                    .arg("NX")
                    .arg("PX")
                    .arg(u64::try_from(2 * auth::MAX_SKEW.as_millis()).unwrap_or(u64::MAX)),
            )
            .await
            .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        if matches!(fresh, redis::Value::Nil) {
            return Err(unauthorized());
        }
        if let Some(a) = account {
            self.touch(&a).await;
        }
        Ok(s.device)
    }

    /// Records that the account is in use, at most once a day (inactivity deletion, M7): a
    /// keyed daily mark in `valkey-hot`, then the registry. Best effort.
    async fn touch(&self, account: &AccountId) {
        let k = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, &self.push_index_secret);
        let mut ctx = aws_lc_rs::hmac::Context::with_key(&k);
        ctx.update(b"songbird.v1/last-seen\0");
        ctx.update(account.as_bytes());
        let mark = format!(
            "ls:{}",
            hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
        );
        let first: Result<redis::Value, _> = self
            .hot
            .query(
                redis::cmd("SET")
                    .arg(mark)
                    .arg(1)
                    .arg("NX")
                    .arg("EX")
                    .arg(86_400),
            )
            .await;
        if matches!(first, Ok(redis::Value::Okay))
            && let Err(e) = self.registry.touch(account).await
        {
            tracing::warn!(error = %e, "last-seen not recorded");
        }
    }

    async fn put_push(&self, device: DeviceId, body: &[u8]) -> Result<Response<Body>, Refusal> {
        let reg = serde_json::from_slice::<PushBody>(body)
            .ok()
            .and_then(PushBody::registration)
            .ok_or(Refusal::Status(StatusCode::BAD_REQUEST))?;
        let index = push_index(&self.push_index_secret, &device);
        let sealed = push_token::seal(&self.push_public, &index, &reg)
            .map_err(|_| Refusal::Status(StatusCode::BAD_REQUEST))?;
        self.push_store
            .put(&index, &sealed)
            .await
            .map_err(|_| Refusal::Status(StatusCode::SERVICE_UNAVAILABLE))?;
        self.metrics.push_registrations.inc();
        Ok(empty(StatusCode::NO_CONTENT))
    }

    async fn delete_account(&self, device: DeviceId) -> Result<Response<Body>, Refusal> {
        // Only the primary may delete the account (D-0052): a linked device, above all a pending
        // one the primary has not vouched for, could otherwise delete it irreversibly.
        match self
            .registry
            .begin_account_deletion_by(&device, |account| {
                Job::new(Work::DeleteAccount { account: *account }).ok()
            })
            .await
        {
            Ok(_) => {}
            Err(songbird_accounts::RegistryError::NotPrimary) => {
                return Ok(json(
                    StatusCode::FORBIDDEN,
                    &serde_json::json!({ "error": "primary" }),
                ));
            }
            Err(_) => return Err(Refusal::Status(StatusCode::SERVICE_UNAVAILABLE)),
        }
        self.metrics.deletions.inc();
        // Relay now; the periodic relay retries if this fails.
        if let Err(e) = songbird_jobs::relay(self.registry.as_ref(), &self.queue).await {
            tracing::warn!(error = %e, "outbox relay deferred");
        }
        Ok(empty(StatusCode::ACCEPTED))
    }
}

/// The HTTP answer for a profile outcome.
fn profile_outcome(o: profiles::Outcome) -> Response<Body> {
    use profiles::Outcome;
    let bytes = |b: Bytes| {
        let mut r = Response::new(Full::new(b).map_err(|e| match e {}).boxed_unsync());
        r.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        r
    };
    match o {
        Outcome::Done => empty(StatusCode::NO_CONTENT),
        Outcome::Json(v) => json(StatusCode::OK, &v),
        Outcome::Blob(b) | Outcome::Sent(b) => bytes(b),
        Outcome::Malformed(what) => json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": what }),
        ),
        Outcome::Unauthorized => empty(StatusCode::UNAUTHORIZED),
        Outcome::NotFound => empty(StatusCode::NOT_FOUND),
        Outcome::Conflict(what) => {
            json(StatusCode::CONFLICT, &serde_json::json!({ "error": what }))
        }
        Outcome::ConflictJson(v) => json(StatusCode::CONFLICT, &v),
        Outcome::Forbidden(what) => {
            json(StatusCode::FORBIDDEN, &serde_json::json!({ "error": what }))
        }
        Outcome::Throttled(wait_us) => {
            let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
            let secs = wait_us.div_ceil(1_000_000).max(1);
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// A 16-byte key in a header, as 32 hex characters.
fn header_key(req: &Request<Incoming>, name: &str) -> Option<[u8; 16]> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let mut k = [0u8; 16];
            hex::decode_to_slice(v, &mut k).ok().map(|()| k)
        })
}

/// 403 for an account-wide change from a pending device (D-0052).
fn pending_refusal() -> Response<Body> {
    json(
        StatusCode::FORBIDDEN,
        &serde_json::json!({ "error": "pending" }),
    )
}

/// A body of at most `max` bytes, read only after `Content-Length` says it fits.
async fn bounded_body(req: Request<Incoming>, max: usize) -> Result<Bytes, Refusal> {
    let length = req
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or(Refusal::Status(StatusCode::LENGTH_REQUIRED))?;
    if length == 0 || length > max {
        return Err(Refusal::Status(StatusCode::PAYLOAD_TOO_LARGE));
    }
    let body = Limited::new(req.into_body(), length)
        .collect()
        .await
        .map_err(|_| Refusal::Status(StatusCode::PAYLOAD_TOO_LARGE))?
        .to_bytes();
    if body.len() != length {
        return Err(Refusal::Status(StatusCode::BAD_REQUEST));
    }
    Ok(body)
}

/// The HTTP answer for a device outcome.
fn device_outcome(o: devices::Outcome) -> Response<Body> {
    use devices::Outcome;
    match o {
        Outcome::Done => empty(StatusCode::NO_CONTENT),
        Outcome::Json(v) => json(StatusCode::OK, &v),
        Outcome::Accepted(v) => json(StatusCode::ACCEPTED, &v),
        Outcome::Blob(b) => {
            let mut r = Response::new(Full::new(b).map_err(|e| match e {}).boxed_unsync());
            r.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            r
        }
        Outcome::Malformed(what) => json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": what }),
        ),
        Outcome::NotFound => empty(StatusCode::NOT_FOUND),
        Outcome::Conflict(what) => {
            json(StatusCode::CONFLICT, &serde_json::json!({ "error": what }))
        }
        Outcome::Stale(epoch) => json(
            StatusCode::CONFLICT,
            &serde_json::json!({ "error": "stale", "epoch": epoch }),
        ),
        Outcome::Forbidden(what) => {
            json(StatusCode::FORBIDDEN, &serde_json::json!({ "error": what }))
        }
        Outcome::Locked(left) => {
            let mut r = json(
                StatusCode::FORBIDDEN,
                &serde_json::json!({ "error": "locked", "retry_after": left }),
            );
            if let Some(secs) = left
                && let Ok(v) = HeaderValue::from_str(&secs.to_string())
            {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Throttled(wait_us) => {
            let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
            let secs = wait_us.div_ceil(1_000_000).max(1);
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// The HTTP answer for a key directory outcome.
fn key_outcome(o: keys::Outcome) -> Response<Body> {
    use keys::Outcome;
    match o {
        Outcome::Done => empty(StatusCode::NO_CONTENT),
        Outcome::Counts(ec, kem) => {
            json(StatusCode::OK, &serde_json::json!({ "ec": ec, "kem": kem }))
        }
        Outcome::KeyPackageCounts(n, lr) => json(
            StatusCode::OK,
            &serde_json::json!({ "single_use": n, "last_resort": lr }),
        ),
        Outcome::Bundles(b) => {
            let mut r = Response::new(Full::new(b).map_err(|e| match e {}).boxed_unsync());
            r.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            r
        }
        Outcome::Malformed(what) => json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": what }),
        ),
        Outcome::NotFound => empty(StatusCode::NOT_FOUND),
        Outcome::Conflict(what) => {
            json(StatusCode::CONFLICT, &serde_json::json!({ "error": what }))
        }
        Outcome::Throttled(wait_us) => {
            let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
            let secs = wait_us.div_ceil(1_000_000).max(1);
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// The HTTP answer for a username outcome.
fn username_outcome(o: usernames::Outcome) -> Response<Body> {
    use usernames::Outcome;
    match o {
        Outcome::Done => empty(StatusCode::NO_CONTENT),
        Outcome::Found(a) => json(
            StatusCode::OK,
            &serde_json::json!({ "account": a.to_hex_unredacted() }),
        ),
        Outcome::Link(h) => json(
            StatusCode::OK,
            &serde_json::json!({ "handle": hex::encode(h) }),
        ),
        Outcome::Blob(b) => {
            let mut r = Response::new(Full::new(b).map_err(|e| match e {}).boxed_unsync());
            r.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            r
        }
        Outcome::Malformed(what) => json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": what }),
        ),
        Outcome::NotFound => empty(StatusCode::NOT_FOUND),
        Outcome::Taken => json(
            StatusCode::CONFLICT,
            &serde_json::json!({ "error": "taken" }),
        ),
        Outcome::NoUsername => json(
            StatusCode::CONFLICT,
            &serde_json::json!({ "error": "no username" }),
        ),
        Outcome::Throttled(wait_us) => {
            let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
            let secs = wait_us.div_ceil(1_000_000).max(1);
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// The HTTP answer for a registration outcome.
fn outcome(o: registration::Outcome) -> Response<Body> {
    use registration::Outcome;
    match o {
        Outcome::Challenge(b) => {
            let mut r = Response::new(Full::new(b).map_err(|e| match e {}).boxed_unsync());
            r.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            r
        }
        Outcome::Registered { account, device } => json(
            StatusCode::CREATED,
            &serde_json::json!({
                "account": account.to_hex_unredacted(),
                "device": device.to_hex_unredacted(),
            }),
        ),
        Outcome::Malformed(what) => json(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": what }),
        ),
        Outcome::Challenge403 => json(
            StatusCode::FORBIDDEN,
            &serde_json::json!({ "error": "challenge" }),
        ),
        Outcome::Invite403 => json(
            StatusCode::FORBIDDEN,
            &serde_json::json!({ "error": "invite" }),
        ),
        Outcome::Throttled(wait_us) => {
            let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
            let secs = wait_us.div_ceil(1_000_000).max(1);
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
            }
            r
        }
        Outcome::Unavailable => empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// An attachment's object name in the store: `a/` and its 64 hex characters (no identifiers).
#[must_use]
pub fn object_name(o: &ObjectKey) -> String {
    let mut name = String::with_capacity(66);
    name.push_str("a/");
    name.push_str(&o.to_hex_unredacted());
    name
}

/// 429 with `Retry-After` in whole seconds (at least one) for a wait in microseconds.
fn retry_after(wait_us: u64) -> Response<Body> {
    let mut r = empty(StatusCode::TOO_MANY_REQUESTS);
    let secs = wait_us.div_ceil(1_000_000).max(1);
    if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
        r.headers_mut().insert(hyper::header::RETRY_AFTER, v);
    }
    r
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    /// Every metric name passes the telemetry layer's redaction rules (invariant 6): a refused
    /// name stops the service at start-up.
    #[test]
    fn every_metric_name_is_accepted() {
        let mut m = songbird_service::telemetry::Metrics::new("api", "test");
        super::ApiMetrics::default().register(&mut m).unwrap();
    }
}
