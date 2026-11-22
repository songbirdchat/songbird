//! Songbird billing (Prompt 11, M3; D-0126 §1–§4, D-0129; PROTOCOL §14.5).
//!
//! Verifies purchases with the App Store, Google Play and Mollie, keeps each subscription's state
//! in **its own database** under keyed hashes of the providers' identifiers, and issues
//! **anonymous plan tokens** (`songbird_tokens`) against it, at most a cap per subscription and
//! month. It never sees an account: no request carries one, no table holds one, and it has no
//! access to the registry. Accounts redeem the tokens at `api`.
//!
//! | Request | Who | What |
//! | --- | --- | --- |
//! | `POST /v1/billing/tokens` | the app, no account | a proof of purchase and a token request → the evaluated tokens |
//! | `POST /v1/billing/apple` | Apple | App Store Server Notifications V2 |
//! | `POST /v1/billing/google` | Google Pub/Sub (OIDC) | real-time developer notifications |
//! | `POST /v1/billing/mollie/checkout` | the app | a Mollie checkout for a subscriber secret |
//! | `POST /v1/billing/mollie/webhook` | Mollie | a payment ID, fetched back |
//! | `POST /v1/billing/mollie/state`, `…/cancel`, `…/resume` | the app | a subscriber's state; cancelling; renewing again |
#![forbid(unsafe_code)]

pub mod apple;
pub mod der;
pub mod google;
pub mod http;
pub mod jose;
pub mod mollie;
pub mod store;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_lc_rs::digest;
use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::Deserialize;
use serde_json::{Value, json};
use songbird_tokens::token::{self, IssueRequest, Keys, Kind};

use crate::store::{State, Store, StoreError, SubKey};

/// Largest request body.
pub const MAX_BODY: usize = 96 * 1024;
/// Tokens a Plus subscription may have issued per month: one and a spare (a reinstall).
pub const PLUS_CAP: u32 = 2;
/// Tokens a Family subscription may have issued per month: six seats and a spare.
pub const FAMILY_CAP: u32 = 7;
/// Days a Mollie period stays paid past its next charge date (SEPA Direct Debit takes days).
pub const MOLLIE_SLACK_DAYS: u32 = 7;
/// Days a handled provider message ID is remembered.
pub const SEEN_DAYS: u32 = 45;

/// The stores' environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Env {
    /// Real money.
    Production,
    /// Apple's sandbox, Google's licence testers, Mollie's test mode.
    Sandbox,
}

/// A provider's answer that is not a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// Not a valid purchase or message for this app and environment.
    #[error("refused")]
    Refused,
    /// The provider could not be reached or answered oddly: retry.
    #[error("unavailable")]
    Unavailable,
}

/// Products: which store product is which plan.
#[derive(Debug, Clone, Default)]
pub struct Products(HashMap<String, &'static str>);

/// One `[[billing.products]]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductSetting {
    /// The store's product or subscription ID.
    pub id: String,
    /// `plus` or `family`.
    pub plan: String,
}

impl Products {
    /// From configuration.
    ///
    /// # Errors
    /// An unknown plan or a repeated ID.
    pub fn new(list: &[ProductSetting]) -> Result<Self, &'static str> {
        let mut m = HashMap::new();
        for p in list {
            let plan = store::plan(&p.plan).ok_or("products: plan must be plus or family")?;
            if m.insert(p.id.clone(), plan).is_some() {
                return Err("products: an ID is listed twice");
            }
        }
        Ok(Self(m))
    }

    /// A product's plan.
    #[must_use]
    pub fn plan(&self, id: &str) -> Option<&'static str> {
        self.0.get(id).copied()
    }
}

/// Unix milliseconds now.
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The UTC day of a Unix millisecond time.
#[must_use]
pub fn day_of_ms(ms: u64) -> u32 {
    u32::try_from(ms / 86_400_000).unwrap_or(u32::MAX)
}

/// Today's UTC day.
#[must_use]
pub fn today() -> u32 {
    day_of_ms(now_ms())
}

