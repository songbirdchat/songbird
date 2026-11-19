//! The message pipeline (A2 M3, D-0138): what turns "send this text to Bob" into envelopes, and
//! an envelope into a stored message.
//!
//! The network sits behind [`Net`], so the same code runs in the app (NSH's HTTP and the delivery
//! client) and in the end-to-end tests (hyper and the client against an in-process server).
//! Every state change is written to the store before the caller acknowledges an envelope
//! (invariant 1 on the device: what was acknowledged is never lost).
//!
//! # Sending (PROTOCOL §3.1, §5.3, §5.10)
//!
//! 1. The content is sealed (`crates/content`: padding, a franking key for visible messages),
//!    stored as `Sending` with its idempotency key in the outbox.
//! 2. The recipient account's devices come from its **signed device list**, verified under its
//!    account identity key (pinned on first sight; a changed key is refused until the user
//!    accepts it), never rolled back. Devices without a session get one from a fetched bundle.
//! 3. One envelope per device; hints: the franking commitment, and the recipient's access key
//!    when their profile key is known, else a message request.
//! 4. A device mismatch refreshes the list (forgetting stale sessions) and retries with the same
//!    idempotency key; a retryable rejection leaves the message in the outbox; anything else
//!    marks it failed.
//! 5. This account's other devices get a `SentTranscript` (sync), so every device shows the chat.
//!
//! # Receiving
//!
//! A franking stamp is split off; system frames are verified with the pinned system key; the
//! sealed layer names the sender, whose device identity comes from their verified list; the
//! ratchet decrypts; `crates/content` opens and validates; a franked message whose commitment
//! does not open is dropped (§5.3); then the message is stored once and a delivery receipt goes
//! back (silent, unfranked).

use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use prost::Message as _;
use songbird_content::schema::{
    self, ChatRef, Content, DataMessage, ReceiptMessage, SentTranscript, SyncMessage, Text,
    content::Body, data_message, receipt_message, sync_message,
};
use songbird_proto::control::DeviceMismatch;
use songbird_proto::envelope::Stamp;
use songbird_proto::frame::{RejectReason, SendHints, SendStatus};
use zeroize::Zeroizing;

use crate::crypto::pqxdh::Bundle;
use crate::crypto::{Reader, random};
use crate::session::{Local, Prekeys, Sessions};
use crate::store::{MessageRow, Status, Store};
use crate::{Error, Result};

/// How many times a send is retried after device mismatches.
const MISMATCH_RETRIES: usize = 3;
/// One-time prekeys kept on the server (refilled below [`REFILL_BELOW`]).
pub const ONE_TIME_TARGET: usize = 100;
/// Refill when fewer than this many one-time keys of a kind are left.
pub const REFILL_BELOW: usize = 10;
const LIST_CONTEXT: &[u8] = b"songbird.v1/device-list\0";

/// The network, as the pipeline needs it. Requests to `api` are signed by the implementor with
/// this device's authentication key (`services/api/src/auth.rs`).
#[allow(
    async_fn_in_trait,
    reason = "used through generics, never as a trait object"
)]
pub trait Net {
    /// A signed `api` request: the status and body.
    async fn api(&self, method: &str, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>)>;
    /// A signed `api` request whose body is not covered by the signature (attachment uploads:
    /// the client's own ciphertext, held only to its length, PROTOCOL §7.2).
    async fn upload(&self, method: &str, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>)>;
    /// An unsigned `GET` of an `api` path or a URL the api gave (downloads carry no account,
    /// §7.3): the status, body and any `Location`.
    async fn fetch(&self, path_or_url: &str) -> Result<(u16, Vec<u8>, Option<String>)>;
    /// An unsigned `POST` with no body (a download's confirmation).
    async fn fetch_post(&self, path: &str) -> Result<u16>;
    /// An ephemeral event to one device (typing): never stored, dropped first under pressure.
    async fn ephemeral(&self, device: DeviceIdBytes, payload: Vec<u8>) -> Result<()>;
    /// A group message: one shared body the server fans out to the group's roster (§6.5).
    async fn send_group(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        body: Vec<u8>,
    ) -> Result<SendStatus>;
    /// A `Send` over the delivery connection (or `PUT /v1/messages`).
    async fn send(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        recipients: Vec<(DeviceIdBytes, Vec<u8>)>,
    ) -> Result<SendStatus>;
}

/// A device ID's 16 bytes.
pub type DeviceIdBytes = [u8; 16];

/// What a received envelope produced, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// A new message in a chat (1:1: the chat is the other account).
    Message(MessageRow),
    /// A status change of one of our messages.
    Status {
        /// The message.
        id: [u8; 16],
        /// Its new status.
        status: Status,
    },
    /// Someone reacted to a message (empty `emoji`: removed).
    Reaction {
        /// The message's author.
        author: [u8; 16],
        /// The message.
        id: [u8; 16],
        /// Who reacted.
        reactor: [u8; 16],
        /// The emoji, empty when removed.
        emoji: String,
    },
    /// A message's author edited it.
    Edited {
        /// The author.
        author: [u8; 16],
        /// The message.
        id: [u8; 16],
        /// Its new text.
        text: String,
    },
    /// A message was deleted for everyone.
    Deleted {
        /// The author.
        author: [u8; 16],
        /// The message.
        id: [u8; 16],
    },
    /// Someone started or stopped typing (an ephemeral event, never stored).
    Typing {
        /// Who.
        account: [u8; 16],
        /// Started (else stopped).
        started: bool,
    },
    /// This device joined a group (a Welcome).
    Joined {
        /// The group.
        group: [u8; 16],
    },
    /// A group's title or membership changed.
    GroupUpdated {
        /// The group.
        group: [u8; 16],
        /// Its title, when it changed.
        title: String,
    },
    /// A system notice was handled (prekeys refilled, the device list refreshed).
    System,
    /// Nothing to show (a duplicate, a dropped or unreadable envelope).
    Nothing,
}

