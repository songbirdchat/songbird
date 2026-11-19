//! Attachments (Prompt 2, D-0036; Prompt 6, D-0079 to D-0081): upload grants, single and
//! resumable uploads to `RustFS`, and downloads.
//!
//! | Request | What it does |
//! | --- | --- |
//! | `POST /v1/attachments` | A grant: `{"size", "fetches"}` → `{"id", "part_size", "parts"}`. `size` must be a length the attachment format produces (PROTOCOL §7.1), within the plan's limit and the account's budgets |
//! | `PUT /v1/attachments/<id>` | The whole ciphertext in one request |
//! | `PUT /v1/attachments/<id>/parts/<n>` | Part `n` (1-based) of a resumable upload, exactly its length |
//! | `GET /v1/attachments/<id>/parts` | The parts received: `{"part_size", "parts": [n, …]}` |
//! | `POST /v1/attachments/<id>/complete` | Assembles the parts; the grant becomes the attachment |
//! | `GET /v1/attachments/<id>` | A download |
//!
//! Every upload request is signed by the granted device (the body is the client's own
//! ciphertext and is not signed, only held to its exact length). A part streams straight into an
//! S3 `UploadPart`; nothing is buffered. Each part keeps the grant alive for its idle time, never
//! past [`songbird_accounts::GRANT_MAX_AGE`]. Object IDs never reach a log line.

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::hmac;
use bytes::Bytes;
use futures_core::Stream;
use http_body_util::BodyExt as _;
use hyper::body::Incoming;
use hyper::header::{CONTENT_LENGTH, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use serde::Deserialize;
use songbird_accounts::{Grant, PendingUpload};
use songbird_jobs::ObjectKey;
use songbird_types::{AccountId, DeviceId};
use songbird_valkey::ValkeyConn;

use crate::config::{MediaBudgets, MediaDownloads};
use crate::{App, Body, Refusal, empty, json, object_name};

/// Budget units: the daily volume is counted in MiB.
const UNIT: u64 = 1 << 20;

fn status(s: StatusCode) -> Refusal {
    Refusal::Status(s)
}

/// The grant request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRequest {
    /// The ciphertext length.
    pub size: u64,
    /// Recipient fetches due.
    pub fetches: u32,
}

impl GrantRequest {
    /// Decodes and checks a grant request against the format and `max_bytes` / `max_fetches`.
    /// `None` for anything else: unknown fields, a length the format never produces, zero or
    /// too many fetches.
    #[must_use]
    pub fn decode(body: &[u8], max_bytes: u64, max_fetches: u32) -> Option<Self> {
        let g: GrantRequest = serde_json::from_slice(body).ok()?;
        (songbird_proto::media::padded_from_ciphertext(g.size).is_some()
            && g.size <= max_bytes
            && (1..=max_fetches).contains(&g.fetches))
        .then_some(g)
    }
}

/// The MAC of a media URL: HMAC-SHA256 over the object, expiry and size, truncated to 16 bytes.
fn url_mac(key: &hmac::Key, object: &ObjectKey, expires: u64, size: u64) -> [u8; 16] {
    let mut ctx = hmac::Context::with_key(key);
    ctx.update(b"songbird.v1/media-url\0");
    ctx.update(&object.0);
    ctx.update(&expires.to_be_bytes());
    ctx.update(&size.to_be_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(ctx.sign().as_ref().get(..16).unwrap_or(&[0; 16]));
    out
}

/// A media URL's query: `e=<expiry>&z=<size>&s=<32 hex>`, in that order, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaQuery {
    /// Expiry, Unix seconds.
    pub expires: u64,
    /// The object's size.
    pub size: u64,
    /// The MAC.
    pub mac: [u8; 16],
}

impl MediaQuery {
    /// Parses a media URL's query strictly; `None` for anything else.
    #[must_use]
    pub fn parse(query: &str) -> Option<Self> {
        if query.len() > 96 {
            return None;
        }
        let mut it = query.split('&');
        let num = |p: Option<&str>, k: &str| {
            p?.strip_prefix(k)
                .filter(|v| {
                    !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit())
                })?
                .parse::<u64>()
                .ok()
        };
        let expires = num(it.next(), "e=")?;
        let size = num(it.next(), "z=")?;
        let s = it.next()?.strip_prefix("s=")?;
        if it.next().is_some() || s.len() != 32 {
            return None;
        }
        let mut mac = [0u8; 16];
        hex::decode_to_slice(s, &mut mac).ok()?;
        Some(Self { expires, size, mac })
    }
}

/// What a `Range` header asks of an object of `size` bytes (RFC 9110 §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeAsk {
    /// The whole object (no header, or one this server ignores: several ranges, bad syntax).
    Whole,
    /// The inclusive byte range.
    Part(u64, u64),
    /// Nothing satisfiable: 416.
    Unsatisfiable,
}