/// `day` plus `months` calendar months (the day clamped to the month's length).
#[must_use]
pub fn add_months(day: u32, months: u32) -> u32 {
    let (y, m, d) = token::civil_from_days(i64::from(day));
    let total = (y * 12 + i64::from(m) - 1) + i64::from(months);
    let (ny, nm) = (
        total.div_euclid(12),
        u32::try_from(total.rem_euclid(12)).unwrap_or(0) + 1,
    );
    let first = token::days_from_civil(ny, nm, 1);
    let next = if nm == 12 {
        token::days_from_civil(ny + 1, 1, 1)
    } else {
        token::days_from_civil(ny, nm + 1, 1)
    };
    let len = u32::try_from(next - first).unwrap_or(28);
    u32::try_from(token::days_from_civil(ny, nm, d.min(len))).unwrap_or(u32::MAX)
}

/// `YYYY-MM-DD` of a day.
#[must_use]
pub fn ymd(day: u32) -> String {
    let (y, m, d) = token::civil_from_days(i64::from(day));
    format!("{y:04}-{m:02}-{d:02}")
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut ctx = digest::Context::new(&digest::SHA256);
    for p in parts {
        ctx.update(p);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

/// A per-node budget per client network (IPv4 /24, IPv6 /48): unauthenticated requests cost
/// provider calls, so each range gets a few a minute. Kept in memory, bounded.
#[derive(Debug)]
pub struct RangeBudget {
    per_minute: f64,
    burst: f64,
    map: Mutex<HashMap<[u8; 7], (f64, u64)>>,
}

impl RangeBudget {
    /// `per_minute` with a `burst`.
    #[must_use]
    pub fn new(per_minute: u32, burst: u32) -> Self {
        Self {
            per_minute: f64::from(per_minute.max(1)),
            burst: f64::from(burst.max(1)),
            map: Mutex::new(HashMap::new()),
        }
    }

    fn range(ip: IpAddr) -> [u8; 7] {
        match ip {
            IpAddr::V4(v) => {
                let o = v.octets();
                [4, o[0], o[1], o[2], 0, 0, 0]
            }
            // The whole /48.
            IpAddr::V6(v) => {
                let o = v.octets();
                [6, o[0], o[1], o[2], o[3], o[4], o[5]]
            }
        }
    }

    /// Spends one; `false` when the range is over its budget.
    pub fn take(&self, ip: IpAddr) -> bool {
        let Ok(mut m) = self.map.lock() else {
            return false;
        };
        let now = now_ms();
        if m.len() > 100_000 {
            // Forget only ranges whose budget has refilled (idle long enough to be full again);
            // a busy range keeps its debt, however many others an attacker brings (D-0134).
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions,
                reason = "a refill time in milliseconds, far below u64::MAX"
            )]
            let full_after = (self.burst * 60_000.0 / self.per_minute) as u64;
            m.retain(|_, e| now.saturating_sub(e.1) < full_after);
        }
        let range = Self::range(ip);
        if m.len() > 100_000 && !m.contains_key(&range) {
            // Still full of busy ranges: a new one waits rather than the map growing.
            return false;
        }
        let e = m.entry(range).or_insert((self.burst, now));
        #[allow(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "milliseconds between two requests fit an f64 exactly"
        )]
        let elapsed = now.saturating_sub(e.1) as f64;
        e.0 = (e.0 + elapsed * self.per_minute / 60_000.0).min(self.burst);
        e.1 = now;
        if e.0 >= 1.0 {
            e.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Billing's state shared by every request.
pub struct Billing {
    /// The store.
    pub store: Store,
    /// Token keys (shared with `api`).
    pub keys: Keys,
    /// Products.
    pub products: Products,
    /// The App Store, if configured.
    pub apple: Option<apple::Apple>,
    /// Google Play, if configured.
    pub google: Option<google::Google>,
    /// Mollie, if configured.
    pub mollie: Option<mollie::Mollie>,
    /// Unauthenticated requests per client range.
    pub budget: RangeBudget,
}

impl std::fmt::Debug for Billing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Billing").finish_non_exhaustive()
    }
}

/// Response body.
pub type Body = Full<Bytes>;

fn reply(status: StatusCode, v: &Value) -> Response<Body> {
    let mut r = Response::new(Full::new(Bytes::from(v.to_string())));
    *r.status_mut() = status;
    r.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    r
}

fn error(status: StatusCode, what: &str) -> Response<Body> {
    reply(status, &json!({ "error": what }))
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Full::new(Bytes::new()));
    *r.status_mut() = status;
    r
}

