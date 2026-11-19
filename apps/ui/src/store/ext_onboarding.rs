//! A4 and A11 additions to the store: invite codes, usernames, the
//! recovery key, account security, linked devices, backups, restore and
//! transfer.
//!
//! **Mock.** app-core is not built yet. The pure rules (formats, the
//! recovery key's groups, the safety number's layout) are plain functions
//! with unit tests; the intents simulate the server and the backup engine
//! with timers. Each says which app-core call takes its place.

use std::time::Duration;

use nsh_core::prelude::*;

use super::{BackupDestination, DeviceKind, NOW, Store, clock};

// ---------------------------------------------------------------- invite codes

/// How many symbols an invite code has: 24 Crockford base32 symbols
/// (proto `registration::invite::parse`).
pub const INVITE_LEN: usize = 24;
/// Symbols per group as the code is shown (`XXXX-XXXX-…`, six groups).
pub const INVITE_GROUP: usize = 4;

/// What registration answers about an invite code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteCheck {
    Valid,
    /// Not the full length yet.
    Incomplete,
    /// Refused at registration. The server gives one answer for unknown,
    /// used, expired and revoked codes (THREAT_MODEL 3.8, enumeration), so
    /// the UI never tells them apart either.
    Refused,
}

/// One typed character as a Crockford base32 symbol: case-insensitive,
/// I and L read as 1, O as 0. `None` for anything else (U is not in the
/// alphabet; hyphens and spaces are separators).
fn crockford(c: char) -> Option<char> {
    match c.to_ascii_uppercase() {
        'I' | 'L' => Some('1'),
        'O' => Some('0'),
        'U' => None,
        c @ ('0'..='9' | 'A'..='Z') => Some(c),
        _ => None,
    }
}

/// The code as typed or pasted, cleaned: Crockford symbols only, upper
/// case, at most [`INVITE_LEN`]. Separators and stray characters drop.
pub fn normalize_invite(raw: &str) -> String {
    raw.chars().filter_map(crockford).take(INVITE_LEN).collect()
}

/// The code in its groups of four (`K7P2-QX9M-…`), as people read it.
pub fn format_invite(code: &str) -> String {
    let code = normalize_invite(code);
    code.as_bytes()
        .chunks(INVITE_GROUP)
        .map(|g| std::str::from_utf8(g).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

/// Whether the code is complete, checked on the device. There is no
/// endpoint to ask about a code before registering.
pub fn invite_complete(code: &str) -> bool {
    normalize_invite(code).len() == INVITE_LEN
}

/// Mock of what registration answers for the code: codes starting with
/// four zeros are refused, any other complete code works.
pub fn check_invite(code: &str) -> InviteCheck {
    let code = normalize_invite(code);
    if code.len() < INVITE_LEN {
        InviteCheck::Incomplete
    } else if code.starts_with("0000") {
        InviteCheck::Refused
    } else {
        InviteCheck::Valid
    }
}

// ---------------------------------------------------------------- usernames

/// Why a username can't be used, or that it can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsernameCheck {
    Empty,
    TooShort,
    TooLong,
    /// Starts with something other than a letter.
    BadStart,
    /// Holds a character other than letters, digits, `.` and `_`.
    BadChars,
    /// Two dots in a row.
    DoubleDot,
    /// Ends with a dot.
    TrailingDot,
    /// Someone claimed it first (learned when claiming, after the account
    /// is made: there is no lookup before registering).
    Taken,
    /// The format is fine.
    Available,
}

pub const USERNAME_MIN: usize = 3;
pub const USERNAME_MAX: usize = 32;

/// The username as typed, minus surrounding spaces and a leading `@`.
/// Case is kept: usernames are unique case-insensitively but shown as
/// their owner typed them (PROTOCOL 403–406).
pub fn normalize_username(raw: &str) -> String {
    raw.trim().trim_start_matches('@').to_string()
}

/// The form two usernames are compared in (uniqueness ignores case).
pub fn username_key(name: &str) -> String {
    normalize_username(name).to_lowercase()
}

/// The format rules, checked on the device. `None`: the format is fine.
pub fn username_format(name: &str) -> Option<UsernameCheck> {
    let n = name.chars().count();
    if n == 0 {
        return Some(UsernameCheck::Empty);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        return Some(UsernameCheck::BadChars);
    }
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Some(UsernameCheck::BadStart);
    }
    if name.contains("..") {
        return Some(UsernameCheck::DoubleDot);
    }
    if n < USERNAME_MIN {
        return Some(UsernameCheck::TooShort);
    }
    if n > USERNAME_MAX {
        return Some(UsernameCheck::TooLong);
    }
    if name.ends_with('.') {
        return Some(UsernameCheck::TrailingDot);
    }
    None
}

