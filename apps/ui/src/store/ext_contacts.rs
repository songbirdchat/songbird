//! Prompt 9, M6: saved contacts (name, notes, Yahoo-style buddy-list
//! groups) and per-chat scenes, both encrypted records in the account's
//! record store (PROTOCOL §10.2), so they follow the user to every device.
//!
//! - A saved contact is `k/<16-byte id, hex>` with a JSON value
//!   `{name, username, notes, groups}`.
//! - A chat's scene is `c/<chat>/scene`, the installed Scene pack's id;
//!   no record means no scene.
//!
//! The records are the truth: intents write them, and the signals here are
//! rebuilt from them at start and whenever a sync brings another device's
//! edits ([`Store::reload_records`]).

use std::collections::HashMap;

use nsh_core::prelude::*;
use serde_json::{Value, json};

use super::{ChatId, Contact, Presence, Store, ext_groups::PackKind};
use crate::backend::records;

/// The buddy-list groups every account starts with.
pub const DEFAULT_GROUPS: [&str; 3] = ["Friends", "Family", "Work"];

/// A contact the user saved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedContact {
    /// 16 random bytes, hex: the record's key, never shown.
    pub id: String,
    pub name: String,
    /// Without the `@`; may be empty.
    pub username: String,
    pub notes: String,
    /// Buddy-list groups ("Friends", "Work"), in the user's order.
    pub groups: Vec<String>,
}

impl SavedContact {
    /// The record's logical key.
    pub fn key(&self) -> String {
        format!("k/{}", self.id)
    }

    /// The record's value.
    pub fn encode(&self) -> String {
        json!({
            "name": self.name,
            "username": self.username,
            "notes": self.notes,
            "groups": self.groups,
        })
        .to_string()
    }

