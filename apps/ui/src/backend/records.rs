//! Settings that follow the user across devices, and settings that stay on
//! this one (Prompt 9, M5; PROTOCOL §10).
//!
//! **Synced** settings are encrypted records in the account's record store:
//! each setting is a logical key (`s/theme`) whose value is a short string,
//! merged by last writer wins on a hybrid logical clock (`songbird-sync`).
//! The replica, with its storage key, is saved in the app's storage after
//! every change, so a setting survives a restart before any server sees it;
//! once the account is live it syncs with `/v1/storage` (encrypted; the server
//! sees padded ciphertext only).
//!
//! **Device-only** settings (app lock, screenshots, enter sends, proxy, crash
//! reports, launch at login, ...) are saved in the app's storage and never
//! leave the device, as WhatsApp and Signal keep them.
//!
//! **Left out**, owned by later prompts: backups (Prompt 10), the plan
//! (Prompt 11), WhatsApp (Prompt 14). Server-enforced settings
//! (discoverable, groups, requests, inactivity) are the server's; this
//! device keeps a copy to show (see `super::account`).

use std::cell::RefCell;

use super::vault::Storage;
use nsh_core::{prelude::*, time::unix_ms};
use songbird_proto::storage::{self as wire, Head, ID_LEN, Ids, Record, Write};
use songbird_sync::{Remote, Replica, StorageKey, SyncError, WriteOutcome};

use crate::store::{
    Audience, Disappear, LockAfter, MESSAGE_TONES, RINGTONES, Settings, SettingsExt, Store,
    ThemeChoice,
};

const K_STATE: &str = "sb.records";
const K_DEVICE: &str = "sb.device_settings";

// ---------------------------------------------------------------- codec

fn b(v: bool) -> String {
    if v { "1" } else { "0" }.into()
}

