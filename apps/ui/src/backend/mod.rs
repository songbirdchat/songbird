//! The real Songbird backend, for what it can do today without app-core.
//!
//! The server only ever receives what the threat model allows: public keys,
//! signatures, proof of work, a username *hash*, and signed control
//! requests. Message content, profiles, groups and backups stay in the mock
//! store until `crates/app-core` (A2) encrypts them; nothing here sends a
//! plaintext the server must not see.
//!
//! What is live when the user turns it on (You → Server connection):
//! - registration: challenge, Argon2id proof of work, both Ed25519 keys,
//!   invite code; then the username claim (`PUT /v1/username`);
//! - the account's devices (`GET /v1/devices`);
//! - invites (`GET`/`POST /v1/invites`);
//! - GIF search through the privacy proxy (`POST /v1/gifs/search`);
//! - account deletion (`DELETE /v1/account`);
//! - the delivery connection (`crates/client`): authenticated, live status,
//!   envelopes counted but left on the server (only app-core can decrypt
//!   and acknowledge them);
//! - Prompt 9: settings and saved contacts as encrypted records
//!   (`records`), the encrypted profile with the custom status (`profile`),
//!   parental supervision policies (`supervision`). Presence (`presence`)
//!   and nudges (`nudge`) are wired, and send once app-core knows contacts'
//!   devices and seals for them.

pub mod account;
pub mod avatar;
pub mod backup;
pub mod calls;
pub mod chat;
pub mod chat_state;
pub mod link;
pub mod lock;
pub mod nudge;
pub mod packs;
pub mod plans;
pub mod presence;
pub mod profile;
pub mod records;
pub mod rules;
pub mod supervision;
pub mod tunnel;
pub mod usage;
pub mod username;
pub mod wire;
mod ws;

use nsh_core::{platform::HttpRequest, prelude::*, storage::Storage, time::unix_ms};
use serde_json::Value;

pub use ws::{connect, disconnect};

/// Where the backend is. Defaults match `config/dev` (plain HTTP in dev).
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    /// The api's base URL.
    pub api: String,
    /// The api's configured `server_name` (what signatures bind).
    pub api_name: String,
    /// Delivery's `host:port`.
    pub ws_addr: String,
    /// Delivery's configured `server_name`.
    pub ws_name: String,
    /// Billing's base URL (Prompt 11): a service of its own, never told the account.
    pub billing: String,
}

impl Default for Endpoint {
    fn default() -> Self {
        // A test build may name its server at build time
        // (`SONGBIRD_SERVER=songbird.chat`): api.<domain> over HTTPS and
        // ws.<domain>:443 for delivery. Editable under You → Server connection.
        if let Some(domain) = option_env!("SONGBIRD_SERVER").filter(|d| !d.is_empty()) {
            return Self {
                api: format!("https://api.{domain}"),
                api_name: format!("api.{domain}"),
                ws_addr: format!("ws.{domain}:443"),
                ws_name: format!("ws.{domain}"),
                billing: format!("https://billing.{domain}"),
            };
        }
        Self {
            api: "http://127.0.0.1:8443".into(),
            api_name: "api.localhost".into(),
            ws_addr: "127.0.0.1:9201".into(),
            ws_name: "localhost".into(),
            billing: "http://127.0.0.1:8445".into(),
        }
    }
}

/// The delivery connection, as the app shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    #[default]
    Off,
    Connecting,
    Connected,
    /// Lost; the client retries by itself.
    Retrying,
    /// The server refused this device (a 4000–4099 close).
    Refused(u16),
}

/// What the app knows about its account on the real backend.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Live {
    /// The user turned the real backend on.
    pub enabled: bool,
    pub endpoint: Endpoint,
    /// The account and this device, once registered (hex).
    pub account: Option<String>,
    pub device: Option<String>,
    pub username: Option<String>,
    pub link: Link,
    /// Envelopes waiting on the server for app-core to decrypt.
    pub waiting: u64,
    /// The last thing that went wrong, in plain words.
    pub problem: Option<String>,
    /// A long task is running ("Solving the sign-up puzzle…").
    pub busy: Option<String>,
}