/// The client core: identity, keys, sessions and the store, over a network.
#[derive(Debug)]
pub struct Messenger<N> {
    /// The store.
    pub store: Store,
    pub(crate) local: Local,
    prekeys: Prekeys,
    pub(crate) sessions: Sessions,
    pub(crate) net: N,
    /// The server's system-notice key (PROTOCOL §3.9), when pinned.
    system_key: Option<[u8; 32]>,
    /// Groups (D-0145): OpenMLS state and this device's group signing key.
    pub(crate) mls: crate::mls::DeviceProvider,
    pub(crate) mls_signer: crate::mls::crypto::DeviceSigner,
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub(crate) fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

pub(crate) fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2 {
        return Err(Error::Malformed);
    }
    let mut out = [0u8; N];
    for (o, i) in out.iter_mut().zip((0..s.len()).step_by(2)) {
        *o = u8::from_str_radix(s.get(i..i + 2).ok_or(Error::Malformed)?, 16)
            .map_err(|_| Error::Malformed)?;
    }
    Ok(out)
}

pub(crate) fn json(body: &[u8]) -> Result<serde_json::Value> {
    serde_json::from_slice(body).map_err(|_| Error::Malformed)
}

/// A verified signed device list (PROTOCOL §4.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceList {
    /// Its version.
    pub version: u64,
    /// The devices, in the list's order.
    pub devices: Vec<ListedDevice>,
}

/// One device of a list: (device, auth key, device identity key `u`, primary).
pub type ListedDevice = ([u8; 16], [u8; 32], [u8; 32], bool);

impl DeviceList {
    /// Parses and verifies a list under the account identity key.
    pub fn verify(bytes: &[u8], account: &[u8; 16], identity: &[u8; 32]) -> Result<Self> {
        let Some(signed_len) = bytes.len().checked_sub(64) else {
            return Err(Error::Malformed);
        };
        let (signed, sig) = bytes.split_at(signed_len);
        let mut msg = LIST_CONTEXT.to_vec();
        msg.extend_from_slice(signed);
        UnparsedPublicKey::new(&ED25519, identity)
            .verify(&msg, sig)
            .map_err(|_| Error::Verify)?;
        let mut r = Reader::new(signed);
        if r.u8()? != 1 || r.array::<16>()? != *account {
            return Err(Error::Verify);
        }
        let version = r.u64()?;
        let _issued = r.u64()?;
        let n = r.u8()?;
        if n == 0 || n > 16 {
            return Err(Error::Malformed);
        }
        let mut devices: Vec<ListedDevice> = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            let d = r.array()?;
            let auth = r.array()?;
            let ik = crate::crypto::parse_x25519(r.take(33)?)?;
            let flags = r.u8()?;
            if flags > 1 || devices.iter().any(|(x, ..)| *x == d) {
                return Err(Error::Malformed);
            }
            devices.push((d, auth, ik, flags == 1));
        }
        r.finish()?;
        if devices.iter().filter(|x| x.3).count() != 1 {
            return Err(Error::Malformed);
        }
        Ok(Self { version, devices })
    }

    /// Encodes and signs a list with the account identity key (the primary only).
    pub fn sign(&self, account: &[u8; 16], key: &Ed25519KeyPair) -> Result<Vec<u8>> {
        let mut o = vec![1];
        o.extend_from_slice(account);
        o.extend_from_slice(&self.version.to_be_bytes());
        o.extend_from_slice(&now_ms().to_be_bytes());
        o.push(u8::try_from(self.devices.len()).map_err(|_| Error::Malformed)?);
        for (d, auth, ik, primary) in &self.devices {
            o.extend_from_slice(d);
            o.extend_from_slice(auth);
            o.extend_from_slice(&crate::crypto::serialize_x25519(ik));
            o.push(u8::from(*primary));
        }
        let mut msg = LIST_CONTEXT.to_vec();
        msg.extend_from_slice(&o);
        o.extend_from_slice(key.sign(&msg).as_ref());
        Ok(o)
    }
}

impl<N: Net> Messenger<N> {
    /// Opens the messenger over a store that holds this device's identity.
    pub fn open(store: Store, net: N, system_key: Option<[u8; 32]>) -> Result<Self> {
        let local = store.local()?.ok_or(Error::NoSession)?;
        let prekeys = store.prekeys()?;
        let sessions = store.sessions()?;
        let mls = match store.secret("mls_state")? {
            Some(b) => crate::mls::DeviceProvider::restore(&b)?,
            None => crate::mls::DeviceProvider::default(),
        };
        let mls_signer = if let Some(seed) = store.secret("mls_signer")? {
            crate::mls::crypto::DeviceSigner::from_seed(&seed).map_err(|_| Error::Storage)?
        } else {
            let s = crate::mls::crypto::DeviceSigner::generate().map_err(|_| Error::Random)?;
            store.put_secret("mls_signer", s.seed())?;
            s
        };
        Ok(Self {
            store,
            local,
            prekeys,
            sessions,
            net,
            system_key,
            mls,
            mls_signer,
        })
    }

