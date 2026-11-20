//! Backups, restore and the transfer to a new phone (Prompt 10, M5, D-0124; PROTOCOL §11).
//!
//! The format, the stores and the runs are `songbird-backup-format` and
//! `songbird-backup-storage`; this module chooses the store for the user's destination, gathers
//! what the app holds into a backup, puts a restored backup back, and drives the transfer and the
//! account recovery. The work runs on NSH's tokio runtime (`background`); the store's signals
//! change only back on the engine thread.
//!
//! What a backup holds today: the account (when registered: its ID, the account identity key's
//! seed, the username and the record store's key), the profile, every synced and device-only
//! setting and saved contacts; for a real account, every chat and message app-core keeps, as its
//! `Content` protobuf, with the files this device has when "Include media" is on (D-0165: read on
//! the backup's own thread from a second connection to app-core's store, and put back into it on
//! restore). Chats only the app shows (the walkthrough's) are kept with their text.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::vault::Storage;
use nsh_core::prelude::*;
use serde_json::Value;
use songbird_backup_format::object::Seen;
use songbird_backup_format::reader::{Backup, MemoryStaging};
use songbird_backup_format::schema::{self, Platform, record::Body as Rec};
use songbird_backup_format::{BackupKeys, OsRng, RecoveryKey};
use songbird_backup_storage::http::{Http, Request};
use songbird_backup_storage::local::LocalStore;
use songbird_backup_storage::run::{self, Options, RunError};
use songbird_backup_storage::s3::{S3Config, S3Store};
use songbird_backup_storage::transfer::{self, Code, Hello, PipeStore};
use songbird_backup_storage::vault::{Passes, VaultStore};
use songbird_backup_storage::webdav::{WebDavConfig, WebDavStore};
use songbird_backup_storage::{BackupStore, StoreError};
use zeroize::Zeroizing;

use super::{Problem, endpoint, explain, live, ok_json, problem, put, records, registered, wire};
use crate::store::{
    BackupDestination, Body, Chat, ChatKind, Message, Store, ext_onboarding::BackupExtra,
};
use crate::ui::Tick;

const K_RECOVERY: &str = "sb.recovery_secret";
const K_SEEN: &str = "sb.backup_seen";

// ---------------------------------------------------------------- the recovery key

/// This device's recovery key, made the first time it is asked for. mock: the device's
/// keychain through app-core (A0) will hold it; NSH's app storage stands in.
pub fn recovery_key() -> RecoveryKey {
    if let Some(k) = Storage::get_item(K_RECOVERY)
        .and_then(|h| wire::unhex(&h))
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
    {
        return RecoveryKey::from_secret(k);
    }
    let k = RecoveryKey::generate(&mut OsRng)
        .unwrap_or_else(|_| RecoveryKey::from_secret(wire::random()));
    adopt_recovery_key(&k);
    k
}

/// Keeps `k` as this device's recovery key (after restoring with it).
pub fn adopt_recovery_key(k: &RecoveryKey) {
    put(K_RECOVERY, &wire::hex(k.secret()));
}

/// The key as shown: 16 groups of four joined by dashes.
pub fn shown_key() -> String {
    recovery_key().display().to_string()
}

/// Where the rollback state of a backup at `dest` is kept: per destination, since each
/// destination's backup has its own generations (D-0165).
fn seen_key(keys: &BackupKeys, dest: BackupDestination) -> String {
    format!("{}/{dest:?}", wire::hex(&keys.vault_id))
}

fn seen(keys: &BackupKeys, dest: BackupDestination) -> Option<Seen> {
    let v: Value = serde_json::from_str(&Storage::get_item(K_SEEN)?).ok()?;
    let e = &v[seen_key(keys, dest)];
    Some(Seen {
        generation: e["g"].as_u64()?,
        hash: wire::unhex(e["h"].as_str()?)?.try_into().ok()?,
    })
}

fn save_seen(keys: &BackupKeys, dest: BackupDestination, s: Seen) {
    let mut v: Value = Storage::get_item(K_SEEN)
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    v[seen_key(keys, dest)] = serde_json::json!({"g": s.generation, "h": wire::hex(&s.hash)});
    put(K_SEEN, &v.to_string());
}

fn forget_seen(keys: &BackupKeys, dest: BackupDestination) {
    if let Some(mut v) =
        Storage::get_item(K_SEEN).and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        if let Some(o) = v.as_object_mut() {
            o.remove(&seen_key(keys, dest));
        }
        put(K_SEEN, &v.to_string());
    }
}

// ---------------------------------------------------------------- the backup settings (D-0165)

const K_CONFIG: &str = "sb.backup_config";

