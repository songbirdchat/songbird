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

use super::vault::Storage;
use nsh_core::{prelude::*, time::unix_ms};
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

pub(super) type Core = Messenger<AppNet>;

thread_local! {
    /// The core, behind an async lock: one operation at a time, held across
    /// the network round trips inside it (NSH runs tasks on one thread).
    static CORE: Rc<tokio::sync::Mutex<Option<Core>>> = Rc::new(tokio::sync::Mutex::new(None));
    /// Real accounts by UI contact ID, and app-core message IDs by UI ID.
    static ACCOUNTS: RefCell<HashMap<ContactId, [u8; 16]>> = RefCell::new(HashMap::new());
    static OUTGOING: RefCell<HashMap<MsgId, [u8; 16]>> = RefCell::new(HashMap::new());
    /// Every real message shown: UI message → (author, app-core message ID).
    static IDS: RefCell<HashMap<MsgId, ([u8; 16], [u8; 16])>> = RefCell::new(HashMap::new());
    /// Real media messages: UI message → (author, app-core message ID).
    static MEDIA: RefCell<HashMap<MsgId, ([u8; 16], [u8; 16])>> = RefCell::new(HashMap::new());
    /// Accounts whose profile was fetched this run.
    static PROFILED: RefCell<std::collections::HashSet<[u8; 16]>> = RefCell::new(std::collections::HashSet::new());
}

/// Fetches a contact's encrypted profile (`POST /v1/profile/fetch`) and opens it with the
/// profile key they shared: their real name, about, status and photo.
pub async fn refresh_profile(account: [u8; 16], key: [u8; 32]) {
    PROFILED.with_borrow_mut(|p| p.insert(account));
    let body = serde_json::json!({ "account": wire::hex(&account) })
        .to_string()
        .into_bytes();
    let Ok((200, answer)) = super::signed("POST", "/v1/profile/fetch", Some(body), true).await
    else {
        return;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&answer) else {
        return;
    };
    let (Some(version), Some(blob)) = (
        v["version"].as_u64(),
        v["profile"].as_str().and_then(wire::unhex),
    ) else {
        return;
    };
    let Ok(p) = songbird_content::profile::open(&key, version, &blob) else {
        return;
    };
    let id = ui_id(&account);
    if let Some(s) = crate::store::try_store() {
        let name = p.name.trim().to_owned();
        s.contacts.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == id) {
                if !name.is_empty() {
                    c.name.clone_from(&name);
                }
                c.about.clone_from(&p.about);
                c.status_line = p
                    .status
                    .as_ref()
                    .map(|st| st.text.clone())
                    .filter(|t| !t.is_empty());
            }
        });
        if !name.is_empty() {
            s.edit_chat(id, |c| c.title.clone_from(&name));
        }
    }
    if let Some(a) = &p.avatar {
        let _ = super::avatar::fetch(id, &a.id, &a.key).await;
    }
}

pub(crate) fn core() -> Rc<tokio::sync::Mutex<Option<Core>>> {
    CORE.with(Rc::clone)
}

/// A real account's UI contact (and direct chat) ID.
pub fn ui_id(bytes: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[..8]);
    u64::from_be_bytes(b) | (1 << 63)
}

thread_local! {
    /// Whether presence's contacts are read again periodically.
    static PRESENCE_TIMER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NOTICING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether the disappearing-message sweep runs.
    static SWEEPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Real groups by UI chat ID (D-0146).
    static GROUPS: RefCell<HashMap<ChatId, [u8; 16]>> = RefCell::new(HashMap::new());
}

/// Whether `chat` is a real chat (1:1 or group) app-core owns.
pub fn owns(chat: ChatId) -> bool {
    ACCOUNTS.with_borrow(|a| a.contains_key(&chat)) || GROUPS.with_borrow(|g| g.contains_key(&chat))
}

pub(super) fn group_of(chat: ChatId) -> Option<[u8; 16]> {
    GROUPS.with_borrow(|g| g.get(&chat).copied())
}

/// Whether every one of `contacts` is a real account (a group can be made of them).
pub fn all_real(contacts: &[ContactId]) -> bool {
    !contacts.is_empty()
        && contacts
            .iter()
            .all(|c| ACCOUNTS.with_borrow(|a| a.contains_key(c)))
}

/// The UI chat for a real group, created on first sight.
fn ensure_group(s: Store, gid: [u8; 16], title: &str) -> ChatId {
    let id = ui_id(&gid);
    GROUPS.with_borrow_mut(|g| g.insert(id, gid));
    if s.chat(id).is_none() {
        let name = if title.is_empty() {
            "Group".to_string()
        } else {
            title.to_owned()
        };
        s.chats.update(|cs| {
            cs.push(crate::store::seed::chat(id, ChatKind::Group, &name));
        });
    } else if !title.is_empty() {
        s.edit_chat(id, |c| c.title = title.to_owned());
    }
    id
}

/// Waits `ms` milliseconds (on the UI's timers, so no runtime timer is needed).
pub(super) async fn pause(ms: u64) {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    set_timeout(std::time::Duration::from_millis(ms), move || {
        let _ = tx.send(());
    });
    let _ = rx.await;
}

/// Whether an operation that failed this way may succeed if tried again: another commit came
/// first (409, the group moved on), a budget ran out (429), the server was busy (503), or the
/// network dropped.
fn worth_retrying(e: &songbird_app_core::Error) -> bool {
    matches!(
        e,
        songbird_app_core::Error::Server(409 | 429 | 503) | songbird_app_core::Error::Network
    )
}

/// A group operation in app-core, its failure shown in plain words. Tried up to four times,
/// 2, 4 and 8 seconds apart, when the failure may pass: the lock is let go in between, so the
/// commit that came first is received and the next try starts from it.
fn group_op<F>(chat: ChatId, failed: &'static str, op: F)
where
    F: for<'a> Fn(
            &'a mut Core,
            [u8; 16],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = songbird_app_core::Result<()>> + 'a>,
        > + 'static,
{
    let Some(gid) = group_of(chat) else { return };
    super::run(async move {
        let mut wait = 2_000;
        for attempt in 0..4 {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            match op(m, gid).await {
                Ok(()) => {
                    // This device's own commit: a call in the group moves to the new keys too.
                    super::calls::group_changed(m, &gid);
                    return Ok(());
                }
                Err(e) if attempt < 3 && worth_retrying(&e) => {
                    drop(guard);
                    pause(wait).await;
                    wait *= 2;
                }
                Err(_) => return Err(super::problem(failed)),
            }
        }
        Err(super::problem(failed))
    });
}

/// Leaves a real group.
pub fn leave_group(chat: ChatId) {
    group_op(chat, "Leaving the group failed. Try again.", |m, gid| {
        Box::pin(async move {
            m.leave_group(&gid).await?;
            if let Some(s) = crate::store::try_store() {
                super::group_sync::mark(s, &gid, false);
            }
            Ok(())
        })
    });
}

/// Adds real contacts to a real group.
pub fn add_to_group(chat: ChatId, contacts: Vec<ContactId>) {
    let accounts: Vec<[u8; 16]> = contacts.iter().filter_map(|c| account_of(*c)).collect();
    group_op(chat, "They couldn't be added. Try again.", move |m, gid| {
        let accounts = accounts.clone();
        Box::pin(async move { m.add_members(&gid, &accounts).await })
    });
}

/// Removes a real contact from a real group (admins; the server checks the role).
pub fn remove_from_group(chat: ChatId, contact: ContactId) {
    let Some(account) = account_of(contact) else {
        return;
    };
    group_op(
        chat,
        "They couldn't be removed. Only admins can remove people.",
        move |m, gid| Box::pin(async move { m.remove_members(&gid, &[account]).await }),
    );
}

/// The invite link of a real group: this device's, else a new one (the proof key committed
/// into the roles first, D-0148). Shown in the chat once made.
pub fn ensure_link(s: Store, chat: ChatId, fresh: bool) {
    let Some(gid) = group_of(chat) else { return };
    let approval = s.chat(chat).is_some_and(|c| c.permissions.approve_joins);
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let current = m.group_link(&gid).ok().flatten().filter(|l| !l.is_empty());
        let url = match (current, fresh) {
            (Some(url), false) => url,
            (old, _) => {
                if let Some(old) = old {
                    let _ = m.revoke_link(&gid, &old).await;
                }
                m.create_link(&gid, approval).await.map_err(|_| {
                    super::problem("The link couldn't be made. Only admins can manage it.")
                })?
            }
        };
        drop(guard);
        s.edit_chat(chat, |c| c.invite_link = url);
        Ok(())
    });
}

/// Loads a real group's pending join requests into its chat (admins).
pub fn refresh_requests(s: Store, chat: ChatId) {
    let Some(gid) = group_of(chat) else { return };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let waiting = m.join_requests(&gid).await.unwrap_or_default();
        drop(guard);
        let ids: Vec<ContactId> = waiting
            .iter()
            .map(|(a, _)| ensure_chat(s, *a, None))
            .collect();
        s.edit_chat(chat, |c| c.join_requests = ids);
        Ok(())
    });
}

/// Approves (adds) or declines a request to join a real group.
pub fn answer_request(chat: ChatId, contact: ContactId, approve: bool) {
    let Some(account) = account_of(contact) else {
        return;
    };
    group_op(
        chat,
        "The request couldn't be answered. Try again.",
        move |m, gid| {
            Box::pin(async move {
                if approve {
                    m.approve_request(&gid, &account).await
                } else {
                    m.decline_request(&gid, &account).await
                }
            })
        },
    );
}