pub fn live() -> Signal<Live> {
    global::<Live>()
}

// mock: the keys belong in the Secure Enclave / Android Keystore through
// app-core (A0 "On-device security"); NSH's app storage stands in so the
// account survives a restart during development.
const K_ENABLED: &str = "sb.live";
const K_ENDPOINT: &str = "sb.endpoint";
const K_IDENTITY: &str = "sb.identity_seed";
const K_DEVICE_SEED: &str = "sb.device_seed";
const K_ACCOUNT: &str = "sb.account";
const K_DEVICE: &str = "sb.device";
const K_USERNAME: &str = "sb.username";

fn put(key: &str, value: &str) {
    if let Err(e) = Storage::set_item(key, value) {
        log::warn!("backend storage: {e}");
    }
}

/// Loads what an earlier run saved. Called once at the root.
pub fn restore() {
    let endpoint = Storage::get_item(K_ENDPOINT)
        .and_then(|s| {
            let v: Value = serde_json::from_str(&s).ok()?;
            Some(Endpoint {
                api: v["api"].as_str()?.into(),
                api_name: v["api_name"].as_str()?.into(),
                ws_addr: v["ws_addr"].as_str()?.into(),
                ws_name: v["ws_name"].as_str()?.into(),
                billing: v["billing"]
                    .as_str()
                    .map_or_else(|| Endpoint::default().billing, Into::into),
            })
        })
        .unwrap_or_default();
    live().set(Live {
        // Always on for a real account; only the screenshot fixtures run without it.
        enabled: !crate::store::demo() || Storage::get_item(K_ENABLED).is_some(),
        endpoint,
        account: Storage::get_item(K_ACCOUNT),
        device: Storage::get_item(K_DEVICE),
        username: Storage::get_item(K_USERNAME),
        ..Live::default()
    });
    if is_live() && registered() {
        connect();
    }
}

/// Whether the real backend is on.
pub fn is_live() -> bool {
    live().with_untracked(|l| l.enabled)
}

/// Whether this device has an account on the real backend.
pub fn registered() -> bool {
    live().with_untracked(|l| l.device.is_some())
}

pub fn set_enabled(on: bool) {
    if on {
        put(K_ENABLED, "1");
    } else {
        Storage::remove_item(K_ENABLED);
        disconnect();
    }
    live().update(|l| l.enabled = on);
    if on && registered() {
        connect();
    }
}

pub fn set_endpoint(endpoint: Endpoint) {
    let json = serde_json::json!({
        "api": endpoint.api, "api_name": endpoint.api_name,
        "ws_addr": endpoint.ws_addr, "ws_name": endpoint.ws_name,
        "billing": endpoint.billing,
    });
    put(K_ENDPOINT, &json.to_string());
    live().update(|l| l.endpoint = endpoint);
}

/// Erases what this device keeps of a deleted account: the encrypted message
/// store, its key, the photo and the account's local keys.
pub fn erase_local() {
    if let Some(d) = data_dir() {
        for f in [
            "songbird-core.db",
            "songbird-core.db-wal",
            "songbird-core.db-shm",
            "songbird-avatar.img",
        ] {
            let _ = std::fs::remove_file(d.join(f));
        }
    }
    for k in [
        "sb.core.db_key",
        "sb.core.device_identity",
        "sb.avatar",
        "sb.username_link",
        "sb.profile_key",
        "sb.profile_version",
    ] {
        Storage::remove_item(k);
    }
    rules::forget();
}

/// Forgets this device's account locally (the server keeps it).
pub fn forget() {
    disconnect();
    chat::stop();
    for k in [K_IDENTITY, K_DEVICE_SEED, K_ACCOUNT, K_DEVICE, K_USERNAME] {
        Storage::remove_item(k);
    }
    live().update(|l| {
        l.account = None;
        l.device = None;
        l.username = None;
        l.waiting = 0;
        l.problem = None;
    });
}

fn seed(key: &str) -> Option<[u8; 32]> {
    wire::unhex(&Storage::get_item(key)?)?.try_into().ok()
}