fn pb(v: &str) -> Option<bool> {
    match v {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn audience(a: Audience) -> String {
    match a {
        Audience::Everyone => "everyone",
        Audience::Contacts => "contacts",
        Audience::Nobody => "nobody",
    }
    .into()
}

fn p_audience(v: &str) -> Option<Audience> {
    match v {
        "everyone" => Some(Audience::Everyone),
        "contacts" => Some(Audience::Contacts),
        "nobody" => Some(Audience::Nobody),
        _ => None,
    }
}

fn theme(t: ThemeChoice) -> String {
    match t {
        ThemeChoice::System => "system",
        ThemeChoice::Light => "light",
        ThemeChoice::Dark => "dark",
        ThemeChoice::HighContrast => "contrast",
    }
    .into()
}

fn p_theme(v: &str) -> Option<ThemeChoice> {
    match v {
        "system" => Some(ThemeChoice::System),
        "light" => Some(ThemeChoice::Light),
        "dark" => Some(ThemeChoice::Dark),
        "contrast" => Some(ThemeChoice::HighContrast),
        _ => None,
    }
}

fn timer(t: Option<Disappear>) -> String {
    match t {
        None => "off",
        Some(Disappear::Day) => "day",
        Some(Disappear::Week) => "week",
        Some(Disappear::FourWeeks) => "four_weeks",
    }
    .into()
}

fn p_timer(v: &str) -> Option<Option<Disappear>> {
    match v {
        "off" => Some(None),
        "day" => Some(Some(Disappear::Day)),
        "week" => Some(Some(Disappear::Week)),
        // "quarter" (90 days) was offered before the protocol's 4-week limit was honoured.
        "four_weeks" | "quarter" => Some(Some(Disappear::FourWeeks)),
        _ => None,
    }
}

fn pair(p: (bool, bool)) -> String {
    format!("{}{}", b(p.0), b(p.1))
}

fn p_pair(v: &str) -> Option<(bool, bool)> {
    let mut c = v.chars();
    let (x, y) = (c.next()?, c.next()?);
    c.next().is_none().then_some(())?;
    Some((pb(&x.to_string())?, pb(&y.to_string())?))
}

fn list(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn p_list(v: &str) -> Option<Vec<String>> {
    serde_json::from_str(v).ok()
}

fn sound(name: &str) -> String {
    name.into()
}

fn p_sound(all: &[&'static str], v: &str) -> Option<&'static str> {
    all.iter().find(|s| **s == v).copied()
}

fn lock(l: LockAfter) -> String {
    match l {
        LockAfter::Immediately => "now",
        LockAfter::Minute => "minute",
        LockAfter::Hour => "hour",
    }
    .into()
}

fn p_lock(v: &str) -> Option<LockAfter> {
    match v {
        "now" => Some(LockAfter::Immediately),
        "minute" => Some(LockAfter::Minute),
        "hour" => Some(LockAfter::Hour),
        _ => None,
    }
}

type Get = fn(&Settings, &SettingsExt) -> String;
type Set = fn(&mut Settings, &mut SettingsExt, &str) -> Option<()>;

macro_rules! flag {
    ($key:literal, s.$f:ident) => {
        (
            $key,
            |s: &Settings, _: &SettingsExt| b(s.$f),
            |s: &mut Settings, _: &mut SettingsExt, v: &str| {
                s.$f = pb(v)?;
                Some(())
            },
        )
    };
    ($key:literal, x.$f:ident) => {
        (
            $key,
            |_: &Settings, x: &SettingsExt| b(x.$f),
            |_: &mut Settings, x: &mut SettingsExt, v: &str| {
                x.$f = pb(v)?;
                Some(())
            },
        )
    };
}

macro_rules! field {
    ($key:literal, s.$f:ident, $enc:expr, $dec:expr) => {
        (
            $key,
            |s: &Settings, _: &SettingsExt| $enc(s.$f.clone()),
            |s: &mut Settings, _: &mut SettingsExt, v: &str| {
                s.$f = $dec(v)?;
                Some(())
            },
        )
    };
    ($key:literal, x.$f:ident, $enc:expr, $dec:expr) => {
        (
            $key,
            |_: &Settings, x: &SettingsExt| $enc(x.$f.clone()),
            |_: &mut Settings, x: &mut SettingsExt, v: &str| {
                x.$f = $dec(v)?;
                Some(())
            },
        )
    };
}

/// Every synced setting: its logical key (PROTOCOL §10.2), how it is
/// written and read.
pub const SYNCED: &[(&str, Get, Set)] = &[
    field!("s/last_seen", s.last_seen, audience, p_audience),
    field!("s/online", s.online, audience, p_audience),
    flag!("s/online_same", x.online_same_as_last_seen),
    field!("s/photo", s.photo, audience, p_audience),
    field!("s/about", s.about, audience, p_audience),
    field!("s/status", s.status, audience, p_audience),
    field!("s/groups_add", s.groups_add, audience, p_audience),
    flag!("s/read_receipts", s.read_receipts),
    flag!("s/typing", s.typing_indicators),
    field!("s/default_timer", s.default_timer, timer, p_timer),
    flag!("s/link_previews", s.link_previews),
    flag!("s/protect_ip", x.protect_ip_in_calls),
    flag!("s/security_notices", x.security_notifications),
    flag!("s/notify_messages", s.notify_messages),
    flag!("s/notify_groups", s.notify_groups),
    flag!("s/notify_calls", s.notify_calls),
    flag!("s/reactions_messages", s.reactions_messages),
    flag!("s/reactions_groups", s.reactions_groups),
    flag!("s/notify_sound", s.notify_sound),
    flag!("s/hide_content", s.hide_notification_content),
    field!("s/ringtone", x.ringtone, |v: &str| sound(v), |v| p_sound(
        &RINGTONES, v
    )),
    field!("s/message_tone", x.message_tone, |v: &str| sound(v), |v| {
        p_sound(&MESSAGE_TONES, v)
    }),
    field!("s/theme", s.theme, theme, p_theme),
    field!(
        "s/app_icon",
        x.app_icon,
        |v: u8| v.to_string(),
        |v: &str| v.parse().ok()
    ),
    field!(
        "s/wallpaper",
        s.chat_wallpaper,
        |v: u8| v.to_string(),
        |v: &str| v.parse().ok()
    ),
    flag!("s/animations", s.animations),
    field!(
        "s/default_reactions",
        s.default_reactions,
        |v: Vec<String>| list(&v),
        p_list
    ),
    flag!("s/keep_archived", s.keep_archived),
    flag!("s/nudges", s.nudges),
    flag!("s/sound_stickers", s.sound_stickers_autoplay),
    field!("s/auto_photos", s.auto_photos, pair, p_pair),
    field!("s/auto_audio", s.auto_audio, pair, p_pair),
    field!("s/auto_video", s.auto_video, pair, p_pair),
    field!("s/auto_docs", s.auto_docs, pair, p_pair),
    flag!("s/hd_uploads", s.hd_uploads),
    field!("s/lists", x.lists, |v: Vec<String>| list(&v), p_list),
];

/// Every device-only setting.
pub const DEVICE: &[(&str, Get, Set)] = &[
    flag!("app_lock", s.app_lock),
    field!("app_lock_after", x.app_lock_after, lock, p_lock),
    flag!("app_lock_biometrics", x.app_lock_biometrics),
    flag!("block_screenshots", s.block_screenshots),
    flag!("hide_in_switcher", s.hide_in_switcher),
    flag!("enter_sends", s.enter_sends),
    flag!("save_to_photos", s.save_to_photos),
    flag!("low_data_calls", s.low_data_calls),
    field!(
        "proxy",
        s.proxy,
        |v: Option<String>| v.unwrap_or_default(),
        |v: &str| Some((!v.is_empty()).then(|| v.to_string()))
    ),
    flag!("crash_reports", s.crash_reports),
    flag!("launch_at_login", s.launch_at_login),
    // The server's settings, kept to show while offline (`super::account`).
    flag!("discoverable", s.discoverable),
    flag!("message_requests", s.message_requests),
    field!(
        "inactivity",
        s.inactivity_months,
        |v: u8| v.to_string(),
        |v: &str| v.parse().ok()
    ),
];

/// The synced settings as `(key, value)`.
#[must_use]
pub fn encode(s: &Settings, x: &SettingsExt) -> Vec<(&'static str, String)> {
    SYNCED.iter().map(|(k, get, _)| (*k, get(s, x))).collect()
}

/// Applies records to settings; unknown keys and bad values are left alone
/// (a newer app's settings, or a damaged record).
pub fn apply<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    s: &mut Settings,
    x: &mut SettingsExt,
) {
    for (k, v) in entries {
        if let Some((_, _, set)) = SYNCED.iter().chain(DEVICE).find(|(key, _, _)| *key == k) {
            let _ = set(s, x, v);
        }
    }
}

// ---------------------------------------------------------------- the engine

struct Engine {
    /// The replica; `None` while a sync holds it.
    replica: Option<Replica>,
    /// Edits made while a sync held the replica, applied when it returns
    /// (`None`: a deletion).
    queued: Vec<(String, Option<String>)>,
    /// What was last written to the replica, to find changes.
    last: Vec<(&'static str, String)>,
    syncing: bool,
    again: bool,
    /// A key from the main phone that arrived while a sync held the
    /// replica: a fresh replica under it replaces the old one on return.
    next_key: Option<StorageKey>,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

fn now_ms() -> u64 {
    unix_ms().max(0.0) as u64
}

fn save(r: &Replica) {
    if let Err(e) = Storage::set_item(K_STATE, &super::wire::hex(&r.save())) {
        log::warn!("settings storage: {e}");
    }
}

fn save_device(s: &Settings, x: &SettingsExt) {
    let map: serde_json::Map<String, serde_json::Value> = DEVICE
        .iter()
        .map(|(k, get, _)| ((*k).to_string(), serde_json::Value::String(get(s, x))))
        .collect();
    if let Err(e) = Storage::set_item(K_DEVICE, &serde_json::Value::Object(map).to_string()) {
        log::warn!("settings storage: {e}");
    }
}

/// Loads the saved replica (or makes one, with a fresh storage key) and the
/// device settings, and applies both to the store. Called once at the root.
pub fn restore(store: Store) {
    let replica = Storage::get_item(K_STATE)
        .and_then(|h| super::wire::unhex(&h))
        .and_then(|b| Replica::load(&b).ok())
        .unwrap_or_else(|| {
            let key = StorageKey::from_bytes(super::wire::random());
            Replica::new(key, super::wire::random())
        });
    let device: Vec<(String, String)> = Storage::get_item(K_DEVICE)
        .and_then(|j| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&j).ok())
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| Some((k, v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let records: Vec<(String, String)> = replica
        .with_prefix("s/")
        .into_iter()
        .filter_map(|(k, v)| Some((k, String::from_utf8(v).ok()?)))
        .collect();
    let ext = store.ext();
    let mut s = store.settings.get_untracked();
    let mut x = ext.get_untracked();
    apply(
        records
            .iter()
            .chain(&device)
            .map(|(k, v)| (k.as_str(), v.as_str())),
        &mut s,
        &mut x,
    );
    let last = encode(&s, &x);
    // Settings that were never written (a first run) become records now.
    let mut replica = replica;
    let now = now_ms();
    for (k, v) in &last {
        if replica.get(k).is_none() {
            let _ = replica.set(k, v.clone().into_bytes(), now);
        }
    }
    save(&replica);
    store.settings.set(s);
    ext.set(x);
    ENGINE.with(|e| {
        *e.borrow_mut() = Some(Engine {
            replica: Some(replica),
            queued: Vec::new(),
            last,
            syncing: false,
            again: false,
            next_key: None,
        });
    });
    store.reload_records();
}

// ---------------------------------------------------------------- other records

/// Every live record under `prefix` (saved contacts `k/`, per-chat `c/`),
/// as text; a record that isn't UTF-8 is skipped.
#[must_use]
pub fn under(prefix: &str) -> Vec<(String, String)> {
    ENGINE.with(|e| {
        let e = e.borrow();
        let Some(engine) = e.as_ref() else {
            return Vec::new();
        };
        let mut out: Vec<(String, String)> = engine
            .replica
            .as_ref()
            .map(|r| {
                r.with_prefix(prefix)
                    .into_iter()
                    .filter_map(|(k, v)| Some((k, String::from_utf8(v).ok()?)))
                    .collect()
            })
            .unwrap_or_default();
        // Edits queued behind a sync in flight are the newest.
        for (k, v) in engine.queued.iter().filter(|(k, _)| k.starts_with(prefix)) {
            out.retain(|(key, _)| key != k);
            if let Some(v) = v {
                out.push((k.clone(), v.clone()));
            }
        }
        out.sort();
        out
    })
}

/// Sets (`Some`) or deletes (`None`) one record that is not a setting: a
/// saved contact or a per-chat preference. Saved at once; a live account
/// syncs soon after, as for settings.
pub fn put(store: Store, key: &str, value: Option<String>) {
    let edited = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        let Some(engine) = e.as_mut() else {
            return false;
        };
        match engine.replica.as_mut() {
            Some(r) => {
                let done = match &value {
                    Some(v) => r.set(key, v.clone().into_bytes(), now_ms()),
                    None => r.delete(key, now_ms()),
                };
                if done.is_err() {
                    return false;
                }
                save(r);
            }
            None => engine.queued.push((key.to_string(), value)),
        }
        true
    });
    if edited && super::is_live() && super::registered() {
        schedule(store);
    }
}

/// Records what changed after a settings intent: synced settings become
/// local edits of the replica (saved at once), device settings are saved;
/// a live account syncs soon after.
pub fn changed(store: Store) {
    let s = store.settings.get_untracked();
    let x = store.ext().get_untracked();
    save_device(&s, &x);
    let now_val = encode(&s, &x);
    let edited = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        let Some(engine) = e.as_mut() else {
            return false;
        };
        let mut edited = false;
        for (k, v) in &now_val {
            let before = engine.last.iter().find(|(key, _)| key == k).map(|(_, v)| v);
            if before != Some(v) {
                match engine.replica.as_mut() {
                    Some(r) => {
                        let _ = r.set(k, v.clone().into_bytes(), now_ms());
                    }
                    None => engine.queued.push(((*k).to_string(), Some(v.clone()))),
                }
                edited = true;
            }
        }
        if edited {
            engine.last = now_val;
            if let Some(r) = &engine.replica {
                save(r);
            }
        }
        edited
    });
    if edited && super::is_live() && super::registered() {
        schedule(store);
    }
}

/// While the account is live, syncs at start and every 30 seconds (another
/// device's changes arrive within that; a `storage` sync message will make it
/// immediate once app-core reads them).
pub fn periodic(store: Store) {
    fn tick(store: Store) {
        if super::is_live() && super::registered() {
            schedule(store);
        }
        set_timeout(std::time::Duration::from_secs(30), move || tick(store));
    }
    tick(store);
}

/// Syncs now (or right after the sync in flight).
pub fn schedule(store: Store) {
    let start = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        let Some(engine) = e.as_mut() else {
            return false;
        };
        if engine.syncing {
            engine.again = true;
            false
        } else {
            engine.syncing = true;
            true
        }
    });
    if start {
        spawn(async move {
            loop {
                let result = run_sync().await;
                match result {
                    Ok(changed) if !changed.is_empty() => pull(store, &changed),
                    Ok(_) => {}
                    Err(e) => super::live().update(|l| l.problem = Some(explain(e))),
                }
                let again = ENGINE.with(|e| {
                    let mut e = e.borrow_mut();
                    let Some(engine) = e.as_mut() else {
                        return false;
                    };
                    let again = std::mem::take(&mut engine.again);
                    engine.syncing = again;
                    again
                });
                if !again {
                    break;
                }
            }
        });
    }
}

fn explain(e: SyncError) -> String {
    match e {
        SyncError::Remote => "Settings couldn't sync: the server isn't reachable.".into(),
        SyncError::Full => "Settings couldn't sync: your settings storage is full.".into(),
        SyncError::KeyChanged => {
            "Settings sync is waiting for the new key from your main phone.".into()
        }
        SyncError::Rollback | SyncError::Tampered => {
            "Settings sync stopped: the server's copy doesn't match. Your settings on this device are unchanged.".into()
        }
        _ => "Settings couldn't sync. Try again later.".into(),
    }
}

/// One sync of the replica against the server; the keys that changed here.
/// The replica leaves the engine while the requests run; edits made
/// meanwhile are queued and applied on top when it returns (with newer
/// stamps, so they win, as they should).
async fn run_sync() -> Result<Vec<String>, SyncError> {
    let Some(mut replica) =
        ENGINE.with(|e| e.borrow_mut().as_mut().and_then(|en| en.replica.take()))
    else {
        return Ok(Vec::new());
    };
    let result = songbird_sync::sync(&mut replica, &Api, now_ms()).await;
    give_back(replica);
    result
}

/// Returns the replica to the engine with the edits queued meanwhile, or
/// replaces it with a fresh one when a new key arrived meanwhile.
fn give_back(mut replica: Replica) {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().as_mut() {
            if let Some(key) = engine.next_key.take() {
                replica = fresh(key);
            }
            for (k, v) in std::mem::take(&mut engine.queued) {
                let _ = match v {
                    Some(v) => replica.set(&k, v.into_bytes(), now_ms()),
                    None => replica.delete(&k, now_ms()),
                };
                engine.again = true;
            }
            save(&replica);
            engine.replica = Some(replica);
        }
    });
}

