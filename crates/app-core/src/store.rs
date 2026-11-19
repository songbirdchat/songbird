//! The on-device store (A2, D-0136, D-0137): `SQLCipher`, the whole file encrypted.
//!
//! - **Key**: 32 random bytes, kept wrapped by the Secure Enclave or Android Keystore by the
//!   platform host and handed in as a raw key (`PRAGMA key = "x'…'"`), so `SQLCipher` runs no
//!   password KDF. Apple builds use `CommonCrypto` for the page cipher; other platforms link a
//!   `libcrypto` at build time (AWS-LC, the same library aws-lc-rs wraps; D-0137). The `openssl`
//!   crates stay banned (`deny.toml`).
//! - **Schema**: versioned by `PRAGMA user_version`, migrated forward in one transaction per step.
//! - **What is stored**: this device's identity, prekeys and sessions (their own encodings, which
//!   zero their secrets in memory); the accounts and devices met, with the account identity key
//!   pinned on first sight and the highest device-list version seen (PROTOCOL §4.6: never roll
//!   back); chats; messages as their encoded `Content` with the columns lists need; outgoing
//!   status per recipient account; the outbox of sends not yet accepted; a full-text index
//!   (FTS5, inside the encrypted file).
//! - **Status only moves forward** (sending → sent → delivered → read → played), except a failed
//!   send that is retried.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

use crate::crypto::X25519Pair;
use crate::session::{Local, Prekeys, Session, Sessions};
use crate::{Error, Result};

/// The schema version this code writes.
pub const SCHEMA: i64 = 3;

/// Outgoing message status, as the UI shows it (`crates/content`'s levels, plus the local ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// The send failed; the user may retry.
    Failed = -1,
    /// Not yet accepted by the server.
    Sending = 0,
    /// Stored by the server (one tick).
    Sent = 1,
    /// Delivered to a recipient device (two ticks).
    Delivered = 2,
    /// Read.
    Read = 3,
    /// Played (voice notes).
    Played = 4,
}

impl Status {
    /// As stored.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        match self {
            Self::Failed => -1,
            Self::Sending => 0,
            Self::Sent => 1,
            Self::Delivered => 2,
            Self::Read => 3,
            Self::Played => 4,
        }
    }

    fn from_i64(v: i64) -> Self {
        match v {
            -1 => Self::Failed,
            1 => Self::Sent,
            2 => Self::Delivered,
            3 => Self::Read,
            4 => Self::Played,
            _ => Self::Sending,
        }
    }

    /// Whether `self` may become `next`.
    #[must_use]
    pub fn may_become(self, next: Self) -> bool {
        next > self || (self == Self::Failed && next == Self::Sending)
    }
}

/// A chat as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRow {
    /// The chat: the peer's account for 1:1, the group ID for groups.
    pub id: [u8; 16],
    /// Whether it is a group.
    pub group: bool,
    /// Time of the last message (ms).
    pub last_ms: u64,
    /// Preview of the last message.
    pub preview: String,
    /// Unread count.
    pub unread: u32,
}

/// A stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    /// Chat.
    pub chat: [u8; 16],
    /// Author account.
    pub author: [u8; 16],
    /// Message ID.
    pub id: [u8; 16],
    /// Sender's clock (ms).
    pub sent_ms: u64,
    /// When this device stored it (ms).
    pub stored_ms: u64,
    /// The text, for lists and search (empty for non-text kinds).
    pub text: String,
    /// The encoded `Content`.
    pub content: Vec<u8>,
    /// Outgoing status (incoming messages: `Delivered`, then `Read` once read here).
    pub status: Status,
}

/// An address and its sessions, as read back.
type Grouped = Option<(([u8; 16], [u8; 16]), Vec<Session>)>;
/// An outbox entry: idempotency key, chat, message ID, content.
pub type OutboxEntry = ([u8; 16], [u8; 16], [u8; 16], Vec<u8>);

/// The store.
#[derive(Debug)]
pub struct Store {
    db: Connection,
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

fn db<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|_| Error::Storage)
}

fn arr16(v: &[u8]) -> Result<[u8; 16]> {
    v.try_into().map_err(|_| Error::Storage)
}