/// Opens a group invite link someone shared: joins, or asks to join.
pub async fn open_group_link(url: &str) -> Result<Option<ChatId>, super::Problem> {
    let lock = core();
    let mut guard = lock.lock().await;
    let m = guard
        .as_mut()
        .ok_or_else(|| super::problem("Not connected yet."))?;
    match m.open_link(url).await {
        Ok(songbird_app_core::mls::links::Opened::Joined(gid)) => {
            let title = m.group_title(&gid).unwrap_or_default();
            drop(guard);
            let s = crate::store::store();
            let chat = ensure_group(s, gid, &title);
            s.push_message(
                chat,
                None,
                Body::System("You joined with an invite link".into()),
            );
            Ok(Some(chat))
        }
        Ok(songbird_app_core::mls::links::Opened::Requested) => {
            toast("Request sent. An admin will let you in.");
            Ok(None)
        }
        Err(_) => Err(super::problem("This invite link doesn't work any more.")),
    }
}

/// A real community's UI ID (its announcement group keeps the group's own).
fn community_ui(cid: &[u8; 16]) -> ChatId {
    ui_id(cid) ^ 1
}

/// Gives a real group's chat its kind: a channel (admin or subscriber), or a community's
/// announcement group (with the community and its groups), from the group's roles (D-0149).
fn apply_kind(
    s: Store,
    chat: ChatId,
    gid: [u8; 16],
    title: &str,
    kind: Option<(bool, bool, songbird_app_core::mls::Role)>,
    community: Option<&[[u8; 16]]>,
) {
    use songbird_app_core::mls::Role as R;
    match kind {
        Some((true, _, role)) => {
            s.edit_chat(chat, |c| {
                c.kind = ChatKind::Channel {
                    followers: 1,
                    admin: role >= R::Admin,
                };
                c.announcement_only = role < R::Admin;
            });
        }
        Some((false, true, role)) => {
            let cid = community_ui(&gid);
            if s.chat(cid).is_none() {
                let mut c = crate::store::seed::chat(cid, ChatKind::Community, title);
                c.members = vec![crate::store::Member {
                    contact: None,
                    role: if role >= R::Owner {
                        crate::store::Role::Owner
                    } else if role >= R::Admin {
                        crate::store::Role::Admin
                    } else {
                        crate::store::Role::Member
                    },
                }];
                s.chats.update(|cs| cs.push(c));
            }
            s.edit_chat(chat, |c| {
                c.community = Some(cid);
                c.announcement_only = role < R::Admin;
                c.title = "Announcements".into();
            });
            for g in community.unwrap_or_default() {
                let gc = ui_id(g);
                if s.chat(gc).is_some() {
                    s.edit_chat(gc, |c| c.community = Some(cid));
                }
            }
        }
        _ => {}
    }
}

/// Creates a real channel: the user posts; others subscribe by its link.
pub fn create_channel(s: Store, title: String) -> ChatId {
    let gid: [u8; 16] = wire::random();
    let chat = ensure_group(s, gid, &title);
    apply_kind(
        s,
        chat,
        gid,
        &title,
        Some((true, false, songbird_app_core::mls::Role::Owner)),
        None,
    );
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.create_channel(gid, &title)
            .await
            .map_err(|_| super::problem("The channel couldn't be created. Try again."))?;
        Ok(())
    });
    chat
}

/// Creates a real community with its announcement group. Answers the community's chat.
pub fn create_community(s: Store, title: String) -> ChatId {
    let cid: [u8; 16] = wire::random();
    let ann = ensure_group(s, cid, &title);
    apply_kind(
        s,
        ann,
        cid,
        &title,
        Some((false, true, songbird_app_core::mls::Role::Owner)),
        None,
    );
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.create_community(cid, &title, &[])
            .await
            .map_err(|_| super::problem("The community couldn't be created. Try again."))?;
        Ok(())
    });
    community_ui(&cid)
}

/// Puts a real group into a real community (an admin of both).
pub fn community_add_group(community: ChatId, group: ChatId) {
    let cid = GROUPS.with_borrow(|g| {
        g.iter()
            .find(|(k, _)| community_ui(&g[*k]) == community)
            .map(|(_, v)| *v)
    });
    let (Some(cid), Some(gid)) = (cid, group_of(group)) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.community_add_group(&cid, &gid).await.map_err(|_| {
            super::problem("The group couldn't be added. You must be an admin of both.")
        })
    });
}

/// Creates a real group with real contacts: the chat shows at once; app-core creates it on the
/// server, adds every member's devices and sends the title (D-0146).
pub fn create_group(s: Store, title: String, members: Vec<ContactId>) -> ChatId {
    let gid: [u8; 16] = wire::random();
    let chat = ensure_group(s, gid, &title);
    let accounts: Vec<[u8; 16]> = members.iter().filter_map(|c| account_of(*c)).collect();
    s.edit_chat(chat, |c| {
        c.members = std::iter::once(crate::store::Member {
            contact: None,
            role: crate::store::Role::Owner,
        })
        .chain(members.iter().map(|&m| crate::store::Member {
            contact: Some(m),
            role: crate::store::Role::Member,
        }))
        .collect();
    });
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.create_group_with_id(gid, &title, &accounts)
            .await
            .map_err(|_| {
                super::problem(
                    "The group couldn't be created. Check your connection and try again.",
                )
            })?;
        Ok(())
    });
    chat
}