/// This device's id and its authentication key's seed.
pub(crate) fn device_key() -> Option<([u8; 16], [u8; 32])> {
    let device: [u8; 16] = wire::unhex(&Storage::get_item(K_DEVICE)?)?
        .try_into()
        .ok()?;
    Some((device, seed(K_DEVICE_SEED)?))
}

// ---------------------------------------------------------------- errors

/// A failure, in words the user can act on.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem(pub String);

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn problem(s: impl Into<String>) -> Problem {
    Problem(s.into())
}

/// The plain-language meaning of an api answer.
pub(crate) fn explain(status: u16, body: &[u8]) -> Problem {
    let word = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_default();
    problem(match (status, word.as_str()) {
        (403, "invite") => {
            "This code doesn't work. It may have been used already or expired. Ask your friend for a new one."
        }
        (403, "challenge") => "The sign-up check expired. Try again.",
        (409, "taken") => "That username is taken. Try another one.",
        (400, "proof") => "That username can't be used.",
        (401, _) => "The server didn't recognise this device.",
        (403, "pending") => "This device is still waiting to be approved by your phone.",
        (403, "primary") => "Only your main phone can do that.",
        (423, _) => "Too many wrong PINs. Wait a while, or set a new PIN on your main phone.",
        (429, "plan") => {
            "You've added all the sticker packs your plan allows this month. Plus allows more."
        }
        (429, _) => "Too many tries. Wait a moment and try again.",
        // Prompt 11: a plan's limit. Nothing already there is ever removed (D-0127).
        (409, "limit") => {
            "You've reached your plan's device limit. Plus allows more, and no device is removed."
        }
        (413, "plan") => "This file is larger than your plan allows. Plus raises the limit.",
        (403, "suspended") => {
            "Your account is suspended, so it can't do this. You can still read, export and appeal."
        }
        (403, "standing") => "Your account is limited for a while, so it can't do this yet.",
        (503, _) => "The server isn't offering this yet.",
        _ => {
            return Problem(format!(
                "The server answered {status}{}.",
                if word.is_empty() {
                    String::new()
                } else {
                    format!(" ({word})")
                }
            ));
        }
    })
}

// ---------------------------------------------------------------- http

fn endpoint() -> Endpoint {
    live().with_untracked(|l| l.endpoint.clone())
}

async fn send(request: HttpRequest) -> Result<(u16, Vec<u8>), Problem> {
    let lane = usage::lane_of(&request.url);
    // The request's line, headers and body, roughly as they go out.
    let body = match &request.body {
        nsh_core::platform::HttpBody::Bytes(b) => b.len(),
        _ => 0,
    };
    let out = body + request.url.len() + 200;
    // Songbird's own hosts go through the proxy when one is set (D-0164), never around it.
    if let Some(t) = tunnel::for_url(&request.url) {
        let t = t?;
        let body = match request.body {
            nsh_core::platform::HttpBody::Bytes(b) => b,
            nsh_core::platform::HttpBody::Empty => Vec::new(),
            _ => return Err(problem("This request can't go through the proxy.")),
        };
        let r = tunnel::through(
            t,
            request.method,
            request.url,
            request.headers,
            body,
            request.follow_redirects,
        )
        .await?;
        usage::count(lane, out, r.body.len() + 200);
        return Ok((r.status, r.body));
    }
    match fetch(request).await {
        Ok(r) => {
            usage::count(lane, out, r.body.len() + 200);
            Ok((r.status(), r.body))
        }
        Err(e) => Err(problem(format!(
            "Can't reach the server ({e:?}). Check the address in Server connection."
        ))),
    }
}