/// "just now", "5 minutes ago", "3 hours ago", "2 days ago".
#[must_use]
pub fn ago(ms: u64) -> String {
    let now = nsh_core::time::unix_ms() as u64;
    let m = now.saturating_sub(ms) / 60_000;
    match m {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{m} minutes ago"),
        60..=119 => "an hour ago".into(),
        120..=1439 => format!("{} hours ago", m / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", m / 1440),
    }
}

fn dest_code(d: BackupDestination) -> String {
    format!("{d:?}")
}

fn dest_from(code: &str) -> Option<BackupDestination> {
    use BackupDestination::*;
    [
        SongbirdServers,
        EncryptedFile,
        ICloudDrive,
        GoogleDrive,
        OneDrive,
        Dropbox,
        Nextcloud,
        S3,
    ]
    .into_iter()
    .find(|d| dest_code(*d) == code)
}

/// Saves the backup settings on this device: on or off, where, how often, media, the second
/// copy, the last backup's time and size, and the destination's sign-in. Device-only (each
/// device backs up its own way); the passwords sit with this device's other secrets (the
/// recovery key, the store key) and move to the keychain with them.
fn save_config(s: Store) {
    let x = s.settings.get_untracked();
    let b = s.backup_extra().get_untracked();
    let v = serde_json::json!({
        "on": x.backup_on,
        "dest": dest_code(x.backup_destination),
        "media": x.backup_include_media,
        "size": x.backup_size,
        "freq": match b.frequency {
            crate::store::ext_onboarding::Frequency::Daily => "d",
            crate::store::ext_onboarding::Frequency::Weekly => "w",
            crate::store::ext_onboarding::Frequency::Monthly => "m",
        },
        "second": b.second.map(dest_code),
        "last": b.last_ms,
        "nc": [b.nextcloud_url, b.nextcloud_user, b.nextcloud_password],
        "s3": [b.s3_endpoint, b.s3_bucket, b.s3_region, b.s3_key, b.s3_secret],
    });
    put(K_CONFIG, &v.to_string());
}

fn load_config(s: Store) {
    use crate::store::ext_onboarding::Frequency;
    let Some(v) = Storage::get_item(K_CONFIG).and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return;
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    s.settings_update(|x| {
        x.backup_on = v["on"].as_bool().unwrap_or(false);
        if let Some(d) = v["dest"].as_str().and_then(dest_from) {
            x.backup_destination = d;
        }
        x.backup_include_media = v["media"].as_bool().unwrap_or(true);
        x.backup_size = text(&v["size"]);
        x.backup_daily = v["freq"] == "d";
    });
    let last = v["last"].as_u64().unwrap_or(0);
    s.settings_update(|x| x.backup_last = (last > 0).then(|| ago(last)));
    s.backup_extra().update(|b| {
        b.frequency = match v["freq"].as_str() {
            Some("w") => Frequency::Weekly,
            Some("m") => Frequency::Monthly,
            _ => Frequency::Daily,
        };
        b.second = v["second"].as_str().and_then(dest_from);
        b.last_ms = last;
        b.nextcloud_url = text(&v["nc"][0]);
        b.nextcloud_user = text(&v["nc"][1]);
        b.nextcloud_password = text(&v["nc"][2]);
        b.s3_endpoint = text(&v["s3"][0]);
        b.s3_bucket = text(&v["s3"][1]);
        b.s3_region = text(&v["s3"][2]);
        b.s3_key = text(&v["s3"][3]);
        b.s3_secret = text(&v["s3"][4]);
    });
}

/// Runs a backup when one is due by the chosen frequency (and none is running).
fn run_if_due(s: Store) {
    let (on, ready) = s
        .settings
        .with_untracked(|x| (x.backup_on, available(x.backup_destination)));
    let b = s.backup_extra().get_untracked();
    let now = nsh_core::time::unix_ms() as u64;
    if on
        && ready
        && b.running.is_none()
        && now.saturating_sub(b.last_ms) >= b.frequency.period_ms()
    {
        s.back_up_now();
    }
    // Keeps "Last backup …" true as time passes.
    if b.last_ms > 0 {
        let label = Some(ago(b.last_ms));
        if s.settings.with_untracked(|x| x.backup_last != label) {
            s.settings_update(|x| x.backup_last = label);
        }
    }
}

/// Loads the backup settings, saves them as they change, and backs up on schedule: at start
/// (a minute in, once the connection is up) and every ten minutes after. Called once at the
/// root.
pub fn install(s: Store) {
    load_config(s);
    let extra = s.backup_extra();
    effect(move || {
        s.settings.with(|x| {
            (
                x.backup_on,
                x.backup_destination,
                x.backup_include_media,
                x.backup_size.clone(),
            )
        });
        extra.with(|b| {
            (
                b.frequency,
                b.second,
                b.last_ms,
                b.nextcloud_url.clone(),
                b.nextcloud_user.clone(),
                b.nextcloud_password.clone(),
                b.s3_endpoint.clone(),
                b.s3_bucket.clone(),
                b.s3_region.clone(),
                b.s3_key.clone(),
                b.s3_secret.clone(),
            )
        });
        untracked(|| save_config(s));
    });
    Owner::detached().run(|| {
        set_timeout(std::time::Duration::from_secs(60), move || run_if_due(s));
        set_interval(std::time::Duration::from_secs(600), move || run_if_due(s));
    });
}

// ---------------------------------------------------------------- where backups go

/// Vault tokens (Prompt 11, D-0130): day passes and creation tokens, asked of `api` as this
/// device (the account's main phone), **blinded**, so the vault never learns the account. The
/// token key is pinned the first time this process sees it; release builds ship it (A12).
struct AppPasses {
    http: Arc<super::tunnel::AppHttp>,
    api: String,
    device: [u8; 16],
    seed: Zeroizing<[u8; 32]>,
    server: String,
}

/// Day passes fetched ahead (`day → token`), kept in app storage: a pass is fetched days
/// before the vault sees it, so the server cannot pair the account's signed request with the
/// vault's write by time (D-0134).
const K_PASSES: &str = "sb.vault_passes";
/// Days of passes kept ahead (issuance allows today and six more).
const PASS_DAYS: u32 = 7;

fn cached_passes() -> std::collections::BTreeMap<u32, Value> {
    Storage::get_item(K_PASSES)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_passes(p: &std::collections::BTreeMap<u32, Value>) {
    put(K_PASSES, &serde_json::to_string(p).unwrap_or_default());
}

impl AppPasses {
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        signed: bool,
    ) -> Result<(u16, Vec<u8>), StoreError> {
        let mut r = Request::new(method, format!("{}{path}", self.api));
        if signed {
            let key = wire::ed25519(&self.seed).ok_or(StoreError::Unauthorized)?;
            let (auth, digest) = wire::sign_request(
                &key,
                &self.device,
                &self.server,
                method,
                path,
                nsh_core::time::unix_ms() as u64,
                body.as_deref(),
            );
            r = r.header("authorization", auth);
            if let Some(d) = digest {
                r = r.header("x-songbird-content-sha256", d);
            }
        }
        if let Some(b) = body {
            r = r.header("content-type", "application/json").body(b);
        }
        let resp = self.http.send(r).await?;
        Ok((resp.status, resp.body))
    }

    /// The current token key, pinned: a server that later shows another key for the same ID
    /// is refused (it could be trying to tell this device's tokens apart).
    async fn pinned(&self) -> Result<(u8, [u8; 32]), StoreError> {
        let (st, body) = self.call("GET", "/v1/tokens/keys", None, false).await?;
        if st != 200 {
            return Err(StoreError::Unavailable { status: st });
        }
        let v: Value = serde_json::from_slice(&body).map_err(|_| StoreError::Protocol("keys"))?;
        let id = v
            .get("current")
            .and_then(Value::as_u64)
            .and_then(|n| u8::try_from(n).ok())
            .ok_or(StoreError::Protocol("keys"))?;
        let pk: [u8; 32] = v
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter()
                    .find(|k| k.get("id").and_then(Value::as_u64) == Some(u64::from(id)))
            })
            .and_then(|k| k.get("public").and_then(Value::as_str))
            .and_then(wire::unhex)
            .and_then(|b| b.try_into().ok())
            .ok_or(StoreError::Protocol("keys"))?;
        // The same trust as plans: a key shipped with the build, or (development) the first seen.
        super::plans::trusted_token_key(id, &pk).map_err(|_| StoreError::Protocol("token key"))?;
        Ok((id, pk))
    }

    /// This account's level today (Free when it can't be read).
    async fn level(&self) -> u8 {
        match self.call("GET", "/v1/entitlements", None, true).await {
            Ok((200, body)) => serde_json::from_slice::<Value>(&body)
                .ok()
                .filter(|v| v.get("level").and_then(Value::as_str) == Some("plus"))
                .map_or(0, |_| 1),
            _ => 0,
        }
    }

    async fn one(
        &self,
        kind: songbird_tokens::token::Kind,
        level: u8,
        period: u32,
    ) -> Result<Value, StoreError> {
        use songbird_tokens::token::{self, Info, IssueResponse};
        let (id, pk) = self.pinned().await?;
        let (pending, req) = token::request(
            id,
            Info {
                kind,
                level,
                period,
            },
            1,
        )
        .map_err(|_| StoreError::Protocol("token"))?;
        let body = serde_json::to_vec(&req).map_err(|_| StoreError::Protocol("token"))?;
        let (st, b) = self
            .call("POST", "/v1/vault/tokens", Some(body), true)
            .await?;
        if st != 200 {
            return Err(if st == 429 {
                StoreError::Throttled {
                    retry_after_ms: 60_000,
                }
            } else {
                StoreError::Unauthorized
            });
        }
        let resp: IssueResponse =
            serde_json::from_slice(&b).map_err(|_| StoreError::Protocol("token"))?;
        let t = pending
            .finish(&pk, &resp)
            .map_err(|_| StoreError::Protocol("token proof"))?
            .pop()
            .ok_or(StoreError::Protocol("token"))?;
        serde_json::to_value(t).map_err(|_| StoreError::Protocol("token"))
    }
}

impl AppPasses {
    /// Tops up the passes kept ahead (called at start-up, never by a backup run).
    async fn prefetch(&self) -> Result<(), StoreError> {
        let today = songbird_backup_storage::vault::today();
        let mut cache = cached_passes();
        cache.retain(|d, _| *d > today);
        if cache.len() >= 3 {
            save_passes(&cache);
            return Ok(());
        }
        let level = self.level().await;
        for day in today + 1..today + PASS_DAYS {
            if !cache.contains_key(&day) {
                let p = self
                    .one(songbird_tokens::token::Kind::VaultDay, level, day)
                    .await?;
                cache.insert(day, p);
            }
        }
        save_passes(&cache);
        Ok(())
    }
}

/// Keeps vault day passes ahead when backups go to Songbird's servers (start-up only).
pub async fn prefetch_passes() -> Result<(), Problem> {
    let (device, seed) = super::device_key().ok_or_else(|| problem("Not registered."))?;
    let http = Arc::new(super::tunnel::AppHttp::new()?);
    let ep = endpoint();
    AppPasses {
        http,
        api: ep.api.clone(),
        device,
        seed: Zeroizing::new(seed),
        server: ep.api_name,
    }
    .prefetch()
    .await
    .map_err(|_| problem("Couldn't fetch backup passes ahead."))
}

#[async_trait::async_trait]
impl Passes for AppPasses {
    async fn day_pass(&self, day: u32) -> Result<Value, StoreError> {
        let mut cache = cached_passes();
        cache.retain(|d, _| *d >= day);
        if let Some(p) = cache.remove(&day) {
            save_passes(&cache);
            return Ok(p);
        }
        // None kept (a first backup, or a long gap): fetched now, the one case where the
        // request and the write are close together.
        let level = self.level().await;
        self.one(songbird_tokens::token::Kind::VaultDay, level, day)
            .await
    }

    async fn creation(&self) -> Result<Value, StoreError> {
        self.one(
            songbird_tokens::token::Kind::VaultCreate,
            0,
            songbird_backup_storage::vault::today() / 180,
        )
        .await
    }
}