fn ms(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn sql_ms(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl Store {
    /// Opens (or creates) the store at `path` with its 32-byte key.
    pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self> {
        Self::init(db(Connection::open(path))?, key)
    }

    /// An in-memory store (tests, and a locked app's empty view).
    pub fn memory(key: &[u8; 32]) -> Result<Self> {
        Self::init(db(Connection::open_in_memory())?, key)
    }

    fn init(db: Connection, key: &[u8; 32]) -> Result<Self> {
        let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex(key)));
        db.execute_batch(&pragma).map_err(|_| Error::Storage)?;
        // A wrong key fails on the first read, not at `PRAGMA key`.
        db.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|_| Error::Locked)?;
        db.execute_batch(
            "PRAGMA cipher_memory_security = ON; PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA secure_delete = ON;",
        )
        .map_err(|_| Error::Storage)?;
        let mut s = Self { db };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&mut self) -> Result<()> {
        let v: i64 = db(self.db.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if v > SCHEMA {
            return Err(Error::Storage);
        }
        if v < 1 {
            let tx = db(self.db.transaction())?;
            db(tx.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
                 CREATE TABLE sessions (account BLOB NOT NULL, device BLOB NOT NULL, idx INTEGER NOT NULL,
                     record BLOB NOT NULL, PRIMARY KEY (account, device, idx));
                 CREATE TABLE accounts (account BLOB PRIMARY KEY, identity_key BLOB NOT NULL,
                     verified INTEGER NOT NULL DEFAULT 0, list_version INTEGER NOT NULL DEFAULT 0,
                     profile_key BLOB);
                 CREATE TABLE devices (account BLOB NOT NULL, device BLOB NOT NULL, identity_key BLOB NOT NULL,
                     PRIMARY KEY (account, device));
                 CREATE TABLE chats (id BLOB PRIMARY KEY, is_group INTEGER NOT NULL, last_ms INTEGER NOT NULL,
                     preview TEXT NOT NULL, unread INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE messages (rowid INTEGER PRIMARY KEY, chat BLOB NOT NULL, author BLOB NOT NULL,
                     id BLOB NOT NULL, sent_ms INTEGER NOT NULL, stored_ms INTEGER NOT NULL, text TEXT NOT NULL,
                     content BLOB NOT NULL, status INTEGER NOT NULL, UNIQUE (author, id));
                 CREATE INDEX messages_by_chat ON messages (chat, sent_ms, rowid);
                 CREATE TABLE receipts (author BLOB NOT NULL, id BLOB NOT NULL, account BLOB NOT NULL,
                     level INTEGER NOT NULL, ms INTEGER NOT NULL, PRIMARY KEY (author, id, account));
                 CREATE TABLE outbox (idem BLOB PRIMARY KEY, created_ms INTEGER NOT NULL, chat BLOB NOT NULL,
                     message BLOB NOT NULL, content BLOB NOT NULL);
                 CREATE VIRTUAL TABLE search USING fts5(text, content='messages', content_rowid='rowid',
                     tokenize='unicode61 remove_diacritics 2');
                 CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
                     INSERT INTO search(rowid, text) VALUES (new.rowid, new.text); END;
                 CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
                     INSERT INTO search(search, rowid, text) VALUES ('delete', old.rowid, old.text); END;
                 CREATE TRIGGER messages_au AFTER UPDATE OF text ON messages BEGIN
                     INSERT INTO search(search, rowid, text) VALUES ('delete', old.rowid, old.text);
                     INSERT INTO search(rowid, text) VALUES (new.rowid, new.text); END;
                 PRAGMA user_version = 1;",
            ))?;
            db(tx.commit())?;
        }
        if v < 2 {
            // Attachments' plaintext, inside the encrypted file like everything else (D-0141).
            let tx = db(self.db.transaction())?;
            db(tx.execute_batch(
                "CREATE TABLE files (author BLOB NOT NULL, id BLOB NOT NULL, idx INTEGER NOT NULL,
                     bytes BLOB NOT NULL, PRIMARY KEY (author, id, idx));
                 PRAGMA user_version = 2;",
            ))?;
            db(tx.commit())?;
        }
        if v < 3 {
            // Reactions: one per reactor and message, the newest counter wins (PROTOCOL §5.5).
            let tx = db(self.db.transaction())?;
            db(tx.execute_batch(
                "CREATE TABLE reactions (author BLOB NOT NULL, id BLOB NOT NULL, reactor BLOB NOT NULL,
                     emoji TEXT NOT NULL, counter INTEGER NOT NULL, PRIMARY KEY (author, id, reactor));
                 PRAGMA user_version = 3;",
            ))?;
            db(tx.commit())?;
        }
        Ok(())
    }

    /// Records `reactor`'s reaction to a message (empty `emoji`: removed), if `counter` is newer
    /// than the one held. Answers whether it changed.
    pub fn react(
        &self,
        author: &[u8; 16],
        id: &[u8; 16],
        reactor: &[u8; 16],
        emoji: &str,
        counter: u64,
    ) -> Result<bool> {
        let n = db(self.db.execute(
            "INSERT INTO reactions (author, id, reactor, emoji, counter) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (author, id, reactor) DO UPDATE SET emoji = ?4, counter = ?5 WHERE counter < ?5",
            params![&author[..], &id[..], &reactor[..], emoji, sql_ms(counter)],
        ))?;
        Ok(n > 0)
    }

    /// A message's reactions: (reactor, emoji), removed ones left out.
    pub fn reactions(&self, author: &[u8; 16], id: &[u8; 16]) -> Result<Vec<([u8; 16], String)>> {
        let mut st = db(self.db.prepare(
            "SELECT reactor, emoji FROM reactions WHERE author = ?1 AND id = ?2 AND emoji != '' ORDER BY counter",
        ))?;
        let rows = db(st.query_map(params![&author[..], &id[..]], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        }))?;
        rows.map(|r| {
            let (a, e) = db(r)?;
            Ok((arr16(&a)?, e))
        })
        .collect()
    }

    /// Deletes a message's content for everyone: the row stays as "deleted", with no text, no
    /// content, no files and no reactions.
    pub fn mark_deleted(&self, author: &[u8; 16], id: &[u8; 16]) -> Result<()> {
        db(self.db.execute(
            "DELETE FROM files WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..]],
        ))?;
        db(self.db.execute(
            "DELETE FROM reactions WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..]],
        ))?;
        db(self.db.execute(
            "UPDATE messages SET text = '', content = x'' WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..]],
        ))
        .map(|_| ())
    }

    /// Keeps an attachment's plaintext (a message's `idx`-th file).
    pub fn put_file(&self, author: &[u8; 16], id: &[u8; 16], idx: u32, bytes: &[u8]) -> Result<()> {
        db(self.db.execute(
            "INSERT OR REPLACE INTO files (author, id, idx, bytes) VALUES (?1, ?2, ?3, ?4)",
            params![&author[..], &id[..], i64::from(idx), bytes],
        ))
        .map(|_| ())
    }

    /// An attachment's plaintext, if this device has it.
    pub fn file(
        &self,
        author: &[u8; 16],
        id: &[u8; 16],
        idx: u32,
    ) -> Result<Option<Zeroizing<Vec<u8>>>> {
        db(self
            .db
            .query_row(
                "SELECT bytes FROM files WHERE author = ?1 AND id = ?2 AND idx = ?3",
                params![&author[..], &id[..], i64::from(idx)],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional())
        .map(|v| v.map(Zeroizing::new))
    }

    fn put_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        db(self.db.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
            params![key, value],
        ))
        .map(|_| ())
    }

    fn meta(&self, key: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        db(self
            .db
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .optional())
        .map(|v| v.map(Zeroizing::new))
    }

    /// Saves this device's identity.
    pub fn save_local(&self, l: &Local) -> Result<()> {
        let mut v = Zeroizing::new(Vec::with_capacity(64));
        v.extend_from_slice(&l.account);
        v.extend_from_slice(&l.device);
        v.extend_from_slice(l.identity.secret());
        self.put_meta("local", &v)
    }

    /// This device's identity, if registered.
    pub fn local(&self) -> Result<Option<Local>> {
        let Some(v) = self.meta("local")? else {
            return Ok(None);
        };
        let mut r = crate::crypto::Reader::new(&v);
        let account = r.array()?;
        let device = r.array()?;
        let secret = Zeroizing::new(r.array::<32>()?);
        r.finish()?;
        Ok(Some(Local {
            account,
            device,
            identity: X25519Pair::from_secret(&secret)?,
        }))
    }

    /// Saves the prekeys.
    pub fn save_prekeys(&self, p: &Prekeys) -> Result<()> {
        self.put_meta("prekeys", &p.encode()?)
    }

    /// The prekeys (empty before the first upload).
    pub fn prekeys(&self) -> Result<Prekeys> {
        match self.meta("prekeys")? {
            Some(v) => Prekeys::decode(&v),
            None => Ok(Prekeys::default()),
        }
    }

    /// A small secret or value under `key` (the profile key, the storage key, …).
    pub fn put_secret(&self, key: &str, value: &[u8]) -> Result<()> {
        self.put_meta(&format!("s.{key}"), value)
    }

    /// Reads what [`Self::put_secret`] stored.
    pub fn secret(&self, key: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        self.meta(&format!("s.{key}"))
    }

    /// Saves every session with one device (newest first).
    pub fn save_sessions(
        &mut self,
        sessions: &Sessions,
        addr: &([u8; 16], [u8; 16]),
    ) -> Result<()> {
        let tx = db(self.db.transaction())?;
        db(tx.execute(
            "DELETE FROM sessions WHERE account = ?1 AND device = ?2",
            params![&addr.0[..], &addr.1[..]],
        ))?;
        for (i, s) in sessions.get(addr).unwrap_or(&[]).iter().enumerate() {
            db(tx.execute(
                "INSERT INTO sessions (account, device, idx, record) VALUES (?1, ?2, ?3, ?4)",
                params![
                    &addr.0[..],
                    &addr.1[..],
                    i64::try_from(i).unwrap_or(0),
                    &s.encode()?[..]
                ],
            ))?;
        }
        db(tx.commit())
    }

    /// Every session.
    pub fn sessions(&self) -> Result<Sessions> {
        let mut st = db(self.db.prepare(
            "SELECT account, device, record FROM sessions ORDER BY account, device, idx",
        ))?;
        let rows = db(st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                Zeroizing::new(r.get::<_, Vec<u8>>(2)?),
            ))
        }))?;
        let mut out = Sessions::default();
        let mut cur: Grouped = None;
        for row in rows {
            let (account, device, rec) = db(row)?;
            let addr = (arr16(&account)?, arr16(&device)?);
            let s = Session::decode(&rec)?;
            match &mut cur {
                Some((k, v)) if *k == addr => v.push(s),
                _ => {
                    if let Some((k, v)) = cur.take() {
                        out.put(k, v);
                    }
                    cur = Some((addr, vec![s]));
                }
            }
        }
        if let Some((k, v)) = cur {
            out.put(k, v);
        }
        Ok(out)
    }

    /// Pins an account's identity key on first sight. Answers whether the stored key equals
    /// `key` (false: the account's identity changed, the UI warns and verification is reset).
    pub fn pin_account(&self, account: &[u8; 16], key: &[u8; 32]) -> Result<bool> {
        let cur: Option<Vec<u8>> = db(self
            .db
            .query_row(
                "SELECT identity_key FROM accounts WHERE account = ?1",
                [&account[..]],
                |r| r.get(0),
            )
            .optional())?;
        match cur {
            None => {
                db(self.db.execute(
                    "INSERT INTO accounts (account, identity_key) VALUES (?1, ?2)",
                    params![&account[..], &key[..]],
                ))?;
                Ok(true)
            }
            Some(k) => Ok(crate::crypto::ct_eq(&k, key)),
        }
    }

    /// Replaces a changed identity key after the user accepted the change; unverifies.
    pub fn accept_identity(&self, account: &[u8; 16], key: &[u8; 32]) -> Result<()> {
        db(self.db.execute(
            "UPDATE accounts SET identity_key = ?2, verified = 0, list_version = 0 WHERE account = ?1",
            params![&account[..], &key[..]],
        ))?;
        db(self
            .db
            .execute("DELETE FROM devices WHERE account = ?1", [&account[..]]))
        .map(|_| ())
    }

    /// The pinned identity key of an account.
    pub fn account_key(&self, account: &[u8; 16]) -> Result<Option<[u8; 32]>> {
        let v: Option<Vec<u8>> = db(self
            .db
            .query_row(
                "SELECT identity_key FROM accounts WHERE account = ?1",
                [&account[..]],
                |r| r.get(0),
            )
            .optional())?;
        v.map(|k| k.as_slice().try_into().map_err(|_| Error::Storage))
            .transpose()
    }

    /// Marks an account verified (safety numbers compared) or not.
    pub fn set_verified(&self, account: &[u8; 16], verified: bool) -> Result<()> {
        db(self.db.execute(
            "UPDATE accounts SET verified = ?2 WHERE account = ?1",
            params![&account[..], i64::from(verified)],
        ))
        .map(|_| ())
    }

    /// Records an account's verified device list, refusing a version not higher than the highest
    /// seen (a server cannot roll back to a removed device). Answers the removed devices.
    pub fn set_devices(
        &self,
        account: &[u8; 16],
        version: u64,
        devices: &[([u8; 16], [u8; 32])],
    ) -> Result<Vec<[u8; 16]>> {
        let cur: Option<i64> = db(self
            .db
            .query_row(
                "SELECT list_version FROM accounts WHERE account = ?1",
                [&account[..]],
                |r| r.get(0),
            )
            .optional())?;
        let Some(cur) = cur else {
            return Err(Error::Verify);
        };
        if sql_ms(version) <= cur && cur != 0 {
            return if sql_ms(version) == cur {
                Ok(Vec::new())
            } else {
                Err(Error::Verify)
            };
        }
        let mut st = db(self
            .db
            .prepare("SELECT device FROM devices WHERE account = ?1"))?;
        let old: Vec<[u8; 16]> = db(st.query_map([&account[..]], |r| r.get::<_, Vec<u8>>(0)))?
            .filter_map(|r| r.ok().and_then(|v| v.as_slice().try_into().ok()))
            .collect();
        let removed: Vec<[u8; 16]> = old
            .into_iter()
            .filter(|d| !devices.iter().any(|(n, _)| n == d))
            .collect();
        db(self
            .db
            .execute("DELETE FROM devices WHERE account = ?1", [&account[..]]))?;
        for (d, k) in devices {
            db(self.db.execute(
                "INSERT INTO devices (account, device, identity_key) VALUES (?1, ?2, ?3)",
                params![&account[..], &d[..], &k[..]],
            ))?;
        }
        db(self.db.execute(
            "UPDATE accounts SET list_version = ?2 WHERE account = ?1",
            params![&account[..], sql_ms(version)],
        ))?;
        Ok(removed)
    }

    /// An account's devices with their identity keys, from its last verified list.
    pub fn devices(&self, account: &[u8; 16]) -> Result<Vec<([u8; 16], [u8; 32])>> {
        let mut st = db(self.db.prepare(
            "SELECT device, identity_key FROM devices WHERE account = ?1 ORDER BY device",
        ))?;
        let rows = db(st.query_map([&account[..]], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
        }))?;
        rows.map(|r| {
            let (d, k) = db(r)?;
            Ok((
                arr16(&d)?,
                k.as_slice().try_into().map_err(|_| Error::Storage)?,
            ))
        })
        .collect()
    }

    /// Stores a contact's profile key (from their messages).
    pub fn set_profile_key(&self, account: &[u8; 16], key: &[u8; 32]) -> Result<()> {
        db(self.db.execute(
            "UPDATE accounts SET profile_key = ?2 WHERE account = ?1",
            params![&account[..], &key[..]],
        ))
        .map(|_| ())
    }

    /// A contact's profile key.
    pub fn profile_key(&self, account: &[u8; 16]) -> Result<Option<Zeroizing<[u8; 32]>>> {
        let v: Option<Option<Vec<u8>>> = db(self
            .db
            .query_row(
                "SELECT profile_key FROM accounts WHERE account = ?1",
                [&account[..]],
                |r| r.get(0),
            )
            .optional())?;
        Ok(v.flatten()
            .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
            .map(Zeroizing::new))
    }

    /// Stores a message once: a repeat of (author, id) is dropped (PROTOCOL §5.1). Answers
    /// whether it was new. Updates the chat's preview and, for incoming ones, its unread count.
    pub fn insert_message(&mut self, m: &MessageRow, incoming: bool) -> Result<bool> {
        let tx = db(self.db.transaction())?;
        let n = db(tx.execute(
            "INSERT OR IGNORE INTO messages (chat, author, id, sent_ms, stored_ms, text, content, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![&m.chat[..], &m.author[..], &m.id[..], sql_ms(m.sent_ms), sql_ms(m.stored_ms), m.text, m.content, m.status.as_i64()],
        ))?;
        if n == 1 {
            db(tx.execute(
                "INSERT INTO chats (id, is_group, last_ms, preview, unread) VALUES (?1, 0, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET last_ms = max(last_ms, ?2),
                   preview = CASE WHEN ?2 >= last_ms THEN ?3 ELSE preview END, unread = unread + ?4",
                params![&m.chat[..], sql_ms(m.sent_ms), m.text, i64::from(incoming)],
            ))?;
        }
        db(tx.commit())?;
        Ok(n == 1)
    }

    /// Marks a chat a group (before its first message).
    pub fn ensure_group_chat(&self, id: &[u8; 16]) -> Result<()> {
        db(self.db.execute(
            "INSERT INTO chats (id, is_group, last_ms, preview, unread) VALUES (?1, 1, 0, '', 0)
             ON CONFLICT (id) DO UPDATE SET is_group = 1",
            [&id[..]],
        ))
        .map(|_| ())
    }

    /// Moves a message's status forward (never back, except a retried failure). Answers whether
    /// it changed.
    pub fn advance(&self, author: &[u8; 16], id: &[u8; 16], next: Status) -> Result<bool> {
        let cur: Option<i64> = db(self
            .db
            .query_row(
                "SELECT status FROM messages WHERE author = ?1 AND id = ?2",
                params![&author[..], &id[..]],
                |r| r.get(0),
            )
            .optional())?;
        let Some(cur) = cur else { return Ok(false) };
        if !Status::from_i64(cur).may_become(next) {
            return Ok(false);
        }
        db(self.db.execute(
            "UPDATE messages SET status = ?3 WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..], next.as_i64()],
        ))?;
        Ok(true)
    }

    /// Records a receipt from `account` for one of our messages. The message's status becomes the
    /// lowest level every one of the `audience` recipient accounts has reached; one that has not
    /// answered counts as `Sent` (1:1: an audience of one).
    pub fn receipt(
        &self,
        author: &[u8; 16],
        id: &[u8; 16],
        account: &[u8; 16],
        level: Status,
        at_ms: u64,
        audience: usize,
    ) -> Result<()> {
        db(self.db.execute(
            "INSERT INTO receipts (author, id, account, level, ms) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (author, id, account) DO UPDATE SET level = max(level, ?4), ms = CASE WHEN ?4 > level THEN ?5 ELSE ms END",
            params![&author[..], &id[..], &account[..], level.as_i64(), sql_ms(at_ms)],
        ))?;
        let (n, min): (i64, Option<i64>) = db(self.db.query_row(
            "SELECT count(*), min(level) FROM receipts WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        if let Some(m) = min
            && usize::try_from(n).unwrap_or(0) >= audience
        {
            self.advance(author, id, Status::from_i64(m))?;
        }
        Ok(())
    }

    /// Edits a message's text (the newest edit wins; `crates/content` decides which).
    pub fn set_text(&self, author: &[u8; 16], id: &[u8; 16], text: &str) -> Result<()> {
        db(self.db.execute(
            "UPDATE messages SET text = ?3 WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..], text],
        ))
        .map(|_| ())
    }

    /// Deletes a message here (delete for everyone, disappearing, or delete for me).
    pub fn delete_message(&self, author: &[u8; 16], id: &[u8; 16]) -> Result<()> {
        db(self.db.execute(
            "DELETE FROM messages WHERE author = ?1 AND id = ?2",
            params![&author[..], &id[..]],
        ))
        .map(|_| ())
    }

    /// The chat list, newest first.
    pub fn chats(&self) -> Result<Vec<ChatRow>> {
        let mut st = db(self.db.prepare(
            "SELECT id, is_group, last_ms, preview, unread FROM chats ORDER BY last_ms DESC",
        ))?;
        let rows = db(st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        }))?;
        rows.map(|r| {
            let (id, g, last, preview, unread) = db(r)?;
            Ok(ChatRow {
                id: arr16(&id)?,
                group: g == 1,
                last_ms: ms(last),
                preview,
                unread: u32::try_from(unread).unwrap_or(0),
            })
        })
        .collect()
    }

    /// Marks a chat read: unread to zero; answers the incoming messages to send read receipts for.
    pub fn mark_read(&self, chat: &[u8; 16], me: &[u8; 16]) -> Result<Vec<([u8; 16], [u8; 16])>> {
        db(self
            .db
            .execute("UPDATE chats SET unread = 0 WHERE id = ?1", [&chat[..]]))?;
        let mut st = db(self.db.prepare(
            "SELECT author, id FROM messages WHERE chat = ?1 AND author != ?2 AND status < 3",
        ))?;
        let out: Vec<_> = db(st.query_map(params![&chat[..], &me[..]], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
        }))?
        .filter_map(|r| {
            r.ok()
                .and_then(|(a, i)| Some((arr16(&a).ok()?, arr16(&i).ok()?)))
        })
        .collect();
        db(self.db.execute(
            "UPDATE messages SET status = 3 WHERE chat = ?1 AND author != ?2 AND status < 3",
            params![&chat[..], &me[..]],
        ))?;
        Ok(out)
    }

    /// A page of a chat's messages, oldest first, before `before_ms` (or the newest page).
    pub fn messages(
        &self,
        chat: &[u8; 16],
        before_ms: Option<u64>,
        limit: u32,
    ) -> Result<Vec<MessageRow>> {
        let mut st = db(self.db.prepare(
            "SELECT chat, author, id, sent_ms, stored_ms, text, content, status FROM messages
             WHERE chat = ?1 AND sent_ms < ?2 ORDER BY sent_ms DESC, rowid DESC LIMIT ?3",
        ))?;
        let rows = db(st.query_map(
            params![
                &chat[..],
                sql_ms(before_ms.unwrap_or(u64::MAX >> 1)),
                i64::from(limit)
            ],
            row_message,
        ))?;
        let mut v: Vec<MessageRow> = rows.map(|r| db(r)?).collect::<Result<_>>()?;
        v.reverse();
        Ok(v)
    }

    /// One message.
    pub fn message(&self, author: &[u8; 16], id: &[u8; 16]) -> Result<Option<MessageRow>> {
        let r = db(self
            .db
            .query_row(
                "SELECT chat, author, id, sent_ms, stored_ms, text, content, status FROM messages WHERE author = ?1 AND id = ?2",
                params![&author[..], &id[..]],
                row_message,
            )
            .optional())?;
        r.transpose()
    }

    /// Full-text search over every chat, newest first.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<MessageRow>> {
        // Each word as a quoted prefix term, so user input is never FTS syntax.
        let q: String = query
            .split_whitespace()
            .take(16)
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = db(self.db.prepare(
            "SELECT m.chat, m.author, m.id, m.sent_ms, m.stored_ms, m.text, m.content, m.status
             FROM search JOIN messages m ON m.rowid = search.rowid WHERE search MATCH ?1
             ORDER BY m.sent_ms DESC LIMIT ?2",
        ))?;
        let rows = db(st.query_map(params![q, i64::from(limit)], row_message))?;
        rows.map(|r| db(r)?).collect()
    }

    /// Queues an accepted-but-unsent message (survives a restart; resent with the same key).
    pub fn outbox_put(
        &self,
        idem: &[u8; 16],
        chat: &[u8; 16],
        message: &[u8; 16],
        content: &[u8],
        now_ms: u64,
    ) -> Result<()> {
        db(self.db.execute(
            "INSERT OR IGNORE INTO outbox (idem, created_ms, chat, message, content) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&idem[..], sql_ms(now_ms), &chat[..], &message[..], content],
        ))
        .map(|_| ())
    }

    /// The outbox, oldest first: (idempotency key, chat, message id, content).
    pub fn outbox(&self) -> Result<Vec<OutboxEntry>> {
        let mut st = db(self
            .db
            .prepare("SELECT idem, chat, message, content FROM outbox ORDER BY created_ms"))?;
        let rows = db(st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        }))?;
        rows.map(|r| {
            let (i, c, m, b) = db(r)?;
            Ok((arr16(&i)?, arr16(&c)?, arr16(&m)?, b))
        })
        .collect()
    }

    /// Removes an outbox entry (accepted, or given up).
    pub fn outbox_done(&self, idem: &[u8; 16]) -> Result<()> {
        db(self
            .db
            .execute("DELETE FROM outbox WHERE idem = ?1", [&idem[..]]))
        .map(|_| ())
    }
}

