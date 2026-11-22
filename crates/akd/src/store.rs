//! akd's storage on SurrealDB.
//!
//! - Azks and tree-node records live in `akdrec:[binary id]`, serialized with serde.
//! - Value states live in `akdval:[label, epoch]` with the label indexed, so a label's history is
//!   one indexed read.
//! - `batch_set` writes in one transaction, the Azks record last (as akd's storage manager orders
//!   them), so a reader never sees an epoch whose nodes are not all stored.
//! - **One writer, enforced**: a batch carrying the Azks record moves `akdhead:h` from epoch
//!   `E - 1` to `E` in the same transaction (a compare and swap), so a second writer that started
//!   from the same epoch fails instead of overwriting it (and its changes are published by the
//!   next pass).

use std::collections::HashMap;
use std::fmt::Write as _;

use akd::errors::StorageError;
use akd::storage::types::{
    DbRecord, KeyData, StorageType, ValueState, ValueStateKey, ValueStateRetrievalFlag,
};
use akd::storage::{Database, DbSetState, Storable};
use akd::{AkdLabel, AkdValue};
use async_trait::async_trait;
use songbird_accounts::Db;
use songbird_surreal::Value;

/// Records per read or write statement batch.
const CHUNK: usize = 400;

/// The schema (created on connect).
pub const SCHEMA: &str = "
DEFINE TABLE IF NOT EXISTS akdrec SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS rec ON akdrec TYPE bytes;
DEFINE TABLE IF NOT EXISTS akdval SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS label ON akdval TYPE bytes;
DEFINE FIELD IF NOT EXISTS epoch ON akdval TYPE int;
DEFINE FIELD IF NOT EXISTS rec ON akdval TYPE bytes;
DEFINE INDEX IF NOT EXISTS akdval_label ON akdval FIELDS label;
DEFINE TABLE IF NOT EXISTS akdhead SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS epoch ON akdhead TYPE int;
DEFINE TABLE IF NOT EXISTS akdroot SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS root ON akdroot TYPE bytes;
";

/// akd's storage on SurrealDB.
#[derive(Debug, Clone)]
pub struct SurrealStore {
    db: std::sync::Arc<Db>,
}

fn bytes(b: &[u8]) -> Value {
    Value::Bytes(bytes::Bytes::copy_from_slice(b))
}

fn int(v: u64) -> Value {
    Value::Int(i64::try_from(v).unwrap_or(i64::MAX))
}

fn other(e: impl std::fmt::Display) -> StorageError {
    StorageError::Other(e.to_string())
}

fn encode(r: &DbRecord) -> Result<Vec<u8>, StorageError> {
    serde_json::to_vec(r).map_err(other)
}

fn decode(v: &Value) -> Result<DbRecord, StorageError> {
    let b = v.as_bytes().ok_or_else(|| other("record shape"))?;
    serde_json::from_slice(b).map_err(other)
}

impl SurrealStore {
    /// Creates the schema if missing.
    ///
    /// # Errors
    /// The database refused.
    pub async fn connect(db: std::sync::Arc<Db>) -> Result<Self, StorageError> {
        db.exec(SCHEMA, Vec::new()).await.map_err(other)?;
        Ok(Self { db })
    }

    /// Records epoch `epoch`'s root (D-0171): devices audit between heads with every root in
    /// between. Writing it again is harmless (a root never changes).
    ///
    /// # Errors
    /// The database refused.
    pub async fn put_root(&self, epoch: u64, root: &[u8; 32]) -> Result<(), StorageError> {
        self.db
            .exec(
                "UPSERT akdroot:[$e] SET root = $r RETURN NONE;",
                vec![("e".to_owned(), int(epoch)), ("r".to_owned(), bytes(root))],
            )
            .await
            .map_err(other)
    }