#[cfg(test)]
thread_local! {
    /// Tests keep folder backups in a temporary folder, never the user's Documents.
    static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The app's own private folder: the platform's app data folder, or, in a debug build run
/// without one (the headless test runner), a folder in the user's home. Never the shared
/// temporary directory, whose names others on the machine can predict and take first.
fn app_dir() -> Option<PathBuf> {
    data_dir().or_else(|| {
        cfg!(debug_assertions)
            .then(|| std::env::var_os("HOME"))
            .flatten()
            .map(|h| PathBuf::from(h).join(".songbird-dev"))
    })
}

/// Where a folder backup lives on this device.
fn folder(keys: &BackupKeys) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(d) = TEST_DIR.with(|t| t.borrow().clone()) {
        return Some(d.join(wire::hex(keys.vault_id.get(..8).unwrap_or_default())));
    }
    // Debug builds keep folder backups in the app's own data folder, so development and the
    // screenshot runs never write into the user's Documents.
    let documents = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join("Documents").join("Songbird Backup"))
        .filter(|_| !cfg!(debug_assertions))
        .filter(|_| {
            matches!(
                os(),
                nsh_core::platform::Os::MacOs
                    | nsh_core::platform::Os::Linux
                    | nsh_core::platform::Os::Windows
            )
        });
    let base = documents.or_else(|| app_dir().map(|d| d.join("backups")))?;
    Some(base.join(wire::hex(keys.vault_id.get(..8).unwrap_or_default())))
}

/// Whether the app can use this destination yet.
pub fn available(d: BackupDestination) -> bool {
    use BackupDestination::*;
    matches!(d, SongbirdServers | EncryptedFile | Nextcloud | S3)
}

/// Why a destination can't be used yet, in words.
pub fn unavailable_reason(d: BackupDestination) -> &'static str {
    use BackupDestination::*;
    match d {
        GoogleDrive | OneDrive | Dropbox => {
            "Coming soon: Songbird needs its own sign-in with this service first."
        }
        ICloudDrive => "Coming soon, with the iPhone and Mac apps.",
        _ => "",
    }
}

/// The store for the user's destination, under `keys`. `write`: vault writes get passes.
fn store_for(
    dest: BackupDestination,
    extra: &BackupExtra,
    keys: &BackupKeys,
    write: bool,
) -> Result<Arc<dyn BackupStore>, Problem> {
    let http = Arc::new(super::tunnel::AppHttp::new()?);
    match dest {
        BackupDestination::SongbirdServers => {
            if !super::is_live() {
                return Err(problem(
                    "Connect to a Songbird server first (You → Server connection).",
                ));
            }
            let ep = endpoint();
            let passes = if write {
                let (device, seed) = super::device_key()
                    .ok_or_else(|| problem("Create your account on the server first."))?;
                Some(Arc::new(AppPasses {
                    http: Arc::clone(&http),
                    api: ep.api.clone(),
                    device,
                    seed: Zeroizing::new(seed),
                    server: ep.api_name.clone(),
                }) as Arc<dyn Passes>)
            } else {
                None
            };
            Ok(Arc::new(VaultStore::new(http, ep.api, keys, passes)))
        }
        BackupDestination::EncryptedFile => Ok(Arc::new(LocalStore::new(
            folder(keys).ok_or_else(|| problem("This device has no folder for backups."))?,
        ))),
        BackupDestination::Nextcloud => Ok(Arc::new(
            WebDavStore::new(
                http,
                &WebDavConfig {
                    url: format!(
                        "{}/{}",
                        extra.nextcloud_url.trim_end_matches('/'),
                        &wire::hex(&keys.vault_id)[..16]
                    ),
                    user: extra.nextcloud_user.clone(),
                    password: Zeroizing::new(extra.nextcloud_password.clone()),
                },
            )
            .map_err(|_| problem("The server address should start with https://."))?,
        )),
        BackupDestination::S3 => Ok(Arc::new(
            S3Store::new(
                http,
                S3Config {
                    endpoint: extra.s3_endpoint.trim_end_matches('/').to_owned(),
                    bucket: extra.s3_bucket.clone(),
                    region: if extra.s3_region.is_empty() {
                        "us-east-1".into()
                    } else {
                        extra.s3_region.clone()
                    },
                    access_key: Zeroizing::new(extra.s3_key.clone()),
                    secret_key: Zeroizing::new(extra.s3_secret.clone()),
                    prefix: format!("songbird/{}/", &wire::hex(&keys.vault_id)[..16]),
                },
            )
            .map_err(|_| problem("The endpoint should look like https://s3.example.com."))?,
        )),
        other => Err(problem(unavailable_reason(other))),
    }
}

fn explain_store(e: &RunError) -> Problem {
    problem(match e {
        RunError::Store(StoreError::Full) => {
            "Your backup space is full. Plus has more room, and nothing already backed up is removed."
        }
        RunError::Store(StoreError::Unauthorized) => {
            "The backup place refused this device. Check the account details and try again."
        }
        RunError::Store(StoreError::NotFound) | RunError::Empty => {
            "There's no backup there for this recovery key."
        }
        RunError::Store(StoreError::Unavailable { .. } | StoreError::Io(_)) => {
            "Couldn't reach the backup place. Check your connection and try again."
        }
        RunError::Store(StoreError::Conflict { .. }) => {
            "Another device changed the backup at the same time. Try again."
        }
        RunError::Store(StoreError::Throttled { .. }) => {
            "Too many tries. Wait a moment and try again."
        }
        RunError::Format(songbird_backup_format::Error::TooNew) => {
            "This backup was made by a newer Songbird. Update Songbird, then try again."
        }
        RunError::Format(
            songbird_backup_format::Error::Rollback | songbird_backup_format::Error::Fork,
        )
        | RunError::Lost => {
            "The backup there is older than one this device made. Nothing was changed."
        }
        RunError::Format(_) => "This backup looks damaged, so nothing was restored.",
        _ => "The backup stopped. Try again.",
    })
}

// ---------------------------------------------------------------- what a backup holds

/// Everything a backup writes, gathered on the engine thread.
#[derive(Clone, Default)]
pub struct Snapshot {
    records: Vec<Rec>,
    /// app-core's store, read on the backup's thread (a real account's chats and messages).
    core: Option<(PathBuf, Zeroizing<[u8; 32]>)>,
    /// Real chats' list state: (chat, title, archived, pinned, muted until).
    real: Vec<([u8; 16], String, bool, bool, u64)>,
    /// Whether the files this device has go in too.
    media: bool,
    /// Messages and chats, for the counts the screens show.
    pub messages: u32,
    pub chats: u32,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("messages", &self.messages)
            .finish_non_exhaustive()
    }
}

fn id16(n: u64) -> Vec<u8> {
    let mut b = vec![0u8; 8];
    b.extend_from_slice(&n.to_be_bytes());
    b
}

/// A UI ID from a backup's 16 bytes; `None` for app-core's (random) IDs.
fn from16(b: &[u8]) -> Option<u64> {
    if b.len() != 16 || b.get(..8)? != [0u8; 8] {
        return None;
    }
    let tail: [u8; 8] = b.get(8..16)?.try_into().ok()?;
    Some(u64::from_be_bytes(tail))
}

fn text_of(b: &Body) -> String {
    match b {
        Body::Text(t) => t.clone(),
        other => other.preview(),
    }
}

