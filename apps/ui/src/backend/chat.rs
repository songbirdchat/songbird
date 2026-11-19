//! Real 1:1 chats through app-core (A2 M4, D-0139).
//!
//! When the real backend is on and this device is registered, app-core
//! (`crates/app-core`) owns the conversation: it holds this device's
//! identity, prekeys and sessions and every message in its SQLCipher store,
//! encrypts what the user sends and decrypts what arrives. This module is
//! the seam between it and the UI store:
//!
//! - **IDs.** The UI counts in `u64`; accounts and messages are 16 random
//!   bytes. A real contact's (and its direct chat's) ID is the first 8 bytes
//!   of the account with the top bit set, a real message's the first 8 of its
//!   ID likewise, so they never meet the mock's small numbers and need no
//!   table on disk. Messages the user writes keep the ID the UI gave them,
//!   mapped to app-core's for the receipts that follow.
//! - **Lifecycle.** [`start`] opens the store (first run: publishes keys,
//!   the signed device list and the profile), retries the outbox and shows
//!   every stored chat; [`receive`] runs for each delivery, in order, before
//!   the delivery client acknowledges it.
//! - **Time.** The UI's clock is minutes from its mock "now" (0); real
//!   messages are placed by their minutes before or after the real now.
//!
//! mock: the store key and the device identity seed live in NSH app storage
//! like the backend's other keys, until the platform keystore modules exist
//! (A0 "On-device security"); the SQLCipher file itself is encrypted.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nsh_core::{prelude::*, storage::Storage, time::unix_ms};
use songbird_app_core::{
    crypto::X25519Pair,
    messenger::{DeviceIdBytes, Messenger, Net, Received},
    session::Local,
    store::{MessageRow, Status, Store as CoreStore},
};
use songbird_proto::frame::{Recipient, SendHints, SendStatus};
use songbird_types::DeviceId;

use super::{live, wire};
use crate::store::{Body, ChatId, ChatKind, Contact, ContactId, MsgId, Presence, Store};
use crate::ui::{Tick, toast};

const K_DB_KEY: &str = "sb.core.db_key";
const K_DEVICE_IDENTITY: &str = "sb.core.device_identity";
const DB_FILE: &str = "songbird-core.db";

type Core = Messenger<AppNet>;

thread_local! {
    /// The core, behind an async lock: one operation at a time, held across
    /// the network round trips inside it (NSH runs tasks on one thread).
    static CORE: Rc<tokio::sync::Mutex<Option<Core>>> = Rc::new(tokio::sync::Mutex::new(None));
    /// Real accounts by UI contact ID, and app-core message IDs by UI ID.
    static ACCOUNTS: RefCell<HashMap<ContactId, [u8; 16]>> = RefCell::new(HashMap::new());
    static OUTGOING: RefCell<HashMap<MsgId, [u8; 16]>> = RefCell::new(HashMap::new());
}

fn core() -> Rc<tokio::sync::Mutex<Option<Core>>> {
    CORE.with(Rc::clone)
}

/// A real account's UI contact (and direct chat) ID.
pub fn ui_id(bytes: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[..8]);
    u64::from_be_bytes(b) | (1 << 63)
}

/// Whether `chat` is a real chat app-core owns.
pub fn owns(chat: ChatId) -> bool {
    ACCOUNTS.with_borrow(|a| a.contains_key(&chat))
}

fn account_of(chat: ChatId) -> Option<[u8; 16]> {
    ACCOUNTS.with_borrow(|a| a.get(&chat).copied())
}

/// The network as app-core needs it: the api signed by this device, and the
/// delivery client.
#[derive(Debug)]
pub struct AppNet;