pub(super) fn account_of(chat: ChatId) -> Option<[u8; 16]> {
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
        let has_body = !body.is_empty();
        let json = body.first() == Some(&b'{');
        super::signed(method, path, has_body.then_some(body), json)
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn upload(
        &self,
        method: &str,
        path: &str,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<(u16, Vec<u8>)> {
        super::signed_upload(method, path, body)
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn fetch(
        &self,
        path_or_url: &str,
    ) -> songbird_app_core::Result<(u16, Vec<u8>, Option<String>)> {
        super::unsigned("GET", path_or_url)
            .await
            .map(|(s, b)| (s, b, None))
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn fetch_post(&self, path: &str) -> songbird_app_core::Result<u16> {
        super::unsigned("POST", path)
            .await
            .map(|(s, _)| s)
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn send_group(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<SendStatus> {
        let client = super::ws::client().ok_or(songbird_app_core::Error::Network)?;
        super::usage::count(super::usage::Lane::Messages, body.len() + 40, 0);
        client
            .send_with(
                idempotency_key,
                hints,
                songbird_proto::frame::SendPayload::Shared {
                    devices: Vec::new(),
                    body: body.into(),
                },
            )
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn ephemeral(
        &self,
        device: DeviceIdBytes,
        payload: Vec<u8>,
    ) -> songbird_app_core::Result<()> {
        let client = super::ws::client().ok_or(songbird_app_core::Error::Network)?;
        super::usage::count(super::usage::Lane::Messages, payload.len() + 24, 0);
        client
            .ephemeral(DeviceId::from_bytes(device), payload.into())
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn attention(
        &self,
        device: DeviceIdBytes,
        payload: Vec<u8>,
    ) -> songbird_app_core::Result<()> {
        let client = super::ws::client().ok_or(songbird_app_core::Error::Network)?;
        super::usage::count(super::usage::Lane::Messages, payload.len() + 24, 0);
        client
            .attention(DeviceId::from_bytes(device), payload.into())
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
        let size: usize = recipients.iter().map(|(_, p)| p.len() + 20).sum();
        super::usage::count(super::usage::Lane::Messages, size + 40, 0);
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
        // Groups (D-0146): KeyPackages others add this device with.
        let _ = m
            .publish_key_packages(songbird_app_core::mls::client::KEY_PACKAGES, true)
            .await;
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
        // The account's groups, recorded for its other devices; ones it joined elsewhere,
        // joined here (D-0167).
        super::group_sync::publish(s, &m);
        super::group_sync::join_missing(s);
    }
    super::calls::show_links(&m);
    if let Some(s) = crate::store::try_store() {
        // A crash report from last time, sent only if the user allows them (D-0170).
        super::crash::flush(s);
        // This device's push token, registered (D-0175).
        super::push::install();
    }
    if let Some(s) = crate::store::try_store() {
        super::chat_state::reload(s);
    }
    if let Some(s) = crate::store::try_store() {
        super::presence::refresh(s);
        // New contacts (a profile key shared since) start getting presence within minutes.
        if !PRESENCE_TIMER.replace(true) {
            Owner::detached().run(|| {
                set_interval(std::time::Duration::from_secs(300), move || {
                    super::presence::refresh(s);
                });
            });
        }
    }
    super::packs::show(&m);
    clear_media_cache();
    // Security notices (key transparency, new devices; D-0168): shown as they come.
    show_notices(&m);
    if !NOTICING.replace(true) {
        Owner::detached().run(|| {
            set_interval(std::time::Duration::from_secs(20), || {
                let lock = core();
                let Ok(guard) = lock.try_lock() else {
                    return;
                };
                if let Some(m) = guard.as_ref() {
                    show_notices(m);
                }
            });
        });
    }
    // Disappearing messages go on time, even with the app left open.
    if !SWEEPING.replace(true) {
        sweep_expired();
        Owner::detached().run(|| {
            set_interval(std::time::Duration::from_secs(15), sweep_expired);
        });
    }
    let keyed: Vec<([u8; 16], [u8; 32])> = m
        .store
        .chats()
        .unwrap_or_default()
        .iter()
        .filter_map(|c| Some((c.id, *m.store.profile_key(&c.id).ok().flatten()?)))
        .collect();
    *guard = Some(m);
    drop(guard);
    for (account, key) in keyed {
        refresh_profile(account, key).await;
    }
    Ok(())
}

/// Closes app-core (disconnect, forget).
pub fn stop() {
    super::calls::stop();
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
    let in_group = GROUPS.with_borrow(|g| g.values().any(|x| *x == row.chat));
    let chat = if in_group {
        ui_id(&row.chat)
    } else {
        ensure_chat(s, row.chat, None)
    };
    let mine = row.author == *me;
    // In a group the sender is a contact of their own.
    let sender = if in_group && !mine {
        Some(ensure_chat(s, row.author, None))
    } else {
        None
    };
    let id = ui_id(&row.id);
    if s.message(chat, id).is_some()
        || OUTGOING.with_borrow(|o| o.iter().any(|(_, v)| *v == row.id))
    {
        return;
    }
    let from = if mine {
        None
    } else {
        Some(sender.unwrap_or(chat))
    };
    let body = body_of(row);
    IDS.with_borrow_mut(|m| m.insert(id, (row.author, row.id)));
    let media = matches!(
        body,
        Body::Image { .. }
            | Body::File { .. }
            | Body::Video { .. }
            | Body::ViewOnce { .. }
            | Body::Voice { .. }
    ) || (matches!(body, Body::Sticker { .. })
        && songbird_app_core::packs::sticker_in(&row.content).is_some_and(|(_, _, img)| img));
    if media {
        MEDIA.with_borrow_mut(|m| m.insert(id, (row.author, row.id)));
    }
    s.messages.update(|m| {
        m.entry(chat).or_default().push(crate::store::Message {
            id,
            chat,
            from,
            body,
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
    // Auto-download (Settings → Storage and data): what the user allowed,
    // by kind. Phones can't tell Wi-Fi from mobile data here, so they follow
    // the mobile choice; desktops the Wi-Fi one.
    if media && !mine {
        let photo = s
            .message(chat, id)
            .is_some_and(|m| matches!(m.body, Body::Image { .. }));
        let (wifi, mobile) = s
            .settings
            .with_untracked(|x| if photo { x.auto_photos } else { x.auto_docs });
        let allowed = if is_desktop() { wifi } else { mobile };
        if allowed && photo {
            load_photo(id);
        }
    }
    if !mine && count_unread {
        s.edit_chat(chat, |c| c.unread += 1);
        s.on_incoming(chat);
        let sender = s.contact(chat).map(|c| c.name);
        s.arrive(crate::store::Arrival::Message {
            chat,
            sender,
            preview: row.text.clone(),
        });
    }
}

/// Shown media by UI message (photos as images).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown(pub HashMap<MsgId, nsh_core::element::ImageSrc>);

fn shown() -> Signal<Shown> {
    global::<Shown>()
}

/// Whether `msg` is a real attachment app-core holds.
pub fn is_media(msg: MsgId) -> bool {
    MEDIA.with_borrow(|m| m.contains_key(&msg))
}

/// The photo of a real media message, once this device has it (tracked).
pub fn media_src(msg: MsgId) -> Option<nsh_core::element::ImageSrc> {
    shown().with(|m| m.0.get(&msg).cloned())
}

fn show_bytes(msg: MsgId, bytes: &[u8]) {
    let src = nsh_core::element::ImageSrc::from(super::avatar::data_uri(bytes));
    shown().update(|m| {
        m.0.insert(msg, src);
    });
}

/// Fetches a real media message's file (from the encrypted store, else the
/// server: downloaded, verified, kept), then `then` with its bytes.
fn with_file(msg: MsgId, then: impl FnOnce(Vec<u8>) + 'static) {
    let Some((author, id)) = MEDIA.with_borrow(|m| m.get(&msg).copied()) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let bytes = m
            .file(&author, &id, 0)
            .await
            .map_err(|_| super::problem("The file couldn't be downloaded. Try again."))?;
        drop(guard);
        then(bytes.to_vec());
        Ok(())
    });
}

/// Saves a real photo or video message to the library (D-0174): the Photos library on iOS, the
/// shared collections on Android, the Pictures folder on a computer. `quiet`: no word on
/// success (the automatic save of received media). View-once media are never saved. Answers
/// whether `msg` is a real media message.
pub fn save_to_library(msg: MsgId, quiet: bool) -> bool {
    use songbird_device::photos::{self, Failure, Kind};
    let Some((author, id)) = MEDIA.with_borrow(|m| m.get(&msg).copied()) else {
        return false;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let row = m
            .store
            .message(&author, &id)
            .ok()
            .flatten()
            .ok_or_else(|| super::problem("This message is gone."))?;
        if songbird_app_core::messenger::view_once_in(&row.content) {
            return Err(super::problem(
                "View-once photos and videos can't be saved.",
            ));
        }
        let pointer = songbird_app_core::messenger::attachments_of(&row.content)
            .into_iter()
            .next()
            .ok_or_else(|| super::problem("There's nothing to save."))?;
        let bytes = m
            .file(&author, &id, 0)
            .await
            .map_err(|_| super::problem("The file couldn't be downloaded. Try again."))?;
        drop(guard);
        let video = pointer.content_type.starts_with("video/");
        let name = if pointer.file_name.is_empty() {
            format!(
                "Songbird-{}.{}",
                row.sent_ms,
                pointer.content_type.rsplit('/').next().unwrap_or("bin")
            )
        } else {
            pointer.file_name.clone()
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        photos::save(
            bytes.to_vec(),
            if video { Kind::Video } else { Kind::Photo },
            &name,
            &pointer.content_type,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        match rx.await.unwrap_or(Err(Failure::Failed)) {
            Ok(()) => {
                if !quiet {
                    crate::ui::toast(if is_desktop_os() {
                        "Saved to Pictures › Songbird"
                    } else {
                        "Saved to your photos"
                    });
                }
                Ok(())
            }
            Err(Failure::Denied) => Err(super::problem(
                "Songbird isn't allowed to add to your photos. Allow it in your device's settings, then try again.",
            )),
            Err(Failure::Failed) => Err(super::problem("It couldn't be saved. Try again.")),
        }
    });
    true
}

fn is_desktop_os() -> bool {
    !matches!(
        os(),
        nsh_core::platform::Os::Ios | nsh_core::platform::Os::Android
    )
}

/// Marks a shown view-once message opened when it was opened here before.
fn show_once_state(s: Store, m: &Core, row: &MessageRow) {
    if songbird_app_core::messenger::view_once_in(&row.content)
        && m.view_once_opened(&row.author, &row.id).unwrap_or(false)
        && let Some(msg) = ui_of(&row.author, &row.id)
    {
        edit_shown(s, msg, |x| {
            if let Body::ViewOnce { opened, .. } = &mut x.body {
                *opened = true;
            }
        });
    }
}

/// Where decrypted videos wait while they play (the platform's player needs a file): inside the
/// app's own data folder, emptied when app-core opens.
fn media_cache() -> Option<std::path::PathBuf> {
    data_dir().map(|d| d.join("media-playing"))
}

/// Empties the playing-media folder (at start, and when a view-once video closes).
pub fn clear_media_cache() {
    if let Some(dir) = media_cache() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A playable file for a real video message, then `then` with its `file://` URL.
pub fn media_file(msg: MsgId, then: impl FnOnce(String) + 'static) {
    with_file(msg, move |bytes| {
        if let Some(url) = write_playing(msg, &bytes) {
            then(url);
        }
    });
}

/// The file's extension the platform's player goes by.
fn extension_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        "wav"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        "mp4"
    } else if bytes.starts_with(b"OggS") {
        "ogg"
    } else {
        "bin"
    }
}

fn write_playing(msg: MsgId, bytes: &[u8]) -> Option<String> {
    let dir = media_cache()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{msg:016x}.{}", extension_of(bytes)));
    std::fs::write(&path, bytes).ok()?;
    Some(format!("file://{}", path.display()))
}

/// Sends a photo to be viewed once in a real chat.
pub fn send_view_once(s: Store, chat: ChatId, f: nsh_core::picker::PickedFile) {
    let Some(target) = core_chat(chat) else {
        return;
    };
    let bytes = f.data.bytes().to_vec();
    if bytes.is_empty() {
        return;
    }
    let ui = s.push_message(
        chat,
        None,
        Body::ViewOnce {
            photo: !f.mime.starts_with("video/"),
            opened: true,
        },
    );
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: f.mime.clone(),
        file_name: f.name.clone(),
        width: u32::try_from(f.size.width.0).unwrap_or(0),
        height: u32::try_from(f.size.height.0).unwrap_or(0),
        ..Default::default()
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let me = m.local().account;
        match m.send_view_once(&target, &bytes, &meta).await {
            Ok(id) => {
                let status = m
                    .store
                    .message(&me, &id)
                    .ok()
                    .flatten()
                    .map_or(Status::Sent, |r| r.status);
                drop(guard);
                OUTGOING.with_borrow_mut(|o| o.insert(ui, id));
                IDS.with_borrow_mut(|x| x.insert(ui, (me, id)));
                set_status(s, &id, status);
                Ok(())
            }
            Err(_) => {
                drop(guard);
                s.fail(chat, ui);
                Err(super::problem("The photo couldn't be sent."))
            }
        }
    });
}

/// Opens a received view-once message: its file is read, deleted from this device at once,
/// and shown in a viewer that keeps no copy.
pub fn open_view_once(s: Store, chat: ChatId, msg: MsgId) {
    let Some((author, id)) = MEDIA.with_borrow(|m| m.get(&msg).copied()) else {
        return;
    };
    super::run(async move {
        let got = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let bytes = m.file(&author, &id, 0).await;
            let video = m
                .store
                .message(&author, &id)
                .ok()
                .flatten()
                .and_then(|r| {
                    songbird_app_core::messenger::attachments_of(&r.content)
                        .into_iter()
                        .next()
                })
                .is_some_and(|p| p.content_type.starts_with("video/"));
            // Gone from here as soon as it is read (as it is opened).
            let _ = m.consume_view_once(&author, &id);
            bytes.map(|b| (b.to_vec(), video))
        };
        let _ = chat;
        edit_shown(s, msg, |x| {
            if let Body::ViewOnce { opened, .. } = &mut x.body {
                *opened = true;
            }
        });
        let (bytes, video) =
            got.map_err(|_| super::problem("This view-once message can't be opened any more."))?;
        let src = if video {
            match write_playing(msg, &bytes) {
                Some(url) => crate::features::media::OnceSource::Video(url),
                None => return Err(super::problem("The video couldn't be shown.")),
            }
        } else {
            crate::features::media::OnceSource::Photo(nsh_core::element::ImageSrc::from(
                super::avatar::data_uri(&bytes),
            ))
        };
        crate::features::media::view_once(src);
        Ok(())
    });
}

/// Shows a photo (downloading it if needed).
pub fn load_photo(msg: MsgId) {
    if media_src(msg).is_none() {
        with_file(msg, move |b| show_bytes(msg, &b));
    }
}

/// Saves a real media message's file where the user chooses.
pub fn save_media(msg: MsgId, name: String) {
    with_file(msg, move |b| crate::ui::save_file(name, b));
}

/// Sends picked files: shown at once as Sending, then encrypted, uploaded
/// and sent by app-core (D-0141).
pub fn send_picked(s: Store, chat: ChatId, files: Vec<nsh_core::picker::PickedFile>, photos: bool) {
    let group = group_of(chat);
    let to = account_of(chat);
    if group.is_none() && to.is_none() {
        return;
    }
    for f in files {
        let bytes = f.data.bytes().to_vec();
        if bytes.is_empty() {
            continue;
        }
        let body = if f.mime.starts_with("video/") {
            Body::Video {
                caption: String::new(),
                secs: 0,
            }
        } else if photos {
            Body::Image {
                caption: String::new(),
                alt: f.name.clone(),
                w: u32::try_from(f.size.width.0).unwrap_or(1).max(1),
                h: u32::try_from(f.size.height.0).unwrap_or(1).max(1),
                hd: s.settings.with_untracked(|x| x.hd_uploads),
            }
        } else {
            Body::File {
                name: f.name.clone(),
                size: human_size(bytes.len()),
                kind: kind_of(&f.mime),
            }
        };
        let ui = s.push_message(chat, None, body);
        if photos {
            show_bytes(ui, &bytes);
        }
        let meta = songbird_app_core::attachments::FileMeta {
            content_type: f.mime.clone(),
            file_name: f.name.clone(),
            width: u32::try_from(f.size.width.0).unwrap_or(0),
            height: u32::try_from(f.size.height.0).unwrap_or(0),
            ..Default::default()
        };
        super::run(async move {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let me = m.local().account;
            let r = match (group, to) {
                (Some(gid), _) => m.send_group_files(&gid, vec![(bytes, meta)], "").await,
                (None, Some(to)) => m.send_files(&to, vec![(bytes, meta)], "").await,
                (None, None) => Err(songbird_app_core::Error::NoSession),
            };
            let st = r.as_ref().ok().and_then(|id| {
                m.store
                    .message(&me, id)
                    .ok()
                    .flatten()
                    .map(|row| (*id, row.status))
            });
            drop(guard);
            match st {
                Some((id, status)) => {
                    OUTGOING.with_borrow_mut(|o| o.insert(ui, id));
                    IDS.with_borrow_mut(|x| x.insert(ui, (me, id)));
                    MEDIA.with_borrow_mut(|x| x.insert(ui, (me, id)));
                    set_status(s, &id, status);
                    Ok(())
                }
                None => {
                    s.fail(chat, ui);
                    Err(super::problem("The file couldn't be sent."))
                }
            }
        });
    }
}

fn human_size(n: usize) -> String {
    let n = n as f64;
    if n >= 1_048_576.0 {
        format!("{:.1} MB", n / 1_048_576.0)
    } else {
        format!("{:.0} KB", (n / 1024.0).max(1.0))
    }
}

fn kind_of(mime: &str) -> String {
    match mime {
        "application/pdf" => "PDF".into(),
        m if m.starts_with("text/") => "Text".into(),
        m if m.starts_with("image/") => "Image".into(),
        m if m.starts_with("audio/") => "Audio".into(),
        m if m.starts_with("video/") => "Video".into(),
        m => m.rsplit('/').next().unwrap_or("File").to_uppercase(),
    }
}

/// What a stored message shows as: its text, or a real place on the map.
fn body_of(row: &MessageRow) -> Body {
    if row.content.is_empty() {
        return Body::Deleted;
    }
    if let Some((pack, emoji, _)) = songbird_app_core::packs::sticker_in(&row.content) {
        let name = if pack == songbird_app_core::packs::BUILTIN_PACK_ID {
            "Songbird friends".to_string()
        } else {
            crate::store::ext_groups::groups_state().with_untracked(|g| {
                g.packs
                    .iter()
                    .find(|p| super::packs::pack_id(p.id) == Some(pack))
                    .map_or_else(|| "a sticker pack".to_string(), |p| p.name.clone())
            })
        };
        return Body::Sticker { emoji, pack: name };
    }
    if let Some(p) = songbird_app_core::messenger::attachments_of(&row.content)
        .into_iter()
        .next()
    {
        if songbird_app_core::messenger::view_once_in(&row.content) {
            // Whether it was opened here is set after it is shown (`show_once_state`).
            return Body::ViewOnce {
                photo: !p.content_type.starts_with("video/"),
                opened: false,
            };
        }
        // A voice note (its flag, or any audio).
        if p.flags & 1 == 1 || p.content_type.starts_with("audio/") {
            return Body::Voice {
                secs: p.duration_ms.div_ceil(1000).max(1),
                played: row.status == Status::Played,
            };
        }
        if p.content_type.starts_with("video/") {
            return Body::Video {
                caption: if row.text == songbird_app_core::messenger::attachment_label(&p) {
                    String::new()
                } else {
                    row.text.clone()
                },
                secs: p.duration_ms.div_ceil(1000),
            };
        }
        let caption = if row.text == songbird_app_core::messenger::attachment_label(&p) {
            String::new()
        } else {
            row.text.clone()
        };
        return if p.content_type.starts_with("image/") {
            Body::Image {
                caption,
                alt: p.file_name,
                w: p.width.max(1),
                h: p.height.max(1),
                hd: false,
            }
        } else {
            Body::File {
                name: if p.file_name.is_empty() {
                    "File".into()
                } else {
                    p.file_name
                },
                size: human_size(usize::try_from(p.size).unwrap_or(0)),
                kind: kind_of(&p.content_type),
            }
        };
    }
    if let Some(p) = poll_in(&row.content) {
        return Body::Poll {
            question: p.question,
            options: p.options.into_iter().map(|o| (o, 0)).collect(),
            mine: Vec::new(),
            multiple: p.multiple_choice,
            closed: false,
        };
    }
    if let Some((lat, lon, name)) = songbird_app_core::messenger::location_of(&row.content) {
        let label = if name.is_empty() {
            format!("{lat:.5}, {lon:.5}")
        } else {
            name
        };
        return Body::Location {
            label,
            live: false,
            at: Some((lat, lon)),
        };
    }
    Body::Text(row.text.clone())
}

fn show_all(s: Store, m: &Core) {
    let me = m.local().account;
    for c in m.store.chats().unwrap_or_default() {
        let chat = if c.group {
            let title = m.group_title(&c.id).unwrap_or_default();
            let chat = ensure_group(s, c.id, &title);
            apply_kind(s, chat, c.id, &title, m.group_kind(&c.id).ok(), None);
            chat
        } else {
            ensure_chat(s, c.id, None)
        };
        for row in m.store.messages(&c.id, None, 200).unwrap_or_default() {
            show(s, &me, &row, false);
            if let Some(msg) = ui_of(&row.author, &row.id) {
                for (reactor, emoji) in m.store.reactions(&row.author, &row.id).unwrap_or_default()
                {
                    let by = (reactor != me).then(|| ui_id(&reactor));
                    apply_reaction(s, msg, by, &emoji);
                }
            }
            if poll_in(&row.content).is_some() {
                show_tally(s, m, &row.author, &row.id);
            }
            show_once_state(s, m, &row);
        }
        for (author, id, _) in m.pins(&c.id).unwrap_or_default() {
            if let Some(msg) = ui_of(&author, &id) {
                edit_shown(s, msg, |x| x.pinned = true);
            }
        }
        let timer = timer_choice(m.timer(&c.id).unwrap_or(0));
        s.edit_chat(chat, |x| {
            x.unread = c.unread;
            x.disappearing = timer;
        });
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
    show_notices(m);
    match m.receive(envelope).await {
        Ok(Received::Message(row)) => {
            let fresh = row.author != me && !PROFILED.with_borrow(|p| p.contains(&row.chat));
            let key = m.store.profile_key(&row.chat).ok().flatten();
            let save = row.author != me
                && !songbird_app_core::messenger::view_once_in(&row.content)
                && songbird_app_core::messenger::attachments_of(&row.content)
                    .first()
                    .is_some_and(|p| {
                        p.content_type.starts_with("image/") || p.content_type.starts_with("video/")
                    });
            if let Some(s) = crate::store::try_store() {
                show(s, &me, &row, row.author != me);
                if poll_in(&row.content).is_some() {
                    show_tally(s, m, &row.author, &row.id);
                }
            }
            drop(guard);
            // "Save to photos" (D-0174): received photos and videos, saved as they arrive.
            if save
                && crate::store::try_store()
                    .is_some_and(|s| s.settings.with_untracked(|x| x.save_to_photos))
                && let Some(msg) = ui_of(&row.author, &row.id)
            {
                save_to_library(msg, true);
            }
            if let (true, Some(key)) = (fresh, key) {
                refresh_profile(row.chat, *key).await;
            }
            true
        }
        Ok(Received::Status { id, status }) => {
            if let Some(s) = crate::store::try_store() {
                set_status(s, &id, status);
            }
            true
        }
        Ok(Received::Reaction {
            author,
            id,
            reactor,
            emoji,
        }) => {
            if let (Some(s), Some(msg)) = (crate::store::try_store(), ui_of(&author, &id)) {
                let by = (reactor != me).then(|| ui_id(&reactor));
                apply_reaction(s, msg, by, &emoji);
            }
            true
        }
        Ok(Received::Edited { author, id, text }) => {
            if let (Some(s), Some(msg)) = (crate::store::try_store(), ui_of(&author, &id)) {
                edit_shown(s, msg, |m| {
                    m.body = Body::Text(text.clone());
                    m.edited = true;
                });
            }
            true
        }
        Ok(Received::Deleted { author, id }) => {
            if let (Some(s), Some(msg)) = (crate::store::try_store(), ui_of(&author, &id)) {
                edit_shown(s, msg, |m| {
                    m.body = Body::Deleted;
                    m.reactions.clear();
                });
            }
            true
        }
        Ok(Received::Typing { account, started }) => {
            // Shown only with Typing indicators on (off: neither sent nor shown).
            if let Some(s) = crate::store::try_store()
                && s.settings.with_untracked(|x| x.typing_indicators)
            {
                let chat = ui_id(&account);
                if owns(chat) {
                    s.edit_chat(chat, |c| c.typing = started.then(|| "typing…".to_string()));
                    if started {
                        // Apps stop showing it 15 s after the last "started" (§5.7).
                        set_timeout(std::time::Duration::from_secs(15), move || {
                            s.edit_chat(chat, |c| c.typing = None);
                        });
                    }
                }
            }
            true
        }
        Ok(Received::Joined { group }) => {
            let title = m.group_title(&group).unwrap_or_default();
            let kind = m.group_kind(&group).ok();
            let community = match kind {
                Some((false, true, _)) => m.community_groups(&group).await.ok().flatten(),
                _ => None,
            };
            if let Some(s) = crate::store::try_store() {
                let chat = ensure_group(s, group, &title);
                apply_kind(s, chat, group, &title, kind, community.as_deref());
                s.push_message(chat, None, Body::System("You were added".into()));
                super::group_sync::mark(s, &group, true);
            }
            true
        }
        Ok(Received::GroupUpdated { group, title }) => {
            // A commit: a call in the group moves to the new epoch's keys.
            super::calls::group_changed(m, &group);
            let devices = m.group_devices(&group).map(|d| d.len()).unwrap_or(0);
            if let Some(s) = crate::store::try_store() {
                let chat = ensure_group(s, group, &title);
                if let Some(ChatKind::Channel { admin, .. }) = s.chat(chat).map(|c| c.kind) {
                    let followers = u32::try_from(devices).unwrap_or(u32::MAX);
                    s.edit_chat(chat, |c| c.kind = ChatKind::Channel { followers, admin });
                }
            }
            true
        }
        Ok(Received::RemovedFromGroup { group }) => {
            super::calls::group_left(&group);
            if let Some(s) = crate::store::try_store() {
                let chat = ui_id(&group);
                super::group_sync::mark(s, &group, false);
                s.push_message(chat, None, Body::System("You were removed".into()));
                s.edit_chat(chat, |c| c.members.retain(|x| x.contact.is_some()));
            }
            true
        }
        Ok(Received::GroupRulesBroken { group }) => {
            if let Some(s) = crate::store::try_store() {
                s.push_message(
                    ui_id(&group),
                    None,
                    Body::System(
                        "Someone made a change this group's rules don't allow, so this device stopped following the group. Ask an admin to add you again."
                            .into(),
                    ),
                );
            }
            true
        }
        Ok(Received::Presence { account, presence }) => {
            drop(guard);
            super::presence::received(account, &presence);
            true
        }
        Ok(Received::TimerChanged { chat, by, secs }) => {
            if let Some(s) = crate::store::try_store() {
                let ui_chat = chat_ui(&chat);
                let t = timer_choice(secs);
                s.edit_chat(ui_chat, |c| c.disappearing = t);
                let who = if by == me {
                    "You".to_string()
                } else {
                    s.contact_name(Some(ui_id(&by)))
                };
                let line = match t {
                    Some(t) => format!(
                        "{who} turned on disappearing messages. New messages will disappear after {}.",
                        t.label()
                    ),
                    None => format!("{who} turned off disappearing messages."),
                };
                s.push_message(ui_chat, None, Body::System(line));
            }
            true
        }
        Ok(Received::Pinned {
            author,
            id,
            until_ms,
            ..
        }) => {
            if let (Some(s), Some(msg)) = (crate::store::try_store(), ui_of(&author, &id)) {
                edit_shown(s, msg, |x| x.pinned = until_ms > 0);
            }
            true
        }
        Ok(Received::PollChanged { author, id }) => {
            if let Some(s) = crate::store::try_store() {
                show_tally(s, m, &author, &id);
            }
            true
        }
        Ok(Received::Call {
            account,
            group,
            call,
            signal,
        }) => {
            super::calls::signal(m, account, group, call, signal).await;
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
    if let Body::Poll {
        question,
        options,
        multiple,
        ..
    } = body
    {
        let Some(target) = core_chat(chat) else {
            return;
        };
        let (question, multiple) = (question.clone(), *multiple);
        let options: Vec<String> = options.iter().map(|o| o.0.clone()).collect();
        super::run(async move {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let me = m.local().account;
            match m.send_poll(&target, &question, &options, multiple).await {
                Ok(id) => {
                    let status = m
                        .store
                        .message(&me, &id)
                        .ok()
                        .flatten()
                        .map_or(Status::Sent, |r| r.status);
                    drop(guard);
                    OUTGOING.with_borrow_mut(|o| o.insert(ui_msg, id));
                    IDS.with_borrow_mut(|x| x.insert(ui_msg, (me, id)));
                    set_status(s, &id, status);
                    Ok(())
                }
                Err(_) => {
                    drop(guard);
                    s.fail(chat, ui_msg);
                    Err(super::problem("The poll couldn't be sent."))
                }
            }
        });
        return;
    }
    if let Some(gid) = group_of(chat) {
        let text = match body {
            Body::Text(t) => t.clone(),
            Body::Link { text, .. } => text.clone(),
            _ => {
                s.fail(chat, ui_msg);
                toast("Only text can be sent in groups for now.");
                return;
            }
        };
        super::run(async move {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let me = m.local().account;
            match m.send_group_text(&gid, &text).await {
                Ok(id) => {
                    drop(guard);
                    OUTGOING.with_borrow_mut(|o| o.insert(ui_msg, id));
                    IDS.with_borrow_mut(|x| x.insert(ui_msg, (me, id)));
                    s.advance(chat, ui_msg, Tick::Sent);
                    Ok(())
                }
                Err(_) => {
                    drop(guard);
                    s.fail(chat, ui_msg);
                    Err(super::problem("The message couldn't be sent to the group."))
                }
            }
        });
        return;
    }
    let Some(to) = account_of(chat) else { return };
    if let Body::Location {
        label,
        at: Some((lat, lon)),
        ..
    } = body
    {
        let (lat, lon, label) = (*lat, *lon, label.clone());
        super::run(async move {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let r = m.send_location(&to, lat, lon, &label).await;
            let st = r.as_ref().ok().and_then(|id| {
                m.store
                    .message(&m.local().account, id)
                    .ok()
                    .flatten()
                    .map(|row| (*id, row.status))
            });
            drop(guard);
            match st {
                Some((id, status)) => {
                    OUTGOING.with_borrow_mut(|o| o.insert(ui_msg, id));
                    IDS.with_borrow_mut(|m| m.insert(ui_msg, (me_account(), id)));
                    set_status(s, &id, status);
                    Ok(())
                }
                None => {
                    s.fail(chat, ui_msg);
                    Err(super::problem("The location couldn't be sent."))
                }
            }
        });
        return;
    }
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
                IDS.with_borrow_mut(|m| m.insert(ui_msg, (me_account(), id)));
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
                        s.set_key_changed(chat, true);
                        "This person's safety number changed. Open it from their profile to continue."
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

pub(super) fn me_account() -> [u8; 16] {
    live()
        .with_untracked(|l| l.account.as_deref().and_then(hex16))
        .unwrap_or([0; 16])
}

/// The UI message for (author, app-core ID), if shown.
fn ui_of(author: &[u8; 16], id: &[u8; 16]) -> Option<MsgId> {
    IDS.with_borrow(|m| {
        m.iter()
            .find(|(_, v)| v.0 == *author && v.1 == *id)
            .map(|(k, _)| *k)
    })
}

/// Marks a real chat read here and, when "Read receipts" is on, tells the
/// authors (Settings → Privacy; off sends nothing, as Signal and WhatsApp do).
pub fn mark_read(chat: ChatId) {
    let Some(account) = account_of(chat) else {
        return;
    };
    let send =
        crate::store::try_store().is_none_or(|s| s.settings.with_untracked(|x| x.read_receipts));
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            if send {
                let _ = m.mark_read(&account).await;
            } else {
                let me = m.local().account;
                let _ = m.store.mark_read(&account, &me);
            }
        }
        Ok(())
    });
}

/// Blocks (or unblocks) a real contact. Blocking tells the server, rotates
/// the profile key (republished, and shared with every other contact by
/// app-core) so the blocked account's old access key stops working, and
/// drops anything it still sends (PROTOCOL §5.11).
pub fn block(s: Store, contact: ContactId, blocked: bool) {
    let Some(account) = account_of(contact) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        if blocked {
            let key = super::profile::rotate();
            m.block(&account, &key)
                .await
                .map_err(|_| super::problem("The block couldn't be saved. Try again."))?;
            drop(guard);
            super::profile::publish(s);
        } else {
            m.unblock(&account).await.map_err(|e| {
                super::problem(match e {
                    songbird_app_core::Error::Server(403) => "Only your main phone can unblock.",
                    _ => "The unblock couldn't be saved. Try again.",
                })
            })?;
        }
        Ok(())
    });
}

/// Reports a real chat: its newest five messages, each with the proof the
/// server checks (PROTOCOL §5.14). Nothing else about the chat leaves.
pub fn report(chat: ChatId) {
    let Some(account) = account_of(chat) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let n = m
            .report(&account, 5, 5)
            .await
            .map_err(|_| super::problem("The report couldn't be sent. Try again."))?;
        drop(guard);
        crate::ui::toast(if n == 0 {
            "There's nothing from them to report yet.".to_string()
        } else {
            format!(
                "Reported {n} message{}. Thank you.",
                if n == 1 { "" } else { "s" }
            )
        });
        Ok(())
    });
}

thread_local! {
    /// The last typing state sent per chat and when (ms).
    static TYPING: RefCell<HashMap<ChatId, (bool, f64)>> = RefCell::new(HashMap::new());
}

/// Tells a real chat the user is (or stopped) typing: "started" at most
/// every 10 s, "stopped" once. Only with Typing indicators on.
pub fn typing(chat: ChatId, started: bool) {
    let Some(account) = account_of(chat) else {
        return;
    };
    let now = nsh_core::time::unix_ms();
    let due = TYPING.with_borrow_mut(|t| {
        let due = match t.get(&chat) {
            Some((was, at)) => *was != started || (started && now - at > 10_000.0),
            None => started,
        };
        if due {
            t.insert(chat, (started, now));
        }
        due
    });
    if !due {
        return;
    }
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            let _ = m.typing(&account, started).await;
        }
        Ok(())
    });
}