/// The store's content in the backup's canonical order (PROTOCOL §11.5): the account, the
/// profile, settings, contacts, every chat, then every message by time.
pub fn snapshot(s: Store) -> Snapshot {
    let mut records = vec![];
    if registered() {
        let l = live().get_untracked();
        let seed = Storage::get_item(super::K_IDENTITY).and_then(|h| wire::unhex(&h));
        let mut a = schema::Account::default();
        a.account_id = l
            .account
            .as_deref()
            .and_then(wire::unhex)
            .unwrap_or_default();
        a.identity_seed = seed.unwrap_or_default();
        a.storage_key = records::storage_key()
            .map(|k| k.to_vec())
            .unwrap_or_default();
        a.username = l.username.unwrap_or_default();
        a.server_name = l.endpoint.api_name;
        records.push(Rec::Account(a));
    }
    let me = s.me.get_untracked();
    records.push(Rec::Profile(schema::Profile {
        name: me.name,
        about: me.about,
        status_text: me.status_line.unwrap_or_default(),
        ..schema::Profile::default()
    }));
    let settings = s.settings.get_untracked();
    let ext = s.ext().get_untracked();
    for (k, v) in records::encode(&settings, &ext) {
        records.push(Rec::Setting(schema::Setting {
            key: k.into(),
            value: v.into_bytes(),
        }));
    }
    for (k, get, _) in records::DEVICE {
        records.push(Rec::Setting(schema::Setting {
            key: format!("d/{k}"),
            value: get(&settings, &ext).into_bytes(),
        }));
    }
    for c in s.contacts.get_untracked() {
        records.push(Rec::Contact(schema::Contact {
            contact_id: id16(c.id),
            name: c.name,
            notes: c.about,
            buddy_groups: vec![c.username],
            verified: c.verified,
            ..schema::Contact::default()
        }));
    }
    let core = if registered() {
        super::chat::core_db(false)
    } else {
        None
    };
    let chats = s.chats.get_untracked();
    let mut real = Vec::new();
    for c in &chats {
        if core.is_some()
            && let Some(id) = super::chat::core_chat(c.id)
        {
            real.push((
                id,
                c.title.clone(),
                c.archived,
                c.pinned,
                if c.muted { c.muted_until } else { 0 },
            ));
            continue;
        }
        let kind = match c.kind {
            ChatKind::Direct(_) | ChatKind::WhatsApp(_) => schema::ChatKind::Direct,
            ChatKind::Group | ChatKind::Community => schema::ChatKind::Group,
            ChatKind::Channel { .. } => schema::ChatKind::Channel,
        };
        records.push(Rec::Chat(schema::Chat {
            chat_id: id16(c.id),
            kind: kind.into(),
            title: c.title.clone(),
            archived: c.archived,
            pinned: c.pinned,
            muted_until_ms: if c.muted { c.muted_until } else { 0 },
            ..schema::Chat::default()
        }));
    }
    let mut messages: Vec<Message> = s.messages.with_untracked(|m| {
        m.values()
            .flatten()
            .filter(|m| core.is_none() || !super::chat::owns(m.chat))
            .cloned()
            .collect()
    });
    messages.sort_by_key(|m| (m.at, m.chat, m.id));
    let n = u32::try_from(messages.len()).unwrap_or(u32::MAX);
    for m in messages {
        records.push(Rec::Message(schema::Message {
            chat_id: id16(m.chat),
            message_id: id16(m.id),
            sender: m.from.map(id16).unwrap_or_default(),
            sent_ms: u64::try_from(m.at).unwrap_or(0).saturating_mul(60_000),
            content: text_of(&m.body).into_bytes(),
            status: if m.mine() {
                schema::MessageStatus::Read
            } else {
                schema::MessageStatus::Received
            }
            .into(),
            starred: m.starred,
            ..schema::Message::default()
        }));
    }
    // The real messages a backup keeps, counted for the progress bar.
    let kept = core
        .as_ref()
        .and_then(|(path, key)| songbird_app_core::store::Store::open(path, key).ok())
        .and_then(|st| st.backup_count().ok())
        .unwrap_or(0);
    Snapshot {
        records,
        messages: n.saturating_add(kept),
        chats: u32::try_from(chats.len()).unwrap_or(u32::MAX),
        core,
        real,
        media: s.settings.with_untracked(|x| x.backup_include_media),
    }
}

/// A restored backup, ready to put back.
#[derive(Clone, Default)]
pub struct Restored {
    records: Vec<Rec>,
    /// Media by reference (small and streamed), for app-core's files.
    media: Arc<std::collections::HashMap<[u8; 32], Zeroizing<Vec<u8>>>>,
    pub messages: u32,
    pub chats: u32,
}

impl std::fmt::Debug for Restored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restored")
            .field("messages", &self.messages)
            .finish_non_exhaustive()
    }
}

impl Restored {
    /// The account record, if the backup has one.
    fn account(&self) -> Option<schema::Account> {
        self.records.iter().find_map(|r| match r {
            Rec::Account(a) => Some(a.clone()),
            _ => None,
        })
    }
}

/// Puts a restored backup back into the store, in one change: settings, profile, contacts,
/// and every chat's messages (chats this device doesn't have yet are added).
pub fn apply(s: Store, r: &Restored) {
    let mut settings = s.settings.get_untracked();
    let mut ext = s.ext().get_untracked();
    let pairs: Vec<(String, String)> = r
        .records
        .iter()
        .filter_map(|x| match x {
            Rec::Setting(st) => Some((
                st.key.strip_prefix("d/").unwrap_or(&st.key).to_owned(),
                String::from_utf8_lossy(&st.value).into_owned(),
            )),
            _ => None,
        })
        .collect();
    records::apply(
        pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        &mut settings,
        &mut ext,
    );
    s.settings.set(settings);
    s.ext().set(ext);
    for x in &r.records {
        if let Rec::Profile(p) = x {
            s.me.update(|me| {
                me.name.clone_from(&p.name);
                me.about.clone_from(&p.about);
                me.status_line = (!p.status_text.is_empty()).then(|| p.status_text.clone());
            });
        }
    }
    // Chats: the ones this device has keep their look; new ones take the first chat's.
    let mut chats = s.chats.get_untracked();
    let template = chats.first().cloned();
    let mut by_chat: std::collections::HashMap<u64, Vec<Message>> =
        std::collections::HashMap::new();
    for x in &r.records {
        match x {
            Rec::Chat(c) => {
                let Some(id) = from16(&c.chat_id) else {
                    continue;
                };
                if let Some(have) = chats.iter_mut().find(|h| h.id == id) {
                    have.title.clone_from(&c.title);
                    have.archived = c.archived;
                    have.pinned = c.pinned;
                } else if let Some(t) = &template {
                    let mut n: Chat = t.clone();
                    n.id = id;
                    n.title.clone_from(&c.title);
                    n.kind = ChatKind::Group;
                    n.members.clear();
                    n.archived = c.archived;
                    n.pinned = c.pinned;
                    n.unread = 0;
                    chats.push(n);
                }
            }
            Rec::Message(m) => {
                let (Some(chat), Some(id)) = (from16(&m.chat_id), from16(&m.message_id)) else {
                    continue;
                };
                by_chat.entry(chat).or_default().push(Message {
                    id,
                    chat,
                    from: from16(&m.sender),
                    body: Body::Text(String::from_utf8_lossy(&m.content).into_owned()),
                    at: crate::store::minutes_of_ms(m.sent_ms),
                    status: Tick::Read,
                    reply_to: None,
                    reactions: vec![],
                    edited: false,
                    starred: m.starred,
                    pinned: false,
                    forwarded: false,
                    receipts: vec![],
                });
            }
            _ => {}
        }
    }
    s.chats.set(chats);
    s.messages.update(|all| {
        for (chat, msgs) in by_chat {
            all.insert(chat, msgs);
        }
    });
}

/// Feeds a snapshot to the writer, counting records for the progress bar.
fn write_all(
    w: &mut songbird_backup_format::writer::Writer<'_, run::ChannelSinkHandle>,
    snap: Snapshot,
    done: &AtomicU32,
) -> Result<(), songbird_backup_format::Error> {
    let mut n = 0u32;
    let mut count = || {
        n = n.saturating_add(1);
        done.store(n, Ordering::Relaxed);
    };
    // Messages come after every chat (the canonical order): the app's own first, then app-core's.
    let (head, tail): (Vec<Rec>, Vec<Rec>) = snap
        .records
        .into_iter()
        .partition(|r| !matches!(r, Rec::Message(_)));
    for r in head {
        w.record(r)?;
        count();
    }
    let core = snap.core.as_ref().map(|(path, key)| {
        songbird_app_core::store::Store::open(path, key)
            .map_err(|_| songbird_backup_format::Error::Store("messages".into()))
    });
    let core = core.transpose()?;
    if let Some(st) = &core {
        let chats = st
            .backup_chats()
            .map_err(|_| songbird_backup_format::Error::Store("chats".into()))?;
        for (id, group) in chats {
            let state = snap.real.iter().find(|r| r.0 == id);
            w.record(Rec::Chat(schema::Chat {
                chat_id: id.to_vec(),
                kind: if group {
                    schema::ChatKind::Group
                } else {
                    schema::ChatKind::Direct
                }
                .into(),
                title: state.map(|r| r.1.clone()).unwrap_or_default(),
                archived: state.is_some_and(|r| r.2),
                pinned: state.is_some_and(|r| r.3),
                muted_until_ms: state.map_or(0, |r| r.4),
                ..schema::Chat::default()
            }))?;
            count();
        }
    }
    for r in tail {
        w.record(r)?;
        count();
    }
    if let Some(st) = &core {
        let mut failed = None;
        let r = st.backup_messages(|k| {
            let mut refs = Vec::new();
            if snap.media {
                let pointers = songbird_app_core::messenger::attachments_of(&k.row.content);
                for idx in &k.files {
                    let Ok(Some(bytes)) = st.file(&k.row.author, &k.row.id, *idx) else {
                        continue;
                    };
                    let p = usize::try_from(*idx).ok().and_then(|i| pointers.get(i));
                    let meta = schema::Media {
                        mime: p.map(|p| p.content_type.clone()).unwrap_or_default(),
                        file_name: p.map(|p| p.file_name.clone()).unwrap_or_default(),
                        width: p.map_or(0, |p| p.width),
                        height: p.map_or(0, |p| p.height),
                        duration_ms: p.map_or(0, |p| p.duration_ms),
                        ..schema::Media::default()
                    };
                    match w.media(meta, &mut bytes.as_slice()) {
                        Ok(r) => refs.push(r.to_vec()),
                        Err(e) => {
                            failed = Some(e);
                            return Err(songbird_app_core::Error::Storage);
                        }
                    }
                }
            }
            let rec = Rec::Message(schema::Message {
                chat_id: k.row.chat.to_vec(),
                message_id: k.row.id.to_vec(),
                sender: k.row.author.to_vec(),
                sent_ms: k.row.sent_ms,
                received_ms: k.row.stored_ms,
                content: k.row.content,
                media: refs,
                status: core_status(k.row.status).into(),
                starred: false,
            });
            if let Err(e) = w.record(rec) {
                failed = Some(e);
                return Err(songbird_app_core::Error::Storage);
            }
            count();
            Ok(())
        });
        if let Some(e) = failed {
            return Err(e);
        }
        r.map_err(|_| songbird_backup_format::Error::Store("messages".into()))?;
    }
    Ok(())
}