impl UsernameCheck {
    /// The inline status line, plain words.
    pub fn message(&self, name: &str) -> String {
        match self {
            UsernameCheck::Empty => {
                "3 to 32 letters, numbers, dots or underscores, starting with a letter.".into()
            }
            UsernameCheck::TooShort => "Use at least 3 characters.".into(),
            UsernameCheck::TooLong => "Use at most 32 characters.".into(),
            UsernameCheck::BadStart => "Start with a letter.".into(),
            UsernameCheck::BadChars => "Use only letters, numbers, dots and underscores.".into(),
            UsernameCheck::DoubleDot => "Use one dot at a time.".into(),
            UsernameCheck::TrailingDot => "End with a letter, number or underscore.".into(),
            UsernameCheck::Taken => format!("Someone just took @{name}. Pick another:"),
            UsernameCheck::Available => format!("Friends will see @{name}, just as you typed it."),
        }
    }
}

/// A stem for suggestions from what was typed: allowed characters only,
/// starting with a letter, single dots, no dot at the end.
fn suggestion_stem(base: &str) -> String {
    let mut out = String::new();
    for c in normalize_username(base).chars() {
        let ok = c.is_ascii_alphanumeric() || c == '.' || c == '_';
        if !ok || (out.is_empty() && !c.is_ascii_alphabetic()) || (c == '.' && out.ends_with('.')) {
            continue;
        }
        out.push(c);
    }
    let mut out: String = out.chars().take(USERNAME_MAX - 4).collect();
    while out.ends_with('.') {
        out.pop();
    }
    if out.is_empty() {
        "songbird.user".to_string()
    } else {
        out
    }
}