fn row_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<MessageRow>> {
    let chat: Vec<u8> = r.get(0)?;
    let author: Vec<u8> = r.get(1)?;
    let id: Vec<u8> = r.get(2)?;
    let sent: i64 = r.get(3)?;
    let stored: i64 = r.get(4)?;
    let text: String = r.get(5)?;
    let content: Vec<u8> = r.get(6)?;
    let status: i64 = r.get(7)?;
    Ok((|| {
        Ok(MessageRow {
            chat: arr16(&chat)?,
            author: arr16(&author)?,
            id: arr16(&id)?,
            sent_ms: ms(sent),
            stored_ms: ms(stored),
            text,
            content,
            status: Status::from_i64(status),
        })
    })())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;

    fn msg(chat: u8, author: u8, id: u8, sent: u64, text: &str) -> MessageRow {
        MessageRow {
            chat: [chat; 16],
            author: [author; 16],
            id: [id; 16],
            sent_ms: sent,
            stored_ms: sent,
            text: text.into(),
            content: vec![1, 2, 3],
            status: Status::Sending,
        }
    }

    #[test]
    fn the_file_is_encrypted_and_a_wrong_key_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "sb-store-{}",
            hex(&*crate::crypto::random::<8>().unwrap())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db");
        {
            let mut s = Store::open(&path, &[7; 32]).unwrap();
            s.insert_message(&msg(1, 2, 3, 10, "a very findable secret phrase"), true)
                .unwrap();
        }
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));
        assert!(!raw.starts_with(b"SQLite format 3"));
        assert_eq!(Store::open(&path, &[8; 32]).unwrap_err(), Error::Locked);
        let s = Store::open(&path, &[7; 32]).unwrap();
        assert_eq!(s.search("findable", 10).unwrap().len(), 1);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn messages_are_stored_once_and_status_only_moves_forward() {
        let mut s = Store::memory(&[1; 32]).unwrap();
        let m = msg(1, 2, 3, 10, "hello");
        assert!(s.insert_message(&m, false).unwrap());
        assert!(!s.insert_message(&m, false).unwrap());
        assert!(s.advance(&m.author, &m.id, Status::Sent).unwrap());
        assert!(s.advance(&m.author, &m.id, Status::Read).unwrap());
        assert!(!s.advance(&m.author, &m.id, Status::Delivered).unwrap());
        assert_eq!(
            s.message(&m.author, &m.id).unwrap().unwrap().status,
            Status::Read
        );
        let f = msg(1, 2, 4, 11, "x");
        s.insert_message(&f, false).unwrap();
        assert!(s.advance(&f.author, &f.id, Status::Failed).is_ok());
        assert!(!s.advance(&f.author, &f.id, Status::Failed).unwrap());
    }

    #[test]
    fn chats_pages_search_and_read_marks() {
        let mut s = Store::memory(&[1; 32]).unwrap();
        for i in 0..5u8 {
            s.insert_message(
                &msg(1, 9, i, u64::from(i) * 10, &format!("hello number {i}")),
                true,
            )
            .unwrap();
        }
        s.insert_message(&msg(2, 8, 50, 1, "Bună ziua"), true)
            .unwrap();
        let chats = s.chats().unwrap();
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].unread, 5);
        assert_eq!(chats[0].preview, "hello number 4");
        let page = s.messages(&[1; 16], Some(30), 2).unwrap();
        assert_eq!(page.iter().map(|m| m.id[0]).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(s.search("buna", 10).unwrap().len(), 1, "diacritics folded");
        assert_eq!(
            s.search("number \"", 10).unwrap().len(),
            5,
            "input is never FTS syntax"
        );
        s.set_text(&[9; 16], &[0; 16], "edited").unwrap();
        assert_eq!(s.search("edited", 10).unwrap().len(), 1);
        assert_eq!(s.mark_read(&[1; 16], &[0; 16]).unwrap().len(), 5);
        assert_eq!(
            s.chats()
                .unwrap()
                .iter()
                .find(|c| c.id == [1; 16])
                .unwrap()
                .unread,
            0
        );
    }

    #[test]
    fn receipts_take_the_lowest_level_of_every_recipient() {
        let mut s = Store::memory(&[1; 32]).unwrap();
        let m = msg(1, 2, 3, 10, "x");
        s.insert_message(&m, false).unwrap();
        s.advance(&m.author, &m.id, Status::Sent).unwrap();
        s.receipt(&m.author, &m.id, &[5; 16], Status::Read, 1, 2)
            .unwrap();
        assert_eq!(
            s.message(&m.author, &m.id).unwrap().unwrap().status,
            Status::Sent
        );
        s.receipt(&m.author, &m.id, &[6; 16], Status::Delivered, 2, 2)
            .unwrap();
        assert_eq!(
            s.message(&m.author, &m.id).unwrap().unwrap().status,
            Status::Delivered
        );
        s.receipt(&m.author, &m.id, &[6; 16], Status::Read, 3, 2)
            .unwrap();
        assert_eq!(
            s.message(&m.author, &m.id).unwrap().unwrap().status,
            Status::Read
        );
    }

    #[test]
    fn identities_pin_and_device_lists_never_roll_back() {
        let s = Store::memory(&[1; 32]).unwrap();
        let a = [1u8; 16];
        assert!(s.pin_account(&a, &[2; 32]).unwrap());
        assert!(s.pin_account(&a, &[2; 32]).unwrap());
        assert!(!s.pin_account(&a, &[3; 32]).unwrap());
        s.set_devices(&a, 2, &[([4; 16], [5; 32]), ([6; 16], [7; 32])])
            .unwrap();
        assert_eq!(
            s.set_devices(&a, 3, &[([4; 16], [5; 32])]).unwrap(),
            vec![[6; 16]]
        );
        assert_eq!(s.set_devices(&a, 2, &[]).unwrap_err(), Error::Verify);
        assert_eq!(s.devices(&a).unwrap().len(), 1);
    }

    #[test]
    fn keys_and_sessions_survive_a_reopen() {
        let s = Store::memory(&[1; 32]).unwrap();
        let local = Local {
            account: [1; 16],
            device: [2; 16],
            identity: X25519Pair::generate().unwrap(),
        };
        s.save_local(&local).unwrap();
        let mut p = Prekeys::default();
        p.upload(&local, true, 3, 3).unwrap();
        s.save_prekeys(&p).unwrap();
        assert_eq!(
            s.local().unwrap().unwrap().identity.public(),
            local.identity.public()
        );
        assert_eq!(s.prekeys().unwrap().counts(), (3, 3));
        assert_eq!(s.sessions().unwrap().addresses().count(), 0);
        s.outbox_put(&[1; 16], &[2; 16], &[3; 16], b"c", 5).unwrap();
        assert_eq!(s.outbox().unwrap().len(), 1);
        s.outbox_done(&[1; 16]).unwrap();
        assert_eq!(s.outbox().unwrap().len(), 0);
    }
}