fn core_status(s: songbird_app_core::store::Status) -> schema::MessageStatus {
    use songbird_app_core::store::Status as S;
    match s {
        S::Failed => schema::MessageStatus::Failed,
        S::Sending => schema::MessageStatus::Unknown,
        S::Sent => schema::MessageStatus::Sent,
        S::Delivered => schema::MessageStatus::Delivered,
        S::Read | S::Played => schema::MessageStatus::Read,
    }
}

fn status_of(s: i32) -> songbird_app_core::store::Status {
    use songbird_app_core::store::Status as S;
    match schema::MessageStatus::try_from(s) {
        Ok(schema::MessageStatus::Sent) => S::Sent,
        Ok(schema::MessageStatus::Delivered | schema::MessageStatus::Received) => S::Delivered,
        Ok(schema::MessageStatus::Read) => S::Read,
        // A send that never finished is put back failed, so the user can see and retry it.
        _ => S::Failed,
    }
}

/// Every media file of a verified backup, by reference.
fn media_of(
    b: &Backup<'_>,
) -> Result<std::collections::HashMap<[u8; 32], Zeroizing<Vec<u8>>>, songbird_backup_format::Error>
{
    use std::io::Read as _;
    let mut out = std::collections::HashMap::new();
    for blob in b.blobs() {
        let (r, bytes) = blob?;
        out.insert(r, Zeroizing::new(bytes.to_vec()));
    }
    for r in b.media_streams() {
        if let Some(mut s) = b.media(&r) {
            let mut v = Zeroizing::new(Vec::new());
            s.read_to_end(&mut v)?;
            s.finish()?;
            out.insert(r, v);
        }
    }
    Ok(out)
}

/// Writes restored chats, messages and their files into app-core's store.
fn import_into(
    st: &mut songbird_app_core::store::Store,
    groups: &[([u8; 16], bool)],
    messages: &[schema::Message],
    media: &std::collections::HashMap<[u8; 32], Zeroizing<Vec<u8>>>,
) -> songbird_app_core::Result<()> {
    for (id, group) in groups {
        st.restore_chat(id, *group)?;
    }
    for m in messages {
        let (Ok(chat), Ok(author), Ok(id)) = (
            <[u8; 16]>::try_from(m.chat_id.as_slice()),
            <[u8; 16]>::try_from(m.sender.as_slice()),
            <[u8; 16]>::try_from(m.message_id.as_slice()),
        ) else {
            continue;
        };
        // One message the format refuses (a timer, view-once) is skipped, not fatal.
        match st.restore_message(
            &chat,
            &author,
            &m.content,
            m.received_ms,
            status_of(m.status),
        ) {
            Ok(_) | Err(songbird_app_core::Error::Malformed) => {}
            Err(e) => return Err(e),
        }
        for (idx, r) in m.media.iter().enumerate() {
            let (Ok(r), Ok(idx)) = (<[u8; 32]>::try_from(r.as_slice()), u32::try_from(idx)) else {
                continue;
            };
            if let Some(bytes) = media.get(&r) {
                st.put_file(&author, &id, idx, bytes)?;
            }
        }
    }
    Ok(())
}

/// Puts a restored real account's chats, messages and files back into app-core's store (on
/// this device's own key; a new device's store is made here), then shows them. Chats the
/// backup kept for the app alone are [`apply`]'s.
pub async fn import_core(s: Store, r: &Restored) -> Result<(), Problem> {
    let real_chats: Vec<schema::Chat> = r
        .records
        .iter()
        .filter_map(|x| match x {
            Rec::Chat(c) if from16(&c.chat_id).is_none() => Some(c.clone()),
            _ => None,
        })
        .collect();
    let real_messages: Vec<schema::Message> = r
        .records
        .iter()
        .filter_map(|x| match x {
            Rec::Message(m) if from16(&m.chat_id).is_none() => Some(m.clone()),
            _ => None,
        })
        .collect();
    if real_chats.is_empty() && real_messages.is_empty() {
        return Ok(());
    }
    let (path, key) = super::chat::core_db(true)
        .ok_or_else(|| problem("This device has no place to keep messages."))?;
    let media = Arc::clone(&r.media);
    let groups_restored: Vec<([u8; 16], bool)> = real_chats
        .iter()
        .filter_map(|c| {
            let id: [u8; 16] = c.chat_id.as_slice().try_into().ok()?;
            Some((id, c.kind == i32::from(schema::ChatKind::Group)))
        })
        .collect();
    // Held while writing: app-core doesn't open (and migrate) the same store meanwhile.
    let lock = super::chat::core();
    let held = lock.lock().await;
    let groups: Vec<([u8; 16], bool)> = real_chats
        .iter()
        .filter_map(|c| {
            let id: [u8; 16] = c.chat_id.as_slice().try_into().ok()?;
            Some((id, c.kind == i32::from(schema::ChatKind::Group)))
        })
        .collect();
    background(async move {
        tokio::task::spawn_blocking(move || {
            let mut st = songbird_app_core::store::Store::open(&path, &key)?;
            import_into(&mut st, &groups, &real_messages, &media)?;
            Ok::<_, songbird_app_core::Error>(())
        })
        .await
    })
    .await
    .ok()
    .and_then(Result::ok)
    .ok_or_else(|| problem("The restore stopped. Try again."))?
    .map_err(|_| problem("Your messages couldn't be written on this device."))?;
    drop(held);
    super::chat::reload_all(s).await;
    // The restored groups: this device joins them by external commit (D-0167).
    for (id, group) in &groups_restored {
        if *group {
            super::group_sync::mark(s, id, true);
        }
    }
    super::group_sync::join_missing(s);
    // The list state the backup kept, on chats this device shows.
    for c in &real_chats {
        let Ok(id) = <[u8; 16]>::try_from(c.chat_id.as_slice()) else {
            continue;
        };
        if let Some(ui) = super::chat::ui_chat(s, &id) {
            s.edit_chat(ui, |x| {
                x.archived = c.archived;
                x.pinned = c.pinned;
                x.muted = c.muted_until_ms > 0;
                x.muted_until = c.muted_until_ms;
            });
        }
    }
    Ok(())
}

fn options() -> Options {
    Options {
        concurrency: 4,
        app_version: env!("CARGO_PKG_VERSION").into(),
        // From the build target: `os()` is the engine thread's, and this also runs on tokio.
        platform: if cfg!(target_os = "ios") {
            Platform::Ios
        } else if cfg!(target_os = "android") {
            Platform::Android
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        },
        now_ms: nsh_core::time::unix_ms() as u64,
    }
}

/// Backs the snapshot up to `dest`; `done` counts records written. Returns what it cost.
pub async fn back_up(
    dest: BackupDestination,
    extra: BackupExtra,
    snap: Snapshot,
    done: Arc<AtomicU32>,
) -> Result<songbird_backup_format::writer::Stats, Problem> {
    let key = recovery_key();
    let keys = Arc::new(
        key.derive()
            .map_err(|_| problem("The recovery key couldn't be read."))?,
    );
    let store = store_for(dest, &extra, &keys, true)?;
    let previous = seen(&keys, dest);
    let opts = options();
    let k = Arc::clone(&keys);
    let result = background(async move {
        run::backup(store, k, previous, opts, move |w| write_all(w, snap, &done)).await
    })
    .await
    .map_err(|_| problem("The backup stopped. Try again."))?;
    match result {
        Ok(d) => {
            save_seen(&keys, dest, d.seen);
            Ok(d.stats)
        }
        Err(e) => Err(explain_store(&e)),
    }
}