    /// The roots of epochs `from..=to`, in order; `None` if any is missing.
    ///
    /// # Errors
    /// The database refused.
    pub async fn roots(&self, from: u64, to: u64) -> Result<Option<Vec<[u8; 32]>>, StorageError> {
        let v = self
            .db
            .query1(
                "SELECT VALUE [meta::id(id)[0], root] FROM akdroot
                 WHERE meta::id(id)[0] >= $f AND meta::id(id)[0] <= $t;",
                vec![("f".to_owned(), int(from)), ("t".to_owned(), int(to))],
            )
            .await
            .map_err(other)?;
        let mut by: std::collections::BTreeMap<u64, [u8; 32]> = std::collections::BTreeMap::new();
        for r in v.into_array().unwrap_or_default() {
            let mut a = r.into_array().unwrap_or_default().into_iter();
            let (Some(e), Some(root)) = (a.next(), a.next()) else {
                continue;
            };
            let (Some(e), Some(root)) = (
                e.as_i64().and_then(|e| u64::try_from(e).ok()),
                root.as_bytes()
                    .and_then(|b| <[u8; 32]>::try_from(b.as_ref()).ok()),
            ) else {
                continue;
            };
            by.insert(e, root);
        }
        let want = usize::try_from(to.saturating_sub(from) + 1).unwrap_or(usize::MAX);
        Ok((by.len() == want).then(|| by.into_values().collect()))
    }

    async fn get_bin(&self, st: StorageType, bin: &[u8]) -> Result<DbRecord, StorageError> {
        if st == StorageType::ValueState {
            let ValueStateKey(label, epoch) =
                ValueState::key_from_full_binary(bin).map_err(other)?;
            let v = self
                .db
                .query1(
                    "RETURN akdval:[$l, $e].rec;",
                    vec![
                        ("l".to_owned(), bytes(&label)),
                        ("e".to_owned(), int(epoch)),
                    ],
                )
                .await
                .map_err(other)?;
            return if v.as_bytes().is_some() {
                decode(&v)
            } else {
                Err(StorageError::NotFound("value state".into()))
            };
        }
        let v = self
            .db
            .query1(
                "RETURN akdrec:[$b].rec;",
                vec![("b".to_owned(), bytes(bin))],
            )
            .await
            .map_err(other)?;
        if v.as_bytes().is_some() {
            decode(&v)
        } else {
            Err(StorageError::NotFound(format!("{st:?}")))
        }
    }

    async fn states(&self, label: &AkdLabel) -> Result<Vec<ValueState>, StorageError> {
        let v = self
            .db
            .query1(
                "SELECT VALUE rec FROM akdval WHERE label = $l ORDER BY epoch;",
                vec![("l".to_owned(), bytes(&label.0))],
            )
            .await
            .map_err(other)?;
        let mut out = Vec::new();
        for r in v.into_array().unwrap_or_default() {
            match decode(&r)? {
                DbRecord::ValueState(s) => out.push(s),
                _ => return Err(other("not a value state")),
            }
        }
        Ok(out)
    }
}

/// akd's own selection rules for a label's states (as its in-memory store applies them).
fn pick(states: &[ValueState], flag: ValueStateRetrievalFlag) -> Option<ValueState> {
    match flag {
        ValueStateRetrievalFlag::MaxEpoch => states.iter().max_by_key(|s| s.epoch).cloned(),
        ValueStateRetrievalFlag::MinEpoch => states.iter().min_by_key(|s| s.epoch).cloned(),
        ValueStateRetrievalFlag::SpecificVersion(v) => {
            states.iter().find(|s| s.version == v).cloned()
        }
        ValueStateRetrievalFlag::SpecificEpoch(e) => states.iter().find(|s| s.epoch == e).cloned(),
        ValueStateRetrievalFlag::LeqEpoch(e) => states
            .iter()
            .filter(|s| s.epoch <= e)
            .max_by_key(|s| s.epoch)
            .cloned(),
    }
}

#[async_trait]
impl Database for SurrealStore {
    async fn set(&self, record: DbRecord) -> Result<(), StorageError> {
        self.batch_set(vec![record], DbSetState::General).await
    }