    /// A new device's messenger: stores its identity first.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the identity moves into the store and the messenger"
    )]
    pub fn create(
        store: Store,
        net: N,
        local: Local,
        system_key: Option<[u8; 32]>,
    ) -> Result<Self> {
        store.save_local(&local)?;
        Self::open(store, net, system_key)
    }

    /// This device.
    #[must_use]
    pub fn local(&self) -> &Local {
        &self.local
    }

    /// The network.
    pub fn net(&self) -> &N {
        &self.net
    }

    /// Uploads prekeys (`PUT /v1/keys`): the identity, signed and last-resort keys the first
    /// time or when `rotate`, and one-time keys up to [`ONE_TIME_TARGET`].
    pub async fn publish_keys(&mut self, rotate: bool) -> Result<()> {
        let (ec, kem) = self.prekeys.counts();
        let body = self.prekeys.upload(
            &self.local,
            rotate,
            ONE_TIME_TARGET.saturating_sub(ec),
            ONE_TIME_TARGET.saturating_sub(kem),
        )?;
        // The new private keys are stored before the server can hand out their public halves.
        self.store.save_prekeys(&self.prekeys)?;
        let (status, _) = self.net.api("PUT", "/v1/keys", body).await?;
        if status != 204 {
            return Err(Error::Server(status));
        }
        self.prekeys.uploaded();
        self.store.save_prekeys(&self.prekeys)
    }

    /// Refills one-time keys when the server holds fewer than [`REFILL_BELOW`] of a kind.
    pub async fn refill_keys(&mut self) -> Result<()> {
        let (status, body) = self.net.api("GET", "/v1/keys", Vec::new()).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let v = json(&body)?;
        let low = |k: &str| {
            v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0)
                < u64::try_from(REFILL_BELOW).unwrap_or(u64::MAX)
        };
        if low("ec") || low("kem") {
            self.publish_keys(false).await?;
        }
        Ok(())
    }

    /// This account's profile key, drawn on first use and kept in the store.
    pub fn profile_key(&self) -> Result<Zeroizing<[u8; 32]>> {
        if let Some(k) = self.store.secret("profile_key")?
            && let Ok(k) = <[u8; 32]>::try_from(k.as_slice())
        {
            return Ok(Zeroizing::new(k));
        }
        let k = random::<32>()?;
        self.store.put_secret("profile_key", k.as_ref())?;
        Ok(k)
    }

    /// Publishes the encrypted profile with this account's access key (`PUT /v1/profile`,
    /// PROTOCOL §4.8; every app publishes one at registration, §5.10). `open`: anyone may send.
    pub async fn publish_profile(
        &mut self,
        profile: &songbird_content::profile::ProfilePlain,
        open: bool,
    ) -> Result<()> {
        let key = self.profile_key()?;
        let version = self
            .store
            .secret("profile_version")?
            .and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
            .map_or(0, u64::from_be_bytes)
            + 1;
        let blob = songbird_content::profile::seal(&key, version, profile)
            .map_err(|_| Error::Malformed)?;
        let mut body = vec![1];
        body.extend_from_slice(&version.to_be_bytes());
        body.extend_from_slice(&songbird_keys::sealed::access_key(&key));
        body.push(u8::from(open));
        body.extend_from_slice(
            &u16::try_from(blob.len())
                .map_err(|_| Error::Malformed)?
                .to_be_bytes(),
        );
        body.extend_from_slice(&blob);
        let (status, _) = self.net.api("PUT", "/v1/profile", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        self.store
            .put_secret("profile_version", &version.to_be_bytes())
    }

    /// Accepts a message request: shares this account's profile key with the requester, who
    /// then sends as a contact (PROTOCOL §5.10). Replying to a request accepts it too.
    pub async fn accept_request(&mut self, from: &[u8; 16]) -> Result<()> {
        let rr = schema::RequestResponse {
            action: schema::request_response::Action::Accept.into(),
            chat: Some(ChatRef {
                account: self.local.account.to_vec(),
                group_id: Vec::new(),
            }),
        };
        let devices = self.devices_of(from, false).await?;
        self.send_silent_with(from, &devices, Body::RequestResponse(rr), true)
            .await
    }

    /// Signs and uploads this account's device list from the server's view (the primary, with
    /// the account identity key): every enabled device whose confirmation `confirm_ok` accepts
    /// (linked devices are checked by the caller against the link's confirmation key).
    pub async fn publish_device_list(
        &mut self,
        account_key: &Ed25519KeyPair,
        confirm_ok: impl Fn(&[u8; 16], &str) -> bool,
    ) -> Result<()> {
        let (status, body) = self.net.api("GET", "/v1/devices", Vec::new()).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let v = json(&body)?;
        let version = v
            .get("list_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            + 1;
        let mut devices = Vec::new();
        for d in v
            .get("devices")
            .and_then(serde_json::Value::as_array)
            .ok_or(Error::Malformed)?
        {
            let s = |k: &str| {
                d.get(k)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            };
            let id: [u8; 16] = unhex(s("device"))?;
            let primary = d
                .get("primary")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !primary && !confirm_ok(&id, s("confirm")) {
                continue;
            }
            let Ok(ik) = unhex::<33>(s("identity_key")) else {
                continue;
            };
            devices.push((
                id,
                unhex(s("auth_key"))?,
                crate::crypto::parse_x25519(&ik)?,
                primary,
            ));
        }
        let list = DeviceList { version, devices };
        let body = list.sign(&self.local.account, account_key)?;
        let (status, _) = self.net.api("PUT", "/v1/devices/list", body).await?;
        if status != 204 {
            return Err(Error::Server(status));
        }
        // Our own account's list, kept like anyone's.
        let key: [u8; 32] = account_key
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        self.store.pin_account(&self.local.account, &key)?;
        let pairs: Vec<_> = list.devices.iter().map(|(d, _, ik, _)| (*d, *ik)).collect();
        self.store
            .set_devices(&self.local.account, list.version, &pairs)?;
        Ok(())
    }

    /// An account's devices from its verified list (fetched when `force` or unknown).
    pub async fn devices_of(
        &mut self,
        account: &[u8; 16],
        force: bool,
    ) -> Result<Vec<([u8; 16], [u8; 32])>> {
        if !force {
            let known = self.store.devices(account)?;
            if !known.is_empty() {
                return Ok(known);
            }
        }
        let body = serde_json::json!({ "account": hex(account) })
            .to_string()
            .into_bytes();
        let identity = if let Some(k) = self.store.account_key(account)? {
            k
        } else {
            let (status, b) = self
                .net
                .api("POST", "/v1/devices/identity", body.clone())
                .await?;
            if status != 200 {
                return Err(Error::Server(status));
            }
            let v = json(&b)?;
            let k: [u8; 32] = unhex(
                v.get("identity_key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )?;
            if !songbird_keys::ed25519_public_key_ok(&k) || !self.store.pin_account(account, &k)? {
                return Err(Error::IdentityChanged);
            }
            k
        };
        let (status, list) = self.net.api("POST", "/v1/devices/list/fetch", body).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let list = DeviceList::verify(&list, account, &identity)?;
        let pairs: Vec<_> = list.devices.iter().map(|(d, _, ik, _)| (*d, *ik)).collect();
        let removed = self.store.set_devices(account, list.version, &pairs)?;
        for d in removed {
            self.sessions.remove(&(*account, d));
            self.store.save_sessions(&self.sessions, &(*account, d))?;
        }
        // A device whose identity differs from our session's (relinked): forget the session.
        for (d, ik) in &pairs {
            if self
                .sessions
                .identity(&(*account, *d))
                .is_some_and(|k| k != *ik)
            {
                self.sessions.remove(&(*account, *d));
                self.store.save_sessions(&self.sessions, &(*account, *d))?;
            }
        }
        Ok(pairs)
    }

    async fn ensure_sessions(
        &mut self,
        account: &[u8; 16],
        devices: &[([u8; 16], [u8; 32])],
    ) -> Result<()> {
        let missing: Vec<_> = devices
            .iter()
            .filter(|(d, _)| !self.sessions.has(&(*account, *d)))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let body = serde_json::json!({ "account": hex(account) })
            .to_string()
            .into_bytes();
        let (status, b) = self.net.api("POST", "/v1/keys/fetch", body).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        for bundle in Bundle::parse_all(&b)? {
            // Only devices the verified list names, with exactly the identity it names.
            let Some((_, ik)) = missing.iter().find(|(d, _)| *d == bundle.device) else {
                continue;
            };
            if bundle.identity != *ik {
                return Err(Error::Verify);
            }
            self.sessions.start(&self.local, *account, &bundle)?;
        }
        Ok(())
    }

    /// Sends a text to a 1:1 chat. Answers the message ID.
    pub async fn send_text(&mut self, to: &[u8; 16], text: &str) -> Result<[u8; 16]> {
        let message = schema::Message {
            text: Some(Text {
                body: text.into(),
                spans: Vec::new(),
            }),
            ..Default::default()
        };
        let data = DataMessage {
            kind: Some(data_message::Kind::Message(message)),
            ..Default::default()
        };
        self.send_data(to, data, text).await
    }

    /// Sends files (photos, documents, voice notes) to a 1:1 chat: each encrypted on this device,
    /// uploaded for every recipient device and this account's other devices, and named by
    /// pointers inside the encrypted message (D-0141). Answers the message ID.
    pub async fn send_files(
        &mut self,
        to: &[u8; 16],
        files: Vec<(Vec<u8>, crate::attachments::FileMeta)>,
        text: &str,
    ) -> Result<[u8; 16]> {
        let theirs = self.devices_of(to, false).await?.len();
        let me = self.local.account;
        let mine = self.store.devices(&me)?.len().saturating_sub(1);
        let mut pointers = Vec::with_capacity(files.len());
        let mut preview = String::new();
        for (bytes, meta) in &files {
            let (mut p, ct) = crate::attachments::encrypt(bytes, meta)?;
            let id = crate::attachments::upload(&self.net, &ct, theirs + mine).await?;
            p.object_id = id.to_vec();
            p.uploaded_at_ms = now_ms();
            if preview.is_empty() {
                preview = attachment_label(&p);
            }
            pointers.push(p);
        }
        let message = schema::Message {
            text: (!text.is_empty()).then(|| Text {
                body: text.into(),
                spans: Vec::new(),
            }),
            attachments: pointers,
            ..Default::default()
        };
        let data = DataMessage {
            kind: Some(data_message::Kind::Message(message)),
            ..Default::default()
        };
        let shown = if text.is_empty() {
            preview
        } else {
            text.to_owned()
        };
        let id = self.send_data(to, data, &shown).await?;
        // The sender keeps its own files (encrypted at rest, in the store).
        for (i, (bytes, _)) in files.iter().enumerate() {
            self.store
                .put_file(&me, &id, u32::try_from(i).unwrap_or(u32::MAX), bytes)?;
        }
        Ok(id)
    }

    /// The `idx`-th file of a stored message: from the store, or downloaded, verified and kept.
    pub async fn file(
        &mut self,
        author: &[u8; 16],
        id: &[u8; 16],
        idx: u32,
    ) -> Result<Zeroizing<Vec<u8>>> {
        if let Some(b) = self.store.file(author, id, idx)? {
            return Ok(b);
        }
        let row = self.store.message(author, id)?.ok_or(Error::Malformed)?;
        let pointer = attachments_of(&row.content)
            .into_iter()
            .nth(usize::try_from(idx).map_err(|_| Error::Malformed)?)
            .ok_or(Error::Malformed)?;
        let bytes = crate::attachments::download(&self.net, &pointer).await?;
        self.store.put_file(author, id, idx, &bytes)?;
        Ok(bytes)
    }

    /// Sends an event about an earlier message (a reaction, an edit, a delete): franked like any
    /// visible message, kept in the outbox until accepted, but not a message of its own.
    async fn send_event(&mut self, to: &[u8; 16], kind: data_message::Kind) -> Result<()> {
        let id = *random::<16>()?;
        let sent_at = now_ms();
        let content = Content {
            version: songbird_content::VERSION,
            required_version: 1,
            message_id: id.to_vec(),
            sent_at_ms: sent_at,
            body: Some(Body::Data(DataMessage {
                kind: Some(kind),
                ..Default::default()
            })),
        };
        let idem = *random::<16>()?;
        self.store
            .outbox_put(&idem, to, &id, &content.encode_to_vec(), sent_at)?;
        self.deliver(idem, to, &id, &content).await
    }

    fn next_counter(&self, key: &str) -> Result<u64> {
        let n = self
            .store
            .secret(key)?
            .and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
            .map_or(0, u64::from_be_bytes)
            .max(now_ms())
            + 1;
        self.store.put_secret(key, &n.to_be_bytes())?;
        Ok(n)
    }

    /// Reacts to a message in a 1:1 chat (empty `emoji` removes this account's reaction).
    pub async fn react(
        &mut self,
        chat: &[u8; 16],
        author: &[u8; 16],
        id: &[u8; 16],
        emoji: &str,
    ) -> Result<()> {
        let counter = self.next_counter("reaction_counter")?;
        let me = self.local.account;
        self.store.react(author, id, &me, emoji, counter)?;
        let r = schema::Reaction {
            target: Some(schema::MessageRef {
                author: author.to_vec(),
                message_id: id.to_vec(),
            }),
            emoji: emoji.into(),
            remove: emoji.is_empty(),
            counter,
        };
        self.send_event(chat, data_message::Kind::Reaction(r)).await
    }

    /// Edits one of this account's messages.
    pub async fn edit(&mut self, chat: &[u8; 16], id: &[u8; 16], text: &str) -> Result<()> {
        let me = self.local.account;
        self.store.message(&me, id)?.ok_or(Error::Malformed)?;
        let revision =
            u32::try_from(self.next_counter("edit_revision")? % u64::from(u32::MAX)).unwrap_or(1);
        self.store.set_text(&me, id, text)?;
        let e = schema::Edit {
            target_message_id: id.to_vec(),
            text: Some(Text {
                body: text.into(),
                spans: Vec::new(),
            }),
            revision,
        };
        self.send_event(chat, data_message::Kind::Edit(e)).await
    }

    /// Deletes one of this account's messages for everyone.
    pub async fn delete_for_everyone(&mut self, chat: &[u8; 16], id: &[u8; 16]) -> Result<()> {
        let me = self.local.account;
        // Only this account's own messages (the recipient checks it too).
        self.store.message(&me, id)?.ok_or(Error::Malformed)?;
        self.store.mark_deleted(&me, id)?;
        let d = schema::Delete {
            target: Some(schema::MessageRef {
                author: me.to_vec(),
                message_id: id.to_vec(),
            }),
        };
        self.send_event(chat, data_message::Kind::Delete(d)).await
    }

    /// Blocks an account (PROTOCOL §5.11): the server drops its requests and sends to an open
    /// account; this device drops what it still sends; and the profile key is rotated, the new
    /// one shared with every remaining contact, so the old access key stops working. `new_key` is
    /// the rotated profile key, already published with the profile.
    pub async fn block(&mut self, account: &[u8; 16], new_key: &[u8; 32]) -> Result<()> {
        let body = serde_json::json!({ "account": hex(account) })
            .to_string()
            .into_bytes();
        let (status, _) = self.net.api("PUT", "/v1/blocks", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        self.store.set_blocked(account, true)?;
        self.store.put_secret("profile_key", new_key)?;
        for contact in self.store.contacts()? {
            if contact == *account || contact == self.local.account {
                continue;
            }
            let Ok(devices) = self.devices_of(&contact, false).await else {
                continue;
            };
            let changed = songbird_content::profile::ProfileChanged { version: 0 };
            let _ = self
                .send_silent_with(&contact, &devices, Body::ProfileChanged(changed), true)
                .await;
        }
        Ok(())
    }

    /// Unblocks an account (`DELETE /v1/blocks`, the primary only).
    pub async fn unblock(&mut self, account: &[u8; 16]) -> Result<()> {
        let body = serde_json::json!({ "account": hex(account) })
            .to_string()
            .into_bytes();
        let (status, _) = self.net.api("DELETE", "/v1/blocks", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        self.store.set_blocked(account, false)
    }

    /// Reports the newest `n` messages from `author` (PROTOCOL §5.14), each with its proof: the
    /// server learns those messages and who sent them, nothing else. Answers how many it took.
    pub async fn report(&mut self, author: &[u8; 16], reason: u8, n: u32) -> Result<usize> {
        let mut sent = 0;
        for (stamp, fk, content) in self.store.franks(author, n)? {
            let mut body = vec![1, reason];
            body.extend_from_slice(&stamp);
            body.extend_from_slice(author);
            body.extend_from_slice(&fk);
            body.extend_from_slice(
                &u32::try_from(content.len())
                    .map_err(|_| Error::Malformed)?
                    .to_be_bytes(),
            );
            body.extend_from_slice(&content);
            let (status, _) = self.net.api("POST", "/v1/reports", body).await?;
            match status {
                204 | 409 => sent += 1,
                s => return Err(Error::Server(s)),
            }
        }
        Ok(sent)
    }

    /// Tells a 1:1 chat's devices this account started or stopped typing (ephemeral, §5.7).
    /// Only devices with a session hear it: typing never starts a session.
    pub async fn typing(&mut self, to: &[u8; 16], started: bool) -> Result<()> {
        let content = Content {
            version: songbird_content::VERSION,
            required_version: 1,
            message_id: random::<16>()?.to_vec(),
            sent_at_ms: now_ms(),
            body: Some(Body::Typing(schema::TypingMessage {
                started,
                group_id: Vec::new(),
            })),
        };
        let (padded, _) = songbird_content::seal(&content, None).map_err(|_| Error::Malformed)?;
        for (d, _) in self.store.devices(to)? {
            let addr = (*to, d);
            if !self.sessions.has(&addr) {
                continue;
            }
            let env = self.sessions.encrypt(&self.local, &addr, &padded)?;
            self.store.save_sessions(&self.sessions, &addr)?;
            let _ = self.net.ephemeral(d, env).await;
        }
        Ok(())
    }

    /// Sends a location the user picked on the map (PROTOCOL §5.4 `LocationPin`).
    pub async fn send_location(
        &mut self,
        to: &[u8; 16],
        lat: f64,
        lon: f64,
        name: &str,
    ) -> Result<[u8; 16]> {
        let message = schema::Message {
            location: Some(schema::LocationPin {
                latitude_e7: crate::maps::e7(lat, 90.0),
                longitude_e7: crate::maps::e7(lon, 180.0),
                name: name.into(),
                address: String::new(),
            }),
            ..Default::default()
        };
        let data = DataMessage {
            kind: Some(data_message::Kind::Message(message)),
            ..Default::default()
        };
        self.send_data(to, data, "Location").await
    }

    /// Sends a visible message (franked) to a 1:1 chat.
    pub async fn send_data(
        &mut self,
        to: &[u8; 16],
        data: DataMessage,
        preview: &str,
    ) -> Result<[u8; 16]> {
        let id = *random::<16>()?;
        let sent_at = now_ms();
        let content = Content {
            version: songbird_content::VERSION,
            required_version: 1,
            message_id: id.to_vec(),
            sent_at_ms: sent_at,
            body: Some(Body::Data(data)),
        };
        let bytes = content.encode_to_vec();
        self.store.insert_message(
            &MessageRow {
                chat: *to,
                author: self.local.account,
                id,
                sent_ms: sent_at,
                stored_ms: sent_at,
                text: preview.into(),
                content: bytes.clone(),
                status: Status::Sending,
            },
            false,
        )?;
        let idem = *random::<16>()?;
        self.store.outbox_put(&idem, to, &id, &bytes, sent_at)?;
        self.deliver(idem, to, &id, &content).await?;
        Ok(id)
    }

    /// Retries everything in the outbox (after a reconnect).
    pub async fn flush_outbox(&mut self) -> Result<()> {
        for (idem, chat, id, bytes) in self.store.outbox()? {
            let content = Content::decode(bytes.as_slice()).map_err(|_| Error::Malformed)?;
            self.deliver(idem, &chat, &id, &content).await?;
        }
        Ok(())
    }

    async fn deliver(
        &mut self,
        idem: [u8; 16],
        to: &[u8; 16],
        id: &[u8; 16],
        content: &Content,
    ) -> Result<()> {
        let franking = *random::<32>()?;
        // Writing to someone shares this account's profile key with them (a request is an
        // invitation to talk; a reply accepts theirs).
        let profile_key = Some(*self.profile_key()?);
        let (padded, commitment) =
            songbird_content::seal_with_profile_key(content, Some(franking), profile_key)
                .map_err(|_| Error::Malformed)?;
        let padded = Zeroizing::new(padded);
        let access = self
            .store
            .profile_key(to)?
            .map(|k| songbird_keys::sealed::access_key(&k));
        let hints = SendHints {
            access,
            request: access.is_none(),
            commitment,
            ..SendHints::default()
        };
        let mut force = false;
        for _ in 0..=MISMATCH_RETRIES {
            let devices = self.devices_of(to, force).await?;
            self.ensure_sessions(to, &devices).await?;
            let mut recipients = Vec::with_capacity(devices.len());
            for (d, _) in &devices {
                recipients.push((*d, self.sessions.encrypt(&self.local, &(*to, *d), &padded)?));
            }
            self.save_sessions_of(to, &devices)?;
            match self.net.send(idem, hints, recipients).await? {
                SendStatus::Accepted => {
                    self.store.advance(&self.local.account, id, Status::Sent)?;
                    self.store.outbox_done(&idem)?;
                    self.sync_sent(to, content, &franking).await;
                    return Ok(());
                }
                SendStatus::DeviceMismatch(m) => {
                    self.on_mismatch(to, &m)?;
                    force = true;
                }
                SendStatus::Rejected {
                    reason: RejectReason::RateLimited | RejectReason::Unavailable,
                    ..
                } => {
                    return Ok(()); // stays in the outbox
                }
                SendStatus::Rejected { .. } => break,
            }
        }
        self.store
            .advance(&self.local.account, id, Status::Failed)?;
        self.store.outbox_done(&idem)
    }

    fn on_mismatch(&mut self, to: &[u8; 16], m: &DeviceMismatch) -> Result<()> {
        for d in m.stale.iter().chain(&m.extra) {
            if let Ok(d) = <[u8; 16]>::try_from(d.as_slice()) {
                self.sessions.remove(&(*to, d));
                self.store.save_sessions(&self.sessions, &(*to, d))?;
            }
        }
        Ok(())
    }

    fn save_sessions_of(
        &mut self,
        account: &[u8; 16],
        devices: &[([u8; 16], [u8; 32])],
    ) -> Result<()> {
        for (d, _) in devices {
            self.store.save_sessions(&self.sessions, &(*account, *d))?;
        }
        Ok(())
    }

    /// Tells this account's other devices what was sent (best effort: a failure here never
    /// fails the send; they also learn of it from the chat's later traffic).
    async fn sync_sent(&mut self, to: &[u8; 16], content: &Content, franking: &[u8; 32]) {
        let sync = SyncMessage {
            kind: Some(sync_message::Kind::Sent(SentTranscript {
                chat: Some(ChatRef {
                    account: to.to_vec(),
                    group_id: Vec::new(),
                }),
                broadcast_to: Vec::new(),
                content: content.encode_to_vec(),
                franking_key: franking.to_vec(),
                stored_at_ms: now_ms(),
            })),
        };
        let _ = self.send_own(Body::Sync(sync)).await;
    }

    async fn send_own(&mut self, body: Body) -> Result<()> {
        let me = self.local.account;
        let mine: Vec<_> = self
            .store
            .devices(&me)?
            .into_iter()
            .filter(|(d, _)| *d != self.local.device)
            .collect();
        if mine.is_empty() {
            return Ok(());
        }
        self.send_silent(&me, &mine, body).await
    }

    async fn send_silent(
        &mut self,
        to: &[u8; 16],
        devices: &[([u8; 16], [u8; 32])],
        body: Body,
    ) -> Result<()> {
        self.send_silent_with(to, devices, body, false).await
    }

    async fn send_silent_with(
        &mut self,
        to: &[u8; 16],
        devices: &[([u8; 16], [u8; 32])],
        body: Body,
        share_profile_key: bool,
    ) -> Result<()> {
        let content = Content {
            version: songbird_content::VERSION,
            required_version: 1,
            message_id: random::<16>()?.to_vec(),
            sent_at_ms: now_ms(),
            body: Some(body),
        };
        let pk = if share_profile_key {
            Some(*self.profile_key()?)
        } else {
            None
        };
        let (padded, _) = songbird_content::seal_with_profile_key(&content, None, pk)
            .map_err(|_| Error::Malformed)?;
        self.ensure_sessions(to, devices).await?;
        let mut recipients = Vec::with_capacity(devices.len());
        for (d, _) in devices {
            recipients.push((*d, self.sessions.encrypt(&self.local, &(*to, *d), &padded)?));
        }
        self.save_sessions_of(to, devices)?;
        let access = if *to == self.local.account {
            None
        } else {
            self.store
                .profile_key(to)?
                .map(|k| songbird_keys::sealed::access_key(&k))
        };
        let hints = SendHints {
            access,
            silent: true,
            ..SendHints::default()
        };
        match self.net.send(*random::<16>()?, hints, recipients).await? {
            SendStatus::Accepted => Ok(()),
            _ => Err(Error::Server(0)),
        }
    }

    /// Sends receipts for `ids` (messages by `author`) at `kind`.
    pub async fn send_receipt(
        &mut self,
        author: &[u8; 16],
        ids: &[[u8; 16]],
        kind: receipt_message::Kind,
    ) -> Result<()> {
        if ids.is_empty() || *author == self.local.account {
            return Ok(());
        }
        let devices = self.devices_of(author, false).await?;
        let receipt = ReceiptMessage {
            kind: kind.into(),
            group_id: Vec::new(),
            message_ids: ids.iter().map(|i| i.to_vec()).collect(),
            up_to: Vec::new(),
        };
        self.send_silent(author, &devices, Body::Receipt(receipt))
            .await
    }

    /// Marks a chat read and tells the authors.
    pub async fn mark_read(&mut self, chat: &[u8; 16]) -> Result<()> {
        let unread = self.store.mark_read(chat, &self.local.account)?;
        let ids: Vec<_> = unread.iter().map(|(_, id)| *id).collect();
        self.send_receipt(chat, &ids, receipt_message::Kind::Read)
            .await
    }

    /// Handles one delivered envelope. Every change is stored before this returns; the caller
    /// then acknowledges its `seq`. Unreadable envelopes answer `Nothing` (they must not block
    /// the mailbox).
    #[allow(
        clippy::too_many_lines,
        reason = "one match over the content kinds, read top to bottom"
    )]
    pub async fn receive(&mut self, envelope: &[u8]) -> Result<Received> {
        let (stamp, envelope) = match Stamp::split(envelope) {
            Ok((s, rest)) => (Some(s), rest),
            Err(_) if songbird_proto::envelope::is_reserved(envelope) => {
                return self.system(envelope).await;
            }
            Err(_) => (None, envelope),
        };
        // MLS (commits, Welcomes, group messages) are `MLSMessage`s, version mls10 (0x0001):
        // they begin with a zero byte, which no sealed envelope does.
        if envelope.first() == Some(&0) {
            return self.receive_mls(envelope, stamp).await;
        }
        let Ok(inner) = Sessions::open(&self.local, envelope) else {
            return Ok(Received::Nothing);
        };
        let (account, device) = (inner.account, inner.device);
        // A blocked account's messages are dropped here too (sealed senders, old access keys).
        if account != self.local.account && self.store.is_blocked(&account)? {
            return Ok(Received::Nothing);
        }
        let mut identity = self
            .store
            .devices(&account)?
            .into_iter()
            .find(|(d, _)| *d == device)
            .map(|(_, k)| k);
        if identity.is_none() {
            identity = self
                .devices_of(&account, true)
                .await
                .ok()
                .and_then(|v| v.into_iter().find(|(d, _)| *d == device).map(|(_, k)| k));
        }
        let Ok(dec) = self
            .sessions
            .decrypt(&self.local, &mut self.prekeys, inner, identity)
        else {
            return Ok(Received::Nothing);
        };
        self.store
            .save_sessions(&self.sessions, &(account, device))?;
        if dec.new_session {
            self.store.save_prekeys(&self.prekeys)?;
        }
        let Ok(opened) = songbird_content::open(&dec.plaintext) else {
            return Ok(Received::Nothing);
        };
        if let Some(pk) = opened.profile_key
            && account != self.local.account
        {
            self.store.set_profile_key(&account, &pk)?;
        }
        let content = opened.content.clone();
        match &content.body {
            Some(Body::Data(data)) => {
                // Visible messages from another account must be franked and the stamp must open.
                let franked_ok = stamp.is_some_and(|s| opened.franking_matches(&s.commitment));
                if account != self.local.account && !franked_ok {
                    return Ok(Received::Nothing);
                }
                match &data.kind {
                    Some(data_message::Kind::Reaction(r)) => {
                        let Some((author, id)) = r.target.as_ref().and_then(|t| {
                            Some((
                                <[u8; 16]>::try_from(t.author.as_slice()).ok()?,
                                <[u8; 16]>::try_from(t.message_id.as_slice()).ok()?,
                            ))
                        }) else {
                            return Ok(Received::Nothing);
                        };
                        let emoji = if r.remove {
                            String::new()
                        } else {
                            r.emoji.clone()
                        };
                        if self
                            .store
                            .react(&author, &id, &account, &emoji, r.counter)?
                        {
                            return Ok(Received::Reaction {
                                author,
                                id,
                                reactor: account,
                                emoji,
                            });
                        }
                        return Ok(Received::Nothing);
                    }
                    Some(data_message::Kind::Edit(e)) => {
                        // Only the author edits, and only an existing message.
                        let Ok(id) = <[u8; 16]>::try_from(e.target_message_id.as_slice()) else {
                            return Ok(Received::Nothing);
                        };
                        let text = e.text.as_ref().map(|t| t.body.clone()).unwrap_or_default();
                        if self.store.message(&account, &id)?.is_some() {
                            self.store.set_text(&account, &id, &text)?;
                            return Ok(Received::Edited {
                                author: account,
                                id,
                                text,
                            });
                        }
                        return Ok(Received::Nothing);
                    }
                    Some(data_message::Kind::Delete(d)) => {
                        let Some(id) = d.target.as_ref().and_then(|t| {
                            (t.author.as_slice() == account.as_slice())
                                .then(|| <[u8; 16]>::try_from(t.message_id.as_slice()).ok())
                                .flatten()
                        }) else {
                            return Ok(Received::Nothing);
                        };
                        if self.store.message(&account, &id)?.is_some() {
                            self.store.mark_deleted(&account, &id)?;
                            return Ok(Received::Deleted {
                                author: account,
                                id,
                            });
                        }
                        return Ok(Received::Nothing);
                    }
                    _ => {}
                }
                let row = data_row(&content, data, account, account, Status::Delivered)?;
                if !self.store.insert_message(&row, true)? {
                    return Ok(Received::Nothing);
                }
                // What reporting it needs, exactly as received (§5.14).
                if let (Some(st), Some(fk)) = (stamp, opened.franking_key) {
                    let mut b = Vec::with_capacity(80);
                    b.extend_from_slice(&st.key_id);
                    b.extend_from_slice(&st.commitment);
                    b.extend_from_slice(&st.stamped_at_ms.to_be_bytes());
                    b.extend_from_slice(&st.tag);
                    self.store
                        .put_frank(&account, &row.id, &b, &fk, &opened.content_bytes)?;
                }
                let _ = self
                    .send_receipt(&account, &[row.id], receipt_message::Kind::Delivered)
                    .await;
                Ok(Received::Message(row))
            }
            Some(Body::Typing(t)) if account != self.local.account => Ok(Received::Typing {
                account,
                started: t.started,
            }),
            Some(Body::Receipt(r)) => {
                let status = match receipt_message::Kind::try_from(r.kind) {
                    Ok(receipt_message::Kind::Delivered) => Status::Delivered,
                    Ok(receipt_message::Kind::Read) => Status::Read,
                    Ok(receipt_message::Kind::Played) => Status::Played,
                    _ => return Ok(Received::Nothing),
                };
                let mut last = Received::Nothing;
                for id in &r.message_ids {
                    let Ok(id) = <[u8; 16]>::try_from(id.as_slice()) else {
                        continue;
                    };
                    self.store
                        .receipt(&self.local.account, &id, &account, status, now_ms(), 1)?;
                    if let Some(m) = self.store.message(&self.local.account, &id)? {
                        last = Received::Status {
                            id,
                            status: m.status,
                        };
                    }
                }
                Ok(last)
            }
            Some(Body::Sync(SyncMessage {
                kind: Some(sync_message::Kind::Sent(t)),
            })) if account == self.local.account => {
                let Ok(c) = Content::decode(t.content.as_slice()) else {
                    return Ok(Received::Nothing);
                };
                let Some(Body::Data(data)) = &c.body else {
                    return Ok(Received::Nothing);
                };
                // Only messages become rows; events about them are applied by the sender's own
                // copy already (reactions, edits and deletes are stored before they are sent).
                if !matches!(data.kind, Some(data_message::Kind::Message(_))) {
                    return Ok(Received::Nothing);
                }
                let Some(chat) = t
                    .chat
                    .as_ref()
                    .and_then(|c| <[u8; 16]>::try_from(c.account.as_slice()).ok())
                else {
                    return Ok(Received::Nothing);
                };
                let row = data_row(&c, data, chat, account, Status::Sent)?;
                if self.store.insert_message(&row, false)? {
                    Ok(Received::Message(row))
                } else {
                    Ok(Received::Nothing)
                }
            }
            _ => Ok(Received::Nothing),
        }
    }

    async fn system(&mut self, envelope: &[u8]) -> Result<Received> {
        let Some(key) = self.system_key else {
            return Ok(Received::Nothing);
        };
        let Ok(u) =
            songbird_proto::system::SystemFrame::decode(&bytes::Bytes::copy_from_slice(envelope))
        else {
            return Ok(Received::Nothing);
        };
        if !songbird_keys::ed25519_verify(&key, &u.message(), &u.signature)
            || *u.frame.device.as_bytes() != self.local.device
        {
            return Ok(Received::Nothing);
        }
        match u.frame.kind {
            songbird_proto::system::Kind::PrekeysLow => {
                let _ = self.publish_keys(false).await;
            }
            songbird_proto::system::Kind::KeyPackagesLow => {
                let _ = self
                    .publish_key_packages(crate::mls::client::KEY_PACKAGES, false)
                    .await;
            }
            songbird_proto::system::Kind::DeviceListChanged => {
                let me = self.local.account;
                let _ = self.devices_of(&me, true).await;
            }
            // A kind this version does not know: verified, then ignored.
            songbird_proto::system::Kind::Other(_) => {}
        }
        Ok(Received::System)
    }
}

/// The attachment pointers of a stored message's content.
#[must_use]
pub fn attachments_of(content: &[u8]) -> Vec<schema::AttachmentPointer> {
    let Ok(c) = Content::decode(content) else {
        return Vec::new();
    };
    match c.body {
        Some(Body::Data(DataMessage {
            kind: Some(data_message::Kind::Message(m)),
            ..
        })) => m.attachments,
        _ => Vec::new(),
    }
}

/// The place a stored message's content shares, if it is a location:
/// (latitude, longitude, name).
#[must_use]
pub fn location_of(content: &[u8]) -> Option<(f64, f64, String)> {
    let c = Content::decode(content).ok()?;
    let Some(Body::Data(d)) = c.body else {
        return None;
    };
    let Some(data_message::Kind::Message(m)) = d.kind else {
        return None;
    };
    let l = m.location?;
    Some((
        f64::from(l.latitude_e7) / 1e7,
        f64::from(l.longitude_e7) / 1e7,
        l.name,
    ))
}

/// How an attachment reads in a list ("Photo", "Voice message", the file's name).
#[must_use]
pub fn attachment_label(p: &schema::AttachmentPointer) -> String {
    if p.flags & 1 != 0 {
        "Voice message".into()
    } else if p.content_type.starts_with("image/") {
        if p.flags & 4 != 0 {
            "GIF".into()
        } else {
            "Photo".into()
        }
    } else if p.content_type.starts_with("video/") {
        "Video".into()
    } else if p.content_type.starts_with("audio/") {
        "Audio".into()
    } else if p.file_name.is_empty() {
        "File".into()
    } else {
        p.file_name.clone()
    }
}

pub(crate) fn data_row(
    content: &Content,
    data: &DataMessage,
    chat: [u8; 16],
    author: [u8; 16],
    status: Status,
) -> Result<MessageRow> {
    let id: [u8; 16] = content
        .message_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::Malformed)?;
    let text = match &data.kind {
        Some(data_message::Kind::Message(m)) => m
            .text
            .as_ref()
            .map(|t| t.body.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| m.attachments.first().map(attachment_label))
            .or_else(|| m.location.as_ref().map(|_| "Location".to_owned()))
            .unwrap_or_default(),
        _ => String::new(),
    };
    Ok(MessageRow {
        chat,
        author,
        id,
        sent_ms: content.sent_at_ms,
        stored_ms: now_ms(),
        text,
        content: content.encode_to_vec(),
        status,
    })
}