/// Turns backups off: the backup at `dest` is deleted.
pub async fn delete(dest: BackupDestination, extra: BackupExtra) -> Result<(), Problem> {
    let keys = Arc::new(
        recovery_key()
            .derive()
            .map_err(|_| problem("The recovery key couldn't be read."))?,
    );
    let store = store_for(dest, &extra, &keys, true)?;
    forget_seen(&keys, dest);
    background(async move { store.destroy().await })
        .await
        .map_err(|_| problem("Couldn't delete the backup. Try again."))?
        .map_err(|e| explain_store(&RunError::Store(e)))
}

/// Finds and verifies the backup `key` opens at `dest` (on a new device, the Songbird server
/// or this device's backup folder), without changing anything.
pub async fn fetch(
    dest: BackupDestination,
    extra: BackupExtra,
    key: &RecoveryKey,
) -> Result<Restored, Problem> {
    let keys = Arc::new(
        key.derive()
            .map_err(|_| problem("The recovery key couldn't be read."))?,
    );
    let store = store_for(dest, &extra, &keys, false)?;
    let previous = seen(&keys, dest);
    let k = Arc::clone(&keys);
    let r = background(async move {
        let r = run::restore(store, Arc::clone(&k), previous, MemoryStaging::default(), 4).await?;
        let b = Backup::new(&k, &r.opened.manifest, &r.staging);
        let records = b
            .records()
            .map(|x| x.map(|r| r.body))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let media = media_of(&b)?;
        Ok::<_, RunError>((records, media, r.opened.seen()))
    })
    .await
    .map_err(|_| problem("The restore stopped. Try again."))?;
    let (records, media, s) = r.map_err(|e| explain_store(&e))?;
    save_seen(&keys, dest, s);
    let messages = records
        .iter()
        .filter(|r| matches!(r, Rec::Message(_)))
        .count();
    let chats = records.iter().filter(|r| matches!(r, Rec::Chat(_))).count();
    Ok(Restored {
        records,
        media: Arc::new(media),
        messages: u32::try_from(messages).unwrap_or(u32::MAX),
        chats: u32::try_from(chats).unwrap_or(u32::MAX),
    })
}

// ---------------------------------------------------------------- account recovery

/// On a device with no account: makes it the account's main phone again with the identity key
/// from the backup (PROTOCOL §4.11), with `handover` from the old phone or the PIN if the
/// account has one. Does nothing when the backup has no account or the server is off.
pub async fn recover(
    r: &Restored,
    handover: Option<([u8; 32], [u8; 16])>,
    pin: Option<String>,
) -> Result<bool, Problem> {
    use songbird_keys::xeddsa::Signer as XSigner;
    use songbird_proto::devices::{
        Factor, LINK_AUTH_CONTEXT, LINK_IDENTITY_CONTEXT, LinkRequest, RECOVER_CONTEXT,
        RecoverRequest, link_transcript,
    };
    let Some(a) = r.account() else {
        return Ok(false);
    };
    if !super::is_live() || registered() || a.identity_seed.len() != 32 {
        return Ok(false);
    }
    let account: [u8; 16] = a
        .account_id
        .as_slice()
        .try_into()
        .map_err(|_| problem("The backup's account is damaged."))?;
    let seed: [u8; 32] = a
        .identity_seed
        .as_slice()
        .try_into()
        .map_err(|_| problem("The backup's account is damaged."))?;
    let ep = endpoint();
    let (token, device) = match handover {
        Some(h) => h,
        None => {
            let body = serde_json::json!({"account": wire::hex(&account)})
                .to_string()
                .into_bytes();
            let req = nsh_core::platform::HttpRequest::post(
                format!("{}/v1/account/recover/token", ep.api),
                body.clone(),
            )
            .header("content-type", "application/json")
            .header("content-length", body.len().to_string());
            let (status, answer) = super::send(req).await?;
            let v = ok_json(status, &answer)?;
            let token = v["token"]
                .as_str()
                .and_then(wire::unhex)
                .and_then(|b| b.try_into().ok());
            let device = v["device"]
                .as_str()
                .and_then(wire::unhex)
                .and_then(|b| b.try_into().ok());
            (
                token.ok_or_else(|| problem("The server's answer was incomplete."))?,
                device.ok_or_else(|| problem("The server's answer was incomplete."))?,
            )
        }
    };
    let auth_seed: [u8; 32] = wire::random();
    let ident_seed: [u8; 32] = wire::random();
    let auth =
        wire::ed25519(&auth_seed).ok_or_else(|| problem("This device couldn't make a key."))?;
    let account_key =
        wire::ed25519(&seed).ok_or_else(|| problem("The backup's account key is damaged."))?;
    let ident = XSigner::new(&ident_seed);
    let mut identity_key = [0u8; 33];
    identity_key[0] = 0x05;
    identity_key[1..].copy_from_slice(&ident.public());
    let mut link = LinkRequest {
        token,
        device: songbird_types::DeviceId::from_bytes(device),
        auth_key: wire::public(&auth),
        identity_key,
        auth_sig: [0; 64],
        identity_sig: [0; 64],
        confirm: [0; 32],
        factor: pin.map_or(Factor::None, Factor::Pin),
    };
    let tr = |c: &[u8], l: &LinkRequest| {
        link_transcript(
            c,
            &ep.api_name,
            &l.token,
            &l.device,
            &l.auth_key,
            &l.identity_key,
        )
        .map_err(|_| problem("The server's name is too long."))
    };
    let sig = auth.sign(&tr(LINK_AUTH_CONTEXT, &link)?);
    link.auth_sig.copy_from_slice(sig.as_ref());
    link.identity_sig = ident.sign(&tr(LINK_IDENTITY_CONTEXT, &link)?, &wire::random());
    let mut account_sig = [0u8; 64];
    account_sig.copy_from_slice(account_key.sign(&tr(RECOVER_CONTEXT, &link)?).as_ref());
    let body = RecoverRequest {
        account: songbird_types::AccountId::from_bytes(account),
        account_sig,
        link,
    }
    .encode();
    let req = nsh_core::platform::HttpRequest::post(
        format!("{}/v1/account/recover", ep.api),
        body.clone(),
    )
    .header("content-type", "application/octet-stream")
    .header("content-length", body.len().to_string());
    let (status, answer) = super::send(req).await?;
    if status == 403 && answer.windows(6).any(|w| w == b"factor") {
        return Err(problem("This account has a PIN. Enter it to restore."));
    }
    if !(200..300).contains(&status) {
        return Err(explain(status, &answer));
    }
    put(super::K_IDENTITY, &wire::hex(&seed));
    put(super::K_DEVICE_SEED, &wire::hex(&auth_seed));
    put(super::K_ACCOUNT, &wire::hex(&account));
    put(super::K_DEVICE, &wire::hex(&device));
    if !a.username.is_empty() {
        put(super::K_USERNAME, &a.username);
    }
    if let Ok(k) = <[u8; 32]>::try_from(a.storage_key.as_slice()) {
        records::adopt_key(k);
    }
    live().update(|l| {
        l.account = Some(wire::hex(&account));
        l.device = Some(wire::hex(&device));
        l.username = (!a.username.is_empty()).then(|| a.username.clone());
    });
    // The new main phone signs a list naming only itself.
    super::link::sign_self_list().await?;
    super::connect();
    Ok(true)
}

// ---------------------------------------------------------------- transfer

/// This device's addresses on the local network (the default route's, and loopback for two
/// apps on one computer).
fn local_addrs() -> Vec<std::net::IpAddr> {
    let mut out = vec![];
    if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0")
        && s.connect("192.0.2.1:9").is_ok()
        && let Ok(a) = s.local_addr()
        && !a.ip().is_unspecified()
    {
        out.push(a.ip());
    }
    out.push(std::net::IpAddr::from([127, 0, 0, 1]));
    out
}

/// The new phone: listens, and gives the code to show.
pub async fn transfer_listen() -> Result<(tokio::net::TcpListener, Code), Problem> {
    background(async move { transfer::listen(local_addrs()).await })
        .await
        .map_err(|_| problem("Couldn't open the connection."))?
        .map_err(|_| problem("This phone couldn't listen on the local network."))
}

/// What the new phone verified: the records and the old phone's `HELLO`.
type Verified = (
    Vec<Rec>,
    std::collections::HashMap<[u8; 32], Zeroizing<Vec<u8>>>,
    Hello,
);