/// An empty replica under `key`. Empty, not this device's settings
/// re-keyed: a device that just linked has only defaults, stamped now, and
/// they would overwrite the account's real settings on the first sync. The
/// first sync reads the store from scratch and applies what it holds.
fn fresh(key: StorageKey) -> Replica {
    Replica::new(key, super::wire::random())
}

// ---------------------------------------------------------------- the storage key

/// The account's storage key, as the main phone sends it to a device it
/// links (provisioning v2, PROTOCOL §4.6). `None` before `restore`.
#[must_use]
pub fn storage_key() -> Option<[u8; 32]> {
    ENGINE
        .with(|e| {
            let e = e.borrow();
            let engine = e.as_ref()?;
            match &engine.next_key {
                Some(k) => Some(*k.as_bytes()),
                None => engine.replica.as_ref().map(|r| *r.key().as_bytes()),
            }
        })
        .or_else(|| {
            // A sync holds the replica: the saved copy has the same key.
            Storage::get_item(K_STATE)
                .and_then(|h| super::wire::unhex(&h))
                .and_then(|b| Replica::load(&b).ok())
                .map(|r| *r.key().as_bytes())
        })
}

/// On a device that just linked: the account's storage key from the
/// provisioning message. The device's own replica (defaults under a key
/// nobody else has) is dropped for an empty one under the account's key.
pub fn adopt_key(key: [u8; 32]) {
    let key = StorageKey::from_bytes(key);
    let replaced = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        let engine = e.as_mut()?;
        if engine.replica.is_some() {
            let r = fresh(key.clone());
            save(&r);
            engine.replica = Some(r);
        } else {
            engine.next_key = Some(key.clone());
        }
        Some(())
    });
    if replaced.is_none() {
        // Before `restore`: saved, and loaded by it.
        save(&fresh(key));
    }
}

