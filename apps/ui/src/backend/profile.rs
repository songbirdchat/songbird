//! The encrypted profile (Prompt 9, M6; PROTOCOL §4.8, §10.4, D-0103):
//! name, about and the custom status with its emoji and expiry, sealed
//! under this account's profile key and uploaded as an opaque blob.
//!
//! - The **profile key** is 32 random bytes kept in the app's storage (as
//!   the backend's other keys are, until app-core moves them to the
//!   platform keystore). Its access key (`songbird_keys::sealed`) goes with
//!   every upload, so contacts holding the profile key can send sealed.
//! - The **version** only grows: one more for every upload.
//! - **Visibility**: only contacts hold the profile key, so a field whose
//!   audience is Nobody is left out of the blob. Narrowing any of them to
//!   Nobody rotates the key (a new key, a new version), so a former
//!   contact holding the old key reads nothing new.
//! - **A status** carries its real expiry (`expires_ms`): every reader
//!   stops showing it then, with no further message.
//!
//! mock: telling contacts "profile changed" (`Content.profile_changed`,
//! sealed and silent) and handing them the new key need app-core's
//! sessions (A2); the photo needs the avatar upload, so the blob carries
//! no avatar yet.

use std::cell::RefCell;

use super::vault::Storage;
use nsh_core::prelude::*;
use songbird_content::profile::{
    self as content, MAX_ABOUT, MAX_EMOJI, MAX_NAME, MAX_STATUS, ProfilePlain, Status,
};
use songbird_proto::profile::ProfileUpload;

use super::wire;
use crate::store::{Audience, Profile, Settings, SettingsExt, Store};

const K_KEY: &str = "sb.profile_key";
const K_VERSION: &str = "sb.profile_version";
/// The user's own profile on this device (name, username, about, status, avatar seed), kept in
/// the vault so it survives a restart (D-0183: it lived in memory only, and came back empty).
const K_ME: &str = "sb.me";

fn save_me(me: &Profile) {
    let v = serde_json::json!({
        "name": me.name,
        "username": me.username,
        "about": me.about,
        "status": me.status_line,
        "seed": me.seed,
    });
    if let Err(e) = Storage::set_item(K_ME, v.to_string()) {
        log::warn!("profile storage: {e}");
    }
}

/// Puts back the profile this device kept (not over a demo's).
fn restore_me(store: Store) {
    if crate::store::demo() {
        return;
    }
    let Some(v) =
        Storage::get_item(K_ME).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return;
    };
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
    store.me.update(|me| {
        me.name = text("name");
        me.username = text("username");
        me.about = text("about");
        me.status_line = v["status"].as_str().map(str::to_owned);
        if let Some(seed) = v["seed"].as_u64() {
            me.seed = seed;
        }
    });
}

/// Who sees the photo, the about line and the status.
pub type Audiences = (Audience, Audience, Audience);

fn audiences(s: &Settings) -> Audiences {
    (s.photo, s.about, s.status)
}

/// Whether any field went from shared to Nobody (the key must change).
pub fn narrowed(before: Audiences, after: Audiences) -> bool {
    let to_nobody = |b: Audience, a: Audience| b != Audience::Nobody && a == Audience::Nobody;
    to_nobody(before.0, after.0) || to_nobody(before.1, after.1) || to_nobody(before.2, after.2)
}