fn target(chat: ChatId, msg: MsgId) -> Option<([u8; 16], [u8; 16], [u8; 16])> {
    let peer = account_of(chat)?;
    let (author, id) = IDS.with_borrow(|m| m.get(&msg).copied())?;
    Some((peer, author, id))
}

/// Reacts in a real chat (empty: the user's reaction removed).
pub fn react(chat: ChatId, msg: MsgId, emoji: String) {
    let Some((peer, author, id)) = target(chat, msg) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.react(&peer, &author, &id, &emoji)
            .await
            .map_err(|_| super::problem("The reaction couldn't be sent."))
    });
}

/// Edits the user's own message in a real chat.
pub fn edit(chat: ChatId, msg: MsgId, text: String) {
    let Some((peer, _, id)) = target(chat, msg) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.edit(&peer, &id, &text)
            .await
            .map_err(|_| super::problem("The edit couldn't be sent."))
    });
}

/// Deletes the user's own message for everyone in a real chat.
pub fn delete_for_everyone(chat: ChatId, msg: MsgId) {
    let Some((peer, _, id)) = target(chat, msg) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.delete_for_everyone(&peer, &id)
            .await
            .map_err(|_| super::problem("The message couldn't be deleted for everyone."))
    });
}

fn apply_reaction(s: Store, msg: MsgId, by: Option<ContactId>, emoji: &str) {
    edit_shown(s, msg, |m| {
        for r in &mut m.reactions {
            r.by.retain(|b| *b != by);
        }
        m.reactions.retain(|r| !r.by.is_empty());
        if !emoji.is_empty() {
            match m.reactions.iter_mut().find(|r| r.emoji == emoji) {
                Some(r) => r.by.push(by),
                None => m.reactions.push(crate::store::Reaction {
                    emoji: emoji.to_owned(),
                    by: vec![by],
                }),
            }
        }
    });
}