/// On the main phone, when it is the account's only device again: a new
/// storage key, every record rewritten under it in one write, so a device
/// that was logged out can't read what comes next. Waits for a sync in
/// flight. (With other devices still linked the new key would have to
/// reach them end to end, which app-core's sync messages do (A2); until
/// then the key stays, D-0106.)
///
/// # Errors
/// [`SyncError`] from the store; the old key stays in use.
pub async fn rotate_key() -> Result<(), SyncError> {
    let mut replica = None;
    for _ in 0..600 {
        replica = ENGINE.with(|e| {
            let mut e = e.borrow_mut();
            let engine = e.as_mut()?;
            if engine.syncing {
                return None;
            }
            let r = engine.replica.take()?;
            engine.syncing = true;
            Some(r)
        });
        if replica.is_some() {
            break;
        }
        nsh_core::prelude::timer(std::time::Duration::from_millis(100)).await;
    }
    let Some(mut replica) = replica else {
        return Err(SyncError::Busy);
    };
    let key = StorageKey::from_bytes(super::wire::random());
    let result = songbird_sync::rotate(&mut replica, &Api, key, now_ms()).await;
    give_back(replica);
    let again = ENGINE.with(|e| {
        e.borrow_mut().as_mut().is_some_and(|engine| {
            engine.syncing = false;
            std::mem::take(&mut engine.again)
        })
    });
    if again {
        schedule(crate::store::store());
    }
    result
}

