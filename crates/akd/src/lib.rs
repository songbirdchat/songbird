//! Key transparency on Meta's `akd` (Prompt 3, D-0057; the owner's decision of 2026-11-16).
//!
//! `akd` is a verifiable key directory: a VRF hides labels, and lookup, history and
//! append-only (audit) proofs let clients and auditors check the directory without trusting the
//! server. Songbird supplies its storage ([`store::SurrealStore`] on SurrealDB), the VRF key from
//! the secrets store ([`SecretVrf`]), and its labels:
//!
//! - **Label** = `kind u8 || account [16]`: deterministic, so an account has exactly one label per
//!   kind and akd's one-value-per-label guarantee means everyone is shown the same value (the VRF
//!   keeps labels out of the tree).
//! - **Value** = `HMAC-SHA256(salt, "songbird.v1/kt-value\0" || kind || raw)`, where `raw` is
//!   SHA-256 of the identity key or of the signed device list, or the username hash, and `salt` is
//!   a random per-account salt given to whoever asks. A server that hands someone another salt
//!   cannot make the one published value match another key (that would need an HMAC key for a
//!   given output). Deleting the salt with the account leaves its values unlinkable (`akd` 0.13
//!   cannot tombstone them); the label keeps the account ID and the epochs of its changes, nothing
//!   more.
//!
//! One writer (the worker) publishes an epoch per batch of changes; `api` nodes read with
//! [`akd::directory::ReadOnlyDirectory`].
#![forbid(unsafe_code)]

pub mod head;
pub mod proofs;
pub mod store;

use akd::directory::{Directory, ReadOnlyDirectory};
use akd::ecvrf::{VRFKeyStorage, VrfError};
use akd::storage::StorageManager;
use akd::{AkdLabel, AkdValue, AzksParallelismConfig};

pub use akd::WhatsAppV1Configuration as Config;
pub use head::SignedHead;
pub use store::SurrealStore;

/// The writer's directory.
pub type Writer = Directory<Config, SurrealStore, SecretVrf>;
/// A reader's directory.
pub type Reader = ReadOnlyDirectory<Config, SurrealStore, SecretVrf>;

/// The VRF private key (an Ed25519 seed) from the secrets store.
#[derive(Clone)]
pub struct SecretVrf(std::sync::Arc<[u8; 32]>);

impl std::fmt::Debug for SecretVrf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretVrf(..)")
    }
}

impl SecretVrf {
    /// From the 32-byte seed.
    #[must_use]
    pub fn new(seed: &[u8; 32]) -> Self {
        Self(std::sync::Arc::new(*seed))
    }
}

#[async_trait::async_trait]
impl VRFKeyStorage for SecretVrf {
    async fn retrieve(&self) -> Result<Vec<u8>, VrfError> {
        Ok(self.0.to_vec())
    }
}

/// What a label is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The account identity key.
    Identity = 1,
    /// The signed device list.
    DeviceList = 2,
    /// The username hash.
    Username = 3,
}

impl Kind {
    /// Wire value.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Identity => 1,
            Self::DeviceList => 2,
            Self::Username => 3,
        }
    }
}

/// The label for one kind of an account's entries: `kind || account`.
#[must_use]
pub fn label(kind: Kind, account: &[u8; 16]) -> AkdLabel {
    let mut l = Vec::with_capacity(17);
    l.push(kind.code());
    l.extend_from_slice(account);
    AkdLabel(l)
}

/// The published value: `HMAC-SHA256(salt, "songbird.v1/kt-value\0" || kind || raw)`.
#[must_use]
pub fn value(salt: &[u8; 32], kind: Kind, raw: &[u8; 32]) -> AkdValue {
    use aws_lc_rs::hmac;
    let k = hmac::Key::new(hmac::HMAC_SHA256, salt);
    let mut ctx = hmac::Context::with_key(&k);
    ctx.update(b"songbird.v1/kt-value\0");
    ctx.update(&[kind.code()]);
    ctx.update(raw);
    AkdValue(ctx.sign().as_ref().to_vec())
}