/// Up to three free names close to `base`, for the chips under a taken
/// name; each one obeys the format rules. `available` is the claim check.
pub fn username_suggestions(base: &str, available: impl Fn(&str) -> bool) -> Vec<String> {
    let stem = suggestion_stem(base);
    let mut out = Vec::new();
    let candidates = [
        format!("{stem}.chat"),
        format!("{stem}_{}", 7 + stem.len() % 3),
        format!("the.{stem}"),
        format!("{stem}{}", 20 + stem.len()),
        format!("{stem}.eu"),
    ];
    for c in candidates {
        if out.len() == 3 {
            break;
        }
        if username_format(&c).is_none() && available(&c) {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------- recovery key

/// The key's groups, as shown ("SB4K", "7Q2M", …).
pub fn key_groups(key: &str) -> Vec<String> {
    key.split(['-', ' '])
        .filter(|g| !g.is_empty())
        .map(|g| g.to_ascii_uppercase())
        .collect()
}

/// The key's characters without separators.
fn key_chars(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Whether `typed` is the key's last four characters (any case, spaces
/// ignored).
pub fn key_tail_matches(key: &str, typed: &str) -> bool {
    let key = key_chars(key);
    let typed = key_chars(typed);
    typed.len() == 4 && key.ends_with(&typed)
}

/// Whether `typed` is the key's last group.
pub fn key_last_group_matches(key: &str, typed: &str) -> bool {
    key_groups(key)
        .last()
        .is_some_and(|g| *g == key_chars(typed))
}

/// Whether `typed` has a whole recovery key's length (64 characters, 16
/// groups of four), for enabling Restore.
pub fn key_well_formed(typed: &str) -> bool {
    let chars = key_chars(typed);
    chars.len() == 64
}

/// Whether `typed` is a recovery key whose built-in check matches (no
/// typo), before anything is fetched (PROTOCOL §11.1).
pub fn key_checks_out(typed: &str) -> bool {
    songbird_backup_format::RecoveryKey::parse(typed).is_ok()
}

/// Formats a key as it is typed: groups of four joined by dashes.
pub fn format_key_input(raw: &str) -> String {
    let chars: Vec<char> = key_chars(raw).chars().take(64).collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

// ---------------------------------------------------------------- safety numbers and codes

/// A safety number's twelve groups of five digits.
pub fn safety_groups(number: &str) -> Vec<String> {
    let digits: Vec<char> = number.chars().filter(char::is_ascii_digit).collect();
    digits.chunks(5).map(|c| c.iter().collect()).collect()
}

/// A stable 64-bit hash (FNV-1a) of `text`, to derive decorative codes.
pub fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A short numeric code to type when a QR code can't be scanned
/// ("482 913").
pub fn pairing_code(seed: &str) -> String {
    let n = hash(seed) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

// ---------------------------------------------------------------- backups

/// How often backups run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frequency {
    #[default]
    Daily,
    Weekly,
    Monthly,
}

impl Frequency {
    pub fn label(self) -> &'static str {
        match self {
            Frequency::Daily => "Daily",
            Frequency::Weekly => "Weekly",
            Frequency::Monthly => "Monthly",
        }
    }
}

/// A backup that didn't finish, in plain words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupProblem {
    /// The destination is full.
    StorageFull,
    /// Songbird was signed out of the destination.
    SignedOut,
    /// Anything else; [`BackupExtra::note`] says what.
    Other,
}

/// Backup state the shared `Settings` doesn't hold yet (lead: candidates
/// for `Settings`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BackupExtra {
    pub frequency: Frequency,
    pub second: Option<BackupDestination>,
    /// A backup running now: 0.0 to 1.0.
    pub running: Option<f32>,
    pub problem: Option<BackupProblem>,
    /// What went wrong, in words, for [`BackupProblem::Other`] and a failed restore.
    pub note: Option<String>,
    /// What the last backup or restore held (the screens' counts).
    pub messages: u32,
    pub chats: u32,
    pub nextcloud_url: String,
    pub nextcloud_user: String,
    /// An app password (never the account's own password); kept on this device.
    pub nextcloud_password: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    /// Empty: `us-east-1`, which most S3-compatible services accept.
    pub s3_region: String,
    pub s3_key: String,
    /// The secret access key; kept on this device.
    pub s3_secret: String,
}

/// Where a restore is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestoreStage {
    #[default]
    Idle,
    Finding,
    Decrypting,
    /// Messages restored so far.
    Restoring(u32),
    Done,
    /// The backup can't be read; nothing was written on this device.
    Failed,
}

/// "1,204".
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl RestoreStage {
    /// Overall progress, 0.0 to 1.0, with `total` messages to restore.
    pub fn fraction(self, total: u32) -> f32 {
        match self {
            RestoreStage::Idle => 0.0,
            RestoreStage::Finding => 0.08,
            RestoreStage::Decrypting => 0.2,
            RestoreStage::Restoring(n) => 0.2 + 0.8 * n as f32 / total.max(1) as f32,
            RestoreStage::Done => 1.0,
            RestoreStage::Failed => 0.0,
        }
    }
}

/// A transfer between two phones.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TransferStage {
    #[default]
    Idle,
    Pairing,
    /// 0.0 to 1.0.
    Moving(f32),
    Done,
}

// ---------------------------------------------------------------- account

/// One of the accounts on this device (the switcher).
#[derive(Debug, Clone, PartialEq)]
pub struct AccountEntry {
    pub name: String,
    pub username: String,
    pub seed: u64,
}

/// Account state the shared store doesn't hold yet.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountExtra {
    pub username_link: String,
    pub passkeys: Vec<String>,
    /// Other accounts signed in on this device.
    pub others: Vec<AccountEntry>,
    /// A data report the user asked for: `None` none, `Some(false)` being
    /// prepared, `Some(true)` ready.
    pub report: Option<bool>,
}

impl Default for AccountExtra {
    fn default() -> Self {
        Self {
            username_link: "https://songbird.chat/u/radu#7Fq2-xN8p".into(),
            passkeys: Vec::new(),
            others: vec![AccountEntry {
                name: "Radu (Work)".into(),
                username: "radu.work".into(),
                seed: 5,
            }],
            report: None,
        }
    }
}

// ---------------------------------------------------------------- intents