/// Applies what the sync brought to the store.
fn pull(store: Store, changed: &[String]) {
    let entries: Vec<(String, String)> = ENGINE.with(|e| {
        e.borrow()
            .as_ref()
            .map(|en| {
                changed
                    .iter()
                    .filter_map(|k| {
                        let v = en.replica.as_ref()?.get(k)?;
                        Some((k.clone(), String::from_utf8(v.to_vec()).ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    let ext = store.ext();
    let mut s = store.settings.get_untracked();
    let mut x = ext.get_untracked();
    apply(
        entries.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        &mut s,
        &mut x,
    );
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().as_mut() {
            engine.last = encode(&s, &x);
        }
    });
    store.settings.set(s);
    ext.set(x);
    // Saved contacts and per-chat records another device changed.
    if changed.iter().any(|k| !k.starts_with("s/")) {
        store.reload_records();
    }
}

/// The record store over HTTP, signed by this device.
struct Api;

async fn call(
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
) -> Result<(u16, Vec<u8>), SyncError> {
    super::signed(method, path, body, false)
        .await
        .map_err(|_| SyncError::Remote)
}

impl Remote for Api {
    async fn head(&self) -> Result<Head, SyncError> {
        match call("GET", "/v1/storage", None).await? {
            (200, b) => Head::decode(&b).map_err(|_| SyncError::Tampered),
            _ => Err(SyncError::Remote),
        }
    }

    async fn ids(&self) -> Result<Ids, SyncError> {
        match call("GET", "/v1/storage/ids", None).await? {
            (200, b) => Ids::decode(&b).map_err(|_| SyncError::Tampered),
            _ => Err(SyncError::Remote),
        }
    }

    async fn read(&self, ids: &[[u8; ID_LEN]]) -> Result<Vec<Record>, SyncError> {
        let body = wire::encode_read(ids).map_err(|_| SyncError::Malformed)?;
        match call("POST", "/v1/storage/read", Some(body)).await? {
            (200, b) => wire::decode_records(&b).map_err(|_| SyncError::Tampered),
            _ => Err(SyncError::Remote),
        }
    }

    async fn write(&self, w: &Write) -> Result<WriteOutcome, SyncError> {
        let (status, b) = call("PUT", "/v1/storage", Some(w.encode())).await?;
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap_or_default();
        match (status, v["error"].as_str()) {
            (200, _) => Ok(WriteOutcome::Applied(v["version"].as_u64().unwrap_or(0))),
            (409, Some("version")) => {
                Ok(WriteOutcome::Conflict(v["version"].as_u64().unwrap_or(0)))
            }
            (409, Some("full")) => Ok(WriteOutcome::Full),
            _ => Err(SyncError::Remote),
        }
    }
}

#[cfg(test)]
mod tests {
    use songbird_sync::memory::{MemoryServer, block_on};

    use super::*;

    fn defaults() -> (Settings, SettingsExt) {
        (
            crate::store::seed_for_tests::seed().settings,
            SettingsExt::default(),
        )
    }

    #[test]
    fn every_synced_and_device_setting_round_trips() {
        let (mut s, mut x) = defaults();
        s.theme = ThemeChoice::HighContrast;
        s.last_seen = Audience::Nobody;
        s.default_timer = Some(Disappear::Week);
        s.auto_video = (false, true);
        s.default_reactions = vec!["🔥".into()];
        s.typing_indicators = false;
        s.proxy = Some("songbird://proxy#p.example".into());
        s.app_lock = true;
        x.ringtone = RINGTONES[2];
        x.lists = vec!["Close friends".into()];
        x.app_lock_after = LockAfter::Hour;
        x.app_icon = 3;
        let records: Vec<(String, String)> = SYNCED
            .iter()
            .chain(DEVICE)
            .map(|(k, get, _)| ((*k).to_string(), get(&s, &x)))
            .collect();
        let (mut s2, mut x2) = defaults();
        apply(
            records.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &mut s2,
            &mut x2,
        );
        assert_eq!(s2, s);
        assert_eq!(x2, x);
    }

    #[test]
    fn bad_values_and_unknown_keys_change_nothing() {
        let (s0, x0) = defaults();
        let (mut s, mut x) = defaults();
        apply(
            [
                ("s/theme", "purple"),
                ("s/auto_photos", "1"),
                ("s/future", "x"),
                ("s/ringtone", "Kazoo"),
            ],
            &mut s,
            &mut x,
        );
        assert_eq!((s, x), (s0, x0));
    }

    #[test]
    fn device_only_settings_are_never_records() {
        for (k, _, _) in DEVICE {
            assert!(!SYNCED.iter().any(|(s, _, _)| s.ends_with(k)), "{k}");
        }
        for (k, _, _) in SYNCED {
            assert!(k.starts_with("s/"), "{k}");
        }
    }

    /// Two devices of one account: settings changed on each reach the other
    /// through the record store, and both show the newer edit of a setting
    /// both changed.
    #[test]
    fn two_devices_converge_through_the_store() -> Result<(), SyncError> {
        let server = MemoryServer::new();
        let key = StorageKey::from_bytes([5; 32]);
        let mut phone = Replica::new(key.clone(), [1; 16]);
        let mut laptop = Replica::new(key, [2; 16]);
        let (mut s, x) = defaults();
        s.theme = ThemeChoice::Dark;
        for (k, v) in encode(&s, &x) {
            phone.set(k, v.into_bytes(), 1_000)?;
        }
        block_on(songbird_sync::sync(&mut phone, &server, 1_000))?;
        block_on(songbird_sync::sync(&mut laptop, &server, 1_000))?;
        laptop.set("s/read_receipts", b"0".to_vec(), 2_000)?;
        phone.set("s/theme", b"light".to_vec(), 3_000)?;
        block_on(songbird_sync::sync(&mut laptop, &server, 3_000))?;
        block_on(songbird_sync::sync(&mut phone, &server, 4_000))?;
        block_on(songbird_sync::sync(&mut laptop, &server, 4_000))?;
        let show = |r: &Replica| {
            let (mut s, mut x) = defaults();
            let e: Vec<(String, String)> = r
                .entries()
                .into_iter()
                .map(|(k, v)| (k, String::from_utf8(v).unwrap_or_default()))
                .collect();
            apply(
                e.iter().map(|(k, v)| (k.as_str(), v.as_str())),
                &mut s,
                &mut x,
            );
            s
        };
        let (p, l) = (show(&phone), show(&laptop));
        assert_eq!(p, l);
        assert_eq!((p.theme, p.read_receipts), (ThemeChoice::Light, false));
        Ok(())
    }

    /// A device that just linked takes the account's settings, not its own
    /// defaults: it adopts the key with an empty replica, so its defaults
    /// (stamped later than the phone's choices) never overwrite them; and a
    /// rotation leaves the old key unable to read the store.
    #[test]
    fn a_linked_device_reads_the_account_settings_and_a_rotation_shuts_out_the_old_key()
    -> Result<(), SyncError> {
        let server = MemoryServer::new();
        let key = StorageKey::from_bytes([7; 32]);
        let mut phone = Replica::new(key.clone(), [1; 16]);
        phone.set("s/theme", b"dark".to_vec(), 1_000)?;
        phone.set("s/read_receipts", b"0".to_vec(), 1_000)?;
        block_on(songbird_sync::sync(&mut phone, &server, 1_000))?;
        // The new device's first run wrote its defaults, later, under a key of its own.
        let (s, x) = defaults();
        let mut laptop = Replica::new(StorageKey::from_bytes([8; 32]), [2; 16]);
        for (k, v) in encode(&s, &x) {
            laptop.set(k, v.into_bytes(), 5_000)?;
        }
        let mut laptop = fresh(StorageKey::from_bytes(*key.as_bytes()));
        block_on(songbird_sync::sync(&mut laptop, &server, 6_000))?;
        assert_eq!(laptop.get("s/theme"), Some(&b"dark"[..]));
        assert_eq!(laptop.get("s/read_receipts"), Some(&b"0"[..]));
        // The laptop is logged out; the phone, alone, rotates.
        let fresh_key = StorageKey::from_bytes([9; 32]);
        block_on(songbird_sync::rotate(&mut phone, &server, fresh_key, 7_000))?;
        phone.set("s/theme", b"light".to_vec(), 8_000)?;
        block_on(songbird_sync::sync(&mut phone, &server, 8_000))?;
        assert!(block_on(songbird_sync::sync(&mut laptop, &server, 9_000)).is_err());
        assert_eq!(laptop.get("s/theme"), Some(&b"dark"[..]));
        Ok(())
    }

    /// Saved contacts are records too: one saved on the phone reaches the
    /// laptop, an edit there comes back, and a deletion converges.
    #[test]
    fn saved_contacts_converge_between_devices() -> Result<(), SyncError> {
        use crate::store::SavedContact;
        let server = MemoryServer::new();
        let key = StorageKey::from_bytes([6; 32]);
        let mut phone = Replica::new(key.clone(), [1; 16]);
        let mut laptop = Replica::new(key, [2; 16]);
        let mut ana = SavedContact {
            id: "0123456789abcdef0123456789abcdef".into(),
            name: "Ana Popescu".into(),
            username: "ana".into(),
            notes: "Climbing on Thursdays".into(),
            groups: vec!["Friends".into()],
        };
        let bo = SavedContact {
            id: "fedcba9876543210fedcba9876543210".into(),
            name: "Bogdan".into(),
            ..SavedContact::default()
        };
        phone.set(&ana.key(), ana.encode().into_bytes(), 1_000)?;
        phone.set(&bo.key(), bo.encode().into_bytes(), 1_000)?;
        block_on(songbird_sync::sync(&mut phone, &server, 1_000))?;
        block_on(songbird_sync::sync(&mut laptop, &server, 1_000))?;
        ana.groups.push("Work".into());
        laptop.set(&ana.key(), ana.encode().into_bytes(), 2_000)?;
        phone.delete(&bo.key(), 2_500)?;
        block_on(songbird_sync::sync(&mut laptop, &server, 3_000))?;
        block_on(songbird_sync::sync(&mut phone, &server, 3_000))?;
        block_on(songbird_sync::sync(&mut laptop, &server, 3_000))?;
        let book = |r: &Replica| -> Vec<SavedContact> {
            r.with_prefix("k/")
                .into_iter()
                .filter_map(|(k, v)| SavedContact::decode(&k, std::str::from_utf8(&v).ok()?))
                .collect()
        };
        assert_eq!(book(&phone), book(&laptop));
        assert_eq!(book(&phone), vec![ana]);
        Ok(())
    }
}

/// The wiring on the real engine, headless: settings changed through the
/// store's intents survive a restart of the app.
#[cfg(test)]
mod headless {
    use std::sync::atomic::{AtomicBool, Ordering};

    use nsh_core::prelude::*;

    use crate::store::{Store, ThemeChoice};

    static CHANGE: AtomicBool = AtomicBool::new(true);

    #[component]
    fn SettingsProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        super::restore(s);
        if CHANGE.load(Ordering::SeqCst) {
            set_timeout(std::time::Duration::from_millis(10), move || {
                s.settings_update(|x| x.typing_indicators = false);
                s.settings_update(|x| x.theme = ThemeChoice::Dark);
                s.settings_update(|x| x.enter_sends = false);
                s.ext_update(|x| x.lists.push("Climbing".into()));
            });
        }
        let ext = s.ext();
        text(move || {
            let (st, x) = (s.settings.get(), ext.get());
            let out = format!(
                "typing {} theme {:?} enter {} lists {}",
                st.typing_indicators,
                st.theme,
                st.enter_sends,
                x.lists.join(",")
            );
            out
        })
    }

    fn start(
        storage: &std::sync::Arc<nsh_testing::MemoryStorage>,
    ) -> Result<nsh_testing::Headless, String> {
        let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<SettingsProbe>());
        config.storage = Some(storage.clone());
        nsh_testing::Headless::start(config).map_err(|e| e.to_string())
    }

    static SAVE: AtomicBool = AtomicBool::new(true);

    /// Saves a contact and a chat scene through the store's intents, and
    /// shows what the records hold.
    #[component]
    fn ContactsProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        super::restore(s);
        if SAVE.load(Ordering::SeqCst) {
            set_timeout(std::time::Duration::from_millis(10), move || {
                s.save_contact(crate::store::SavedContact {
                    name: "Ana Popescu".into(),
                    username: "@ana".into(),
                    notes: "Climbing".into(),
                    groups: vec!["Friends".into()],
                    ..Default::default()
                });
                s.save_from("Bogdan", "bogdan");
                s.set_chat_scene(4, Some(3));
            });
        }
        let book = s.book();
        let scenes = s.scenes();
        text(move || {
            let names = book.with(|b| {
                b.0.iter()
                    .map(|c| format!("{}/{}/{}", c.name, c.username, c.groups.join("+")))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            let scene = scenes.with(|m| m.0.get(&4).copied().unwrap_or(0));
            format!("book {names} scene {scene}")
        })
    }

    #[test]
    fn saved_contacts_and_scenes_survive_a_restart() -> Result<(), String> {
        let storage = nsh_testing::MemoryStorage::new([]);
        let start = |storage: &std::sync::Arc<nsh_testing::MemoryStorage>| {
            let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<ContactsProbe>());
            config.storage = Some(storage.clone());
            nsh_testing::Headless::start(config).map_err(|e| e.to_string())
        };
        let want = "text:book Ana Popescu/ana/Friends,Bogdan/bogdan/ scene 3";
        SAVE.store(true, Ordering::SeqCst);
        let mut first = start(&storage)?;
        first.wait_for(want)?;
        drop(first);
        SAVE.store(false, Ordering::SeqCst);
        let mut second = start(&storage)?;
        second.wait_for(want)?;
        Ok(())
    }

    #[test]
    fn settings_survive_a_restart() -> Result<(), String> {
        // One storage for both runs, as a device keeps its app storage.
        let storage = nsh_testing::MemoryStorage::new([]);
        CHANGE.store(true, Ordering::SeqCst);
        let mut first = start(&storage)?;
        first.wait_for(
            "text:typing false theme Dark enter false lists Family,Work,Friends,Climbing",
        )?;
        drop(first);
        CHANGE.store(false, Ordering::SeqCst);
        let mut second = start(&storage)?;
        second.wait_for(
            "text:typing false theme Dark enter false lists Family,Work,Friends,Climbing",
        )?;
        // Synced settings are in the replica (to sync when the account is live); device ones
        // apart, never in it.
        let saved = storage.entries();
        // Kept under the vault key (D-0172): sealed in app storage.
        let sealed = saved.get(super::K_STATE).ok_or("no replica")?;
        assert!(sealed.starts_with("v1:"));
        let replica = songbird_sync::Replica::load(
            &crate::backend::wire::unhex(
                &crate::backend::vault::reveal(super::K_STATE, sealed).ok_or("sealed")?,
            )
            .ok_or("hex")?,
        )
        .map_err(|e| e.to_string())?;
        assert_eq!(replica.get("s/typing"), Some(&b"0"[..]));
        assert_eq!(replica.get("s/theme"), Some(&b"dark"[..]));
        assert!(replica.get("enter_sends").is_none() && replica.get("s/enter_sends").is_none());
        assert!(
            saved
                .get(super::K_DEVICE)
                .is_some_and(|d| d.contains(r#""enter_sends":"0""#))
        );
        Ok(())
    }
}
