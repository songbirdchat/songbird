//! Chat-list state that follows the user across devices (PROTOCOL §10.2, D-0163): each real
//! chat's archived, pinned, muted-until, marked-unread and favourite state, its theme (D-0184),
//! and the custom lists it is in, as one encrypted record `c/<account or group, hex>/state`,
//! last writer wins.
//!
//! A change on this device is written when the chat list changes (an effect compares each real
//! chat's state with what was last written); a change from another device arrives with a sync
//! and is applied to the chats here. Locked chats stay this device's own (a lock is about who
//! holds this device).

use std::{cell::RefCell, collections::HashMap};

use nsh_core::prelude::*;

use super::records;
use crate::store::{Chat, ChatId, Store};

thread_local! {
    /// What was last written or applied, per record key.
    static LAST: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// A chat's state as its record holds it.
#[must_use]
pub fn encode(c: &Chat) -> String {
    let mut lists = c.lists.clone();
    lists.sort();
    let theme = crate::store::ChatTheme::ALL
        .iter()
        .position(|t| *t == c.theme)
        .unwrap_or(0);
    format!(
        "a={};p={};m={};u={};f={};t={};l={}",
        u8::from(c.archived),
        u8::from(c.pinned),
        if c.muted { c.muted_until } else { 0 },
        u8::from(c.marked_unread),
        u8::from(c.favourite),
        theme,
        lists
            .iter()
            .map(|l| l.replace([';', ','], " "))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Applies a record's state to a chat; unknown or bad fields are left alone.
pub fn apply(v: &str, c: &mut Chat) {
    for part in v.split(';') {
        let Some((k, val)) = part.split_once('=') else {
            continue;
        };
        let flag = val == "1";
        match k {
            "a" => c.archived = flag,
            "p" => c.pinned = flag,
            "u" => c.marked_unread = flag,
            "f" => c.favourite = flag,
            "m" => {
                if let Ok(until) = val.parse::<u64>() {
                    c.muted = until > 0;
                    c.muted_until = until;
                }
            }
            // The chat's theme (this user's view of it only; the others never see it).
            "t" => {
                if let Some(t) = val
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| crate::store::ChatTheme::ALL.get(i))
                {
                    c.theme = *t;
                }
            }
            "l" => {
                c.lists = val
                    .split(',')
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
}

fn key_of(chat: ChatId) -> Option<String> {
    super::chat::core_chat(chat).map(|id| format!("c/{}/state", super::wire::hex(&id)))
}

/// Writes the state of every real chat whose state changed since it was last written.
fn write_changes(s: Store) {
    let states: Vec<(String, String)> = s.chats.with_untracked(|cs| {
        cs.iter()
            .filter(|c| super::chat::owns(c.id))
            .filter_map(|c| Some((key_of(c.id)?, encode(c))))
            .collect()
    });
    if states.is_empty() {
        return;
    }
    // A chat this device has not written yet takes the synced state first (a newer device's
    // choice is not overwritten by this one's defaults); `reload` applies it.
    let synced: HashMap<String, String> = records::under("c/").into_iter().collect();
    let mut pending = false;
    for (k, v) in states {
        let unseen = LAST.with_borrow(|l| !l.contains_key(&k));
        if let Some(remote) = synced.get(&k).filter(|_| unseen) {
            if *remote == v {
                LAST.with_borrow_mut(|l| l.insert(k, v));
            } else {
                pending = true;
            }
            continue;
        }
        let fresh = LAST.with_borrow(|l| l.get(&k) != Some(&v));
        if fresh {
            LAST.with_borrow_mut(|l| l.insert(k.clone(), v.clone()));
            records::put(s, &k, Some(v));
        }
    }
    if pending {
        // After this turn: the chat list is being changed right now.
        set_timeout(std::time::Duration::ZERO, move || reload(s));
    }
}

/// Applies the records another device wrote (at start and after a sync).
pub fn reload(s: Store) {
    let records: HashMap<String, String> = records::under("c/")
        .into_iter()
        .filter(|(k, _)| k.ends_with("/state"))
        .collect();
    if records.is_empty() {
        return;
    }
    let ids: Vec<(ChatId, String)> = s.chats.with_untracked(|cs| {
        cs.iter()
            .filter(|c| super::chat::owns(c.id))
            .filter_map(|c| Some((c.id, key_of(c.id)?)))
            .collect()
    });
    let mut changes = Vec::new();
    for (id, k) in ids {
        if let Some(v) = records.get(&k) {
            let seen = LAST.with_borrow(|l| l.get(&k) == Some(v));
            if !seen {
                LAST.with_borrow_mut(|l| l.insert(k.clone(), v.clone()));
                changes.push((id, v.clone()));
            }
        }
    }
    if !changes.is_empty() {
        s.chats.update(|cs| {
            for (id, v) in &changes {
                if let Some(c) = cs.iter_mut().find(|c| c.id == *id) {
                    apply(v, c);
                }
            }
        });
    }
}

/// Follows the chat list: real chats' state is written as it changes. Called once at the root.
pub fn install(s: Store) {
    effect(move || {
        s.chats.with(|_| ());
        untracked(|| write_changes(s));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_ignores_what_it_does_not_know() {
        let mut c = crate::store::seed::chat(1, crate::store::ChatKind::Group, "x");
        c.archived = true;
        c.muted = true;
        c.muted_until = 99;
        c.lists = vec!["Work".into(), "Family".into()];
        c.theme = crate::store::ChatTheme::Moss;
        let v = encode(&c);
        let mut d = crate::store::seed::chat(1, crate::store::ChatKind::Group, "x");
        apply(&format!("{v};z=7"), &mut d);
        assert_eq!(d.theme, crate::store::ChatTheme::Moss);
        assert_eq!(
            (d.archived, d.muted, d.muted_until, d.lists.clone()),
            (
                true,
                true,
                99,
                vec!["Family".to_string(), "Work".to_string()]
            )
        );
        apply("m=0", &mut d);
        assert!(!d.muted);
    }
}