/// Runs `f` after `ms`, whatever happens to the screen that asked: the
/// store's simulated work belongs to the store, not to a view (a timer
/// made under a step's owner would stop when the step changes).
fn later(ms: u64, f: impl FnOnce() + 'static) {
    Owner::detached().run(|| {
        let _ = set_timeout(Duration::from_millis(ms), f);
    });
}

impl Store {
    pub fn backup_extra(&self) -> Signal<BackupExtra> {
        global::<BackupExtra>()
    }

    pub fn restore_stage(&self) -> Signal<RestoreStage> {
        global::<RestoreStage>()
    }

    pub fn transfer_stage(&self) -> Signal<TransferStage> {
        global::<TransferStage>()
    }

    pub fn account_extra(&self) -> Signal<AccountExtra> {
        global::<AccountExtra>()
    }

    /// Mock: registration (invite, username, Argon2id proof of work) as
    /// app-core's `register` will do it; here the name is just kept.
    pub fn create_account(&self, username: String) {
        self.profile_update(|p| p.username = username.clone());
        self.account_extra().update(|a| {
            a.username_link = format!(
                "https://songbird.chat/u/{username}#{:x}",
                hash(&username) & 0xffff_ff
            );
        });
    }

    /// This account's personal invite link (PROTOCOL §14.10): its card in the
    /// fragment, which no server sees. Empty before registration.
    pub fn invite_link(&self) -> String {
        crate::backend::plans::my_invite_link().unwrap_or_default()
    }

    /// Mock: a new username link; the old one stops working.
    pub fn reset_username_link(&self) {
        let me = self.me.with_untracked(|m| m.username.clone());
        self.account_extra().update(|a| {
            let next = hash(&a.username_link) & 0xffff_ff;
            a.username_link = format!("https://songbird.chat/u/{me}#{next:x}");
        });
    }