fn edit_shown(s: Store, msg: MsgId, f: impl Fn(&mut crate::store::Message)) {
    s.messages.update(|all| {
        for v in all.values_mut() {
            if let Some(m) = v.iter_mut().find(|x| x.id == msg) {
                f(m);
                return;
            }
        }
    });
}

/// The UI chat of a real chat ID (an account or a group).
fn chat_ui(id: &[u8; 16]) -> ChatId {
    ui_id(id)
}

/// The app-core chat ID of a real UI chat: its group, else its account.
pub fn core_chat(chat: ChatId) -> Option<[u8; 16]> {
    group_of(chat).or_else(|| account_of(chat))
}

/// The poll a message holds, if any.
fn poll_in(content: &[u8]) -> Option<songbird_content::schema::PollCreate> {
    songbird_app_core::messenger::poll_in(content)
}

/// Shows a poll's tally as app-core counts it.
fn show_tally(s: Store, m: &Core, author: &[u8; 16], id: &[u8; 16]) {
    let Some(msg) = ui_of(author, id) else { return };
    let Ok((counts, mine, closed)) = m.poll_tally(author, id) else {
        return;
    };
    edit_shown(s, msg, |x| {
        if let Body::Poll {
            options,
            mine: shown,
            closed: c,
            ..
        } = &mut x.body
        {
            for (o, n) in options.iter_mut().zip(&counts) {
                o.1 = *n;
            }
            *shown = mine
                .iter()
                .filter_map(|o| usize::try_from(*o).ok())
                .collect();
            *c = closed;
        }
    });
}