impl Net for AppNet {
    async fn api(
        &self,
        method: &str,
        path: &str,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<(u16, Vec<u8>)> {
        let has_body = method != "GET" && method != "DELETE";
        let json = body.first() == Some(&b'{');
        super::signed(method, path, has_body.then_some(body), json)
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn send(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        recipients: Vec<(DeviceIdBytes, Vec<u8>)>,
    ) -> songbird_app_core::Result<SendStatus> {
        let client = super::ws::client().ok_or(songbird_app_core::Error::Network)?;
        let r = recipients
            .into_iter()
            .map(|(d, e)| Recipient {
                device: DeviceId::from_bytes(d),
                envelope: e.into(),
            })
            .collect();
        client
            .send_hinted(idempotency_key, hints, r)
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }
}

fn stored_seed(key: &str) -> [u8; 32] {
    if let Some(s) = Storage::get_item(key)
        .and_then(|h| wire::unhex(&h))
        .and_then(|v| <[u8; 32]>::try_from(v).ok())
    {
        return s;
    }
    let s: [u8; 32] = wire::random();
    if let Err(e) = Storage::set_item(key, wire::hex(&s)) {
        log::warn!("core storage: {e}");
    }
    s
}

fn hex16(s: &str) -> Option<[u8; 16]> {
    wire::unhex(s)?.try_into().ok()
}

/// Opens app-core for this device (once per connection), publishing its keys,
/// device list and profile on the first run. Shows every stored chat.
pub async fn start() -> Result<(), super::Problem> {
    let lock = core();
    let mut guard = lock.lock().await;
    if guard.is_some() {
        return Ok(());
    }
    let (Some(account), Some(device)) = live().with_untracked(|l| {
        (
            l.account.as_deref().and_then(hex16),
            l.device.as_deref().and_then(hex16),
        )
    }) else {
        return Ok(());
    };
    let path = data_dir()
        .ok_or_else(|| super::problem("This device has no place to keep messages."))?
        .join(DB_FILE);
    let db_key = stored_seed(K_DB_KEY);
    let store = CoreStore::open(&path, &db_key)
        .map_err(|_| super::problem("Your messages on this device couldn't be opened."))?;
    // The profile key the app publishes (`profile`) is the one app-core shares.
    store
        .put_secret("profile_key", &super::profile::profile_key())
        .map_err(|_| super::problem("Your messages on this device couldn't be opened."))?;
    let first = store.local().ok().flatten().is_none();
    let mut m = if first {
        let identity = X25519Pair::from_secret(&stored_seed(K_DEVICE_IDENTITY))
            .map_err(|_| super::problem("This device's key is damaged."))?;
        Messenger::create(
            store,
            AppNet,
            Local {
                account,
                device,
                identity,
            },
            None,
        )
    } else {
        Messenger::open(store, AppNet, None)
    }
    .map_err(|_| super::problem("Your messages on this device couldn't be opened."))?;
    if first {
        m.publish_keys(true)
            .await
            .map_err(|_| super::problem("This device's keys couldn't be published."))?;
        // The primary signs the account's device list with the account key.
        if let Some(key) = Storage::get_item("sb.identity_seed")
            .and_then(|h| wire::unhex(&h))
            .and_then(|v| <[u8; 32]>::try_from(v).ok())
            .and_then(|s| wire::ed25519(&s))
        {
            let _ = m.publish_device_list(&key, |_, _| false).await;
        }
        if let Some(s) = crate::store::try_store() {
            super::profile::publish(s);
        }
    } else {
        let _ = m.refill_keys().await;
    }
    let _ = m.flush_outbox().await;
    if let Some(s) = crate::store::try_store() {
        show_all(s, &m);
    }
    *guard = Some(m);
    Ok(())
}

/// Closes app-core (disconnect, forget).
pub fn stop() {
    let lock = core();
    if let Ok(mut g) = lock.try_lock() {
        *g = None;
    }
    ACCOUNTS.with_borrow_mut(HashMap::clear);
    OUTGOING.with_borrow_mut(HashMap::clear);
}

fn minutes(ms: u64) -> i64 {
    let now = unix_ms() as i64;
    (ms as i64 - now).div_euclid(60_000)
}

fn tick(s: Status) -> Tick {
    match s {
        Status::Failed => Tick::Failed,
        Status::Sending => Tick::Sending,
        Status::Sent => Tick::Sent,
        Status::Delivered => Tick::Delivered,
        Status::Read => Tick::Read,
        Status::Played => Tick::Played,
    }
}

/// The UI contact and chat for a real account, created on first sight.
fn ensure_chat(s: Store, account: [u8; 16], name: Option<&str>) -> ChatId {
    let id = ui_id(&account);
    ACCOUNTS.with_borrow_mut(|a| a.insert(id, account));
    if s.contact(id).is_none() {
        let hex = wire::hex(&account);
        let title = name.map_or_else(|| format!("Songbird user {}", &hex[..6]), str::to_owned);
        s.contacts.update(|cs| {
            cs.push(Contact {
                id,
                name: title.clone(),
                username: name.unwrap_or_default().to_owned(),
                about: String::new(),
                status_line: None,
                presence: Presence::LastSeen(None),
                verified: false,
                key_changed: false,
                safety_number: String::new(),
                blocked: false,
                nudges_off: false,
            });
        });
    }
    if s.chat(id).is_none() {
        let title = s.contact_name(Some(id));
        s.chats.update(|cs| {
            cs.push(crate::store::seed::chat(id, ChatKind::Direct(id), &title));
        });
    }
    id
}

fn show(s: Store, me: &[u8; 16], row: &MessageRow, count_unread: bool) {
    let chat = ensure_chat(s, row.chat, None);
    let mine = row.author == *me;
    let id = ui_id(&row.id);
    if s.message(chat, id).is_some()
        || OUTGOING.with_borrow(|o| o.iter().any(|(_, v)| *v == row.id))
    {
        return;
    }
    let from = (!mine).then_some(chat);
    s.messages.update(|m| {
        m.entry(chat).or_default().push(crate::store::Message {
            id,
            chat,
            from,
            body: Body::Text(row.text.clone()),
            at: minutes(row.sent_ms),
            status: tick(row.status),
            reply_to: None,
            reactions: Vec::new(),
            edited: false,
            starred: false,
            pinned: false,
            forwarded: false,
            receipts: Vec::new(),
        });
    });
    if !mine && count_unread {
        s.edit_chat(chat, |c| c.unread += 1);
        let sender = s.contact(chat).map(|c| c.name);
        s.arrive(crate::store::Arrival::Message {
            chat,
            sender,
            preview: row.text.clone(),
        });
    }
}

fn show_all(s: Store, m: &Core) {
    let me = m.local().account;
    for c in m.store.chats().unwrap_or_default() {
        let chat = ensure_chat(s, c.id, None);
        for row in m.store.messages(&c.id, None, 200).unwrap_or_default() {
            show(s, &me, &row, false);
        }
        s.edit_chat(chat, |x| x.unread = c.unread);
    }
}

/// Handles one delivered envelope: decrypt, store, show. Answers whether the
/// caller may acknowledge it (everything was stored).
pub async fn receive(envelope: &[u8]) -> bool {
    let lock = core();
    let mut guard = lock.lock().await;
    let Some(m) = guard.as_mut() else {
        return false;
    };
    let me = m.local().account;
    match m.receive(envelope).await {
        Ok(Received::Message(row)) => {
            if let Some(s) = crate::store::try_store() {
                show(s, &me, &row, row.author != me);
            }
            true
        }
        Ok(Received::Status { id, status }) => {
            if let Some(s) = crate::store::try_store() {
                set_status(s, &id, status);
            }
            true
        }
        Ok(_) => true,
        // A store failure: leave it on the server, it comes again.
        Err(_) => false,
    }
}

fn set_status(s: Store, core_id: &[u8; 16], status: Status) {
    let ui = OUTGOING
        .with_borrow(|o| o.iter().find(|(_, v)| *v == core_id).map(|(k, _)| *k))
        .unwrap_or_else(|| ui_id(core_id));
    let chats: Vec<ChatId> = ACCOUNTS.with_borrow(|a| a.keys().copied().collect());
    for chat in chats {
        if s.message(chat, ui).is_some() {
            if status == Status::Failed {
                s.fail(chat, ui);
            } else {
                s.advance(chat, ui, tick(status));
            }
        }
    }
}

/// Sends what the user wrote in a real chat (`ui_id` is the message the UI
/// already shows as Sending).
pub fn send(s: Store, chat: ChatId, ui_msg: MsgId, body: &Body) {
    let Some(to) = account_of(chat) else { return };
    let text = match body {
        Body::Text(t) => t.clone(),
        Body::Link { text, .. } => text.clone(),
        // Attachments, polls and the rest come with A2's later milestones.
        _ => {
            s.fail(chat, ui_msg);
            toast("This kind of message can't be sent in this chat yet.");
            return;
        }
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        match m.send_text(&to, &text).await {
            Ok(id) => {
                OUTGOING.with_borrow_mut(|o| o.insert(ui_msg, id));
                let status = m
                    .store
                    .message(&m.local().account, &id)
                    .ok()
                    .flatten()
                    .map_or(Status::Sending, |r| r.status);
                drop(guard);
                set_status(s, &id, status);
                Ok(())
            }
            Err(e) => {
                drop(guard);
                s.fail(chat, ui_msg);
                Err(super::problem(match e {
                    songbird_app_core::Error::IdentityChanged => {
                        "This person's safety number changed. Check it before sending."
                    }
                    songbird_app_core::Error::Network => {
                        "Can't reach the server. The message will be sent when you're back online."
                    }
                    _ => "The message couldn't be sent.",
                }))
            }
        }
    });
}

/// Tells the authors of a real chat's unread messages that they were read.
pub fn mark_read(chat: ChatId) {
    let Some(account) = account_of(chat) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            let _ = m.mark_read(&account).await;
        }
        Ok(())
    });
}