    /// Mock: app-core reports that a contact's identity key changed.
    pub fn set_key_changed(&self, contact: super::ContactId, changed: bool) {
        self.contacts.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
                c.key_changed = changed;
            }
        });
    }

    pub fn rename_device(&self, id: u64, name: String) {
        self.devices.update(|d| {
            if let Some(x) = d.iter_mut().find(|x| x.id == id) {
                x.name = name;
            }
        });
    }

    /// Mock: linking a new device after the scan (app-core `link_device`,
    /// which needs the two-step PIN when it is on).
    pub fn link_scanned_device(&self) -> String {
        let n = self.devices.with_untracked(|d| d.len());
        let (name, kind) = match n % 3 {
            0 => ("iPad", DeviceKind::Tablet),
            1 => ("MacBook Pro", DeviceKind::Desktop),
            _ => ("Linux laptop", DeviceKind::Desktop),
        };
        self.link_device(name.into(), kind);
        name.into()
    }

    /// Mock: a passkey made by the platform (app-core `add_passkey`).
    pub fn add_passkey(&self) {
        let name = if is_desktop() {
            "Passkey on this computer"
        } else {
            "Passkey on this phone"
        };
        self.account_extra()
            .update(|a| a.passkeys.push(name.into()));
        self.settings_update(|s| s.passkey = true);
    }

    pub fn remove_passkey(&self, index: usize) {
        self.account_extra().update(|a| {
            if index < a.passkeys.len() {
                a.passkeys.remove(index);
            }
        });
        let none = self
            .account_extra()
            .with_untracked(|a| a.passkeys.is_empty());
        if none {
            self.settings_update(|s| s.passkey = false);
        }
    }

    /// Mock: the account report (app-core `export_account`), ready after a
    /// moment.
    pub fn request_account_report(&self) {
        let extra = self.account_extra();
        extra.update(|a| a.report = Some(false));
        // The server's export (`GET /v1/account/export`): ready when it answers.
        crate::backend::run(async move {
            let r = crate::backend::account::export().await;
            extra.update(|a| a.report = if r.is_ok() { Some(true) } else { None });
            r.map(|_| ())
        });
    }

    /// Mock: switching accounts swaps the profile shown (app-core keeps a
    /// store per account).
    pub fn switch_account(&self, index: usize) {
        let extra = self.account_extra();
        let Some(next) = extra.with_untracked(|a| a.others.get(index).cloned()) else {
            return;
        };
        let me = self.me.get_untracked();
        extra.update(|a| {
            a.others[index] = AccountEntry {
                name: me.name.clone(),
                username: me.username.clone(),
                seed: me.seed,
            };
        });
        self.profile_update(|p| {
            p.name = next.name;
            p.username = next.username;
            p.seed = next.seed;
        });
    }

    pub fn set_backup_frequency(&self, f: Frequency) {
        self.backup_extra().update(|b| b.frequency = f);
        self.settings_update(|s| s.backup_daily = f == Frequency::Daily);
    }

    /// Turns backups on to `dest` (the wizard's end) and runs the first.
    pub fn enable_backups(&self, dest: BackupDestination, media: bool, f: Frequency) {
        self.settings_update(|s| {
            s.backup_on = true;
            s.backup_destination = dest;
            s.backup_include_media = media;
        });
        self.set_backup_frequency(f);
        self.backup_extra().update(|b| b.problem = None);
        self.back_up_now();
    }

    pub fn disable_backups(&self) {
        // The backup at the destination is deleted (the vault: rows at once,
        // objects by the worker; elsewhere: every object).
        let dest = self.settings.with_untracked(|s| s.backup_destination);
        if crate::backend::backup::available(dest) {
            let target = self.backup_extra().get_untracked();
            Owner::detached().run(|| {
                spawn(async move {
                    if let Err(p) = crate::backend::backup::delete(dest, target).await {
                        log::warn!("backup not deleted: {p}");
                    }
                });
            });
        }
        self.settings_update(|s| {
            s.backup_on = false;
            s.backup_last = None;
        });
        self.backup_extra().update(|b| {
            b.running = None;
            b.problem = None;
            b.second = None;
        });
    }

    /// A backup run (Prompt 10): the store's content, encrypted under the
    /// recovery key, to the chosen destination; progress from the records
    /// written. A destination the app can't use yet, or a failure, shows a
    /// problem in words.
    pub fn back_up_now(&self) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let extra = self.backup_extra();
        if extra.with_untracked(|b| b.running.is_some()) {
            return;
        }
        let store = *self;
        let dest = self.settings.with_untracked(|s| s.backup_destination);
        if !crate::backend::backup::available(dest) {
            extra.update(|b| {
                b.problem = Some(BackupProblem::Other);
                b.note = Some(crate::backend::backup::unavailable_reason(dest).into());
            });
            return;
        }
        let snap = crate::backend::backup::snapshot(store);
        let total = snap.messages.max(1);
        let records = Arc::new(AtomicU32::new(0));
        extra.update(|b| {
            b.running = Some(0.0);
            b.problem = None;
            b.note = None;
        });
        let counted = records.clone();
        let poll = Owner::detached().run(|| {
            set_interval(Duration::from_millis(150), move || {
                let n = counted.load(Ordering::Relaxed);
                // Records include the settings and chats; the bar tracks
                // messages, the bulk, and waits at 95% for the upload.
                let p = (n as f32 / (total + 200) as f32).min(0.95);
                extra.update(|b| {
                    if b.running.is_some() {
                        b.running = Some(p);
                    }
                });
            })
        });
        let messages = snap.messages;
        let chats = snap.chats;
        let target = extra.get_untracked();
        Owner::detached().run(|| {
            spawn(async move {
                let r = crate::backend::backup::back_up(dest, target, snap, records).await;
                poll.cancel();
                match r {
                    Ok(_) => {
                        extra.update(|b| {
                            b.running = None;
                            b.messages = messages;
                            b.chats = chats;
                        });
                        store.settings_update(|s| {
                            s.backup_last = Some(format!("Today at {}", clock(NOW)))
                        });
                    }
                    Err(p) => extra.update(|b| {
                        b.running = None;
                        b.problem = Some(if p.0.contains("full") {
                            BackupProblem::StorageFull
                        } else {
                            BackupProblem::Other
                        });
                        b.note = Some(p.0);
                    }),
                }
            });
        });
    }

    /// Demo: the last backup failed (to show the error state).
    pub fn simulate_backup_problem(&self, problem: BackupProblem) {
        self.backup_extra().update(|b| {
            b.running = None;
            b.problem = Some(problem);
        });
    }

    /// Restores a backup (Prompt 10): finds it (the chosen destination,
    /// or on a new device the Songbird server, else this device's backup
    /// folder), verifies every part, then puts it back in one change and
    /// recovers the account onto this device when it has none. `key`: the
    /// key a new device's user typed (`None`: this device's own). Nothing
    /// changes on any failure. `damaged` (debug builds' review shortcut):
    /// shows the failure without fetching.
    pub fn restore_backup(&self, damaged: bool) {
        self.restore_backup_with(None, damaged);
    }

    /// As [`Self::restore_backup`], with a typed key.
    pub fn restore_backup_with(&self, key: Option<String>, damaged: bool) {
        let stage = self.restore_stage();
        let extra = self.backup_extra();
        stage.set(RestoreStage::Finding);
        if damaged {
            later(900, move || stage.set(RestoreStage::Decrypting));
            later(2000, move || stage.set(RestoreStage::Failed));
            return;
        }
        let store = *self;
        let key = match key {
            Some(t) => match songbird_backup_format::RecoveryKey::parse(&t) {
                Ok(k) => k,
                Err(_) => {
                    extra.update(|b| {
                        b.note = Some("That key doesn't check out. Check it for a typo.".into())
                    });
                    stage.set(RestoreStage::Failed);
                    return;
                }
            },
            None => crate::backend::backup::recovery_key(),
        };
        let on = self.settings.with_untracked(|s| s.backup_on);
        let chosen = self.settings.with_untracked(|s| s.backup_destination);
        let mut places = vec![];
        if on && crate::backend::backup::available(chosen) {
            places.push(chosen);
        }
        for d in [
            BackupDestination::SongbirdServers,
            BackupDestination::EncryptedFile,
        ] {
            if !places.contains(&d) {
                places.push(d);
            }
        }
        let target = extra.get_untracked();
        Owner::detached().run(|| {
            spawn(async move {
                let mut last = None;
                let mut found = None;
                for d in places {
                    match crate::backend::backup::fetch(d, target.clone(), &key).await {
                        Ok(r) => {
                            found = Some(r);
                            break;
                        }
                        Err(p) => last = Some(p),
                    }
                }
                let Some(r) = found else {
                    extra.update(|b| b.note = last.map(|p| p.0));
                    stage.set(RestoreStage::Failed);
                    return;
                };
                stage.set(RestoreStage::Decrypting);
                if let Err(p) = crate::backend::backup::recover(&r, None, None).await {
                    extra.update(|b| b.note = Some(p.0));
                    stage.set(RestoreStage::Failed);
                    return;
                }
                crate::backend::backup::adopt_recovery_key(&key);
                store
                    .session
                    .update(|s| s.recovery_key = key.display().to_string());
                crate::backend::backup::apply(store, &r);
                extra.update(|b| {
                    b.messages = r.messages;
                    b.chats = r.chats;
                    b.note = None;
                });
                stage.set(RestoreStage::Restoring(r.messages));
                later(300, move || stage.set(RestoreStage::Done));
            });
        });
    }

    pub fn reset_restore(&self) {
        self.backup_extra().update(|b| b.note = None);
        self.restore_stage().set(RestoreStage::Idle);
    }

    /// The transfer's stage (the screens drive the real transfer through
    /// `backend::backup`).
    pub fn set_transfer(&self, stage: TransferStage) {
        self.transfer_stage().set(stage);
    }

    pub fn reset_transfer(&self) {
        self.transfer_stage().set(TransferStage::Idle);
    }
}