/// Votes in a real chat's poll (pressing an option again takes it back).
pub fn vote(s: Store, chat: ChatId, msg: MsgId, option: usize) {
    let (Some(target), Some((author, id))) =
        (core_chat(chat), IDS.with_borrow(|x| x.get(&msg).copied()))
    else {
        return;
    };
    let (mine, multiple) = s
        .message(chat, msg)
        .and_then(|m| match m.body {
            Body::Poll { mine, multiple, .. } => Some((mine, multiple)),
            _ => None,
        })
        .unwrap_or_default();
    let next: Vec<u32> = Store::after_press(&mine, option, multiple)
        .into_iter()
        .filter_map(|o| u32::try_from(o).ok())
        .collect();
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let r = m.vote(&target, &author, &id, &next).await;
        show_tally(s, m, &author, &id);
        r.map_err(|_| super::problem("Your vote couldn't be sent. The poll may be closed."))
    });
}

/// Closes the user's own poll in a real chat.
pub fn close_poll(s: Store, chat: ChatId, msg: MsgId) {
    let (Some(target), Some((author, id))) =
        (core_chat(chat), IDS.with_borrow(|x| x.get(&msg).copied()))
    else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let r = m.close_poll(&target, &id).await;
        show_tally(s, m, &author, &id);
        r.map_err(|_| super::problem("The poll couldn't be closed."))
    });
}

/// How long a pin lasts: 7 days (the protocol offers 24 hours, 7 days or 30 days).
const PIN_SECS: u32 = 604_800;

/// Pins (or unpins) a message for everyone in a real chat.
pub fn pin(s: Store, chat: ChatId, msg: MsgId, on: bool) {
    let (Some(target), Some((author, id))) =
        (core_chat(chat), IDS.with_borrow(|x| x.get(&msg).copied()))
    else {
        toast("This message can't be pinned yet.");
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.pin(&target, &author, &id, PIN_SECS, !on)
            .await
            .map_err(|_| super::problem("The pin couldn't be sent."))?;
        drop(guard);
        edit_shown(s, msg, |x| x.pinned = on);
        Ok(())
    });
}

/// A disappearing timer in seconds, from the UI's choice.
pub fn timer_secs(t: Option<crate::store::Disappear>) -> u32 {
    match t {
        None => 0,
        Some(crate::store::Disappear::Day) => 86_400,
        Some(crate::store::Disappear::Week) => 604_800,
        Some(crate::store::Disappear::FourWeeks) => 2_419_200,
    }
}

/// The UI's choice for a timer in seconds (the nearest it offers, rounding down).
fn timer_choice(secs: u32) -> Option<crate::store::Disappear> {
    match secs {
        0 => None,
        s if s >= 2_419_200 => Some(crate::store::Disappear::FourWeeks),
        s if s >= 604_800 => Some(crate::store::Disappear::Week),
        _ => Some(crate::store::Disappear::Day),
    }
}

/// Sets a real chat's disappearing timer for everyone in it.
pub fn set_timer(chat: ChatId, t: Option<crate::store::Disappear>) {
    let Some(target) = core_chat(chat) else {
        return;
    };
    let secs = timer_secs(t);
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.set_timer(&target, secs)
            .await
            .map_err(|_| super::problem("The timer couldn't be changed."))
    });
}