/// Starts (or opens) a chat with the account that holds `username`
/// (`POST /v1/username/lookup`: only the username's hash leaves the device).
pub async fn start_with_username(username: &str) -> Result<ChatId, super::Problem> {
    let name = songbird_keys::username::Username::parse(username.trim().trim_start_matches('@'))
        .map_err(|_| super::problem("That isn't a username."))?;
    let body = serde_json::json!({ "hash": wire::hex(&name.hash()) })
        .to_string()
        .into_bytes();
    let (status, answer) = super::signed("POST", "/v1/username/lookup", Some(body), true).await?;
    if status == 404 {
        return Err(super::problem("No one on Songbird has that username."));
    }
    let v = super::ok_json(status, &answer)?;
    let account = v["account"]
        .as_str()
        .and_then(hex16)
        .ok_or_else(|| super::problem("The server's answer was incomplete."))?;
    if live().with_untracked(|l| l.account.as_deref().and_then(hex16)) == Some(account) {
        return Err(super::problem("That's you."));
    }
    // Resolve their devices now, so a changed or missing identity shows here.
    {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            m.devices_of(&account, false).await.map_err(|e| {
                super::problem(match e {
                    songbird_app_core::Error::IdentityChanged => {
                        "This person's safety number changed."
                    }
                    _ => "Their devices couldn't be checked. Try again.",
                })
            })?;
        }
    }
    let s = crate::store::store();
    Ok(ensure_chat(
        s,
        account,
        Some(username.trim().trim_start_matches('@')),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_ids_never_meet_the_mocks() {
        let a = [0u8; 16];
        let b = [0xFFu8; 16];
        assert!(ui_id(&a) >= 1 << 63 && ui_id(&b) >= 1 << 63);
        assert_ne!(ui_id(&[1; 16]), ui_id(&[2; 16]));
        // The mock's ids are small counters and seeds.
        assert!(ui_id(&a) > 1_000_000_000);
    }

    #[test]
    fn statuses_map_to_ticks() {
        assert_eq!(tick(Status::Sending), Tick::Sending);
        assert_eq!(tick(Status::Read), Tick::Read);
        assert_eq!(tick(Status::Failed), Tick::Failed);
    }
}