/// A request signed by this device (`authorization: Songbird …`).
pub(crate) async fn signed(
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    json: bool,
) -> Result<(u16, Vec<u8>), Problem> {
    let ep = endpoint();
    let (device, seed) =
        device_key().ok_or_else(|| problem("This device isn't registered yet."))?;
    let key = wire::ed25519(&seed).ok_or_else(|| problem("This device's key is damaged."))?;
    let (auth, digest) = wire::sign_request(
        &key,
        &device,
        &ep.api_name,
        method,
        path,
        unix_ms() as u64,
        body.as_deref(),
    );
    let mut req =
        HttpRequest::new(method, format!("{}{path}", ep.api)).header("authorization", auth);
    if let Some(d) = digest {
        req = req.header("x-songbird-content-sha256", d);
    }
    if let Some(b) = body {
        req = req
            .header(
                "content-type",
                if json {
                    "application/json"
                } else {
                    "application/octet-stream"
                },
            )
            .header("content-length", b.len().to_string())
            .body(b);
    }
    send(req).await
}

/// A request signed by this device whose body the signature does not cover
/// (attachment uploads, PROTOCOL §7.2: the client's own ciphertext, held
/// only to its length).
pub(crate) async fn signed_upload(
    method: &str,
    path: &str,
    body: Vec<u8>,
) -> Result<(u16, Vec<u8>), Problem> {
    let ep = endpoint();
    let (device, seed) =
        device_key().ok_or_else(|| problem("This device isn't registered yet."))?;
    let key = wire::ed25519(&seed).ok_or_else(|| problem("This device's key is damaged."))?;
    let (auth, _) = wire::sign_request(
        &key,
        &device,
        &ep.api_name,
        method,
        path,
        unix_ms() as u64,
        None,
    );
    let req = HttpRequest::new(method, format!("{}{path}", ep.api))
        .header("authorization", auth)
        .header("content-type", "application/octet-stream")
        .header("content-length", body.len().to_string())
        .body(body);
    send(req).await
}

/// An unsigned request to an api path (downloads and their confirmations
/// carry no account, PROTOCOL §7.3). Redirects are followed.
pub(crate) async fn unsigned(method: &str, path: &str) -> Result<(u16, Vec<u8>), Problem> {
    let url = if path.starts_with("http") {
        path.to_owned()
    } else {
        format!("{}{path}", endpoint().api)
    };
    send(HttpRequest::new(method, url)).await
}

pub(crate) fn ok_json(status: u16, body: &[u8]) -> Result<Value, Problem> {
    if (200..300).contains(&status) {
        Ok(serde_json::from_slice(body).unwrap_or(Value::Null))
    } else {
        Err(explain(status, body))
    }
}

// ---------------------------------------------------------------- registration

fn busy(what: Option<&str>) {
    live().update(|l| l.busy = what.map(str::to_owned));
}

/// Creates the account on the real backend: challenge, proof of work (on a
/// blocking thread), both Ed25519 keys, the invite code; then claims
/// `username`. The keys stay on this device.
pub async fn register(invite: Option<String>, username: Option<String>) -> Result<(), Problem> {
    let ep = endpoint();
    busy(Some("Getting a sign-up check from the server…"));
    let result = async {
        let (status, challenge) = send(HttpRequest::post(
            format!("{}/v1/registration/challenge", ep.api),
            Vec::new(),
        ))
        .await?;
        if status != 200 {
            return Err(explain(status, &challenge));
        }
        busy(Some("Solving the sign-up puzzle on your device…"));
        let bytes = challenge.clone();
        let solved = blocking(move || {
            let c = songbird_pow::decode(&bytes).ok()?;
            songbird_pow::solve(&c).ok().flatten()
        })
        .await
        .map_err(|_| problem("The sign-up puzzle stopped. Try again."))?
        .ok_or_else(|| problem("The server's sign-up check didn't make sense. Try again."))?;

        let identity_seed: [u8; 32] = wire::random();
        let device_seed: [u8; 32] = wire::random();
        let body = wire::registration(
            &ep.api_name,
            &challenge,
            solved,
            invite.as_deref(),
            &identity_seed,
            &device_seed,
        )
        .map_err(|e| match e {
            wire::BuildError::Invite => {
                problem("That code isn't in the right format. Codes are 24 letters and numbers.")
            }
            _ => problem("The sign-up request couldn't be built."),
        })?;
        busy(Some("Creating your account…"));
        let request = HttpRequest::post(format!("{}/v1/registration", ep.api), body.clone())
            .header("content-type", "application/octet-stream")
            .header("content-length", body.len().to_string());
        let (status, answer) = send(request).await?;
        let v = ok_json(status, &answer)?;
        let (Some(account), Some(device)) = (v["account"].as_str(), v["device"].as_str()) else {
            return Err(problem("The server's answer was incomplete."));
        };
        put(K_IDENTITY, &wire::hex(&identity_seed));
        put(K_DEVICE_SEED, &wire::hex(&device_seed));
        put(K_ACCOUNT, account);
        put(K_DEVICE, device);
        live().update(|l| {
            l.account = Some(account.into());
            l.device = Some(device.into());
        });
        if let Some(name) = username.filter(|n| !n.trim().is_empty()) {
            claim_username(&name).await?;
        }
        Ok(())
    }
    .await;
    busy(None);
    live().update(|l| l.problem = result.as_ref().err().map(|p| p.0.clone()));
    if result.is_ok() {
        connect();
    }
    result
}