// Signals in globals need `PartialEq` and `Default`; these are checked here.
const _: fn() = || {
    fn needs<T: Default + PartialEq + 'static>() {}
    needs::<BackupExtra>();
    needs::<RestoreStage>();
    needs::<TransferStage>();
    needs::<AccountExtra>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes() {
        // Crockford: case-insensitive, I/L → 1, O → 0, U dropped, separators ignored.
        assert_eq!(normalize_invite(" k7p2-qx9m "), "K7P2QX9M");
        assert_eq!(normalize_invite("iLoU"), "110");
        let pasted = "k7p2-qx9m-ab3d-eh4j-nr5s-tv6w";
        assert_eq!(normalize_invite(pasted).len(), INVITE_LEN);
        assert_eq!(format_invite(pasted), "K7P2-QX9M-AB3D-EH4J-NR5S-TV6W");
        assert_eq!(
            normalize_invite(&format!("{pasted}-EXTRA")).len(),
            INVITE_LEN
        );
        assert!(invite_complete(pasted));
        assert!(!invite_complete("K7P2QX9M"));
        assert_eq!(check_invite("abc"), InviteCheck::Incomplete);
        assert_eq!(
            check_invite("0000-QX9M-AB3D-EH4J-NR5S-TV6W"),
            InviteCheck::Refused
        );
        assert_eq!(check_invite(pasted), InviteCheck::Valid);
    }

    #[test]
    fn username_rules() {
        // Shown as typed: case is kept; compared without it.
        assert_eq!(normalize_username(" @Radu.D "), "Radu.D");
        assert_eq!(username_key("@Radu.D"), "radu.d");
        assert_eq!(username_format(""), Some(UsernameCheck::Empty));
        assert_eq!(username_format("ab"), Some(UsernameCheck::TooShort));
        assert_eq!(username_format("1abc"), Some(UsernameCheck::BadStart));
        assert_eq!(username_format("_abc"), Some(UsernameCheck::BadStart));
        assert_eq!(username_format("a b"), Some(UsernameCheck::BadChars));
        assert_eq!(username_format("ana-p"), Some(UsernameCheck::BadChars));
        assert_eq!(username_format("ana..p"), Some(UsernameCheck::DoubleDot));
        assert_eq!(username_format("ana.p."), Some(UsernameCheck::TrailingDot));
        assert_eq!(
            username_format(&"a".repeat(33)),
            Some(UsernameCheck::TooLong)
        );
        assert_eq!(username_format(&"a".repeat(32)), None);
        assert_eq!(username_format("Ana.P_2"), None);
        assert_eq!(username_format("ana_"), None);
    }

    #[test]
    fn suggestions_are_free_and_valid() {
        let taken = |n: &str| n != "ana.chat";
        let s = username_suggestions("ana", taken);
        assert_eq!(s.len(), 3);
        assert!(!s.contains(&"ana.chat".to_string()));
        assert!(s.iter().all(|n| username_format(n).is_none()));
        // Case is kept, and odd input still gives valid names.
        assert!(
            username_suggestions("Ana", |_| true)
                .iter()
                .all(|n| n.starts_with("Ana") || n.starts_with("the.Ana"))
        );
        for odd in ["9..ana..", "__", "a.", "...", &"x".repeat(40)] {
            let s = username_suggestions(odd, |_| true);
            assert!(!s.is_empty(), "{odd}");
            assert!(
                s.iter().all(|n| username_format(n).is_none()),
                "{odd}: {s:?}"
            );
        }
    }

    #[test]
    fn recovery_key() {
        let real = songbird_backup_format::RecoveryKey::from_secret([7; 32]);
        let key = real.display().to_string();
        assert_eq!(key_groups(&key).len(), 16);
        let last = key_groups(&key).last().unwrap().clone();
        assert!(key_tail_matches(&key, &last.to_lowercase()));
        assert!(!key_tail_matches(&key, &last[..3]));
        assert!(key_last_group_matches(&key, &format!(" {last}")));
        assert!(key_well_formed(&key.to_lowercase().replace('-', "")));
        assert!(key_checks_out(&key.to_lowercase()));
        let mut typo: Vec<char> = key.chars().collect();
        typo[5] = if typo[5] == 'A' { 'B' } else { 'A' };
        let typo: String = typo.into_iter().collect();
        assert!(key_well_formed(&typo) && !key_checks_out(&typo));
        assert!(!key_well_formed("SB4K-7Q2M"));
        assert_eq!(format_key_input("sb4k7q2mx"), "SB4K-7Q2M-X");
    }

    #[test]
    fn safety_number_layout() {
        let n = "12345 67890 12345 67890 12345 67890 12345 67890 12345 67890 12345 67890";
        let g = safety_groups(n);
        assert_eq!(g.len(), 12);
        assert!(g.iter().all(|x| x.len() == 5));
    }

    #[test]
    fn numbers_and_progress() {
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(pairing_code("x").len(), 7);
        assert!(RestoreStage::Restoring(1204).fraction(1204) > 0.99);
        assert_eq!(RestoreStage::Failed.fraction(10), 0.0);
    }
}