/// Deletes the messages whose time is up, here and from the screen; every 15 seconds while
/// app-core is open.
fn sweep_expired() {
    super::run(async move {
        let (gone, retried) = {
            let lock = core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            // Messages a budget or a busy server held back go out again (they wait in the
            // outbox, in order).
            let waiting: Vec<[u8; 16]> = m
                .store
                .outbox()
                .unwrap_or_default()
                .into_iter()
                .map(|(_, _, id, _)| id)
                .collect();
            if !waiting.is_empty() {
                let _ = m.flush_outbox().await;
            }
            let me = m.local().account;
            let retried: Vec<([u8; 16], Status)> = waiting
                .into_iter()
                .filter_map(|id| Some((id, m.store.message(&me, &id).ok().flatten()?.status)))
                .collect();
            (m.expire_messages().unwrap_or_default(), retried)
        };
        if let Some(s) = crate::store::try_store() {
            for (id, status) in retried {
                set_status(s, &id, status);
            }
        }
        if let Some(s) = crate::store::try_store() {
            for (chat, author, id) in gone {
                if let Some(msg) = ui_of(&author, &id) {
                    let ui_chat = chat_ui(&chat);
                    s.messages.update(|all| {
                        if let Some(v) = all.get_mut(&ui_chat) {
                            v.retain(|m| m.id != msg);
                        }
                    });
                    IDS.with_borrow_mut(|x| x.remove(&msg));
                    MEDIA.with_borrow_mut(|x| x.remove(&msg));
                }
            }
        }
        Ok(())
    });
}

/// Opens a real contact's safety number: fetched from both accounts' identity keys, then shown.
pub fn open_safety(contact: ContactId) {
    let Some(account) = account_of(contact) else {
        return;
    };
    super::run(async move {
        let got = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            m.safety(&account).await
        };
        match got {
            Ok((number, verified)) => {
                if let Some(s) = crate::store::try_store() {
                    s.contacts.update(|cs| {
                        if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
                            c.safety_number = number.digits.clone();
                            c.verified = verified;
                        }
                    });
                }
                crate::features::devices::push_safety(contact);
                Ok(())
            }
            Err(songbird_app_core::Error::IdentityChanged) => {
                if let Some(s) = crate::store::try_store() {
                    s.set_key_changed(contact, true);
                }
                crate::features::devices::push_safety(contact);
                Ok(())
            }
            Err(_) => Err(super::problem("The safety number couldn't be loaded.")),
        }
    });
}

/// Keeps whether the user verified a real contact.
pub fn set_verified(contact: ContactId, verified: bool) {
    let Some(account) = account_of(contact) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let guard = lock.lock().await;
        if let Some(m) = guard.as_ref() {
            let _ = m.set_verified(&account, verified);
        }
        Ok(())
    });
}

/// Accepts a real contact's new identity key (after it changed): their new safety number
/// shows, unverified, and messages to them work again.
pub fn accept_identity(contact: ContactId) {
    let Some(account) = account_of(contact) else {
        return;
    };
    super::run(async move {
        let got = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            match m.accept_identity_change(&account).await {
                Ok(()) => m.safety(&account).await,
                Err(e) => Err(e),
            }
        };
        let (number, _) =
            got.map_err(|_| super::problem("The new safety number couldn't be accepted."))?;
        if let Some(s) = crate::store::try_store() {
            s.contacts.update(|cs| {
                if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
                    c.safety_number = number.digits.clone();
                    c.verified = false;
                    c.key_changed = false;
                }
            });
        }
        crate::nav::pop();
        crate::features::devices::push_safety(contact);
        toast("New safety number accepted. Compare it to verify them again.");
        Ok(())
    });
}

/// Reads a real group's change log (D-0156) into "Member changes", newest first.
pub fn load_member_changes(chat: ChatId) {
    let Some(gid) = group_of(chat) else { return };
    super::run(async move {
        let (log, me) = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            let me = m.local().account;
            (m.change_log(&gid).await, me)
        };
        let log = log.map_err(|_| super::problem("The member changes couldn't be loaded."))?;
        let Some(s) = crate::store::try_store() else {
            return Ok(());
        };
        let name = |a: &[u8; 16], start: bool| {
            if *a == me {
                if start { "You" } else { "you" }.to_string()
            } else {
                s.contact_name(Some(ui_id(a)))
            }
        };
        let list = |v: &[[u8; 16]]| {
            v.iter()
                .map(|a| name(a, false))
                .collect::<Vec<_>>()
                .join(", ")
        };
        use songbird_content::schema::group_update::Kind as K;
        let lines: Vec<crate::store::ext_groups::MemberChange> = log
            .iter()
            .rev()
            .filter_map(|c| {
                let who = name(&c.actor, true);
                let text = match c.kind {
                    K::Created => format!("{who} created the group"),
                    K::MembersAdded => format!("{who} added {}", list(&c.subjects)),
                    K::MembersRemoved => format!("{who} removed {}", list(&c.subjects)),
                    K::MemberLeft if c.subjects.is_empty() => {
                        format!("{who} committed someone's leaving")
                    }
                    K::MemberLeft => format!("{} left", list(&c.subjects)),
                    K::MemberJoinedByLink => format!("{who} joined using the invite link"),
                    K::RoleChanged => format!("{who} changed {}'s role", list(&c.subjects)),
                    K::SettingsChanged => format!("{who} changed the group's settings"),
                    K::TitleChanged => format!("{who} changed the group's name"),
                    _ => return None,
                };
                Some(crate::store::ext_groups::MemberChange {
                    at: minutes(c.at_ms),
                    text,
                })
            })
            .collect();
        global::<crate::store::ext_groups::RealChanges>().update(|r| {
            r.0.insert(chat, lines);
        });
        Ok(())
    });
}

/// Sends a sticker in a real chat: the built-in pack's (`pack` `None`) or an installed pack's
/// asset `object`.
pub fn send_sticker(
    s: Store,
    chat: ChatId,
    pack: Option<u64>,
    object: u32,
    emoji: String,
    name: String,
) {
    let Some(target) = core_chat(chat) else {
        return;
    };
    let pack_id = match pack {
        Some(ui) => match super::packs::pack_id(ui) {
            Some(id) => Some(id),
            None => return,
        },
        None => None,
    };
    let ui = s.push_message(
        chat,
        None,
        Body::Sticker {
            emoji: emoji.clone(),
            pack: name,
        },
    );
    if let Some(ui_pack) = pack
        && let Some(src) = super::packs::image(ui_pack, object)
    {
        shown().update(|m| {
            m.0.insert(ui, src);
        });
    }
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let me = m.local().account;
        match m
            .send_sticker(&target, pack_id.as_ref(), object, &emoji)
            .await
        {
            Ok(id) => {
                let status = m
                    .store
                    .message(&me, &id)
                    .ok()
                    .flatten()
                    .map_or(Status::Sent, |r| r.status);
                drop(guard);
                OUTGOING.with_borrow_mut(|o| o.insert(ui, id));
                IDS.with_borrow_mut(|x| x.insert(ui, (me, id)));
                if pack_id.is_some() {
                    MEDIA.with_borrow_mut(|x| x.insert(ui, (me, id)));
                }
                set_status(s, &id, status);
                Ok(())
            }
            Err(_) => {
                drop(guard);
                s.fail(chat, ui);
                Err(super::problem("The sticker couldn't be sent."))
            }
        }
    });
}

/// Whether a real contact saved the user (shared their profile key): only they get broadcasts.
pub fn is_contact(chat: ChatId) -> bool {
    let Some(account) = account_of(chat) else {
        return false;
    };
    let lock = core();
    let Ok(guard) = lock.try_lock() else {
        return true;
    };
    guard
        .as_ref()
        .and_then(|m| m.store.profile_key(&account).ok().flatten())
        .is_some()
}

/// Sends a nudge to a real chat's other person.
pub fn nudge(chat: ChatId) {
    let Some(account) = account_of(chat) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        m.nudge(&account)
            .await
            .map(|_| ())
            .map_err(|_| super::problem("The nudge couldn't be sent."))
    });
}

/// An attention event (a nudge) from the delivery connection.
pub async fn receive_attention(payload: &[u8]) {
    let lock = core();
    let mut guard = lock.lock().await;
    let Some(m) = guard.as_mut() else { return };
    if let Ok(Received::Nudge { account }) = m.receive_attention(payload).await {
        drop(guard);
        if let Some(s) = crate::store::try_store() {
            let chat = ui_id(&account);
            // Nudges off (globally or for them), or blocked: nothing shows.
            let allowed = s.settings.with_untracked(|x| x.nudges)
                && !super::rules::nudges_off()
                && s.contact(chat).is_some_and(|c| !c.nudges_off && !c.blocked);
            if owns(chat) && allowed {
                s.push_message(chat, Some(chat), Body::Nudge);
            }
        }
    }
}