/// The new phone: waits for the old one; `codes` gets the 6-digit code to compare; then
/// receives and verifies the backup, and returns it and the hand-over only once `confirmed`
/// says the user saw the same digits on this phone too: nothing is answered or applied before
/// (D-0125).
pub async fn transfer_receive(
    l: tokio::net::TcpListener,
    code: Code,
    codes: tokio::sync::mpsc::Sender<u32>,
    confirmed: tokio::sync::oneshot::Receiver<bool>,
    into_dir: PathBuf,
) -> Result<(Restored, Hello), Problem> {
    let r = background(async move {
        let ch = transfer::accept(&l, &code).await.map_err(|_| "connect")?;
        let _ = codes.send(ch.code()).await;
        let into: Arc<dyn BackupStore> = Arc::new(LocalStore::new(&into_dir));
        let verified: Arc<std::sync::Mutex<Option<Verified>>> = Arc::default();
        let v2 = Arc::clone(&verified);
        let store = Arc::clone(&into);
        let hello = transfer::receive(ch, into, move |h: Hello| async move {
            let Ok(keys) = RecoveryKey::from_secret(*h.secret).derive() else {
                return false;
            };
            let keys = Arc::new(keys);
            let Ok(r) =
                run::restore(store, Arc::clone(&keys), None, MemoryStaging::default(), 4).await
            else {
                return false;
            };
            let read = {
                let b = Backup::new(&keys, &r.opened.manifest, &r.staging);
                b.records()
                    .map(|x| x.map(|r| r.body))
                    .collect::<Result<Vec<_>, _>>()
                    .and_then(|records| Ok((records, media_of(&b)?)))
            };
            let Ok((records, media)) = read else {
                return false;
            };
            drop(r);
            // The user compares the digits on this phone too before anything is answered.
            if confirmed.await != Ok(true) {
                return false;
            }
            *v2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some((records.into_iter().flatten().collect(), media, h));
            true
        })
        .await
        .map_err(|_| "transfer")?;
        let _ = std::fs::remove_dir_all(&into_dir);
        let got = verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        got.map(|(records, media, _)| (records, media, hello))
            .ok_or("verify")
    })
    .await
    .map_err(|_| problem("The transfer stopped."))?
    .map_err(|_| problem("The transfer didn't finish, so nothing was changed on this phone."))?;
    let (records, media, hello) = r;
    let messages = records
        .iter()
        .filter(|r| matches!(r, Rec::Message(_)))
        .count();
    let chats = records.iter().filter(|r| matches!(r, Rec::Chat(_))).count();
    Ok((
        Restored {
            records,
            media: Arc::new(media),
            messages: u32::try_from(messages).unwrap_or(u32::MAX),
            chats: u32::try_from(chats).unwrap_or(u32::MAX),
        },
        hello,
    ))
}

/// The old phone: connected to the new one; the code to compare.
pub struct Sending {
    ch: Option<transfer::Channel<tokio::net::TcpStream>>,
    /// The 6-digit code both phones show.
    pub code: u32,
}

impl std::fmt::Debug for Sending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sending { .. }")
    }
}

/// The old phone: connects to the code it scanned.
pub async fn transfer_connect(text: String) -> Result<Sending, Problem> {
    let code =
        Code::parse(text.trim()).map_err(|_| problem("That isn't a Songbird transfer code."))?;
    let ch = background(async move { transfer::connect(&code).await })
        .await
        .map_err(|_| problem("Couldn't connect."))?
        .map_err(|_| {
            problem("Couldn't reach the new phone. Check both are on the same Wi\u{2011}Fi.")
        })?;
    Ok(Sending {
        code: ch.code(),
        ch: Some(ch),
    })
}

/// A folder for what a transfer receives (removed once verified), in the app's own folder.
pub fn transfer_folder() -> Option<PathBuf> {
    app_dir().map(|d| d.join(format!("transfer-{}", wire::hex(&wire::random::<8>()))))
}

/// A fresh folder for a test's files, under the crate's own `target` (never the shared
/// temporary directory).
#[cfg(test)]
fn test_folder(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-files")
        .join(format!("{name}-{}", wire::hex(&wire::random::<6>())))
}

/// A hand-over for the new phone, when this is the account's main phone on a server:
/// `(account, token, device)`.
pub async fn handover() -> Option<([u8; 16], [u8; 32], [u8; 16])> {
    if !(registered() && super::is_live()) {
        return None;
    }
    let account = live()
        .get_untracked()
        .account
        .as_deref()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())?;
    let (status, body) = super::signed("POST", "/v1/account/handover", None, false)
        .await
        .ok()?;
    let v = ok_json(status, &body).ok()?;
    let token = v["token"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())?;
    let device = v["device"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())?;
    Some((account, token, device))
}

/// The old phone, after the user compared the codes: sends everything, with the hand-over
/// from [`handover`] if there is one.
pub async fn transfer_send(
    mut sending: Sending,
    snap: Snapshot,
    done: Arc<AtomicU32>,
    handing: Option<([u8; 16], [u8; 32], [u8; 16])>,
) -> Result<(), Problem> {
    let ch = sending
        .ch
        .take()
        .ok_or_else(|| problem("The connection was already used."))?;
    let mut hello = Hello {
        account: [0; 16],
        handover: Zeroizing::new([0; 32]),
        device: [0; 16],
        secret: Zeroizing::new(wire::random()),
    };
    if let Some((account, token, device)) = handing {
        hello.account = account;
        *hello.handover = token;
        hello.device = device;
    }
    let keys = Arc::new(
        RecoveryKey::from_secret(*hello.secret)
            .derive()
            .map_err(|_| problem("This phone couldn't make a key."))?,
    );
    let opts = options();
    background(async move {
        let pipe = Arc::new(PipeStore::new(ch, &hello).await.map_err(|_| "hello")?);
        run::backup(pipe.clone(), keys, None, opts, move |w| {
            write_all(w, snap, &done)
        })
        .await
        .map_err(|_| "backup")?;
        pipe.finish().await.map_err(|_| "finish")
    })
    .await
    .map_err(|_| problem("The transfer stopped."))?
    .map_err(|_| problem("The transfer didn't finish. Nothing changed on your new phone."))
}