/// Claims `name` for this account (`PUT /v1/username`): only its hash and
/// a proof go to the server.
pub async fn claim_username(name: &str) -> Result<(), Problem> {
    let body = wire::username_claim(name).ok_or_else(|| {
        problem("Usernames are 3 to 32 letters, numbers, _ or ., and start with a letter.")
    })?;
    let (status, answer) = signed("PUT", "/v1/username", Some(body), false).await?;
    if (200..300).contains(&status) {
        put(K_USERNAME, name);
        live().update(|l| l.username = Some(name.into()));
        Ok(())
    } else {
        Err(explain(status, &answer))
    }
}

// ---------------------------------------------------------------- account

/// A device the server lists for the account.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDevice {
    pub id: String,
    pub primary: bool,
    pub pending: bool,
    pub this_device: bool,
}

/// `GET /v1/devices`.
pub async fn devices() -> Result<Vec<RemoteDevice>, Problem> {
    let (status, body) = signed("GET", "/v1/devices", None, false).await?;
    let v = ok_json(status, &body)?;
    let me = live()
        .with_untracked(|l| l.device.clone())
        .unwrap_or_default();
    Ok(v["devices"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    let id = d["device"].as_str()?.to_string();
                    Some(RemoteDevice {
                        this_device: id.eq_ignore_ascii_case(&me),
                        primary: d["primary"].as_bool().unwrap_or(false),
                        pending: d["pending"].as_bool().unwrap_or(false),
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

thread_local! {
    /// Real device IDs (hex) by their UI ID.
    static DEVICE_HEX: std::cell::RefCell<std::collections::HashMap<u64, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

const K_DEVICE_NAMES: &str = "sb.device_names";

fn device_names() -> serde_json::Map<String, Value> {
    Storage::get_item(K_DEVICE_NAMES)
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Names this device gave the account's devices (a local label).
pub fn rename_device(id: u64, name: &str) {
    let Some(hex) = DEVICE_HEX.with_borrow(|m| m.get(&id).cloned()) else {
        return;
    };
    let mut names = device_names();
    names.insert(hex, Value::String(name.into()));
    put(K_DEVICE_NAMES, &Value::Object(names).to_string());
}

/// Fills the Linked devices page from the server's list (`GET /v1/devices`).
pub fn refresh_device_list(s: crate::store::Store) {
    if !registered() {
        return;
    }
    run(async move {
        let list = devices().await?;
        let names = device_names();
        let mut out = Vec::with_capacity(list.len());
        // This device first, as the page expects.
        let mut sorted = list;
        sorted.sort_by_key(|d| !d.this_device);
        for d in sorted {
            let bytes: [u8; 16] = wire::unhex(&d.id)
                .and_then(|b| b.try_into().ok())
                .unwrap_or([0; 16]);
            let id = chat::ui_id(&bytes);
            DEVICE_HEX.with_borrow_mut(|m| m.insert(id, d.id.clone()));
            let label = names.get(&d.id).and_then(Value::as_str).map_or_else(
                || {
                    if d.this_device {
                        "This device".to_string()
                    } else if d.primary {
                        "Main phone".to_string()
                    } else {
                        format!("Linked device {}", &d.id[..6])
                    }
                },
                str::to_owned,
            );
            out.push(crate::store::Device {
                id,
                name: label,
                kind: if d.primary {
                    crate::store::DeviceKind::Phone
                } else {
                    crate::store::DeviceKind::Desktop
                },
                last_active: if d.pending {
                    "Waiting for your main phone".into()
                } else if d.this_device {
                    "Active now".into()
                } else {
                    "Linked".into()
                },
                this_device: d.this_device,
            });
        }
        s.devices.set(out);
        Ok(())
    });
}

/// Logs a device out (`DELETE` through the signed device list, `link::remove`).
pub fn remove_device(s: crate::store::Store, id: u64) {
    let Some(hex) = DEVICE_HEX.with_borrow(|m| m.get(&id).cloned()) else {
        return;
    };
    run(async move {
        link::remove(&hex).await?;
        refresh_device_list(s);
        Ok(())
    });
}

/// `GET /v1/invites`: how many codes the account may create.
pub async fn invite_allowance() -> Result<(u64, bool), Problem> {
    let (status, body) = signed("GET", "/v1/invites", None, false).await?;
    let v = ok_json(status, &body)?;
    Ok((
        v["allowance"].as_u64().unwrap_or(0),
        v["invite_only"].as_bool().unwrap_or(true),
    ))
}

/// `POST /v1/invites`: a new code, shown once.
pub async fn create_invite() -> Result<String, Problem> {
    let (status, body) = signed("POST", "/v1/invites", Some(b"{}".to_vec()), true).await?;
    let v = ok_json(status, &body)?;
    let code = v["code"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    // Kept on this device: the only place that knows which codes are this account's (the
    // server keeps no per-inviter list, D-0038).
    let mut mine = plans::my_invites();
    mine.push(code.clone());
    put(
        plans::K_MY_INVITES,
        &serde_json::to_string(&mine).unwrap_or_default(),
    );
    Ok(code)
}

/// A GIF from the privacy proxy (`/v1/gifs/media/<token>` URLs: the GIF
/// provider never sees who is searching).
#[derive(Debug, Clone, PartialEq)]
pub struct Gif {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub still: String,
    pub gif: String,
}

/// `POST /v1/gifs/search` (or trending when `query` is empty).
pub async fn gifs(query: &str) -> Result<Vec<Gif>, Problem> {
    let (path, body) = if query.trim().is_empty() {
        ("/v1/gifs/trending", serde_json::json!({}))
    } else {
        ("/v1/gifs/search", serde_json::json!({ "q": query.trim() }))
    };
    let (status, answer) = signed("POST", path, Some(body.to_string().into_bytes()), true).await?;
    let v = ok_json(status, &answer)?;
    let base = endpoint().api;
    let abs = |u: &str| {
        if u.starts_with('/') {
            format!("{base}{u}")
        } else {
            u.to_string()
        }
    };
    Ok(v["gifs"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|g| {
                    Some(Gif {
                        id: g["id"].as_str()?.into(),
                        width: g["width"].as_u64().unwrap_or(1) as u32,
                        height: g["height"].as_u64().unwrap_or(1) as u32,
                        still: abs(g["still"].as_str()?),
                        gif: abs(g["gif"].as_str()?),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// `DELETE /v1/account` (main phone only), then forgets it here.
pub async fn delete_account() -> Result<(), Problem> {
    let (status, body) = signed("DELETE", "/v1/account", None, false).await?;
    if (200..300).contains(&status) {
        forget();
        erase_local();
        Ok(())
    } else {
        Err(explain(status, &body))
    }
}

/// Runs `job` and records its failure as the visible problem.
///
/// The task has no owner: it is the account's, not the view's. Started from a
/// button whose own view re-renders (saving the address updates `live`), an
/// owned task would be cancelled before its first poll and nothing would run.
pub fn run(job: impl std::future::Future<Output = Result<(), Problem>> + 'static) {
    Owner::detached().run(|| {
        spawn(async move {
            if let Err(p) = job.await {
                live().update(|l| l.problem = Some(p.0));
            }
        })
    });
}