/// Sends a voice note in a real chat.
pub fn send_voice(s: Store, chat: ChatId, rec: songbird_recorder::Recording) {
    let Some(target) = core_chat(chat) else {
        return;
    };
    let ui = s.push_message(
        chat,
        None,
        Body::Voice {
            secs: rec.duration_ms.div_ceil(1000).max(1),
            played: false,
        },
    );
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: "audio/wav".into(),
        file_name: "voice-note.wav".into(),
        duration_ms: rec.duration_ms,
        flags: 1, // FLAG_VOICE_NOTE
        waveform: rec.waveform.clone(),
        ..Default::default()
    };
    let bytes = rec.wav;
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        let m = guard
            .as_mut()
            .ok_or_else(|| super::problem("Not connected yet."))?;
        let me = m.local().account;
        let r = if m.is_group(&target) {
            m.send_group_files(&target, vec![(bytes, meta)], "").await
        } else {
            m.send_files(&target, vec![(bytes, meta)], "").await
        };
        match r {
            Ok(id) => {
                let status = m
                    .store
                    .message(&me, &id)
                    .ok()
                    .flatten()
                    .map_or(Status::Sent, |r| r.status);
                drop(guard);
                OUTGOING.with_borrow_mut(|o| o.insert(ui, id));
                IDS.with_borrow_mut(|x| x.insert(ui, (me, id)));
                MEDIA.with_borrow_mut(|x| x.insert(ui, (me, id)));
                set_status(s, &id, status);
                Ok(())
            }
            Err(_) => {
                drop(guard);
                s.fail(chat, ui);
                Err(super::problem("The voice message couldn't be sent."))
            }
        }
    });
}

/// A real voice note's waveform in `bars` bars (0 to 1), from what its sender measured.
pub fn waveform(msg: MsgId, bars: usize) -> Option<Vec<f32>> {
    let (author, id) = MEDIA.with_borrow(|m| m.get(&msg).copied())?;
    let lock = core();
    let guard = lock.try_lock().ok()?;
    let row = guard.as_ref()?.store.message(&author, &id).ok()??;
    let w = songbird_app_core::messenger::attachments_of(&row.content)
        .into_iter()
        .next()?
        .waveform;
    if w.is_empty() || bars == 0 {
        return None;
    }
    Some(
        (0..bars)
            .map(|i| {
                let at = i * w.len() / bars;
                (f32::from(w.get(at).copied().unwrap_or(0)) / 255.0).max(0.08)
            })
            .collect(),
    )
}

/// A received voice note began playing: kept as played, and its author told (1:1 chats).
pub fn played(msg: MsgId) {
    let Some((author, id)) = MEDIA.with_borrow(|m| m.get(&msg).copied()) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            let _ = m.mark_played(&author, &id).await;
        }
        Ok(())
    });
}

thread_local! {
    /// Kept files by their storage-page ID: (author, message ID).
    static KEPT: RefCell<HashMap<u64, ([u8; 16], [u8; 16])>> = RefCell::new(HashMap::new());
}

/// The files app-core keeps on this device, for Manage storage.
pub fn kept_files() -> Vec<crate::store::ext_groups::StoredFile> {
    use crate::store::ext_groups::{FileKind, StoredFile};
    let lock = core();
    let Ok(guard) = lock.try_lock() else {
        return Vec::new();
    };
    let Some(m) = guard.as_ref() else {
        return Vec::new();
    };
    let files = m.store.file_index().unwrap_or_default();
    drop(guard);
    files
        .into_iter()
        .map(|f| {
            let p = songbird_app_core::messenger::attachments_of(&f.content)
                .into_iter()
                .nth(usize::try_from(f.idx).unwrap_or(0));
            let (kind, name) = match p {
                Some(p) if p.flags & 1 == 1 || p.content_type.starts_with("audio/") => {
                    (FileKind::Voice, "Voice message".to_string())
                }
                Some(p) if p.content_type.starts_with("image/") => (
                    FileKind::Photo,
                    if p.file_name.is_empty() {
                        "Photo".into()
                    } else {
                        p.file_name
                    },
                ),
                Some(p) if p.content_type.starts_with("video/") => (
                    FileKind::Video,
                    if p.file_name.is_empty() {
                        "Video".into()
                    } else {
                        p.file_name
                    },
                ),
                Some(p) => (
                    FileKind::Document,
                    if p.file_name.is_empty() {
                        "File".into()
                    } else {
                        p.file_name
                    },
                ),
                None => (FileKind::Document, "File".into()),
            };
            let id = ui_id(&f.id) ^ u64::from(f.idx);
            KEPT.with_borrow_mut(|k| k.insert(id, (f.author, f.id)));
            StoredFile {
                id,
                chat: ui_id(&f.chat),
                name,
                kind,
                bytes: f.bytes,
                at: minutes(f.stored_ms),
            }
        })
        .collect()
}

/// Deletes kept files from this device (the messages stay; a file is fetched again only while
/// the server still holds it).
pub fn delete_kept(ids: &[u64]) {
    let targets: Vec<([u8; 16], [u8; 16])> =
        KEPT.with_borrow(|k| ids.iter().filter_map(|i| k.get(i).copied()).collect());
    if targets.is_empty() {
        return;
    }
    super::run(async move {
        let lock = core();
        let guard = lock.lock().await;
        if let Some(m) = guard.as_ref() {
            for (a, i) in targets {
                let _ = m.store.drop_files(&a, &i);
            }
        }
        Ok(())
    });
}

/// Marks every real chat read (Chats → Mark all as read).
pub fn mark_all_read(s: Store) {
    let chats: Vec<ChatId> = s.chats.with_untracked(|cs| {
        cs.iter()
            .filter(|c| !c.archived && !c.request && owns(c.id))
            .map(|c| c.id)
            .collect()
    });
    for chat in chats {
        mark_read(chat);
    }
}

/// Starts (or opens) a chat with the account that holds `username`
/// (`POST /v1/username/lookup`: only the username's hash leaves the device).
pub async fn start_with_username(username: &str) -> Result<ChatId, super::Problem> {
    if username.trim().starts_with("https://songbird.chat/g#") {
        return open_group_link(username)
            .await?
            .ok_or_else(|| super::problem("Waiting for an admin to let you in."));
    }
    if username.trim().starts_with("https://songbird.chat/u#") {
        let (name, handle) = super::username::open(username).await?;
        return start_with(&name, Some(handle)).await;
    }
    start_with(username, None).await
}

async fn start_with(username: &str, link: Option<[u8; 16]>) -> Result<ChatId, super::Problem> {
    let name = songbird_keys::username::Username::parse(username.trim().trim_start_matches('@'))
        .map_err(|_| super::problem("That isn't a username."))?;
    let mut q = serde_json::json!({ "hash": wire::hex(&name.hash()) });
    if let Some(h) = link {
        q["link"] = serde_json::Value::String(wire::hex(&h));
    }
    let body = q.to_string().into_bytes();
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
    // A supervised child's new contact waits for the parent's approval (D-0166).
    if !owns(ui_id(&account))
        && !super::rules::may_start(&account, username.trim().trim_start_matches('@'))
    {
        return Err(super::problem(super::rules::NEEDS_APPROVAL));
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

// ---------------------------------------------------------------- security notices (D-0168)

/// Shows what app-core noticed about the people the user talks to: new devices, a device list
/// the key log doesn't vouch for, a log that contradicts itself.
fn show_notices(m: &Core) {
    use songbird_app_core::kt::Notice;
    let Ok(notices) = m.take_notices() else {
        return;
    };
    let Some(s) = crate::store::try_store() else {
        return;
    };
    let me = m.local().account;
    for n in notices {
        match n {
            Notice::NewDevices { account, count } if account == me => {
                let what = if count == 1 {
                    "A new device was"
                } else {
                    "New devices were"
                };
                crate::ui::toast(format!(
                    "{what} linked to your account. If it wasn't you, remove it in Linked devices."
                ));
            }
            Notice::NewDevices { account, count } => {
                let chat = ui_id(&account);
                if owns(chat) {
                    let name = s.chat(chat).map(|c| c.title).unwrap_or_default();
                    let line = if count == 1 {
                        format!("{name} linked a new device")
                    } else {
                        format!("{name} linked {count} new devices")
                    };
                    s.push_message(chat, None, Body::System(line));
                }
            }
            Notice::ListNotInLog { account } => {
                let chat = ui_id(&account);
                if owns(chat) {
                    s.push_message(
                        chat,
                        None,
                        Body::System(
                            "Songbird's key log doesn't match this person's devices. Check your safety number with them before sharing anything sensitive."
                                .into(),
                        ),
                    );
                }
            }
            Notice::Fork => {
                live().update(|l| {
                    l.problem = Some(
                        "Songbird's key log contradicts itself: the server may be showing you something others don't see. Check safety numbers in person."
                            .into(),
                    );
                });
            }
        }
    }
}

// ---------------------------------------------------------------- backups (D-0165)

/// app-core's database for a backup's own connection: (path, key). `create`: a restore on a
/// new device makes the key the store will be opened with; a backup needs a store that exists.
pub fn core_db(create: bool) -> Option<(std::path::PathBuf, zeroize::Zeroizing<[u8; 32]>)> {
    let path = data_dir()?.join(DB_FILE);
    if !create && (!path.exists() || Storage::get_item(K_DB_KEY).is_none()) {
        return None;
    }
    Some((path, zeroize::Zeroizing::new(stored_seed(K_DB_KEY))))
}

/// The UI chat showing app-core chat `real`, if shown.
pub fn ui_chat(s: Store, real: &[u8; 16]) -> Option<ChatId> {
    s.chats.with_untracked(|cs| {
        cs.iter()
            .map(|c| c.id)
            .find(|id| core_chat(*id) == Some(*real))
    })
}

/// Shows every stored chat again (after a restore wrote into the store).
pub async fn reload_all(s: Store) {
    let lock = core();
    let guard = lock.lock().await;
    if let Some(m) = guard.as_ref() {
        show_all(s, m);
    }
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
