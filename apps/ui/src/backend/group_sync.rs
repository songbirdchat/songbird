//! The account's groups follow it to its new devices (PROTOCOL §6.3 rule 5, §10.2; D-0167).
//!
//! Every device keeps an encrypted record `g/<group ID hex>` for each group the account is in
//! (the records server sees opaque records only). A device that finds a record for a group it
//! isn't in joins by external commit, which the server allows only to a listed device of an
//! account in the group and every member checks against that account's signed device list.
//! Leaving or being removed deletes the record.

use std::cell::Cell;

use nsh_core::prelude::*;

use super::{chat, records, wire};
use crate::store::Store;

thread_local! {
    static JOINING: Cell<bool> = const { Cell::new(false) };
}

fn key(gid: &[u8; 16]) -> String {
    format!("g/{}", wire::hex(gid))
}

/// Records that the account is in `gid` (or no longer is).
pub fn mark(s: Store, gid: &[u8; 16], in_it: bool) {
    let k = key(gid);
    let have = records::under("g/").into_iter().any(|(x, _)| x == k);
    if in_it != have {
        records::put(s, &k, in_it.then(|| "1".to_owned()));
    }
}

/// Records every group this device is in (at start, after the chats show).
pub fn publish(s: Store, m: &chat::Core) {
    for c in m.store.chats().unwrap_or_default() {
        if c.group && m.in_group(&c.id).unwrap_or(false) {
            mark(s, &c.id, true);
        }
    }
}

/// Joins the account's groups this device isn't in yet, one at a time, and shows them.
pub fn join_missing(s: Store) {
    let wanted: Vec<[u8; 16]> = records::under("g/")
        .into_iter()
        .filter_map(|(k, _)| wire::unhex(k.strip_prefix("g/")?)?.try_into().ok())
        .collect();
    if wanted.is_empty() || JOINING.replace(true) {
        return;
    }
    Owner::detached().run(|| {
        spawn(async move {
            let lock = chat::core();
            let mut joined = false;
            {
                let mut guard = lock.lock().await;
                if let Some(m) = guard.as_mut() {
                    for gid in wanted {
                        if m.in_group(&gid).unwrap_or(true) {
                            continue;
                        }
                        match m.join_own_group(&gid).await {
                            Ok(()) => joined = true,
                            Err(e) => log::info!("group join: {e:?}"),
                        }
                    }
                }
            }
            JOINING.set(false);
            if joined {
                chat::reload_all(s).await;
            }
        });
    });
}