    async fn batch_set(
        &self,
        mut records: Vec<DbRecord>,
        _state: DbSetState,
    ) -> Result<(), StorageError> {
        if records.is_empty() {
            return Ok(());
        }
        // The Azks record last: readers see the new epoch only once its nodes are stored.
        records.sort_by_key(|r| u8::from(matches!(r, DbRecord::Azks(_))));
        let mut sql = String::from("BEGIN TRANSACTION;\n");
        let mut vars = Vec::with_capacity(records.len() * 3 + 2);
        // The epoch compare-and-swap: only from the epoch before this one.
        if let Some(DbRecord::Azks(a)) = records.iter().find(|r| matches!(r, DbRecord::Azks(_))) {
            sql.push_str(
                "IF (akdhead:h.epoch ?? -1) != $prev_epoch { THROW 'songbird:epoch' };
                 UPSERT akdhead:h SET epoch = $new_epoch RETURN NONE;\n",
            );
            vars.push((
                "prev_epoch".to_owned(),
                Value::Int(i64::try_from(a.latest_epoch).unwrap_or(i64::MAX) - 1),
            ));
            vars.push(("new_epoch".to_owned(), int(a.latest_epoch)));
        }
        for (i, r) in records.iter().enumerate() {
            let rec = encode(r)?;
            if let DbRecord::ValueState(s) = r {
                let _ = writeln!(
                    sql,
                    "UPSERT akdval:[$l{i}, $e{i}] SET label = $l{i}, epoch = $e{i}, rec = $r{i} RETURN NONE;"
                );
                vars.push((format!("l{i}"), bytes(&s.username.0)));
                vars.push((format!("e{i}"), int(s.epoch)));
            } else {
                let _ = writeln!(sql, "UPSERT akdrec:[$b{i}] SET rec = $r{i} RETURN NONE;");
                vars.push((format!("b{i}"), bytes(&r.get_full_binary_id())));
            }
            vars.push((format!("r{i}"), bytes(&rec)));
        }
        sql.push_str("COMMIT TRANSACTION;");
        self.db
            .query(&sql, vars)
            .await
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn get<St: Storable>(&self, id: &St::StorageKey) -> Result<DbRecord, StorageError> {
        self.get_bin(St::data_type(), &St::get_full_binary_key_id(id))
            .await
    }

    async fn batch_get<St: Storable>(
        &self,
        ids: &[St::StorageKey],
    ) -> Result<Vec<DbRecord>, StorageError> {
        let st = St::data_type();
        let mut out = Vec::with_capacity(ids.len());
        if st == StorageType::ValueState {
            for id in ids {
                if let Ok(r) = self.get_bin(st, &St::get_full_binary_key_id(id)).await {
                    out.push(r);
                }
            }
            return Ok(out);
        }
        for chunk in ids.chunks(CHUNK) {
            let mut sql = String::new();
            let mut vars = Vec::with_capacity(chunk.len());
            for (i, id) in chunk.iter().enumerate() {
                let _ = writeln!(sql, "RETURN akdrec:[$b{i}].rec;");
                vars.push((format!("b{i}"), bytes(&St::get_full_binary_key_id(id))));
            }
            for v in self.db.query(&sql, vars).await.map_err(other)? {
                if v.as_bytes().is_some() {
                    out.push(decode(&v)?);
                }
            }
        }
        Ok(out)
    }

    async fn get_user_data(&self, username: &AkdLabel) -> Result<KeyData, StorageError> {
        let states = self.states(username).await?;
        if states.is_empty() {
            return Err(StorageError::NotFound("value state".into()));
        }
        Ok(KeyData { states })
    }

    async fn get_user_state(
        &self,
        username: &AkdLabel,
        flag: ValueStateRetrievalFlag,
    ) -> Result<ValueState, StorageError> {
        pick(&self.states(username).await?, flag)
            .ok_or_else(|| StorageError::NotFound("value state".into()))
    }

    async fn get_user_state_versions(
        &self,
        usernames: &[AkdLabel],
        flag: ValueStateRetrievalFlag,
    ) -> Result<HashMap<AkdLabel, (u64, AkdValue)>, StorageError> {
        let mut map = HashMap::new();
        for u in usernames {
            if let Some(s) = pick(&self.states(u).await?, flag) {
                map.insert(
                    AkdLabel(s.username.0.clone()),
                    (s.version, AkdValue(s.value.0.clone())),
                );
            }
        }
        Ok(map)
    }
}