/// A hand-over the new phone received, if the old one sent one.
pub fn handover_of(h: &Hello) -> Option<([u8; 32], [u8; 16])> {
    (*h.handover != [0; 32]).then(|| (*h.handover, h.device))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::store::ext_onboarding::RestoreStage;

    /// What the first device showed: its key, for the second to type.
    static KEY: Mutex<String> = Mutex::new(String::new());
    static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ROLE: Mutex<&str> = Mutex::new("backup");

    fn dir() -> PathBuf {
        DIR.lock().unwrap().clone().unwrap()
    }

    /// D-0165: a real account's chats, messages and files go into a backup from app-core's
    /// store and come back into another store; disappearing and view-once messages don't, and
    /// without "Include media" the files stay out.
    #[test]
    fn app_core_chats_and_media_round_trip_through_a_backup() {
        use prost::Message as _;
        use songbird_app_core::store::{Status, Store as CoreStore};
        use songbird_content::schema::{
            AttachmentPointer, Content, DataMessage, Message as Msg, Text, content, data_message,
        };
        let dir = test_folder("sb-core-backup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.db");
        let key = [3u8; 32];
        let (chat, me) = ([7u8; 16], [8u8; 16]);
        let msg = |id: u8, text: &str, timer: u32, photo: bool| {
            Content {
                required_version: 1,
                message_id: vec![id; 16],
                sent_at_ms: u64::from(id) * 1000,
                body: Some(content::Body::Data(DataMessage {
                    expire_timer_s: timer,
                    kind: Some(data_message::Kind::Message(Msg {
                        text: Some(Text {
                            body: text.into(),
                            ..Default::default()
                        }),
                        attachments: if photo {
                            vec![AttachmentPointer {
                                content_type: "image/png".into(),
                                file_name: "p.png".into(),
                                width: 4,
                                height: 3,
                                ..Default::default()
                            }]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            }
            .encode_to_vec()
        };
        {
            let mut a = CoreStore::open(&path, &key).unwrap();
            a.restore_chat(&chat, false).unwrap();
            a.restore_message(&chat, &me, &msg(1, "hello", 0, false), 1, Status::Read)
                .unwrap();
            a.restore_message(&chat, &me, &msg(2, "look", 0, true), 2, Status::Read)
                .unwrap();
            a.put_file(&me, &[2; 16], 0, &[9u8; 3000]).unwrap();
            // A disappearing message, stored as it arrives.
            let c = msg(3, "gone soon", 60, false);
            let row = songbird_app_core::store::MessageRow {
                chat,
                author: me,
                id: [3; 16],
                sent_ms: 3000,
                stored_ms: 3,
                text: "gone soon".into(),
                content: c,
                status: Status::Read,
            };
            a.insert_message(&row, false).unwrap();
        }
        let run_backup = |media: bool, folder: &std::path::Path| {
            let keys = Arc::new(RecoveryKey::generate(&mut OsRng).unwrap().derive().unwrap());
            let snap = Snapshot {
                core: Some((path.clone(), Zeroizing::new(key))),
                media,
                ..Snapshot::default()
            };
            let store: Arc<dyn BackupStore> = Arc::new(LocalStore::new(folder));
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let done = Arc::new(AtomicU32::new(0));
                run::backup(
                    Arc::clone(&store),
                    Arc::clone(&keys),
                    None,
                    options(),
                    move |w| write_all(w, snap, &done),
                )
                .await
                .unwrap();
                let r = run::restore(store, Arc::clone(&keys), None, MemoryStaging::default(), 2)
                    .await
                    .unwrap();
                let b = Backup::new(&keys, &r.opened.manifest, &r.staging);
                let records: Vec<Rec> = b.records().map(|x| x.unwrap().body.unwrap()).collect();
                (records, media_of(&b).unwrap())
            })
        };
        let (records, media) = run_backup(true, &dir.join("with"));
        let messages: Vec<schema::Message> = records
            .iter()
            .filter_map(|r| match r {
                Rec::Message(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(messages.len(), 2, "the disappearing message stays out");
        assert_eq!(messages[1].media.len(), 1);
        assert_eq!(media.len(), 1);
        let (_, without) = run_backup(false, &dir.join("without"));
        assert!(without.is_empty(), "no media without Include media");

        let mut b = CoreStore::open(&dir.join("b.db"), &[4u8; 32]).unwrap();
        import_into(&mut b, &[(chat, false)], &messages, &media).unwrap();
        let back = b.messages(&chat, None, 10).unwrap();
        assert_eq!(
            back.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["hello", "look"]
        );
        assert_eq!(
            b.file(&me, &[2; 16], 0).unwrap().unwrap().as_slice(),
            &[9u8; 3000][..]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Device A backs up to a folder; device B (another storage) wipes its chats and restores
    /// with the key A showed, or with a typo, or from a damaged folder.
    #[component]
    fn BackupProbe() -> impl IntoElement {
        TEST_DIR.with(|t| *t.borrow_mut() = Some(dir()));
        let s = Store::new();
        crate::store::install(s);
        let role = *ROLE.lock().unwrap();
        set_timeout(std::time::Duration::from_millis(10), move || match role {
            "backup" => {
                *KEY.lock().unwrap() = s.session.get_untracked().recovery_key;
                s.enable_backups(BackupDestination::EncryptedFile, false, Default::default());
            }
            _ => {
                // A brand-new device: nothing of A's, its chats wiped.
                s.messages.update(|m| m.clear());
                s.chats.update(|c| c.truncate(1));
                let mut key = KEY.lock().unwrap().clone();
                if role == "typo" {
                    let c: Vec<char> = key.chars().collect();
                    let i = 7;
                    key = c
                        .iter()
                        .enumerate()
                        .map(|(j, ch)| {
                            if j == i {
                                if *ch == 'A' { 'B' } else { 'A' }
                            } else {
                                *ch
                            }
                        })
                        .collect();
                }
                s.restore_backup_with(Some(key), false);
            }
        });
        let extra = s.backup_extra();
        let stage = s.restore_stage();
        text(move || {
            let b = extra.get();
            let n: usize = s.messages.with(|m| m.values().map(Vec::len).sum());
            format!(
                "last {} running {} messages {} stage {:?} note {} store {}",
                s.settings.with(|x| x.backup_last.is_some()),
                b.running.is_some(),
                b.messages,
                stage.get(),
                b.note.unwrap_or_default(),
                n,
            )
        })
    }

    /// Runs the app until a text on screen contains every part of `want`.
    fn wait_until(h: &mut nsh_testing::Headless, want: &[&str]) -> Result<String, String> {
        let owned: Vec<String> = want.iter().map(|w| (*w).to_owned()).collect();
        h.wait_until("the status", std::time::Duration::from_secs(60), move |s| {
            s.texts()
                .iter()
                .any(|t| owned.iter().all(|w| t.contains(w.as_str())))
        })?;
        Ok(h.screen()
            .texts()
            .into_iter()
            .find(|t| want.iter().all(|w| t.contains(w)))
            .unwrap_or_default()
            .to_owned())
    }

    fn start(
        role: &'static str,
        storage: &std::sync::Arc<nsh_testing::MemoryStorage>,
    ) -> Result<nsh_testing::Headless, String> {
        *ROLE.lock().unwrap() = role;
        let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<BackupProbe>());
        config.storage = Some(storage.clone());
        nsh_testing::Headless::start(config).map_err(|e| e.to_string())
    }

    #[test]
    fn a_new_device_restores_with_only_the_key_and_damage_changes_nothing() -> Result<(), String> {
        let tmp = test_folder("sb-backup-test");
        *DIR.lock().unwrap() = Some(tmp.clone());
        // Device A: a backup to the folder.
        let a = nsh_testing::MemoryStorage::new([]);
        let mut first = start("backup", &a)?;
        let sent = wait_until(&mut first, &["last true running false"])?;
        drop(first);
        let want = sent.split("store ").nth(1).ok_or("count")?.to_owned();
        assert!(
            want.parse::<usize>().map_err(|e| e.to_string())? > 10,
            "{sent}"
        );
        // Device B, with only the key: everything comes back.
        let b = nsh_testing::MemoryStorage::new([]);
        let mut second = start("restore", &b)?;
        wait_until(
            &mut second,
            &["stage Done note  store ", &format!("store {want}")],
        )?;
        drop(second);
        // A typo: refused before anything is read; nothing changes.
        let c = nsh_testing::MemoryStorage::new([]);
        let mut third = start("typo", &c)?;
        wait_until(
            &mut third,
            &["stage Failed note That key doesn't check out", "store 0"],
        )?;
        drop(third);
        // A damaged chunk: the restore fails loudly and restores nothing.
        for e in std::fs::read_dir(&tmp).map_err(|e| e.to_string())? {
            let chunks = e.map_err(|e| e.to_string())?.path().join("chunks");
            for f in std::fs::read_dir(&chunks)
                .map_err(|e| e.to_string())?
                .take(1)
            {
                let p = f.map_err(|e| e.to_string())?.path();
                let mut bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
                let at = bytes.len() / 2;
                bytes[at] ^= 1;
                std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
            }
        }
        let d = nsh_testing::MemoryStorage::new([]);
        let mut fourth = start("restore", &d)?;
        wait_until(
            &mut fourth,
            &["stage Failed note This backup looks damaged", "store 0"],
        )?;
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    /// Two phones over loopback through the app's own transfer calls: the old phone's
    /// snapshot arrives whole and verified at the new one, and both see the same code.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_transfer_moves_a_snapshot_between_two_app_instances() {
        let mut records = vec![Rec::Profile(schema::Profile {
            name: "Ada".into(),
            ..Default::default()
        })];
        for i in 0..500u64 {
            records.push(Rec::Message(schema::Message {
                chat_id: id16(1),
                message_id: id16(i),
                sent_ms: i * 60_000,
                content: format!("message {i}").into_bytes(),
                ..Default::default()
            }));
        }
        let snap = Snapshot {
            records,
            messages: 500,
            chats: 1,
            ..Snapshot::default()
        };
        let (l, code) = transfer_listen().await.unwrap();
        let qr = code.to_qr().to_string();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let into = test_folder("sb-transfer");
        let (yes, confirmed) = tokio::sync::oneshot::channel();
        let new_phone =
            tokio::spawn(async move { transfer_receive(l, code, tx, confirmed, into).await });
        let sending = transfer_connect(qr).await.unwrap();
        let shown_old = sending.code;
        let shown_new = rx.recv().await.unwrap();
        assert_eq!(shown_old, shown_new);
        // The new phone's user confirms the digits too.
        yes.send(true).unwrap();
        transfer_send(sending, snap, Arc::new(AtomicU32::new(0)), None)
            .await
            .unwrap();
        let (restored, hello) = new_phone.await.unwrap().unwrap();
        assert_eq!((restored.messages, restored.chats), (500, 0));
        assert!(handover_of(&hello).is_none(), "no server: no hand-over");
        // A code that isn't a transfer code is refused before any connection.
        assert!(transfer_connect("songbird-link:abc".into()).await.is_err());
        let _ = RestoreStage::Idle;
    }
}