/// Opens (or creates) the writer's directory.
///
/// # Errors
/// The storage or the directory refused.
pub async fn writer(store: SurrealStore, vrf: SecretVrf) -> Result<Writer, akd::errors::AkdError> {
    Directory::new(
        StorageManager::new_no_cache(store),
        vrf,
        AzksParallelismConfig::disabled(),
    )
    .await
}

/// Opens a reader's directory (the writer must have created it).
///
/// # Errors
/// The storage refused, or no directory exists yet.
pub async fn reader(store: SurrealStore, vrf: SecretVrf) -> Result<Reader, akd::errors::AkdError> {
    ReadOnlyDirectory::new(
        StorageManager::new_no_cache(store),
        vrf,
        AzksParallelismConfig::disabled(),
    )
    .await
}

fn kind_of(k: songbird_accounts::kt::KtKind) -> Kind {
    match k {
        songbird_accounts::kt::KtKind::Identity => Kind::Identity,
        songbird_accounts::kt::KtKind::DeviceList => Kind::DeviceList,
        songbird_accounts::kt::KtKind::Username => Kind::Username,
    }
}

/// Publishes what changed (the worker's pass): one epoch for up to `limit` accounts, then marks
/// them published. Returns the entries published (0: no epoch).
///
/// # Errors
/// The registry or `akd` refused; nothing is marked, so the next pass retries.
pub async fn publish_pending(
    registry: &songbird_accounts::Registry,
    writer: &Writer,
    limit: usize,
) -> Result<usize, PublishError> {
    let pending = registry
        .kt_pending(limit)
        .await
        .map_err(PublishError::Registry)?;
    let updates: Vec<(AkdLabel, AkdValue)> = pending
        .iter()
        .flat_map(|p| {
            p.changes.iter().map(move |(k, v)| {
                let kind = kind_of(*k);
                (label(kind, p.account.as_bytes()), value(&p.salt, kind, v))
            })
        })
        .collect();
    let n = updates.len();
    if n > 0 {
        writer.publish(updates).await.map_err(PublishError::Akd)?;
    }
    for p in &pending {
        registry
            .kt_logged(p)
            .await
            .map_err(PublishError::Registry)?;
    }
    Ok(n)
}

/// The writer's directory, for the service that owns it (the worker), and its store (where
/// each epoch's root is recorded, D-0171).
pub struct Publisher(pub Writer, SurrealStore);

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Publisher(..)")
    }
}

impl Publisher {
    /// Opens (or creates) the directory.
    ///
    /// # Errors
    /// The storage or the directory refused.
    pub async fn open(store: SurrealStore, vrf: SecretVrf) -> Result<Self, akd::errors::AkdError> {
        Ok(Self(writer(store.clone(), vrf).await?, store))
    }

    /// Records the current epoch's root (so a crash between publishing and recording leaves
    /// no gap: the next pass records it).
    async fn record_root(&self) -> Result<(), PublishError> {
        let h = self.0.get_epoch_hash().await.map_err(PublishError::Akd)?;
        if h.epoch() > 0 {
            self.1
                .put_root(h.epoch(), &h.hash())
                .await
                .map_err(|e| PublishError::Akd(akd::errors::AkdError::Storage(e)))?;
        }
        Ok(())
    }

    /// [`publish_pending`].
    ///
    /// # Errors
    /// [`PublishError`].
    pub async fn publish_pending(
        &self,
        registry: &songbird_accounts::Registry,
        limit: usize,
    ) -> Result<usize, PublishError> {
        self.record_root().await?;
        let n = publish_pending(registry, &self.0, limit).await?;
        if n > 0 {
            self.record_root().await?;
        }
        Ok(n)
    }
}

/// Publishing failed.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// The registry.
    #[error("registry: {0}")]
    Registry(songbird_accounts::RegistryError),
    /// `akd`.
    #[error("akd: {0}")]
    Akd(akd::errors::AkdError),
}