    /// A record back into a contact; `None` for a key or value that isn't
    /// one (a newer app's record, or a damaged one).
    pub fn decode(key: &str, value: &str) -> Option<Self> {
        let id = key.strip_prefix("k/")?;
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v: Value = serde_json::from_str(value).ok()?;
        let name = v["name"].as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_ascii_lowercase(),
            name,
            username: v["username"].as_str().unwrap_or_default().to_string(),
            notes: v["notes"].as_str().unwrap_or_default().to_string(),
            groups: v["groups"]
                .as_array()
                .map(|g| {
                    g.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// Saved contacts A–Z (case ignored), the order the Contacts screen shows.
pub fn sorted_az(mut list: Vec<SavedContact>) -> Vec<SavedContact> {
    list.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    list
}

/// The address book, rebuilt from the records.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContactBook(pub Vec<SavedContact>);

/// Each chat's scene: the Scene pack's id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatScenes(pub HashMap<ChatId, u64>);

/// The per-chat record key: `<chat>` is the chat's ID, derived from the peer's account ID or
/// the group ID (`backend::chat::ui_id`), the same on each of the user's devices.
pub fn chat_key(chat: ChatId, name: &str) -> String {
    format!("c/{chat:032x}/{name}")
}

fn chat_of_key(key: &str, name: &str) -> Option<ChatId> {
    let rest = key.strip_prefix("c/")?.strip_suffix(name)?;
    let hex = rest.strip_suffix('/')?;
    u64::from_str_radix(hex, 16).ok()
}

/// A presence as the user may see it: WhatsApp's reciprocity, a user who
/// shares their last seen with nobody sees nobody's
/// (`songbird_content::presence::may_show_last_seen`).
pub fn presence_shown(p: &Presence, own_last_seen: super::Audience) -> Presence {
    use songbird_content::presence::{Audience, may_show_last_seen};
    let own = if own_last_seen == super::Audience::Nobody {
        Audience::Nobody
    } else {
        Audience::Contacts
    };
    match p {
        Presence::LastSeen(Some(_)) if !may_show_last_seen(own) => Presence::LastSeen(None),
        other => other.clone(),
    }
}

impl Store {
    /// The saved contacts, A–Z.
    pub fn book(&self) -> Signal<ContactBook> {
        global::<ContactBook>()
    }

    pub fn scenes(&self) -> Signal<ChatScenes> {
        global::<ChatScenes>()
    }

    /// Rebuilds saved contacts and chat scenes from the records (at start,
    /// and after a sync brought changes).
    pub fn reload_records(&self) {
        let book: Vec<SavedContact> = records::under("k/")
            .iter()
            .filter_map(|(k, v)| SavedContact::decode(k, v))
            .collect();
        let scenes: HashMap<ChatId, u64> = records::under("c/")
            .iter()
            .filter_map(|(k, v)| Some((chat_of_key(k, "scene")?, v.parse().ok()?)))
            .collect();
        self.book().set(ContactBook(sorted_az(book)));
        self.scenes().set(ChatScenes(scenes));
        // People the user muted nudges from, as the records say (D-0184).
        let muted: std::collections::HashSet<ChatId> = records::under("c/")
            .iter()
            .filter(|(_, v)| v == "off")
            .filter_map(|(k, _)| chat_of_key(k, "nudges"))
            .collect();
        self.contacts.update(|cs| {
            for c in cs.iter_mut().filter(|c| crate::backend::chat::owns(c.id)) {
                c.nudges_off = muted.contains(&c.id);
            }
        });
        // Real chats' list state another device wrote (D-0163), and their stars (D-0184).
        crate::backend::chat_state::reload(*self);
        crate::backend::chat::apply_stars(*self);
        // Groups the account joined on another device (D-0167).
        crate::backend::group_sync::join_missing(*self);
    }

    /// Saves a contact (a new one when its id is empty) and returns its id.
    pub fn save_contact(&self, mut c: SavedContact) -> String {
        c.name = c.name.trim().to_string();
        c.username = c.username.trim().trim_start_matches('@').to_string();
        c.notes = c.notes.trim().to_string();
        if c.id.is_empty() {
            c.id = crate::backend::wire::hex(&crate::backend::wire::random::<16>());
        }
        records::put(*self, &c.key(), Some(c.encode()));
        let id = c.id.clone();
        self.book().update(|b| {
            b.0.retain(|x| x.id != c.id);
            b.0.push(c);
            b.0 = sorted_az(std::mem::take(&mut b.0));
        });
        id
    }

    pub fn delete_saved_contact(&self, id: &str) {
        records::put(*self, &format!("k/{id}"), None);
        self.book().update(|b| b.0.retain(|x| x.id != id));
    }

    /// The saved contact for a username, if the user saved them.
    pub fn saved_by_username(&self, username: &str) -> Option<SavedContact> {
        let u = username.trim_start_matches('@');
        if u.is_empty() {
            return None;
        }
        self.book().with_untracked(|b| {
            b.0.iter()
                .find(|c| c.username.eq_ignore_ascii_case(u))
                .cloned()
        })
    }

    /// Saves a person from a chat (contact info, a shared contact).
    pub fn save_from(&self, name: &str, username: &str) -> String {
        match self.saved_by_username(username) {
            Some(c) => c.id,
            None => self.save_contact(SavedContact {
                name: name.into(),
                username: username.into(),
                ..SavedContact::default()
            }),
        }
    }

    /// Every buddy-list group in use, after the defaults.
    pub fn buddy_groups(&self) -> Vec<String> {
        let mut out: Vec<String> = DEFAULT_GROUPS.iter().map(|g| g.to_string()).collect();
        self.book().with(|b| {
            for c in &b.0 {
                for g in &c.groups {
                    if !out.iter().any(|x| x.eq_ignore_ascii_case(g)) {
                        out.push(g.clone());
                    }
                }
            }
        });
        out
    }

    /// The chat's scene (an installed Scene pack), or none.
    pub fn set_chat_scene(&self, chat: ChatId, pack: Option<u64>) {
        records::put(*self, &chat_key(chat, "scene"), pack.map(|p| p.to_string()));
        self.scenes().update(|m| match pack {
            Some(p) => {
                m.0.insert(chat, p);
            }
            None => {
                m.0.remove(&chat);
            }
        });
    }

    /// The chat's scene pack, while it is still installed.
    pub fn chat_scene(&self, chat: ChatId) -> Option<super::ext_groups::Pack> {
        let id = self.scenes().with(|m| m.0.get(&chat).copied())?;
        super::ext_groups::groups_state().with(|g| {
            g.packs
                .iter()
                .find(|p| p.id == id && p.kind == PackKind::Scenes)
                .cloned()
        })
    }

    /// Whether a sticker comes from an installed sound-sticker pack.
    pub fn is_sound_sticker(&self, pack_name: &str) -> bool {
        super::ext_groups::groups_state().with_untracked(|g| {
            g.packs
                .iter()
                .any(|p| p.kind == PackKind::SoundStickers && p.name == pack_name)
        })
    }

    /// A contact's presence as the user may see it (reciprocity).
    pub fn presence_of(&self, c: &Contact) -> Presence {
        presence_shown(&c.presence, self.settings.with(|x| x.last_seen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Audience;

    #[test]
    fn a_saved_contact_round_trips_and_bad_records_are_skipped() {
        let c = SavedContact {
            id: "00112233445566778899aabbccddeeff".into(),
            name: "Ana Popescu".into(),
            username: "ana".into(),
            notes: "Met at the climbing gym".into(),
            groups: vec!["Friends".into(), "Climbing".into()],
        };
        assert_eq!(SavedContact::decode(&c.key(), &c.encode()), Some(c.clone()));
        assert_eq!(SavedContact::decode("k/xyz", &c.encode()), None);
        assert_eq!(SavedContact::decode(&c.key(), "not json"), None);
        assert_eq!(SavedContact::decode(&c.key(), r#"{"name":"  "}"#), None);
        assert_eq!(SavedContact::decode("s/theme", &c.encode()), None);
    }

    #[test]
    fn contacts_sort_a_to_z_ignoring_case() {
        let c = |id: &str, name: &str| SavedContact {
            id: id.into(),
            name: name.into(),
            ..SavedContact::default()
        };
        let sorted = sorted_az(vec![c("1", "zoe"), c("2", "Ana"), c("3", "bogdan")]);
        let names: Vec<&str> = sorted.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Ana", "bogdan", "zoe"]);
    }

    #[test]
    fn chat_keys_round_trip() {
        let k = chat_key(42, "scene");
        assert_eq!(k, "c/0000000000000000000000000000002a/scene");
        assert_eq!(chat_of_key(&k, "scene"), Some(42));
        assert_eq!(chat_of_key(&k, "wallpaper"), None);
    }

    #[test]
    fn last_seen_is_hidden_when_mine_is_shared_with_nobody() {
        let seen = Presence::LastSeen(Some(-30));
        assert_eq!(presence_shown(&seen, Audience::Contacts), seen);
        assert_eq!(presence_shown(&seen, Audience::Everyone), seen);
        assert_eq!(
            presence_shown(&seen, Audience::Nobody),
            Presence::LastSeen(None)
        );
        // Online, away and busy are not last seen: they still show.
        assert_eq!(
            presence_shown(&Presence::Online, Audience::Nobody),
            Presence::Online
        );
    }
}