/// `s` cut to at most `max` bytes, on a character boundary.
fn clip(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// What the blob holds for this profile and these settings.
pub fn plain(me: &Profile, s: &Settings, x: &SettingsExt) -> ProfilePlain {
    let status = (s.status != Audience::Nobody)
        .then_some(me.status_line.as_deref())
        .flatten()
        .filter(|t| !t.trim().is_empty())
        .map(|t| Status {
            text: clip(t.trim(), MAX_STATUS),
            emoji: clip(&x.status_emoji, MAX_EMOJI),
            expires_ms: x.status_expires_ms,
        });
    ProfilePlain {
        name: clip(me.name.trim(), MAX_NAME),
        about: if s.about == Audience::Nobody {
            String::new()
        } else {
            clip(me.about.trim(), MAX_ABOUT)
        },
        status,
        // Set by `publish` from `avatar::mine` (kept out of this pure function).
        avatar: None,
        links: x
            .links
            .iter()
            .filter(|l| !l.is_empty() && l.len() <= content::MAX_LINK)
            .take(content::MAX_LINKS)
            .cloned()
            .collect(),
    }
}

/// This account's profile key, made on first use.
pub fn profile_key() -> [u8; 32] {
    if let Some(k) = Storage::get_item(K_KEY)
        .and_then(|h| wire::unhex(&h))
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
    {
        return k;
    }
    rotate()
}

/// A new profile key (after narrowing who sees a field, or a block).
pub(crate) fn rotate() -> [u8; 32] {
    let k: [u8; 32] = wire::random();
    if let Err(e) = Storage::set_item(K_KEY, wire::hex(&k)) {
        log::warn!("profile storage: {e}");
    }
    k
}

/// The version last uploaded (0: never).
pub fn version() -> u64 {
    Storage::get_item(K_VERSION)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The upload for `p` at `version` under `key`.
pub fn upload(key: &[u8; 32], version: u64, p: &ProfilePlain) -> Option<ProfileUpload> {
    let blob = content::seal(key, version, p).ok()?;
    Some(ProfileUpload {
        profile_version: version,
        access_key: songbird_keys::sealed::access_key(key),
        open: false,
        blob,
    })
}

/// Seals the profile and, when the account is live, uploads it
/// (`PUT /v1/profile`, signed).
pub fn publish(store: Store) {
    let p = {
        let s = store.settings.get_untracked();
        let x = store.ext().get_untracked();
        let mut p = store.me.with_untracked(|me| plain(me, &s, &x));
        // The encrypted avatar object and its key, when the audience allows.
        if s.photo != Audience::Nobody {
            p.avatar = super::avatar::mine().map(|(id, key)| content::Avatar {
                id: id.to_vec(),
                key: key.to_vec(),
            });
        }
        p
    };
    if !(super::is_live() && super::registered()) {
        return;
    }
    let version = version() + 1;
    let Some(up) = upload(&profile_key(), version, &p) else {
        super::live().update(|l| l.problem = Some("Your profile couldn't be saved.".into()));
        return;
    };
    super::run(async move {
        let (status, body) = super::signed("PUT", "/v1/profile", Some(up.encode()), false).await?;
        if (200..300).contains(&status) {
            if let Err(e) = Storage::set_item(K_VERSION, version.to_string()) {
                log::warn!("profile storage: {e}");
            }
            Ok(())
        } else {
            Err(super::explain(status, &body))
        }
    });
}

thread_local! {
    /// What was last published (or found at start), to see what changed.
    static LAST: RefCell<Option<(ProfilePlain, Audiences)>> = const { RefCell::new(None) };
}

/// Publishes the profile whenever what it holds changes: a name or about
/// edit, a status set, cleared or expired, a narrower or wider audience.
/// Called once at the root (its effect belongs to it).
pub fn install(store: Store) {
    restore_me(store);
    let ext = store.ext();
    effect(move || {
        store.me.with(|me| untracked(|| save_me(me)));
        let now = store.me.with(|me| {
            store
                .settings
                .with(|s| ext.with(|x| (plain(me, s, x), audiences(s))))
        });
        untracked(|| {
            let before = LAST.with(|l| l.borrow_mut().replace(now.clone()));
            match before {
                // First run: upload only a profile never uploaded.
                None => {
                    if version() == 0 {
                        publish(store);
                    }
                }
                Some(b) if b != now => {
                    if narrowed(b.1, now.1) {
                        rotate();
                    }
                    publish(store);
                }
                Some(_) => {}
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Profile {
        let mut p = crate::store::seed_for_tests::seed().me;
        p.name = "Radu".into();
        p.about = "Building Songbird".into();
        p.status_line = Some("In a meeting".into());
        p
    }

    fn settings() -> (Settings, SettingsExt) {
        let s = crate::store::seed_for_tests::seed().settings;
        let x = SettingsExt {
            status_emoji: "📅".into(),
            status_expires_ms: 1_800_000_000_000,
            ..SettingsExt::default()
        };
        (s, x)
    }

    #[test]
    fn a_profile_seals_and_opens_with_its_status_and_expiry() {
        let (mut s, x) = settings();
        s.about = Audience::Contacts;
        s.status = Audience::Contacts;
        let p = plain(&me(), &s, &x);
        let key = [9u8; 32];
        let up = upload(&key, 3, &p).expect("sealed");
        assert_eq!(up.access_key, songbird_keys::sealed::access_key(&key));
        assert!(!up.open);
        let opened = content::open(&key, 3, &up.blob).expect("opens");
        assert_eq!(opened, p);
        assert_eq!(opened.about, "Building Songbird");
        let status = opened.status.as_ref().expect("status");
        assert_eq!(
            (status.text.as_str(), status.emoji.as_str()),
            ("In a meeting", "📅")
        );
        assert_eq!(status.expires_ms, 1_800_000_000_000);
        // Readers stop showing it at its expiry.
        assert!(opened.status_at(1_800_000_000_000).is_none());
        assert!(opened.status_at(1_799_999_999_999).is_some());
        // Another key or another version opens nothing.
        assert!(content::open(&[8; 32], 3, &up.blob).is_err());
        assert!(content::open(&key, 4, &up.blob).is_err());
    }

    #[test]
    fn fields_shared_with_nobody_are_left_out() {
        let (mut s, x) = settings();
        s.about = Audience::Nobody;
        s.status = Audience::Nobody;
        let p = plain(&me(), &s, &x);
        assert_eq!(p.name, "Radu");
        assert!(p.about.is_empty());
        assert!(p.status.is_none());
        let opened = content::open(&[1; 32], 1, &upload(&[1; 32], 1, &p).expect("sealed").blob)
            .expect("opens");
        assert!(opened.about.is_empty() && opened.status.is_none());
    }

    #[test]
    fn narrowing_to_nobody_rotates_and_widening_does_not() {
        use Audience::*;
        assert!(narrowed(
            (Contacts, Contacts, Contacts),
            (Contacts, Nobody, Contacts)
        ));
        assert!(narrowed(
            (Everyone, Contacts, Contacts),
            (Nobody, Contacts, Contacts)
        ));
        assert!(!narrowed(
            (Contacts, Nobody, Contacts),
            (Contacts, Contacts, Contacts)
        ));
        assert!(!narrowed(
            (Everyone, Contacts, Nobody),
            (Contacts, Contacts, Nobody)
        ));
    }

    #[test]
    fn long_fields_are_clipped_to_the_limits() {
        let (mut s, x) = settings();
        s.status = Audience::Contacts;
        let mut m = me();
        m.status_line = Some("ă".repeat(200));
        let p = plain(&m, &s, &x);
        assert!(p.validate().is_ok());
        assert!(p.status.expect("status").text.len() <= MAX_STATUS);
    }
}