/// A proof of purchase.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
enum Proof {
    /// `StoreKit` 2's signed transaction.
    Apple(String),
    /// Google Play's purchase token.
    Google(String),
    /// A Mollie subscriber secret (64 hex).
    Mollie(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokensBody {
    proof: Proof,
    request: IssueRequest,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckoutBody {
    plan: String,
    interval: String,
    subscriber: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriberBody {
    subscriber: String,
}

fn secret32(hex_str: &str) -> Option<[u8; 32]> {
    let mut b = [0u8; 32];
    hex::decode_to_slice(hex_str, &mut b).ok().map(|()| b)
}

impl Billing {
    /// Handles one request from `peer`.
    pub async fn handle(
        &self,
        req: Request<hyper::body::Incoming>,
        peer: IpAddr,
    ) -> Response<Body> {
        let (parts, body) = req.into_parts();
        if parts.method != Method::POST {
            return empty(StatusCode::NOT_FOUND);
        }
        let Ok(body) = Limited::new(body, MAX_BODY).collect().await else {
            return empty(StatusCode::PAYLOAD_TOO_LARGE);
        };
        let body = body.to_bytes();
        let bearer = parts
            .headers
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        match parts.uri.path() {
            "/v1/billing/tokens" => self.tokens(&body, peer).await,
            "/v1/billing/apple" => self.apple_notification(&body).await,
            "/v1/billing/google" => self.google_notification(bearer.as_deref(), &body).await,
            "/v1/billing/mollie/checkout" => self.checkout(&body, peer).await,
            "/v1/billing/mollie/webhook" => self.mollie_webhook(&body).await,
            "/v1/billing/mollie/state" => self.mollie_state(&body, peer, false).await,
            "/v1/billing/mollie/cancel" => self.mollie_state(&body, peer, true).await,
            "/v1/billing/mollie/resume" => self.mollie_resume(&body, peer).await,
            _ => empty(StatusCode::NOT_FOUND),
        }
    }

    /// The subscription a proof names, refreshed from its provider where one can be asked. The
    /// error is the answer to send.
    #[allow(
        clippy::result_large_err,
        reason = "the error is the response itself, built once per failed request"
    )]
    async fn resolve(&self, proof: &Proof) -> Result<(SubKey, State), Response<Body>> {
        let unavailable = || error(StatusCode::SERVICE_UNAVAILABLE, "provider");
        let refused = || error(StatusCode::FORBIDDEN, "proof");
        let store_err = |_| empty(StatusCode::SERVICE_UNAVAILABLE);
        let key = match proof {
            Proof::Apple(jws) => {
                let a = self
                    .apple
                    .as_ref()
                    .ok_or_else(|| error(StatusCode::NOT_FOUND, "apple"))?;
                let t = a.transaction(jws).map_err(|_| refused())?;
                let plan = self.products.plan(&t.product).ok_or_else(refused)?;
                let key = self.store.key("apple", t.original.as_bytes());
                match a.status(&t.original, &self.products).await {
                    Ok(Some((s, stamp))) => self
                        .store
                        .apply(&key, &s, stamp, true)
                        .await
                        .map_err(store_err)?,
                    Ok(None) => self
                        .store
                        .apply(
                            &key,
                            &apple::Apple::state(&t, None, plan),
                            // The app fetched this transaction just now: its signing time
                            // would date the token request, so only its day is kept.
                            Some(t.signed_ms / 86_400_000 * 86_400_000),
                            true,
                        )
                        .await
                        .map_err(store_err)?,
                    Err(ProviderError::Refused) => return Err(refused()),
                    Err(ProviderError::Unavailable) => return Err(unavailable()),
                };
                key
            }
            Proof::Google(purchase_token) => {
                let g = self
                    .google
                    .as_ref()
                    .ok_or_else(|| error(StatusCode::NOT_FOUND, "google"))?;
                let p = match g.purchase(purchase_token, &self.products).await {
                    Ok(p) => p,
                    Err(ProviderError::Refused) => return Err(refused()),
                    Err(ProviderError::Unavailable) => return Err(unavailable()),
                };
                let key = self.store.key("google", purchase_token.as_bytes());
                self.store
                    .apply(&key, &p.state, None, false)
                    .await
                    .map_err(store_err)?;
                if p.needs_ack && g.acknowledge(&p.product, purchase_token).await.is_err() {
                    tracing::warn!("google: acknowledgement failed; retried at the next request");
                }
                key
            }
            Proof::Mollie(s) => {
                if self.mollie.is_none() {
                    return Err(error(StatusCode::NOT_FOUND, "mollie"));
                }
                let secret = secret32(s).ok_or_else(refused)?;
                self.store.key("mollie", &secret)
            }
        };
        let row = self
            .store
            .get(&key)
            .await
            .map_err(store_err)?
            .ok_or_else(refused)?;
        Ok((key, row.state))
    }

    /// `POST /v1/billing/tokens`.
    async fn tokens(&self, body: &[u8], peer: IpAddr) -> Response<Body> {
        if !self.budget.take(peer) {
            return error(StatusCode::TOO_MANY_REQUESTS, "budget");
        }
        let Ok(b) = serde_json::from_slice::<TokensBody>(body) else {
            return error(StatusCode::BAD_REQUEST, "body");
        };
        let Ok(info) = b.request.info() else {
            return error(StatusCode::BAD_REQUEST, "request");
        };
        // Every paid plan is level 1 (a Family seat is Plus, D-0126 §5).
        if info.kind != Kind::Plan || info.level != 1 {
            return error(StatusCode::BAD_REQUEST, "request");
        }
        let today = today();
        let month = token::month_of(today);
        let needed = if info.period == month {
            today
        } else if info.period == month + 1 {
            token::first_day(info.period)
        } else {
            return error(StatusCode::CONFLICT, "period");
        };
        let (key, state) = match self.resolve(&b.proof).await {
            Ok(x) => x,
            Err(r) => return r,
        };
        if state.revoked || state.paid < needed {
            return error(StatusCode::PAYMENT_REQUIRED, "not_paid");
        }
        // Evaluate first (a malformed request spends nothing), then spend the cap.
        let Ok(resp) = self.keys.issue(&b.request) else {
            return error(StatusCode::BAD_REQUEST, "request");
        };
        let cap = if state.plan == "family" {
            FAMILY_CAP
        } else {
            PLUS_CAP
        };
        let n = u32::try_from(b.request.blinded.len()).unwrap_or(u32::MAX);
        match self
            .store
            .spend(&key, info.period, n, cap, token::last_day(info.period) + 1)
            .await
        {
            Ok(()) => reply(StatusCode::OK, &json!(resp)),
            Err(StoreError::Capped) => error(StatusCode::TOO_MANY_REQUESTS, "cap"),
            Err(_) => empty(StatusCode::SERVICE_UNAVAILABLE),
        }
    }

    /// `POST /v1/billing/apple`: App Store Server Notifications V2. A notification already
    /// handled changes nothing; the state comes from the App Store Server API when configured,
    /// else from the notification, never older than what the row holds.
    async fn apple_notification(&self, body: &[u8]) -> Response<Body> {
        let Some(a) = &self.apple else {
            return empty(StatusCode::NOT_FOUND);
        };
        let Ok(n) = a.notification(body) else {
            return empty(StatusCode::BAD_REQUEST);
        };
        let seen = sha256(&[b"apple\0", n.uuid.as_bytes()]);
        if matches!(self.store.get_seen(&seen).await, Ok(true)) {
            return empty(StatusCode::OK);
        }
        if let Some(t) = &n.transaction
            && let Some(plan) = self.products.plan(&t.product)
        {
            let key = self.store.key("apple", t.original.as_bytes());
            let r = match a.status(&t.original, &self.products).await {
                Ok(Some((s, stamp))) => self.store.apply(&key, &s, stamp, true).await.map(|_| ()),
                Ok(None) => {
                    let s = apple::Apple::state(t, n.renewal, plan);
                    let applied = self.store.apply(&key, &s, Some(n.signed_ms), true).await;
                    match (n.kind.as_str(), applied) {
                        ("REFUND" | "REVOKE", Ok(_)) => self.store.revoke(&key).await.map(|_| ()),
                        (_, r) => r.map(|_| ()),
                    }
                }
                Err(ProviderError::Refused) => Ok(()),
                Err(ProviderError::Unavailable) => return empty(StatusCode::SERVICE_UNAVAILABLE),
            };
            if r.is_err() {
                return empty(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        match self.store.first_time(&seen, today() + SEEN_DAYS).await {
            Ok(_) => empty(StatusCode::OK),
            Err(_) => empty(StatusCode::SERVICE_UNAVAILABLE),
        }
    }

    /// `POST /v1/billing/google`: a Pub/Sub push with Google's OIDC token.
    async fn google_notification(&self, bearer: Option<&str>, body: &[u8]) -> Response<Body> {
        let Some(g) = &self.google else {
            return empty(StatusCode::NOT_FOUND);
        };
        match g.verify_push(bearer.unwrap_or_default()).await {
            Ok(()) => {}
            Err(ProviderError::Refused) => return empty(StatusCode::UNAUTHORIZED),
            Err(ProviderError::Unavailable) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        }
        let Ok(n) = g.rtdn(body) else {
            return empty(StatusCode::BAD_REQUEST);
        };
        let id = match &n {
            google::Rtdn::Subscription { id, .. }
            | google::Rtdn::Voided { id, .. }
            | google::Rtdn::Other { id } => id.clone(),
        };
        let seen = sha256(&[b"google\0", id.as_bytes()]);
        if matches!(self.store.get_seen(&seen).await, Ok(true)) {
            return empty(StatusCode::OK);
        }
        let r = match &n {
            google::Rtdn::Subscription { token, .. } => match g
                .purchase(token, &self.products)
                .await
            {
                Ok(p) => {
                    let key = self.store.key("google", token.as_bytes());
                    if p.needs_ack {
                        let _ = g.acknowledge(&p.product, token).await;
                    }
                    self.store
                        .apply(&key, &p.state, None, false)
                        .await
                        .map(|_| ())
                }
                Err(ProviderError::Refused) => Ok(()),
                Err(ProviderError::Unavailable) => return empty(StatusCode::SERVICE_UNAVAILABLE),
            },
            google::Rtdn::Voided { token, .. } => self
                .store
                .revoke(&self.store.key("google", token.as_bytes()))
                .await
                .map(|_| ()),
            google::Rtdn::Other { .. } => Ok(()),
        };
        if r.is_err() {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        }
        match self.store.first_time(&seen, today() + SEEN_DAYS).await {
            Ok(_) => empty(StatusCode::OK),
            Err(_) => empty(StatusCode::SERVICE_UNAVAILABLE),
        }
    }

    /// `POST /v1/billing/mollie/checkout`.
    async fn checkout(&self, body: &[u8], peer: IpAddr) -> Response<Body> {
        let Some(m) = &self.mollie else {
            return empty(StatusCode::NOT_FOUND);
        };
        if !self.budget.take(peer) {
            return error(StatusCode::TOO_MANY_REQUESTS, "budget");
        }
        let Ok(b) = serde_json::from_slice::<CheckoutBody>(body) else {
            return error(StatusCode::BAD_REQUEST, "body");
        };
        let (Some(plan), Some(secret)) = (store::plan(&b.plan), secret32(&b.subscriber)) else {
            return error(StatusCode::BAD_REQUEST, "body");
        };
        if m.prices.of(plan, &b.interval).is_none() {
            return error(StatusCode::BAD_REQUEST, "interval");
        }
        let key = self.store.key("mollie", &secret);
        match self.store.get(&key).await {
            // A row that was ever subscribed, or revoked (a refund, a chargeback), takes no new
            // checkout: the app starts over with a new subscriber secret (a fresh row).
            Ok(Some(r)) if r.state.revoked => return error(StatusCode::CONFLICT, "revoked"),
            Ok(Some(r)) if r.subscription.is_some() => {
                return error(StatusCode::CONFLICT, "exists");
            }
            Ok(_) => {}
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        }
        let Ok(customer) = m.customer().await else {
            return error(StatusCode::SERVICE_UNAVAILABLE, "provider");
        };
        if self
            .store
            .mollie_started(&key, plan, &b.interval, &customer)
            .await
            .is_err()
        {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        }
        match m
            .first_payment(&customer, plan, &b.interval, &hex::encode(key))
            .await
        {
            Ok(url) => reply(StatusCode::OK, &json!({ "checkout": url })),
            Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "provider"),
        }
    }

    /// `POST /v1/billing/mollie/webhook`: `id=tr_…`, fetched back. Mollie retries anything
    /// but 200, so only a failure worth retrying answers otherwise.
    #[allow(
        clippy::too_many_lines,
        reason = "one webhook's checks in order: fetch, row, reversal, once per payment, subscribe, store"
    )]
    async fn mollie_webhook(&self, body: &[u8]) -> Response<Body> {
        let Some(m) = &self.mollie else {
            return empty(StatusCode::NOT_FOUND);
        };
        let id = std::str::from_utf8(body)
            .ok()
            .and_then(|s| s.strip_prefix("id="))
            .map(str::trim)
            .unwrap_or_default();
        let p = match m.payment(id).await {
            Ok(p) => p,
            Err(ProviderError::Refused) => return empty(StatusCode::OK),
            Err(ProviderError::Unavailable) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        };
        let key: Option<SubKey> = if p.sequence == "first" {
            p.row
                .as_deref()
                .and_then(|r| hex::decode(r).ok())
                .and_then(|b| b.try_into().ok())
        } else if let Some(s) = &p.subscription {
            match self.store.by_mollie_subscription(s).await {
                Ok(k) => k,
                Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
            }
        } else {
            None
        };
        let Some(key) = key else {
            return empty(StatusCode::OK);
        };
        let row = match self.store.get(&key).await {
            Ok(Some(r)) if r.state.provider == "mollie" => r,
            Ok(_) => return empty(StatusCode::OK),
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        };
        // A first payment's customer must be the row's (metadata is ours, but check anyway).
        if p.sequence == "first" && p.customer.is_some() && p.customer != row.customer {
            return empty(StatusCode::OK);
        }
        if p.reversed {
            return match self.store.revoke(&key).await {
                Ok(_) => empty(StatusCode::OK),
                Err(_) => empty(StatusCode::SERVICE_UNAVAILABLE),
            };
        }
        if p.status != "paid" {
            return empty(StatusCode::OK);
        }
        // Each payment counts once, whoever replays its webhook and whenever (D-0134): the
        // period runs from the day it was paid, never from the day the webhook came.
        let Some(paid_day) = p.paid_day else {
            return empty(StatusCode::OK);
        };
        let payment = sha256(&[b"mollie-pay\0", id.as_bytes()]);
        match self.store.first_time(&payment, today() + 400).await {
            Ok(true) => {}
            Ok(false) => return empty(StatusCode::OK),
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        }
        let months = if row.interval.as_deref() == Some("year") {
            12
        } else {
            1
        };
        let next = add_months(paid_day, months);
        let paid = next + MOLLIE_SLACK_DAYS;
        let mut subscription = p.subscription.clone();
        if p.sequence == "first" && row.subscription.is_none() {
            // One subscription per row, even if Mollie calls twice at once.
            let claim = sha256(&[b"mollie-sub\0", &key]);
            match self.store.first_time(&claim, today() + 3_650).await {
                Ok(true) => {}
                Ok(false) => return empty(StatusCode::OK),
                Err(_) => {
                    let _ = self.store.forget(&payment).await;
                    return empty(StatusCode::SERVICE_UNAVAILABLE);
                }
            }
            let (Some(customer), Some(interval)) =
                (row.customer.as_deref(), row.interval.as_deref())
            else {
                return empty(StatusCode::OK);
            };
            if let Ok(id) = m
                .subscribe(customer, row.state.plan, interval, &ymd(next))
                .await
            {
                subscription = Some(id);
            } else {
                let _ = self.store.forget(&claim).await;
                let _ = self.store.forget(&payment).await;
                return empty(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        if self
            .store
            .mollie_paid(&key, subscription.as_deref(), paid, p.sequence == "first")
            .await
            .is_err()
        {
            let _ = self.store.forget(&payment).await;
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        }
        empty(StatusCode::OK)
    }

    /// `POST /v1/billing/mollie/state` and `…/cancel`.
    async fn mollie_state(&self, body: &[u8], peer: IpAddr, cancel: bool) -> Response<Body> {
        let Some(m) = &self.mollie else {
            return empty(StatusCode::NOT_FOUND);
        };
        if !self.budget.take(peer) {
            return error(StatusCode::TOO_MANY_REQUESTS, "budget");
        }
        let Some(secret) = serde_json::from_slice::<SubscriberBody>(body)
            .ok()
            .and_then(|b| secret32(&b.subscriber))
        else {
            return error(StatusCode::BAD_REQUEST, "body");
        };
        let key = self.store.key("mollie", &secret);
        let row = match self.store.get(&key).await {
            Ok(Some(r)) if r.state.provider == "mollie" => r,
            Ok(_) => return empty(StatusCode::NOT_FOUND),
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        };
        if cancel {
            if let (Some(c), Some(s)) = (row.customer.as_deref(), row.subscription.as_deref())
                && m.cancel(c, s).await.is_err()
            {
                return error(StatusCode::SERVICE_UNAVAILABLE, "provider");
            }
            if self.store.mollie_cancelled(&key).await.is_err() {
                return empty(StatusCode::SERVICE_UNAVAILABLE);
            }
            // Cancelled again: a later resume may start renewal once more.
            let _ = self.store.forget(&resume_claim(&key, row.state.paid)).await;
            return empty(StatusCode::NO_CONTENT);
        }
        reply(
            StatusCode::OK,
            &json!({
                "plan": row.state.plan,
                "interval": row.interval,
                "paid_through": row.state.paid,
                "renews": row.state.renews,
                "revoked": row.state.revoked,
            }),
        )
    }
}

/// The mark that one resume of this row's paid period is under way.
fn resume_claim(key: &[u8], paid: u32) -> [u8; 32] {
    sha256(&[b"mollie-resume\0", key, &paid.to_be_bytes()])
}

impl Billing {
    /// `POST /v1/billing/mollie/resume`: renewal on again after a cancellation, while the paid
    /// period lasts (D-0169). Mollie can't reactivate a cancelled subscription, so a new one
    /// starts on the customer's mandate, its first charge the day the paid period would have
    /// renewed (never sooner than tomorrow): nothing is charged twice. 204; 404 for no such
    /// subscriber; 409 `revoked`, `expired` (buy again), `renews` (nothing to resume),
    /// `mandate` (Mollie refused: pay again by checkout).
    async fn mollie_resume(&self, body: &[u8], peer: IpAddr) -> Response<Body> {
        let Some(m) = &self.mollie else {
            return empty(StatusCode::NOT_FOUND);
        };
        if !self.budget.take(peer) {
            return error(StatusCode::TOO_MANY_REQUESTS, "budget");
        }
        let Some(secret) = serde_json::from_slice::<SubscriberBody>(body)
            .ok()
            .and_then(|b| secret32(&b.subscriber))
        else {
            return error(StatusCode::BAD_REQUEST, "body");
        };
        let key = self.store.key("mollie", &secret);
        let row = match self.store.get(&key).await {
            Ok(Some(r)) if r.state.provider == "mollie" => r,
            Ok(_) => return empty(StatusCode::NOT_FOUND),
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        };
        if row.state.revoked {
            return error(StatusCode::CONFLICT, "revoked");
        }
        if row.state.renews {
            return error(StatusCode::CONFLICT, "renews");
        }
        let renew_on = row.state.paid.saturating_sub(MOLLIE_SLACK_DAYS);
        if row.state.paid < today() {
            return error(StatusCode::CONFLICT, "expired");
        }
        let (Some(customer), Some(interval)) = (row.customer.as_deref(), row.interval.as_deref())
        else {
            return error(StatusCode::CONFLICT, "mandate");
        };
        // One resume at a time per row (two taps, two subscriptions otherwise).
        let claim = resume_claim(&key, row.state.paid);
        match self.store.first_time(&claim, today() + 3_650).await {
            Ok(true) => {}
            Ok(false) => return error(StatusCode::CONFLICT, "renews"),
            Err(_) => return empty(StatusCode::SERVICE_UNAVAILABLE),
        }
        let start = ymd(renew_on.max(today() + 1));
        let id = match m
            .subscribe(customer, row.state.plan, interval, &start)
            .await
        {
            Ok(id) => id,
            Err(ProviderError::Refused) => {
                let _ = self.store.forget(&claim).await;
                return error(StatusCode::CONFLICT, "mandate");
            }
            Err(_) => {
                let _ = self.store.forget(&claim).await;
                return error(StatusCode::SERVICE_UNAVAILABLE, "provider");
            }
        };
        if self.store.mollie_resumed(&key, &id).await.is_err() {
            // Not recorded: the new subscription is cancelled again, so nothing charges unseen.
            let _ = m.cancel(customer, &id).await;
            let _ = self.store.forget(&claim).await;
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        }
        empty(StatusCode::NO_CONTENT)
    }
}

/// Serves until `stop`: HTTP/1.1, TLS when given, at most `max_connections` at once.
pub async fn serve(
    listener: tokio::net::TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
    billing: Arc<Billing>,
    max_connections: usize,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let permits = Arc::new(tokio::sync::Semaphore::new(max_connections));
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .max_buf_size(32 * 1024);
    let builder = Arc::new(builder);
    loop {
        let accepted = tokio::select! {
            a = listener.accept() => a,
            _ = stop.changed() => return,
        };
        let Ok((tcp, peer)) = accepted else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            continue;
        };
        let (billing, builder, tls) = (Arc::clone(&billing), Arc::clone(&builder), tls.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let svc = hyper::service::service_fn(move |req| {
                let billing = Arc::clone(&billing);
                async move { Ok::<_, Infallible>(billing.handle(req, peer.ip()).await) }
            });
            match tls {
                Some(t) => {
                    if let Ok(Ok(s)) =
                        tokio::time::timeout(Duration::from_secs(10), t.accept(tcp)).await
                    {
                        let _ = builder.serve_connection(TokioIo::new(s), svc).await;
                    }
                }
                None => {
                    let _ = builder.serve_connection(TokioIo::new(tcp), svc).await;
                }
            }
        });
    }
}

/// How long a request to a provider may take.
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_add_on_the_calendar() {
        let d = |y, m, dd| u32::try_from(token::days_from_civil(y, m, dd)).unwrap_or(0);
        assert_eq!(add_months(d(2026, 1, 31), 1), d(2026, 2, 28));
        assert_eq!(add_months(d(2024, 1, 31), 1), d(2024, 2, 29));
        assert_eq!(add_months(d(2026, 11, 19), 1), d(2026, 12, 19));
        assert_eq!(add_months(d(2026, 12, 19), 1), d(2027, 1, 19));
        assert_eq!(add_months(d(2026, 11, 19), 12), d(2027, 11, 19));
        assert_eq!(ymd(d(2026, 3, 4)), "2026-03-04");
    }

    #[test]
    fn ranges_have_a_budget() {
        let b = RangeBudget::new(1, 3);
        let ip: IpAddr = "192.0.2.7".parse().unwrap_or(IpAddr::from([0, 0, 0, 0]));
        let same: IpAddr = "192.0.2.200".parse().unwrap_or(IpAddr::from([0, 0, 0, 0]));
        let other: IpAddr = "198.51.100.1".parse().unwrap_or(IpAddr::from([0, 0, 0, 0]));
        assert!(b.take(ip) && b.take(same) && b.take(ip));
        assert!(!b.take(same), "a /24 shares one budget");
        assert!(b.take(other));
        // IPv6: one /48 shares a budget; the next /48 has its own (D-0134).
        let v6 = |s: &str| s.parse::<IpAddr>().unwrap_or(IpAddr::from([0u8; 16]));
        let b = RangeBudget::new(1, 1);
        assert!(b.take(v6("2001:db8:1::1")));
        assert!(!b.take(v6("2001:db8:1:ffff::2")));
        assert!(b.take(v6("2001:db8:2::1")));
        // Two /48s whose bytes 4 and 5 XOR alike are not one range.
        assert!(b.take(v6("2001:db8:0101::1")));
        assert!(b.take(v6("2001:db8:0202::1")));
    }

    #[test]
    fn products_map_to_plans() {
        let p = Products::new(&[
            ProductSetting {
                id: "plus.m".into(),
                plan: "plus".into(),
            },
            ProductSetting {
                id: "family.y".into(),
                plan: "family".into(),
            },
        ])
        .unwrap_or_default();
        assert_eq!(p.plan("family.y"), Some("family"));
        assert_eq!(p.plan("gems"), None);
        assert!(
            Products::new(&[ProductSetting {
                id: "x".into(),
                plan: "teams".into()
            }])
            .is_err()
        );
    }
}
