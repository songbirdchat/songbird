//! Support tickets and legal-request counts (Prompt 11, M6; `docs/PROTOCOL.md` §14.8; D-0132).
//!
//! **Tickets** are opened by a user, never by Songbird. Each holds messages **sealed** to the
//! support team's key (the user's) or to the ticket's own reply key (the team's): the registry
//! and its backups hold ciphertext only. A ticket names an account only when its user attached
//! it (an appeal, a billing question). It is found by a random ID and read with a bearer whose
//! hash alone is stored. Closing deletes it with every message, logs included; one untouched
//! for [`TICKET_DAYS`] goes too.
//!
//! **Legal requests** leave counts only (half-year, requesting country, kind, outcome) for the
//! transparency report; case files stay offline with counsel.
//!
//! | Table | Key | Holds |
//! | --- | --- | --- |
//! | `ticket` | ticket ID (16 random bytes) | kind, `SHA-256` of the bearer, the reply key (X25519 public), the account if attached, priority, the last UTC day it changed |
//! | `ticket_msg` | `[ticket, n]` | who wrote it (`user`, `staff`), the sealed message, the UTC day |
//! | `legal` | `[half, country, kind, outcome]` | a count |

use bytes::Bytes;
use songbird_surreal::Value;
use songbird_types::AccountId;

use crate::db::DbError;
use crate::{Registry, RegistryError, bytes, first_int, id16, int, rows, var};

/// Days a ticket nobody touches is kept.
pub const TICKET_DAYS: u32 = 90;
/// Most messages in one ticket.
pub const MAX_MESSAGES: u64 = 50;

/// A ticket as staff list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketRow {
    /// Its ID.
    pub id: [u8; 16],
    /// `help`, `appeal`, `billing` or `crash`.
    pub kind: String,
    /// The attached account, if any.
    pub account: Option<AccountId>,
    /// Answered first (an attached account on Plus).
    pub priority: bool,
    /// The reply key (X25519 public).
    pub reply_key: [u8; 32],
    /// The last UTC day it changed.
    pub touched: u32,
    /// Messages in it.
    pub messages: u64,
}

/// One message of a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketMessage {
    /// `user` or `staff`.
    pub from: String,
    /// Sealed (to the support key, or to the ticket's reply key).
    pub sealed: Bytes,
    /// The UTC day.
    pub day: u32,
}

fn thrown(e: DbError) -> RegistryError {
    match e {
        DbError::Statement(k) if k == "Thrown:none" => RegistryError::NotFound,
        DbError::Statement(k) if k == "Thrown:full" => RegistryError::Full,
        e => e.into(),
    }
}

