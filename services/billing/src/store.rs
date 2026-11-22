//! Billing's own database (D-0126 §3, D-0129): its own SurrealDB database and credentials,
//! never the registry's. **No account ID is ever stored here**, and nothing here names one.
//!
//! | Table | Key | What |
//! | --- | --- | --- |
//! | `sub` | `HMAC(billing secret, provider ‖ 0 ‖ provider's ID)[..16]` | provider, plan, the last UTC day paid for, revoked, renews, the time of the newest store message applied; for Mollie also its customer and subscription IDs and the interval (needed to cancel) |
//! | `subtok` | (sub key, period) | how many tokens were issued for it that period, until when the row is kept |
//! | `notif` | `SHA-256` of a provider's message ID | that it was handled (replays change nothing), until when |
//!
//! Apple's and Google's identifiers are never stored: the client or the provider sends them
//! again whenever they matter, and only their keyed hash names a row.

use aws_lc_rs::hmac;
use bytes::Bytes;
use songbird_accounts::{Db, DbError};
use songbird_surreal::Value;

/// Days after its last paid day that a subscription that does not renew is kept: past the
/// stores' refund windows and chargebacks (up to about 13 months for cards).
pub const LAPSED_DAYS: u32 = 400;

/// A subscription row's key.
pub type SubKey = [u8; 16];

const SCHEMA: &str = "
DEFINE TABLE IF NOT EXISTS sub SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS provider ON sub TYPE string;
DEFINE FIELD IF NOT EXISTS plan ON sub TYPE string;
DEFINE FIELD IF NOT EXISTS paid ON sub TYPE int;
DEFINE FIELD IF NOT EXISTS revoked ON sub TYPE bool;
DEFINE FIELD IF NOT EXISTS renews ON sub TYPE bool;
DEFINE FIELD IF NOT EXISTS stamp ON sub TYPE int;
DEFINE FIELD IF NOT EXISTS interval ON sub TYPE option<string>;
DEFINE FIELD IF NOT EXISTS mcust ON sub TYPE option<string>;
DEFINE FIELD IF NOT EXISTS msub ON sub TYPE option<string>;
DEFINE FIELD IF NOT EXISTS at ON sub TYPE datetime;
DEFINE INDEX IF NOT EXISTS sub_msub ON sub FIELDS msub;
DEFINE TABLE IF NOT EXISTS subtok SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS n ON subtok TYPE int;
DEFINE FIELD IF NOT EXISTS until ON subtok TYPE int;
DEFINE INDEX IF NOT EXISTS subtok_until ON subtok FIELDS until;
DEFINE TABLE IF NOT EXISTS notif SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS until ON notif TYPE int;
DEFINE INDEX IF NOT EXISTS notif_until ON notif FIELDS until;
";

/// Store failures (no identifier in any).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The database is unreachable or failed.
    #[error(transparent)]
    Db(#[from] DbError),
    /// A row did not have the expected shape.
    #[error("unexpected row shape")]
    Shape,
    /// The period's token cap is reached.
    #[error("token cap reached")]
    Capped,
}

/// What a provider says about a subscription now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// `apple`, `google` or `mollie`.
    pub provider: &'static str,
    /// `plus` or `family`.
    pub plan: &'static str,
    /// The last UTC day paid for (grace included); 0 when never.
    pub paid: u32,
    /// Refunded, charged back or revoked.
    pub revoked: bool,
    /// Renews automatically.
    pub renews: bool,
}

/// A subscription row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubRow {
    /// Its state.
    pub state: State,
    /// The newest provider message applied (milliseconds; 0 for none).
    pub stamp: u64,
    /// Mollie only: monthly or yearly.
    pub interval: Option<String>,
    /// Mollie only: its customer ID.
    pub customer: Option<String>,
    /// Mollie only: its subscription ID.
    pub subscription: Option<String>,
}

fn provider(s: &str) -> Option<&'static str> {
    ["apple", "google", "mollie"].into_iter().find(|p| *p == s)
}

/// A plan's stored name.
#[must_use]
pub fn plan(s: &str) -> Option<&'static str> {
    ["plus", "family"].into_iter().find(|p| *p == s)
}

fn var(name: &str, v: Value) -> (String, Value) {
    (name.to_owned(), v)
}