/// Reads a `Range` header: one `bytes=` range (`a-b`, `a-`, `-n`); anything else is ignored.
#[must_use]
pub fn parse_range(header: Option<&str>, size: u64) -> RangeAsk {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeAsk::Whole;
    };
    if spec.contains(',') || spec.len() > 48 {
        return RangeAsk::Whole;
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return RangeAsk::Whole;
    };
    let digits = |s: &str| {
        (!s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    match (a.is_empty(), digits(a), digits(b)) {
        // `-n`: the last n bytes.
        (true, _, Some(n)) => {
            if n == 0 || size == 0 {
                RangeAsk::Unsatisfiable
            } else {
                RangeAsk::Part(size.saturating_sub(n), size - 1)
            }
        }
        (false, Some(first), last) if b.is_empty() || last.is_some() => {
            if last.is_some_and(|l| l < first) {
                return RangeAsk::Whole;
            }
            if first >= size {
                return RangeAsk::Unsatisfiable;
            }
            RangeAsk::Part(first, last.map_or(size - 1, |l| l.min(size - 1)))
        }
        _ => RangeAsk::Whole,
    }
}

/// How many parts an upload of `size` bytes has, in parts of `part_size`.
#[must_use]
pub fn part_count(size: u64, part_size: u64) -> u64 {
    size.div_ceil(part_size.max(1)).max(1)
}

/// The exact length of part `n` (1-based), or `None` past the last.
#[must_use]
pub fn part_len(size: u64, part_size: u64, n: u64) -> Option<u64> {
    (n >= 1 && n <= part_count(size, part_size))
        .then(|| size.saturating_sub((n - 1) * part_size).min(part_size))
}

/// `[api.media]`: the keyed budget records, and (later milestones) download URLs, packs and
/// GIFs.
pub struct Media {
    settings: MediaBudgets,
    downloads: MediaDownloads,
    record_key: hmac::Key,
    url_key: hmac::Key,
    gifs: Option<crate::gifs::Gifs>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// A key for one purpose, derived from the media secret.
fn derive(secret: &[u8; 32], purpose: &[u8]) -> [u8; 32] {
    let k = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut out = [0u8; 32];
    out.copy_from_slice(hmac::sign(&k, purpose).as_ref());
    out
}

impl Media {
    /// From the settings and the media secret.
    #[must_use]
    pub fn new(settings: MediaBudgets, downloads: MediaDownloads, secret: &[u8; 32]) -> Self {
        Self {
            settings,
            downloads,
            record_key: hmac::Key::new(
                hmac::HMAC_SHA256,
                &derive(secret, b"songbird.v1/media-records"),
            ),
            url_key: hmac::Key::new(hmac::HMAC_SHA256, &derive(secret, b"songbird.v1/media-url")),
            gifs: None,
        }
    }

    /// With the GIF proxy.
    #[must_use]
    pub fn with_gifs(mut self, gifs: crate::gifs::Gifs) -> Self {
        self.gifs = Some(gifs);
        self
    }

    /// The GIF proxy, if configured.
    #[must_use]
    pub fn gifs(&self) -> Option<&crate::gifs::Gifs> {
        self.gifs.as_ref()
    }

    /// How long a signed media URL lives, seconds.
    #[must_use]
    pub fn url_ttl(&self) -> u64 {
        self.downloads.url_ttl_secs
    }

    /// The signed media URL for `object` of `size` bytes, valid until `expires` (Unix seconds).
    #[must_use]
    pub fn media_url(&self, object: &ObjectKey, size: u64, expires: u64) -> String {
        // Built without `format!`: an object ID is never formatted where a log line could be
        // (invariant 6; the semgrep rule `songbird-unredacted-id-in-format`).
        let mut url = String::with_capacity(160);
        url.push_str("/v1/media/");
        url.push_str(&object.to_hex_unredacted());
        url.push_str("?e=");
        url.push_str(&expires.to_string());
        url.push_str("&z=");
        url.push_str(&size.to_string());
        url.push_str("&s=");
        url.push_str(&hex::encode(url_mac(&self.url_key, object, expires, size)));
        url
    }

    /// Checks a media URL's signature (constant time) and expiry; its object size if valid.
    #[must_use]
    pub fn check_url(&self, object: &ObjectKey, q: &MediaQuery, now: u64) -> Option<u64> {
        let mac = url_mac(&self.url_key, object, q.expires, q.size);
        (aws_lc_rs::constant_time::verify_slices_are_equal(&mac, &q.mac).is_ok()
            && q.expires > now
            && q.expires <= now.saturating_add(self.downloads.url_ttl_secs + 60))
        .then_some(q.size)
    }

    /// Spends one request of the IP range's download budget (`kind`: `d` redirects, `f` fetch
    /// confirmations). `Ok(0)` when allowed, else the wait in microseconds.
    pub(crate) async fn admit_range(
        &self,
        hot: &ValkeyConn,
        kind: &str,
        ip: std::net::IpAddr,
    ) -> Result<u64, ()> {
        let d = &self.downloads;
        let range = crate::registration::range_of(ip, d.ipv4_prefix, d.ipv6_prefix);
        let mut ctx = hmac::Context::with_key(&self.record_key);
        ctx.update(kind.as_bytes());
        ctx.update(&[1]);
        ctx.update(&range);
        let record = format!(
            "m:{kind}:{}",
            hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
        );
        // Signed-URL requests (`v`: every range a player asks for) have a budget of their own.
        // Map tiles (`t`, D-0142) share it: a map view asks for a few dozen tiles at once.
        let (per_hour, burst) = if kind == "v" || kind == "t" {
            (d.serve_per_hour, d.serve_burst)
        } else {
            (d.per_hour, d.burst)
        };
        Self::spend(
            hot,
            &record,
            3_600_000_000 / u64::from(per_hour.max(1)),
            u64::from(burst),
            1,
        )
        .await
    }

    /// Whether this is the first confirmation of `object` from this address (an IPv4 address,
    /// an IPv6 /64): one source counts once (D-0084), so one recipient cannot confirm every
    /// fetch and delete a file before the others have it. Kept in `valkey-hot` under a keyed
    /// name for the attachment's lifetime.
    pub(crate) async fn first_confirmation(
        &self,
        hot: &ValkeyConn,
        object: &ObjectKey,
        ip: std::net::IpAddr,
    ) -> Result<bool, ()> {
        let source = crate::registration::range_of(ip, 32, 64);
        let mut ctx = hmac::Context::with_key(&self.record_key);
        ctx.update(b"c\0");
        ctx.update(&object.0);
        ctx.update(&source);
        let record = format!(
            "m:c:{}",
            hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
        );
        let set: redis::Value = hot
            .query(
                redis::cmd("SET")
                    .arg(record)
                    .arg(1)
                    .arg("NX")
                    .arg("EX")
                    .arg(songbird_accounts::ATTACHMENT_MAX_AGE.as_secs()),
            )
            .await
            .map_err(|_| ())?;
        Ok(matches!(set, redis::Value::Okay))
    }

    /// The settings.
    #[must_use]
    pub fn settings(&self) -> &MediaBudgets {
        &self.settings
    }

    /// A budget record: keyed, so `valkey-hot` holds no account ID (`m:<kind>:<16 bytes hex>`).
    fn record(&self, kind: &str, account: &AccountId) -> String {
        let mut ctx = hmac::Context::with_key(&self.record_key);
        ctx.update(kind.as_bytes());
        ctx.update(&[0]);
        ctx.update(account.as_bytes());
        format!(
            "m:{kind}:{}",
            hex::encode(ctx.sign().as_ref().get(..16).unwrap_or_default())
        )
    }

    pub(crate) async fn spend(
        hot: &ValkeyConn,
        record: &str,
        interval_us: u64,
        burst: u64,
        cost: u64,
    ) -> Result<u64, ()> {
        hot.query::<i64>(
            redis::cmd("EVAL")
                .arg(songbird_send::gate::GCRA_COST)
                .arg(1)
                .arg(record)
                .arg(interval_us)
                .arg(interval_us.saturating_mul(burst.max(1)))
                .arg(cost.max(1)),
        )
        .await
        .map(|w| u64::try_from(w).unwrap_or(0))
        .map_err(|_| ())
    }

    /// Spends one of the account's new packs: `packs_per_month` per 30 days, all of them at once
    /// if it likes (D-0127). `Ok(0)` when allowed, else the wait in microseconds.
    ///
    /// # Errors
    /// `valkey-hot` is unreachable.
    pub(crate) async fn admit_pack(
        &self,
        hot: &ValkeyConn,
        account: &AccountId,
        level: songbird_entitlements::Level,
        packs_per_month: u32,
    ) -> Result<u64, ()> {
        let n = u64::from(packs_per_month.max(1));
        Self::spend(
            hot,
            &self.record(level_kind("p", level), account),
            30 * 86_400_000_000 / n,
            n,
            1,
        )
        .await
    }

    /// Spends one grant and `size` bytes of the account's budgets. `Ok(0)` when allowed, else
    /// the wait in microseconds.
    ///
    /// # Errors
    /// `valkey-hot` is unreachable.
    pub(crate) async fn admit_grant(
        &self,
        hot: &ValkeyConn,
        account: &AccountId,
        size: u64,
        (level, bytes_per_day): (songbird_entitlements::Level, u64),
    ) -> Result<u64, ()> {
        let s = &self.settings;
        let per_hour = u64::from(s.grants_per_hour.max(1));
        let wait = Self::spend(
            hot,
            &self.record("g", account),
            3_600_000_000 / per_hour,
            u64::from(s.grant_burst),
            1,
        )
        .await?;
        if wait > 0 {
            return Ok(wait);
        }
        // The lower of the deployment's budget and the plan's (D-0127).
        let units = (s.bytes_per_day.min(bytes_per_day) / UNIT).max(1);
        Self::spend(
            hot,
            &self.record(level_kind("b", level), account),
            86_400_000_000 / units,
            units,
            size.div_ceil(UNIT),
        )
        .await
    }
}

/// A budget record's kind per level (D-0127): each level has its own record, so an upgrade
/// gives the new level's whole budget at once instead of inheriting what the old one spent.
/// Free keeps the names records had before plans.
fn level_kind(kind: &'static str, level: songbird_entitlements::Level) -> &'static str {
    use songbird_entitlements::Level;
    match (kind, level) {
        ("b", Level::Plus) => "b1",
        ("p", Level::Free) => "p",
        ("p", Level::Plus) => "p1",
        (k, _) => k,
    }
}

/// Counters (aggregate only).
#[derive(Debug, Clone, Default)]
pub struct MediaMetrics {
    /// Parts of resumable uploads stored.
    pub parts: songbird_service::telemetry::sharded::ShardedCounter,
    /// Grants, redirects and confirmations refused by a budget.
    pub throttled: songbird_service::telemetry::sharded::ShardedCounter,
    /// Download redirects issued.
    pub redirects: songbird_service::telemetry::sharded::ShardedCounter,
    /// Media URL requests served (whole or a range).
    pub served: songbird_service::telemetry::sharded::ShardedCounter,
    /// Packs completed.
    pub packs: songbird_service::telemetry::sharded::ShardedCounter,
    /// GIF searches answered.
    pub gif_searches: songbird_service::telemetry::sharded::ShardedCounter,
    /// GIF files served.
    pub gif_files: songbird_service::telemetry::sharded::ShardedCounter,
}

impl MediaMetrics {
    /// Registers every metric.
    ///
    /// # Errors
    /// A refused name.
    pub fn register(
        &self,
        m: &mut songbird_service::telemetry::Metrics,
    ) -> Result<(), songbird_service::telemetry::TelemetryError> {
        for (name, help, c) in [
            (
                "api_upload_parts",
                "Parts of resumable uploads stored",
                &self.parts,
            ),
            (
                "api_media_throttled",
                "Grants, redirects and confirmations refused by a budget",
                &self.throttled,
            ),
            (
                "api_media_redirects",
                "Download redirects issued",
                &self.redirects,
            ),
            (
                "api_media_served",
                "Media URL requests served",
                &self.served,
            ),
            ("api_packs_completed", "Packs completed", &self.packs),
            (
                "api_gif_searches",
                "GIF searches answered",
                &self.gif_searches,
            ),
            ("api_gif_files", "GIF files served", &self.gif_files),
        ] {
            m.register(name, help, c.clone())?;
        }
        Ok(())
    }
}

/// `/v1/attachments…`.
pub(crate) async fn route(
    app: &Arc<App>,
    req: Request<Incoming>,
) -> Result<Response<Body>, Refusal> {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();
    let rest = path.strip_prefix("/v1/attachments").unwrap_or_default();
    if rest.is_empty() {
        return if method == Method::POST {
            grant(app, req).await
        } else {
            Err(status(StatusCode::NOT_FOUND))
        };
    }
    let rest = rest
        .strip_prefix('/')
        .ok_or(status(StatusCode::NOT_FOUND))?;
    let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let object = ObjectKey::parse_hex(id).ok_or(status(StatusCode::NOT_FOUND))?;
    match (&method, tail) {
        (&Method::PUT, "") => upload(app, req, object).await,
        (&Method::GET, "") => redirect(app, &req, object).await,
        (&Method::POST, "fetched") => fetched(app, &req, object).await,
        (&Method::GET, "parts") => list(app, req, object).await,
        (&Method::POST, "complete") => complete(app, req, object).await,
        (&Method::PUT, t) => {
            let n = t
                .strip_prefix("parts/")
                .and_then(|n| n.parse::<u16>().ok())
                .ok_or(status(StatusCode::NOT_FOUND))?;
            put_part(app, req, object, n).await
        }
        _ => Err(status(StatusCode::NOT_FOUND)),
    }
}

async fn grant(app: &Arc<App>, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
    let (device, body) = app.authenticate_json(req).await?;
    let l = &app.limits;
    let g = GrantRequest::decode(&body, l.max_bytes, l.max_fetches)
        .ok_or(status(StatusCode::BAD_REQUEST))?;
    let account = account_of(app, &device).await?;
    // The plan's largest file (D-0127): 413 with the limit, so the app can explain it.
    let level = app.plans.level(&account);
    let plan = *app.plans.catalog.limits(level);
    if g.size > plan.file_bytes {
        return Ok(json(
            StatusCode::PAYLOAD_TOO_LARGE,
            &serde_json::json!({ "error": "plan", "file_bytes": plan.file_bytes }),
        ));
    }
    if let Some(m) = &app.media {
        match m
            .admit_grant(
                &app.hot,
                &account,
                g.size,
                (level, plan.upload_bytes_per_day),
            )
            .await
        {
            Ok(0) => {}
            Ok(wait) => {
                app.metrics.media.throttled.inc();
                return Ok(crate::retry_after(wait));
            }
            Err(()) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
        }
    }
    let object = ObjectKey::random().map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?;
    app.registry
        .create_grant(
            &object,
            &Grant {
                device,
                size: g.size,
                fetches: g.fetches,
            },
            Duration::from_secs(l.grant_ttl_secs),
        )
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?;
    Ok(json(
        StatusCode::CREATED,
        &serde_json::json!({
            "id": object.to_hex_unredacted(),
            "part_size": l.part_size,
            "parts": part_count(g.size, l.part_size),
        }),
    ))
}

pub(crate) async fn account_of(app: &App, device: &DeviceId) -> Result<AccountId, Refusal> {
    app.registry
        .device(device)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?
        .map(|r| r.account)
        .ok_or(status(StatusCode::UNAUTHORIZED))
}

/// The granted device's pending upload, or 404 for anyone else (an unknown ID and someone
/// else's grant look the same).
async fn pending(
    app: &App,
    parts: &hyper::http::request::Parts,
    object: &ObjectKey,
) -> Result<PendingUpload, Refusal> {
    // The body is the client's own ciphertext: unsigned, held to its exact length.
    let device = app
        .verify(&parts.method, parts.uri.path(), &parts.headers, &[0; 32])
        .await?;
    app.registry
        .pending_upload(object)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?
        .filter(|p| p.grant.device == device)
        .ok_or(status(StatusCode::NOT_FOUND))
}

fn content_length(parts: &hyper::http::request::Parts) -> Option<u64> {
    parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

async fn upload(
    app: &Arc<App>,
    req: Request<Incoming>,
    object: ObjectKey,
) -> Result<Response<Body>, Refusal> {
    let (parts, body) = req.into_parts();
    let p = pending(app, &parts, &object).await?;
    let size = p.grant.size;
    if content_length(&parts) != Some(size) {
        return Err(status(StatusCode::BAD_REQUEST));
    }
    match app
        .s3
        .put_object(&object_name(&object), size, body.into_data_stream())
        .await
    {
        Ok(()) => {}
        Err(songbird_s3::Error::LengthMismatch | songbird_s3::Error::BodySource) => {
            return Err(status(StatusCode::BAD_REQUEST));
        }
        Err(_) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
    finish(app, &object).await
}

/// The grant becomes the attachment record; an object whose grant expired meanwhile goes.
async fn finish(app: &App, object: &ObjectKey) -> Result<Response<Body>, Refusal> {
    match app
        .registry
        .finish_upload(object)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?
    {
        songbird_accounts::UploadEnd::Completed => {
            app.metrics.uploads.inc();
            Ok(empty(StatusCode::CREATED))
        }
        // A retried completion: the attachment stands; never delete its object.
        songbird_accounts::UploadEnd::Attached => Ok(empty(StatusCode::CREATED)),
        // No live grant and no attachment: what was stored belongs to nothing.
        songbird_accounts::UploadEnd::Gone => {
            let _ = app.s3.delete_object(&object_name(object)).await;
            Err(status(StatusCode::NOT_FOUND))
        }
    }
}

/// The grant's multipart upload, opened on first use. Two first parts racing open two; the
/// registry keeps one and the loser is aborted.
async fn open(app: &App, object: &ObjectKey, p: &PendingUpload) -> Result<String, Refusal> {
    if let Some(u) = &p.upload {
        return Ok(u.clone());
    }
    let name = object_name(object);
    let mine = app
        .s3
        .create_multipart_upload(&name)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?;
    let kept = app
        .registry
        .set_grant_upload(object, &mine)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?;
    if kept.as_deref() != Some(mine.as_str()) {
        let _ = app.s3.abort_multipart_upload(&name, &mine).await;
    }
    kept.ok_or(status(StatusCode::NOT_FOUND))
}

/// Parts the store holds with exactly their expected length, in order.
async fn received(
    app: &App,
    object: &ObjectKey,
    p: &PendingUpload,
) -> Result<Vec<songbird_s3::multipart::Part>, Refusal> {
    let Some(upload) = &p.upload else {
        return Ok(Vec::new());
    };
    let part_size = app.limits.part_size;
    match app.s3.list_parts(&object_name(object), upload).await {
        Ok(parts) => Ok(parts
            .into_iter()
            .filter(|x| part_len(p.grant.size, part_size, u64::from(x.number)) == Some(x.size))
            .collect()),
        Err(songbird_s3::Error::NotFound) => Err(status(StatusCode::NOT_FOUND)),
        Err(_) => Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
}

async fn put_part(
    app: &Arc<App>,
    req: Request<Incoming>,
    object: ObjectKey,
    n: u16,
) -> Result<Response<Body>, Refusal> {
    let (parts, body) = req.into_parts();
    let p = pending(app, &parts, &object).await?;
    let len = part_len(p.grant.size, app.limits.part_size, u64::from(n))
        .ok_or(status(StatusCode::NOT_FOUND))?;
    if content_length(&parts) != Some(len) {
        return Err(status(StatusCode::BAD_REQUEST));
    }
    // Retries yes, endless rewrites no: at most twice the parts, and a few more (D-0084).
    let most = 2 * part_count(p.grant.size, app.limits.part_size) + 8;
    if !app
        .registry
        .spend_part(&object, most)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?
    {
        return Err(status(StatusCode::CONFLICT));
    }
    let upload = open(app, &object, &p).await?;
    send_part(app, &object, &upload, n, len, body.into_data_stream()).await?;
    Ok(empty(StatusCode::NO_CONTENT))
}

async fn send_part<S, E>(
    app: &App,
    object: &ObjectKey,
    upload: &str,
    n: u16,
    len: u64,
    body: S,
) -> Result<(), Refusal>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: 'static,
{
    match app
        .s3
        .upload_part(&object_name(object), upload, n, len, body)
        .await
    {
        Ok(_) => {}
        Err(songbird_s3::Error::LengthMismatch | songbird_s3::Error::BodySource) => {
            return Err(status(StatusCode::BAD_REQUEST));
        }
        Err(songbird_s3::Error::NotFound) => return Err(status(StatusCode::NOT_FOUND)),
        Err(_) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
    app.metrics.media.parts.inc();
    app.registry
        .touch_grant(object, Duration::from_secs(app.limits.grant_ttl_secs))
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))
}

async fn list(
    app: &Arc<App>,
    req: Request<Incoming>,
    object: ObjectKey,
) -> Result<Response<Body>, Refusal> {
    let (parts, _) = req.into_parts();
    let p = pending(app, &parts, &object).await?;
    let got: Vec<u16> = received(app, &object, &p)
        .await?
        .iter()
        .map(|x| x.number)
        .collect();
    Ok(json(
        StatusCode::OK,
        &serde_json::json!({ "part_size": app.limits.part_size, "parts": got }),
    ))
}

async fn complete(
    app: &Arc<App>,
    req: Request<Incoming>,
    object: ObjectKey,
) -> Result<Response<Body>, Refusal> {
    let (parts, _) = req.into_parts();
    let p = pending(app, &parts, &object).await?;
    assemble(app, &object, &p).await
}

/// Completes the multipart upload if every part is there with its exact length; 409 otherwise.
async fn assemble(
    app: &App,
    object: &ObjectKey,
    p: &PendingUpload,
) -> Result<Response<Body>, Refusal> {
    let upload = p.upload.as_deref().ok_or(status(StatusCode::CONFLICT))?;
    let got = received(app, object, p).await?;
    let want = part_count(p.grant.size, app.limits.part_size);
    let complete = u64::try_from(got.len()).is_ok_and(|n| n == want)
        && got.iter().zip(1u16..).all(|(x, n)| x.number == n);
    if !complete {
        return Err(status(StatusCode::CONFLICT));
    }
    match app
        .s3
        .complete_multipart_upload(&object_name(object), upload, &got)
        .await
    {
        Ok(()) => {}
        Err(songbird_s3::Error::NotFound) => return Err(status(StatusCode::NOT_FOUND)),
        Err(_) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
    finish(app, object).await
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(crate) fn peer(req: &Request<Incoming>) -> Result<std::net::IpAddr, Refusal> {
    req.extensions()
        .get::<crate::registration::Peer>()
        .map(|p| p.0)
        .ok_or(status(StatusCode::SERVICE_UNAVAILABLE))
}

/// The media settings, or 503 where `[api.media]` is not configured.
fn media(app: &App) -> Result<&Media, Refusal> {
    app.media
        .as_ref()
        .ok_or(status(StatusCode::SERVICE_UNAVAILABLE))
}

pub(crate) async fn throttle(
    app: &App,
    m: &Media,
    kind: &str,
    ip: std::net::IpAddr,
) -> Option<Response<Body>> {
    match m.admit_range(&app.hot, kind, ip).await {
        Ok(0) => None,
        Ok(wait) => {
            app.metrics.media.throttled.inc();
            Some(crate::retry_after(wait))
        }
        Err(()) => Some(empty(StatusCode::SERVICE_UNAVAILABLE)),
    }
}

/// `GET /v1/attachments/<id>`: no account. The 256-bit ID (inside an end-to-end encrypted
/// message) is the capability; the answer is a 303 to a short-lived signed media URL, so the
/// server never learns which device fetched which object (D-0079).
async fn redirect(
    app: &App,
    req: &Request<Incoming>,
    object: ObjectKey,
) -> Result<Response<Body>, Refusal> {
    let m = media(app)?;
    if let Some(r) = throttle(app, m, "d", peer(req)?).await {
        return Ok(r);
    }
    let a = app
        .registry
        .attachment(&object)
        .await
        .map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?
        .ok_or(status(StatusCode::NOT_FOUND))?;
    let url = m.media_url(&object, a.size, now_secs() + m.downloads.url_ttl_secs);
    let mut r = empty(StatusCode::SEE_OTHER);
    let h = r.headers_mut();
    h.insert(
        hyper::header::LOCATION,
        HeaderValue::from_str(&url).map_err(|_| status(StatusCode::SERVICE_UNAVAILABLE))?,
    );
    h.insert(
        hyper::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    app.metrics.media.redirects.inc();
    Ok(r)
}

/// `POST /v1/attachments/<id>/fetched`: a recipient device has the whole file, verified. The
/// last confirmation deletes it (D-0082).
async fn fetched(
    app: &App,
    req: &Request<Incoming>,
    object: ObjectKey,
) -> Result<Response<Body>, Refusal> {
    let m = media(app)?;
    let ip = peer(req)?;
    if let Some(r) = throttle(app, m, "f", ip).await {
        return Ok(r);
    }
    // A second confirmation from the same source counts nothing (D-0084).
    match m.first_confirmation(&app.hot, &object, ip).await {
        Ok(true) => {}
        Ok(false) => return Ok(empty(StatusCode::NO_CONTENT)),
        Err(()) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
    match app.registry.count_fetch(&object).await {
        Ok(Some(0)) => {
            app.metrics.downloads.inc();
            // The last fetch: delete now (the worker's sweep catches a lost job).
            if let Ok(job) = songbird_jobs::Job::new(songbird_jobs::Work::DeleteObject { object })
                && app.queue.enqueue(&[job]).await.is_err()
            {
                tracing::warn!("cleanup job not queued; the sweep will delete the object");
            }
            Ok(empty(StatusCode::NO_CONTENT))
        }
        Ok(Some(_)) => {
            app.metrics.downloads.inc();
            Ok(empty(StatusCode::NO_CONTENT))
        }
        Ok(None) => Err(status(StatusCode::NOT_FOUND)),
        Err(_) => Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    }
}

/// The object store's body as response frames.
pub(crate) struct Frames(pub(crate) songbird_s3::ObjectBody);

impl Stream for Frames {
    type Item = Result<hyper::body::Frame<Bytes>, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.0).poll_next(cx).map(|o| {
            o.map(|r| {
                r.map(hyper::body::Frame::data)
                    .map_err(|_| std::io::Error::other("object store"))
            })
        })
    }
}

/// `GET /v1/media/<id>?e=…&z=…&s=…`: the signed URL. Checks only its MAC and expiry (no
/// database access), then serves the object or one byte range of it from the store.
pub(crate) async fn serve(app: &App, req: Request<Incoming>) -> Result<Response<Body>, Refusal> {
    if req.method() != Method::GET {
        return Err(status(StatusCode::NOT_FOUND));
    }
    let m = media(app)?;
    if let Some(r) = throttle(app, m, "v", peer(&req)?).await {
        return Ok(r);
    }
    let object = req
        .uri()
        .path()
        .strip_prefix("/v1/media/")
        .and_then(ObjectKey::parse_hex)
        .ok_or(status(StatusCode::NOT_FOUND))?;
    let q = req
        .uri()
        .query()
        .and_then(MediaQuery::parse)
        .ok_or(status(StatusCode::NOT_FOUND))?;
    let now = now_secs();
    let size = m
        .check_url(&object, &q, now)
        .ok_or(status(StatusCode::FORBIDDEN))?;
    let ask = parse_range(
        req.headers()
            .get(hyper::header::RANGE)
            .and_then(|v| v.to_str().ok()),
        size,
    );
    let range = match ask {
        RangeAsk::Whole => None,
        RangeAsk::Part(a, b) => Some((a, b)),
        RangeAsk::Unsatisfiable => {
            let mut r = empty(StatusCode::RANGE_NOT_SATISFIABLE);
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
                r.headers_mut().insert(hyper::header::CONTENT_RANGE, v);
            }
            return Ok(r);
        }
    };
    let got = match app.s3.get_object(&object_name(&object), range).await {
        Ok(g) => g,
        Err(songbird_s3::Error::NotFound) => return Err(status(StatusCode::NOT_FOUND)),
        Err(_) => return Err(status(StatusCode::SERVICE_UNAVAILABLE)),
    };
    let mut r = Response::new(http_body_util::StreamBody::new(Frames(got.body)).boxed_unsync());
    let h = r.headers_mut();
    h.insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(CONTENT_LENGTH, HeaderValue::from(got.length));
    h.insert(
        hyper::header::ACCEPT_RANGES,
        HeaderValue::from_static("bytes"),
    );
    // Immutable ciphertext behind a capability URL: the device may cache it until the URL ends;
    // shared caches may not.
    if let Ok(v) = HeaderValue::from_str(&format!(
        "private, max-age={}, immutable",
        q.expires.saturating_sub(now)
    )) {
        h.insert(hyper::header::CACHE_CONTROL, v);
    }
    if let Some((a, b)) = range {
        *r.status_mut() = StatusCode::PARTIAL_CONTENT;
        if let Ok(v) = HeaderValue::from_str(&format!("bytes {a}-{b}/{size}")) {
            r.headers_mut().insert(hyper::header::CONTENT_RANGE, v);
        }
    }
    app.metrics.media.served.inc();
    Ok(r)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn parts_cover_the_upload_exactly() {
        let p = 8 << 20;
        assert_eq!(part_count(1_073, p), 1);
        assert_eq!(part_len(1_073, p, 1), Some(1_073));
        assert_eq!(part_len(1_073, p, 2), None);
        assert_eq!(part_len(1_073, p, 0), None);
        let size = 3 * p + 17;
        assert_eq!(part_count(size, p), 4);
        let total: u64 = (1..=4).map(|n| part_len(size, p, n).unwrap()).sum();
        assert_eq!(total, size);
        assert_eq!(part_len(size, p, 4), Some(17));
        assert_eq!(part_count(2 * p, p), 2);
        assert_eq!(part_len(2 * p, p, 2), Some(p));
    }

    #[test]
    fn grant_requests_hold_the_format_and_the_limits() {
        let legal = songbird_proto::media::ciphertext_len(5_000).unwrap();
        let ok = format!(r#"{{"size":{legal},"fetches":3}}"#);
        assert_eq!(
            GrantRequest::decode(ok.as_bytes(), 1 << 30, 1024),
            Some(GrantRequest {
                size: legal,
                fetches: 3
            })
        );
        for bad in [
            format!(r#"{{"size":{},"fetches":3}}"#, legal + 1),
            format!(r#"{{"size":{legal},"fetches":0}}"#),
            format!(r#"{{"size":{legal},"fetches":1025}}"#),
            format!(r#"{{"size":{legal},"fetches":1,"x":1}}"#),
            r#"{"size":4096,"fetches":1}"#.to_owned(),
            "[]".to_owned(),
        ] {
            assert_eq!(
                GrantRequest::decode(bad.as_bytes(), 1 << 30, 1024),
                None,
                "{bad}"
            );
        }
        assert_eq!(GrantRequest::decode(ok.as_bytes(), legal - 1, 1024), None);
    }

    #[test]
    fn ranges_follow_rfc_9110() {
        use RangeAsk::{Part, Unsatisfiable, Whole};
        let n = 1_000;
        for (h, want) in [
            (None, Whole),
            (Some("bytes=0-99"), Part(0, 99)),
            (Some("bytes=990-"), Part(990, 999)),
            (Some("bytes=-10"), Part(990, 999)),
            (Some("bytes=-5000"), Part(0, 999)),
            (Some("bytes=10-5000"), Part(10, 999)),
            (Some("bytes=1000-"), Unsatisfiable),
            (Some("bytes=-0"), Unsatisfiable),
            (Some("bytes=5-1"), Whole),
            (Some("bytes=0-1,4-5"), Whole),
            (Some("bytes=a-b"), Whole),
            (Some("bytes=-"), Whole),
            (Some("pages=0-1"), Whole),
            (Some("bytes=+1-2"), Whole),
        ] {
            assert_eq!(parse_range(h, n), want, "{h:?}");
        }
        assert_eq!(parse_range(Some("bytes=0-"), 0), Unsatisfiable);
    }

    #[test]
    fn media_urls_are_signed_bound_and_short_lived() {
        let m = Media::new(MediaBudgets::default(), MediaDownloads::default(), &[3; 32]);
        let o = ObjectKey([0x42; 32]);
        let url = m.media_url(&o, 1_073, 2_000_000_600);
        let (path, query) = url.split_once('?').unwrap();
        assert_eq!(
            path.strip_prefix("/v1/media/"),
            Some(o.to_hex_unredacted().as_str())
        );
        let q = MediaQuery::parse(query).unwrap();
        assert_eq!((q.expires, q.size), (2_000_000_600, 1_073));
        assert_eq!(m.check_url(&o, &q, 2_000_000_000), Some(1_073));
        // Expired; too far ahead; another object; another key.
        assert_eq!(m.check_url(&o, &q, 2_000_000_600), None);
        assert_eq!(m.check_url(&o, &q, 1_000_000_000), None);
        assert_eq!(m.check_url(&ObjectKey([0x43; 32]), &q, 2_000_000_000), None);
        let other = Media::new(MediaBudgets::default(), MediaDownloads::default(), &[4; 32]);
        assert_eq!(other.check_url(&o, &q, 2_000_000_000), None);
        for bad in [
            "",
            "e=1&z=2",
            "z=2&e=1&s=00000000000000000000000000000000",
            "e=1&z=2&s=0000000000000000000000000000000",
            "e=1&z=2&s=000000000000000000000000000000000",
            "e=1&z=2&s=0000000000000000000000000000000g",
            "e=1&z=2&s=00000000000000000000000000000000&x=1",
            "e=+1&z=2&s=00000000000000000000000000000000",
            "e=99999999999999999999999&z=2&s=00000000000000000000000000000000",
        ] {
            assert_eq!(MediaQuery::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn budget_records_are_keyed_and_reveal_no_account() {
        let m = Media::new(MediaBudgets::default(), MediaDownloads::default(), &[7; 32]);
        let a = AccountId::random().unwrap();
        let r = m.record("b", &a);
        assert!(r.starts_with("m:b:") && r.len() == 4 + 32);
        assert!(!r.contains(&a.to_hex_unredacted()));
        assert_ne!(r, m.record("g", &a));
        assert_ne!(
            r,
            Media::new(MediaBudgets::default(), MediaDownloads::default(), &[8; 32])
                .record("b", &a)
        );
    }
}