fn ticket_rows(v: &Value) -> Result<Vec<TicketRow>, RegistryError> {
    rows(v.clone())
        .iter()
        .map(|r| {
            Some(TicketRow {
                id: id16(r.get("i")?)?,
                kind: r.get("kind")?.as_text()?.to_owned(),
                account: r
                    .get("account")
                    .and_then(Value::as_bytes)
                    .and_then(|b| AccountId::from_slice(b).ok()),
                priority: matches!(r.get("priority"), Some(Value::Bool(true))),
                reply_key: r.get("reply")?.as_bytes()?.as_ref().try_into().ok()?,
                touched: u32::try_from(r.get("touched")?.as_i64()?).ok()?,
                messages: u64::try_from(r.get("n").and_then(Value::as_i64).unwrap_or(0)).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(RegistryError::Shape)
}

impl Registry {
    /// Opens a ticket with its first message.
    ///
    /// # Errors
    /// [`RegistryError::Exists`] for an ID taken (draw another).
    #[allow(clippy::too_many_arguments, reason = "a ticket's fields")]
    pub async fn ticket_open(
        &self,
        id: &[u8; 16],
        kind: &str,
        bearer: &[u8; 32],
        reply_key: &[u8; 32],
        account: Option<&AccountId>,
        priority: bool,
        sealed: &[u8],
        today: u32,
    ) -> Result<(), RegistryError> {
        self.db
            .query(
                "BEGIN TRANSACTION;
                 CREATE ticket:[$i] SET kind = $k, bearer = $b, reply = $r, account = $a,
                   priority = $p, touched = $t, msgs = 1 RETURN NONE;
                 CREATE ticket_msg:[$i, 0] SET ticket = $i, from = 'user', sealed = $s, day = $t
                   RETURN NONE;
                 COMMIT TRANSACTION;",
                vec![
                    var("i", bytes(id)),
                    var("k", Value::Text(kind.to_owned())),
                    var("b", bytes(bearer)),
                    var("r", bytes(reply_key)),
                    var("a", account.map_or(Value::none(), |a| bytes(a.as_bytes()))),
                    var("p", Value::Bool(priority)),
                    var("s", Value::Bytes(Bytes::copy_from_slice(sealed))),
                    var("t", int(u64::from(today))),
                ],
            )
            .await
            .map_err(|e| match e {
                DbError::Statement(k) if k == "AlreadyExists" => RegistryError::Exists,
                e => e.into(),
            })?;
        Ok(())
    }

    /// Adds a message: the user's (`bearer` given, must match) or staff's (`None`).
    ///
    /// # Errors
    /// [`RegistryError::NotFound`] (no ticket, a wrong bearer), [`RegistryError::Full`].
    pub async fn ticket_post(
        &self,
        id: &[u8; 16],
        bearer: Option<&[u8; 32]>,
        sealed: &[u8],
        today: u32,
    ) -> Result<(), RegistryError> {
        self.db
            .query(
                "BEGIN TRANSACTION;
                 LET $row = (SELECT bearer, msgs FROM ONLY ticket:[$i]);
                 IF $row = NONE OR ($user AND $row.bearer != $h) { THROW 'songbird:none' };
                 IF $row.msgs >= $max { THROW 'songbird:full' };
                 CREATE ticket_msg:[$i, $row.msgs] SET ticket = $i, from = IF $user { 'user' } ELSE { 'staff' },
                   sealed = $s, day = $t RETURN NONE;
                 UPDATE ticket:[$i] SET touched = $t, msgs += 1 RETURN NONE;
                 COMMIT TRANSACTION;",
                vec![
                    var("i", bytes(id)),
                    var("user", Value::Bool(bearer.is_some())),
                    var("h", bytes(bearer.unwrap_or(&[0; 32]))),
                    var("s", Value::Bytes(Bytes::copy_from_slice(sealed))),
                    var("max", int(MAX_MESSAGES)),
                    var("t", int(u64::from(today))),
                ],
            )
            .await
            .map_err(thrown)?;
        Ok(())
    }

    /// A ticket's messages, oldest first: the user's (`bearer`, must match) or staff's (`None`).
    ///
    /// # Errors
    /// [`RegistryError::NotFound`].
    pub async fn ticket_read(
        &self,
        id: &[u8; 16],
        bearer: Option<&[u8; 32]>,
    ) -> Result<(String, Vec<TicketMessage>), RegistryError> {
        let v = self
            .db
            .query(
                "LET $row = (SELECT kind, bearer FROM ONLY ticket:[$i]);
                 IF $row = NONE OR ($user AND $row.bearer != $h) { THROW 'songbird:none' };
                 RETURN $row.kind;
                 RETURN SELECT from, sealed, day, meta::id(id)[1] AS n FROM ticket_msg
                   WHERE ticket = $i ORDER BY n;",
                vec![
                    var("i", bytes(id)),
                    var("user", Value::Bool(bearer.is_some())),
                    var("h", bytes(bearer.unwrap_or(&[0; 32]))),
                ],
            )
            .await
            .map_err(thrown)?;
        let kind = v
            .iter()
            .find_map(|x| x.as_text().map(str::to_owned))
            .ok_or(RegistryError::Shape)?;
        let msgs = v
            .into_iter()
            .rev()
            .find(|x| x.as_array().is_some())
            .map(rows)
            .unwrap_or_default()
            .iter()
            .map(|r| {
                Some(TicketMessage {
                    from: r.get("from")?.as_text()?.to_owned(),
                    sealed: r.get("sealed")?.as_bytes()?.clone(),
                    day: u32::try_from(r.get("day")?.as_i64()?).ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(RegistryError::Shape)?;
        Ok((kind, msgs))
    }

    /// Closes a ticket: it and every message go. The user (`bearer`, must match) or staff.
    ///
    /// # Errors
    /// [`RegistryError::NotFound`].
    pub async fn ticket_close(
        &self,
        id: &[u8; 16],
        bearer: Option<&[u8; 32]>,
    ) -> Result<(), RegistryError> {
        self.db
            .query(
                "BEGIN TRANSACTION;
                 LET $b = (SELECT VALUE bearer FROM ONLY ticket:[$i]);
                 IF $b = NONE OR ($user AND $b != $h) { THROW 'songbird:none' };
                 DELETE ticket_msg WHERE ticket = $i RETURN NONE;
                 DELETE ticket:[$i] RETURN NONE;
                 COMMIT TRANSACTION;",
                vec![
                    var("i", bytes(id)),
                    var("user", Value::Bool(bearer.is_some())),
                    var("h", bytes(bearer.unwrap_or(&[0; 32]))),
                ],
            )
            .await
            .map_err(thrown)?;
        Ok(())
    }

    /// Tickets for staff, priority first, then the oldest change.
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn tickets(&self, limit: usize) -> Result<Vec<TicketRow>, RegistryError> {
        let v = self
            .db
            .query1(
                "SELECT meta::id(id)[0] AS i, kind, account, priority, reply, touched, msgs AS n
                 FROM ticket ORDER BY priority DESC, touched ASC LIMIT $l;",
                vec![var("l", int(u64::try_from(limit).unwrap_or(u64::MAX)))],
            )
            .await?;
        ticket_rows(&v)
    }

    /// One ticket's row, for staff.
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn ticket(&self, id: &[u8; 16]) -> Result<Option<TicketRow>, RegistryError> {
        let v = self
            .db
            .query1(
                "SELECT meta::id(id)[0] AS i, kind, account, priority, reply, touched, msgs AS n
                 FROM ticket:[$i];",
                vec![var("i", bytes(id))],
            )
            .await?;
        Ok(ticket_rows(&v)?.pop())
    }

    /// The tickets an account attached itself to (its GDPR export; messages stay sealed).
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn tickets_of(&self, account: &AccountId) -> Result<Vec<TicketRow>, RegistryError> {
        let v = self
            .db
            .query1(
                "SELECT meta::id(id)[0] AS i, kind, account, priority, reply, touched, msgs AS n
                 FROM ticket WHERE account = $a;",
                vec![var("a", bytes(account.as_bytes()))],
            )
            .await?;
        ticket_rows(&v)
    }

    /// Deletes tickets untouched since before `before` (a UTC day), with their messages.
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn sweep_tickets(&self, before: u32) -> Result<(), RegistryError> {
        self.db
            .query(
                "LET $gone = (SELECT VALUE meta::id(id)[0] FROM ticket WHERE touched < $b);
                 DELETE ticket_msg WHERE ticket IN $gone RETURN NONE;
                 DELETE ticket WHERE touched < $b RETURN NONE;",
                vec![var("b", int(u64::from(before)))],
            )
            .await?;
        Ok(())
    }

    /// Counts one legal request (counts only, D-0132).
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn count_legal(
        &self,
        half: &str,
        country: &str,
        kind: &str,
        outcome: &str,
    ) -> Result<(), RegistryError> {
        self.db
            .query(
                "UPSERT legal:[$h, $c, $k, $o] SET n = (n ?? 0) + 1 RETURN NONE;",
                vec![
                    var("h", Value::Text(half.to_owned())),
                    var("c", Value::Text(country.to_owned())),
                    var("k", Value::Text(kind.to_owned())),
                    var("o", Value::Text(outcome.to_owned())),
                ],
            )
            .await?;
        Ok(())
    }

    /// The legal counts of a half-year: `(country, kind, outcome, n)`.
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn legal_counts(
        &self,
        half: &str,
    ) -> Result<Vec<(String, String, String, u64)>, RegistryError> {
        let v = self
            .db
            .query1(
                "SELECT meta::id(id) AS k, n FROM legal WHERE meta::id(id)[0] = $h;",
                vec![var("h", Value::Text(half.to_owned()))],
            )
            .await?;
        rows(v)
            .iter()
            .map(|r| {
                let k = r.get("k")?.as_array()?;
                let t = |i: usize| k.get(i).and_then(Value::as_text).map(str::to_owned);
                Some((
                    t(1)?,
                    t(2)?,
                    t(3)?,
                    u64::try_from(first_int(r.get("n")?)?).ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(RegistryError::Shape)
    }

    /// Standings now, counted by kind and source (the transparency report).
    ///
    /// # Errors
    /// [`RegistryError`].
    pub async fn standing_counts(
        &self,
        today: u32,
    ) -> Result<Vec<(String, String, u64)>, RegistryError> {
        Ok(self
            .standings(today)
            .await?
            .into_iter()
            .fold(std::collections::BTreeMap::new(), |mut m, r| {
                *m.entry((r.kind, r.source)).or_insert(0u64) += 1;
                m
            })
            .into_iter()
            .map(|((k, s), n)| (k, s, n))
            .collect())
    }
}