fn bytes(b: &[u8]) -> Value {
    Value::Bytes(Bytes::copy_from_slice(b))
}

fn int(v: u64) -> Value {
    Value::Int(i64::try_from(v).unwrap_or(i64::MAX))
}

fn text(s: &str) -> Value {
    Value::Text(s.to_owned())
}

fn opt_text(s: Option<&str>) -> Value {
    s.map_or(Value::Null, text)
}

fn sub_row(v: &Value) -> Option<SubRow> {
    let t = |k: &str| v.get(k).and_then(Value::as_text).map(str::to_owned);
    Some(SubRow {
        state: State {
            provider: provider(v.get("provider")?.as_text()?)?,
            plan: plan(v.get("plan")?.as_text()?)?,
            paid: u32::try_from(v.get("paid")?.as_i64()?).ok()?,
            revoked: matches!(v.get("revoked")?, Value::Bool(true)),
            renews: matches!(v.get("renews")?, Value::Bool(true)),
        },
        stamp: u64::try_from(v.get("stamp")?.as_i64()?).ok()?,
        interval: t("interval"),
        customer: t("mcust"),
        subscription: t("msub"),
    })
}

/// The billing store.
#[derive(Debug)]
pub struct Store {
    db: Db,
    key: hmac::Key,
}

impl Store {
    /// Runs a statement, retrying SurrealDB's lost write conflicts (concurrent requests for one
    /// subscription) with a short random backoff.
    async fn retrying(
        &self,
        sql: &str,
        vars: impl Fn() -> Vec<(String, Value)>,
    ) -> Result<Vec<Value>, DbError> {
        let mut attempt = 0u32;
        loop {
            match self.db.query(sql, vars()).await {
                Err(e) if songbird_accounts::is_conflict(&e) && attempt < 30 => {
                    attempt += 1;
                    let mut r = [0u8; 2];
                    let _ = getrandom::fill(&mut r);
                    let jitter = u64::from(u16::from_le_bytes(r) % 20);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        jitter + u64::from(attempt.min(10)) * 2,
                    ))
                    .await;
                }
                r => return r,
            }
        }
    }

    /// Connects, defines the schema; `secret` keys the row names.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn connect(db: Db, secret: &[u8; 32]) -> Result<Self, StoreError> {
        db.exec(SCHEMA, Vec::new()).await?;
        Ok(Self {
            db,
            key: hmac::Key::new(hmac::HMAC_SHA256, secret),
        })
    }

    /// The database (tests read it whole).
    #[must_use]
    pub const fn db(&self) -> &Db {
        &self.db
    }

    /// The row key of a provider's subscription identifier.
    #[must_use]
    pub fn key(&self, provider: &str, id: &[u8]) -> SubKey {
        let mut ctx = hmac::Context::with_key(&self.key);
        ctx.update(provider.as_bytes());
        ctx.update(&[0]);
        ctx.update(id);
        let mut out = [0u8; 16];
        out.copy_from_slice(ctx.sign().as_ref().get(..16).unwrap_or(&[0; 16]));
        out
    }

    /// A row.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn get(&self, key: &SubKey) -> Result<Option<SubRow>, StoreError> {
        let v = self
            .db
            .query1("SELECT * FROM ONLY sub:[$k];", vec![var("k", bytes(key))])
            .await?;
        if v.is_absent() {
            return Ok(None);
        }
        sub_row(&v).map(Some).ok_or(StoreError::Shape)
    }

    /// Applies a provider's state if it is not older than what the row holds (`stamp`, the
    /// provider's own time of the message, milliseconds; `None` for an answer from the
    /// provider's API, which is the state now and always applies, and keeps the row's stamp: the
    /// row never records when a token request came, D-0134). With `clears` false, a
    /// revocation already recorded stays (Google: a voided purchase token is over for good);
    /// Apple's newer state may clear it (a later period paid). Returns whether it applied.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn apply(
        &self,
        key: &SubKey,
        s: &State,
        stamp: Option<u64>,
        clears: bool,
    ) -> Result<bool, StoreError> {
        let v = self
            .retrying(
                "BEGIN TRANSACTION;
                 LET $row = (SELECT stamp, revoked FROM ONLY sub:[$k]);
                 LET $old = $row.stamp ?? -1;
                 LET $go = $now OR $s >= $old;
                 LET $keep = IF $now { math::max([$old, 0]) } ELSE { $s };
                 LET $rv = $r OR (!$clear AND ($row.revoked ?? false));
                 IF $go {
                   UPSERT sub:[$k] SET provider = $p, plan = $pl, paid = $d, revoked = $rv,
                     renews = $n AND !$rv, stamp = $keep, at = time::floor(time::now(), 1d) RETURN NONE;
                 };
                 RETURN $go;
                 COMMIT TRANSACTION;",
                || {
                    vec![
                        var("k", bytes(key)),
                        var("p", text(s.provider)),
                        var("pl", text(s.plan)),
                        var("d", int(u64::from(s.paid))),
                        var("r", Value::Bool(s.revoked)),
                        var("n", Value::Bool(s.renews)),
                        var("s", int(stamp.unwrap_or(0))),
                        var("now", Value::Bool(stamp.is_none())),
                        var("clear", Value::Bool(clears)),
                    ]
                },
            )
            .await?;
        Ok(v.iter().rev().any(|x| matches!(x, Value::Bool(true))))
    }

    /// Marks a subscription revoked (a refund, a chargeback, a voided purchase), whatever its
    /// stamp. Returns whether a row existed.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn revoke(&self, key: &SubKey) -> Result<bool, StoreError> {
        let v = self
            .retrying(
                "UPDATE sub:[$k] SET revoked = true, renews = false, at = time::floor(time::now(), 1d);",
                || vec![var("k", bytes(key))],
            )
            .await?;
        let v = v.into_iter().next().unwrap_or(Value::Null);
        Ok(!v.is_absent() && v.as_array().is_none_or(|a| !a.is_empty()))
    }

    /// Spends `n` of a period's token cap. Atomic: concurrent requests cannot both pass it.
    ///
    /// # Errors
    /// [`StoreError::Capped`] (nothing spent).
    pub async fn spend(
        &self,
        key: &SubKey,
        period: u32,
        n: u32,
        cap: u32,
        keep_until: u32,
    ) -> Result<(), StoreError> {
        let r = self
            .retrying(
                "BEGIN TRANSACTION;
                 LET $c = (SELECT VALUE n FROM ONLY subtok:[$k, $m]) ?? 0;
                 IF $c + $n > $cap { THROW 'songbird:capped' };
                 UPSERT subtok:[$k, $m] SET n = $c + $n, until = $u RETURN NONE;
                 COMMIT TRANSACTION;",
                || {
                    vec![
                        var("k", bytes(key)),
                        var("m", int(u64::from(period))),
                        var("n", int(u64::from(n))),
                        var("cap", int(u64::from(cap))),
                        var("u", int(u64::from(keep_until))),
                    ]
                },
            )
            .await;
        match r {
            Ok(_) => Ok(()),
            Err(DbError::Statement(k)) if k == "Thrown:capped" => Err(StoreError::Capped),
            Err(e) => Err(e.into()),
        }
    }

    /// Records a provider message ID as handled; `false` if it already was (a replay).
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn first_time(
        &self,
        id_hash: &[u8; 32],
        keep_until: u32,
    ) -> Result<bool, StoreError> {
        let r = self
            .retrying(
                "BEGIN TRANSACTION;
                 IF record::exists(notif:[$h]) { THROW 'songbird:seen' };
                 CREATE notif:[$h] SET until = $u RETURN NONE;
                 COMMIT TRANSACTION;",
                || {
                    vec![
                        var("h", bytes(id_hash)),
                        var("u", int(u64::from(keep_until))),
                    ]
                },
            )
            .await;
        match r {
            Ok(_) => Ok(true),
            Err(DbError::Statement(k)) if k == "Thrown:seen" => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Whether a provider message ID was handled already.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn get_seen(&self, id_hash: &[u8; 32]) -> Result<bool, StoreError> {
        let v = self
            .db
            .query1(
                "RETURN record::exists(notif:[$h]);",
                vec![var("h", bytes(id_hash))],
            )
            .await?;
        Ok(matches!(v, Value::Bool(true)))
    }

    /// Forgets a handled mark (an action after it failed and must be retried).
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn forget(&self, id_hash: &[u8; 32]) -> Result<(), StoreError> {
        self.db
            .query(
                "DELETE notif:[$h] RETURN NONE;",
                vec![var("h", bytes(id_hash))],
            )
            .await?;
        Ok(())
    }

    /// Mollie: a new checkout's row (not paid yet).
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn mollie_started(
        &self,
        key: &SubKey,
        plan: &'static str,
        interval: &str,
        customer: &str,
    ) -> Result<(), StoreError> {
        self.db
            .query(
                "UPSERT sub:[$k] SET provider = 'mollie', plan = $pl, paid = paid ?? 0,
                   revoked = revoked ?? false, renews = false, stamp = stamp ?? 0, interval = $i,
                   mcust = $c, at = time::floor(time::now(), 1d) RETURN NONE;",
                vec![
                    var("k", bytes(key)),
                    var("pl", text(plan)),
                    var("i", text(interval)),
                    var("c", text(customer)),
                ],
            )
            .await?;
        Ok(())
    }

    /// Mollie: a paid period (first or recurring) moves `paid` to `paid` (never back) and records
    /// the subscription ID.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn mollie_paid(
        &self,
        key: &SubKey,
        subscription: Option<&str>,
        paid: u32,
        first: bool,
    ) -> Result<(), StoreError> {
        // Only the first payment (which starts the subscription) says it renews; a recurring
        // one never turns renewal back on after a cancellation.
        self.retrying(
            "UPDATE sub:[$k] SET paid = math::max([paid, $d]), renews = IF $f { true } ELSE { renews },
                   msub = $s ?? msub, at = time::floor(time::now(), 1d) RETURN NONE;",
            || {
                vec![
                    var("k", bytes(key)),
                    var("d", int(u64::from(paid))),
                    var("s", opt_text(subscription)),
                    var("f", Value::Bool(first)),
                ]
            },
        )
        .await?;
        Ok(())
    }

    /// Mollie: renewal off (cancelled); what was paid stays.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn mollie_cancelled(&self, key: &SubKey) -> Result<(), StoreError> {
        self.db
            .query(
                "UPDATE sub:[$k] SET renews = false, at = time::floor(time::now(), 1d) RETURN NONE;",
                vec![var("k", bytes(key))],
            )
            .await?;
        Ok(())
    }

    /// Mollie: renewal on again with a new subscription (D-0169).
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn mollie_resumed(&self, key: &SubKey, subscription: &str) -> Result<(), StoreError> {
        self.db
            .query(
                "UPDATE sub:[$k] SET renews = true, msub = $s, at = time::floor(time::now(), 1d) RETURN NONE;",
                vec![var("k", bytes(key)), var("s", text(subscription))],
            )
            .await?;
        Ok(())
    }

    /// Mollie: the row of a subscription ID.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn by_mollie_subscription(&self, id: &str) -> Result<Option<SubKey>, StoreError> {
        let v = self
            .db
            .query1(
                "SELECT VALUE meta::id(id)[0] FROM sub WHERE msub = $s LIMIT 1;",
                vec![var("s", text(id))],
            )
            .await?;
        Ok(v.as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_bytes)
            .and_then(|b| b.as_ref().try_into().ok()))
    }

    /// Deletes cap and message rows past their day, and subscriptions that neither renew nor
    /// were paid for in the last [`LAPSED_DAYS`] (past any refund or chargeback window); returns
    /// how many rows went.
    ///
    /// # Errors
    /// [`StoreError`].
    pub async fn sweep(&self, today: u32) -> Result<u64, StoreError> {
        let v = self
            .db
            .query(
                "RETURN count((DELETE subtok WHERE until < $t RETURN BEFORE))
                   + count((DELETE notif WHERE until < $t RETURN BEFORE))
                   + count((DELETE sub WHERE renews = false AND paid < $lapsed RETURN BEFORE));",
                vec![
                    var("t", int(u64::from(today))),
                    var("lapsed", int(u64::from(today.saturating_sub(LAPSED_DAYS)))),
                ],
            )
            .await?;
        Ok(v.iter()
            .rev()
            .find_map(Value::as_i64)
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0))
    }
}
