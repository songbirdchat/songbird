//! MLS groups (Prompt 5, M1; D-0066, D-0067): the delivery service's HTTP side.
//!
//! - `POST /v1/groups`: a device creates a group: `version u8 (1) | group [16] | info_len u32 |
//!   GroupInfo | tree_len u32 | ratchet tree`. 204; 409 `exists`; 400 with the reason.
//! - `POST /v1/groups/commit`: a member commits: `version u8 (1) | group [16] | bundle`
//!   (`songbird_groups::wire::Bundle`). 200 `{"epoch": n}` once every recipient's mailbox holds
//!   the commit (and the added devices' the Welcome); 409 `{"error": "stale", "epoch": n}` if
//!   another commit won; 404 for an unknown group; 400 with the reason; 503 to retry (a retried
//!   commit that already won is answered 200 again, and its fan-out finished).
//! - `POST /v1/groups/state`: `version u8 (1) | group [16]`. 200 `epoch u64 | info_len u32 |
//!   GroupInfo | tree_len u32 | ratchet tree | meta_v u64 | meta_len u32 | meta | n u16 |
//!   n × (device [16] | account [16])` for a device in the group, or a listed device of an
//!   account in it (to rejoin by external commit); 404 otherwise, the same as for no group. The
//!   last part names each member device's account (D-0155): devices check every pair against
//!   the account's signed device list, so the server can leave one out but not lie.
//!
//! - Invite links and join requests (M3, D-0071): `POST /v1/groups/links`, `/links/revoke`,
//!   `/links/open`, `/join`, `/requests`, `/requests/list`, `/requests/decline` (`PROTOCOL.md`
//!   §6.8).
//!
//! Commits and Welcomes reach devices as ordinary mailbox envelopes: the `MLSMessage` bytes as
//! they were uploaded. The fan-out is recorded with the commit in one transaction
//! (`grp_fanout`), stored under keys only the servers can derive (a member cannot pre-empt them
//! with a send of its own), then forgotten; [`redrive`] finishes any a crash interrupted.

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::{digest, hkdf, hmac};
use bytes::Bytes;
use songbird_accounts::RegistryError;
use songbird_groups::external::DsSigner;
use songbird_groups::roles::Denied;
use songbird_groups::wire::Bundle;
use songbird_groups::{Group, GroupError, Limits, Record};
use songbird_types::{AccountId, DeviceId};

use crate::devices::Outcome;

/// What a group add may need to carry (Prompt 9): the `Group-Add-Access-Key` header (the XOR of
/// the access keys of every added account that takes adds from contacts only, D-0103) and the
/// `Group-Add-Approval` header (a parent's approvals for supervised children, D-0105:
/// comma-separated hex of `expires_s u64 | signature [64]`, at most 8).
#[derive(Debug, Clone, Default)]
pub struct AddProofs {
    /// The combined access key.
    pub add_key: Option<[u8; 16]>,
    /// Approvals: expiry and signature.
    pub approvals: Vec<(u64, [u8; 64])>,
}

impl AddProofs {
    /// Parses the approvals header (malformed entries are ignored: they approve nothing).
    #[must_use]
    pub fn approvals_from(header: Option<&str>) -> Vec<(u64, [u8; 64])> {
        header
            .map(|h| {
                h.split(',')
                    .take(8)
                    .filter_map(|x| hex::decode(x.trim()).ok())
                    .filter_map(|b| songbird_proto::supervision::split_approval(&b).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}
use crate::messages::Messages;

/// Request format version.
const VERSION: u8 = 1;
/// Longest `POST /v1/groups` or `/v1/groups/commit` body (the bundle limit plus framing).
pub const MAX_BODY: usize = (4 << 20) + 64;
/// Fan-outs older than this are re-driven.
const REDRIVE_AFTER_SECS: u64 = 10;
/// Re-drive ticks (5 s) between sweeps of lapsed join requests and expired links: an hour.
const PURGE_EVERY_TICKS: u64 = 720;

/// The groups feature.
pub struct Groups {
    /// Keys the fan-out idempotency keys and the rate records.
    key: hmac::Key,
    /// The delivery service's external-sender key (D-0075): every group names its public half;
    /// it signs removals of devices that left the directory.
    ds: Arc<DsSigner>,
    /// Group size and bundle limits.
    pub limits: Limits,
    /// Commits (and creations) per second per device.
    pub commits_per_sec: u32,
    /// Their burst.
    pub commit_burst: u32,
    /// Link opens, joins and join requests per second per device (D-0071).
    pub links_per_sec: u32,
    /// Their burst.
    pub link_burst: u32,
    /// Reads (state, change log, community state) per second per device (D-0074): a state read
    /// rebuilds the whole tree.
    pub reads_per_sec: u32,
    /// Their burst.
    pub read_burst: u32,
}

impl std::fmt::Debug for Groups {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Groups")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Groups {
    /// Derives the groups key and the external-sender key from the cluster secret (every `api`
    /// node derives the same, so every node signs with the key every group names); `None` if
    /// key derivation failed. `limits.external_sender` is set to that key: every group must
    /// name the service (D-0075).
    #[must_use]
    pub fn new(cluster_secret: &[u8; 32], mut limits: Limits) -> Option<Self> {
        let derive = |label: &[u8]| {
            let mut k = [0u8; 32];
            hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird.v1/groups")
                .extract(cluster_secret)
                .expand(&[label], hkdf::HKDF_SHA256)
                .ok()?
                .fill(&mut k)
                .ok()?;
            Some(k)
        };
        let k = derive(b"group-fanout")?;
        let ds = DsSigner::from_seed(&derive(b"external-sender")?)?;
        limits.external_sender = Some(ds.public_key());
        Some(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &k),
            ds: Arc::new(ds),
            limits,
            commits_per_sec: 2,
            commit_burst: 10,
            links_per_sec: 1,
            link_burst: 5,
            reads_per_sec: 5,
            read_burst: 20,
        })
    }

    /// The delivery service's external-sender public key, which every group's context names
    /// (`POST /v1/groups/ds`; apps also pin it).
    #[must_use]
    pub fn external_sender(&self) -> [u8; 32] {
        self.ds.public_key()
    }

    fn idempotency(&self, kind: &[u8], group: &[u8; 16], epoch: u64) -> [u8; 16] {
        self.idempotency_of(kind, group, &epoch.to_be_bytes())
    }

    /// A fan-out key only servers can derive: HMAC over a length-framed kind, the group and
    /// `data` (the epoch for commits and Welcomes, the message's hash for proposals).
    fn idempotency_of(&self, kind: &[u8], group: &[u8; 16], data: &[u8]) -> [u8; 16] {
        let mut ctx = hmac::Context::with_key(&self.key);
        ctx.update(&[u8::try_from(kind.len()).unwrap_or(u8::MAX)]);
        ctx.update(kind);
        ctx.update(group);
        ctx.update(data);
        let tag = ctx.sign();
        let mut out = [0u8; 16];
        out.copy_from_slice(tag.as_ref().get(..16).unwrap_or(&[0; 16]));
        out
    }

    /// The `valkey-hot` record of a device's commit budget (a keyed hash, no device ID).
    pub(crate) fn rate_record(&self, device: &DeviceId) -> String {
        let tag = hmac::sign(&self.key, device.as_bytes());
        format!(
            "gc:{}",
            hex::encode(tag.as_ref().get(..16).unwrap_or_default())
        )
    }
}

/// The directory the delivery-service core checks devices against: listed devices only.
struct Listed(Arc<songbird_send::Directory>);

impl songbird_groups::Directory for Listed {
    fn account_of(&self, device: &DeviceId) -> Option<AccountId> {
        self.0.listed_account(device)
    }
}

/// A recorded fan-out (postcard): the commit to the members, the Welcome to the added devices.
#[derive(serde::Serialize, serde::Deserialize)]
struct Plan {
    commit: Vec<u8>,
    members: Vec<[u8; 16]>,
    welcome: Option<Vec<u8>>,
    added: Vec<[u8; 16]>,
}

fn sha256(b: &[u8]) -> [u8; 32] {
    let d = digest::digest(&digest::SHA256, b);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

/// The registry's roster (D-0070); an unreadable one names nobody.
pub(crate) fn roster_of(roster: &[u8]) -> Option<songbird_send::Roster> {
    songbird_send::Roster::from_bytes(0, roster)
}

fn devices_of(roster: &[u8]) -> Vec<DeviceId> {
    roster_of(roster).map_or_else(Vec::new, |r| r.devices().copied().collect())
}

/// The policy byte for "edit the group's info" (`songbird_groups::roles::Policy::to_bytes`).
const POLICY_EDIT_INFO: usize = 2;

fn reason(e: &GroupError) -> &'static str {
    match e {
        GroupError::Roles => "roles",
        GroupError::Forbidden(Denied::Listed) => "listed",
        GroupError::Forbidden(_) => "role",
        GroupError::LogSignature => "log",
        GroupError::LinkProof(_) => "proof",
        GroupError::Wire(_) | GroupError::Malformed => "malformed",
        GroupError::Kind => "kind",
        GroupError::OtherGroup => "group",
        GroupError::StaleEpoch { .. } => "stale",
        GroupError::Ciphersuite => "ciphersuite",
        GroupError::Sender => "sender",
        GroupError::Credential => "credential",
        GroupError::Proposal => "proposal",
        GroupError::External => "external",
        GroupError::Duplicate => "duplicate",
        GroupError::TooLarge { .. } => "size",
        GroupError::Welcome => "welcome",
        GroupError::GroupInfo => "group_info",
        GroupError::Invalid => "invalid",
        GroupError::State(_) => "state",
    }
}

/// `version | group [16] | rest`.
pub(crate) fn split(body: &[u8]) -> Option<([u8; 16], &[u8])> {
    let (&v, rest) = body.split_first()?;
    if v != VERSION || rest.len() < 16 {
        return None;
    }
    let (g, rest) = rest.split_at(16);
    Some((g.try_into().ok()?, rest))
}

/// `len u32 | bytes`, twice, exactly.
fn part(b: &[u8]) -> Option<(&[u8], usize)> {
    let n = usize::try_from(u32::from_be_bytes(b.get(..4)?.try_into().ok()?)).ok()?;
    let end = n.checked_add(4)?;
    Some((b.get(4..end)?, end))
}

fn two_parts(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let (a, used) = part(b)?;
    let (c, used2) = part(b.get(used..)?)?;
    (used + used2 == b.len() && !a.is_empty() && !c.is_empty()).then_some((a, c))
}

impl Groups {
    /// `POST /v1/groups`.
    pub(crate) async fn create(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Some((info, tree)) = two_parts(rest) else {
            return Outcome::Malformed("request");
        };
        let dir = Listed(m.path.directory());
        let (info, tree) = (info.to_vec(), tree.to_vec());
        let limits = self.limits;
        let made = tokio::task::spawn_blocking(move || {
            let g = Group::create(group, &device, &info, &tree, &dir, &limits)?;
            Ok::<_, GroupError>((g.record().encode()?, g.roster_bytes(&dir)?))
        })
        .await;
        let (state, roster) = match made {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Outcome::Malformed(reason(&e)),
            Err(_) => return Outcome::Unavailable,
        };
        match registry.create_group(&group, &state, &roster).await {
            Ok(()) => Outcome::Done,
            Err(RegistryError::Exists) => Outcome::Conflict("exists"),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/groups/commit`, with the proofs a group add may need (Prompt 9).
    pub(crate) async fn commit(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
        proofs: AddProofs,
    ) -> Outcome {
        let Some((group, bundle)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        self.store_commit(registry, m, device, group, bundle, None, proofs)
            .await
    }

    /// Who may add whom (Prompt 9, D-0103): an added account that takes group adds from
    /// contacts only must be proved a contact by its access key, unless it is the committer's
    /// own account or asked to join this group. Every gated account's key goes into one XOR,
    /// compared in constant time, as for broadcasts (§5.10): the answer is the same whichever
    /// account was missing.
    async fn check_adds(
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: &DeviceId,
        group: &[u8; 16],
        joined: &[[u8; 16]],
        proofs: &AddProofs,
    ) -> Result<(), Outcome> {
        let dir = m.path.directory();
        let add_key = proofs.add_key;
        let committer = dir.listed_account(device);
        let mut combined = [0u8; 16];
        let mut gated = false;
        let mut keyless = false;
        for a in joined {
            let account = AccountId::from_bytes(*a);
            if Some(account) == committer {
                continue;
            }
            let access = dir.access(&account);
            if !access.groups_contacts {
                continue;
            }
            match registry.has_join_request(group, &account).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(_) => return Err(Outcome::Unavailable),
            }
            gated = true;
            match access.key {
                Some(k) => combined.iter_mut().zip(k).for_each(|(c, x)| *c ^= x),
                None => keyless = true,
            }
        }
        if gated {
            let proved = add_key.is_some_and(|k| {
                aws_lc_rs::constant_time::verify_slices_are_equal(&k, &combined).is_ok()
            });
            if !proved || keyless {
                return Err(Outcome::Forbidden("access"));
            }
        }
        // After the contacts check (D-0108): a supervised child takes adds from contacts only, so
        // a stranger is refused as for any such account and never learns of the supervision.
        // A supervised child whose policy needs the parent's approval for groups (D-0105): every
        // add of its devices, its own joins included, carries an approval for this group signed by
        // the parent's policy key, not expired and at most a week ahead.
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        for a in joined {
            let account = AccountId::from_bytes(*a);
            let Some(key) = dir.access(&account).approval_key else {
                continue;
            };
            let approved = proofs.approvals.iter().any(|(expires, sig)| {
                *expires > now_s
                    && *expires <= now_s.saturating_add(songbird_proto::supervision::MAX_APPROVAL_S)
                    && songbird_keys::ed25519_verify(
                        &key,
                        &songbird_proto::supervision::approval_bytes(group, &account, *expires),
                        sig,
                    )
            });
            if !approved {
                return Err(Outcome::Forbidden("approval"));
            }
        }
        Ok(())
    }

    /// Validates, stores and fans out a commit; `link` is `(handle, usable)` for a join by
    /// invite link (its use is counted with the store, D-0071).
    #[allow(
        clippy::too_many_arguments,
        reason = "the commit, how it came (link) and its proof of contact (add key)"
    )]
    async fn store_commit(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        group: [u8; 16],
        bundle: &[u8],
        link: Option<([u8; 16], bool)>,
        proofs: AddProofs,
    ) -> Outcome {
        let Ok(parts) = Bundle::parse(bundle, self.limits.max_bundle) else {
            return Outcome::Malformed("malformed");
        };
        let hash = sha256(parts.commit);
        let row = match registry.group(&group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::NotFound,
            Err(_) => return Outcome::Unavailable,
        };
        // Outsiders get the answer for no group, before any work (D-0074); a join by link is
        // the one way in for them.
        if link.is_none() && !Self::may_read(m, &row.roster, &device) {
            return Outcome::NotFound;
        }
        // A spent link is refused before any work, unless this is its last join again.
        if matches!(link, Some((_, false))) && row.last.as_ref() != hash {
            return Outcome::NotFound;
        }
        let Ok(record) = Record::decode(&row.state) else {
            return Outcome::Unavailable;
        };
        let dir = Listed(m.path.directory());
        let limits = self.limits;
        let bundle_owned = bundle.to_vec();
        let by_link = link.map(|(handle, _)| handle);
        let validated = tokio::task::spawn_blocking(move || {
            validate(record, &device, &bundle_owned, &dir, &limits, by_link)
        })
        .await;
        let Validated {
            state,
            roster,
            roster_bytes,
            applied,
            dead,
        } = match validated {
            Ok(Ok(v)) => v,
            // The same commit again (an answer that was lost): it won; finish its fan-out.
            Ok(Err(GroupError::StaleEpoch { current })) if row.last.as_ref() == hash => {
                return self
                    .finish(registry, m, &group, current.saturating_sub(1), current)
                    .await;
            }
            Ok(Err(GroupError::StaleEpoch { current })) => {
                return Outcome::Stale(current);
            }
            // A server removal of a device this node still lists (D-0075). The service proposes
            // only removals the registry confirmed, and a device never comes back, so this
            // node's directory snapshot is behind the registry (it follows within seconds):
            // retry, rather than refuse a commit every up-to-date member accepts.
            Ok(Err(GroupError::Forbidden(Denied::Listed))) => {
                tracing::warn!("a commit removes a device this node still lists; retry");
                return Outcome::Unavailable;
            }
            // The roles rule refused it (D-0070).
            Ok(Err(GroupError::Forbidden(_))) => return Outcome::Forbidden("role"),
            // A join without a valid proof that its device held the link (D-0076).
            Ok(Err(GroupError::LinkProof(_))) => return Outcome::Forbidden("proof"),
            Ok(Err(e)) => return Outcome::Malformed(reason(&e)),
            Err(_) => return Outcome::Unavailable,
        };

        // Every added leaf is a KeyPackage its device published (D-0074): a member cannot add
        // a leaf it made up under another device's ID and speak as that device.
        match registry.key_packages_published(&applied.key_packages).await {
            Ok(true) => {}
            Ok(false) => return Outcome::Forbidden("key_package"),
            Err(_) => return Outcome::Unavailable,
        }
        let Ok(plan_bytes) =
            postcard::to_stdvec(&plan_of(&row.roster, &roster, device, &parts, &applied))
        else {
            return Outcome::Unavailable;
        };
        let joined = joined_accounts(m, &applied.added);
        if let Err(o) = Self::check_adds(registry, m, &device, &group, &joined, &proofs).await {
            return o;
        }
        let from = applied.from_epoch;
        match registry
            .commit_group(
                &group,
                from,
                &songbird_accounts::groups::GroupCommit {
                    state: &state,
                    roster: &roster_bytes,
                    last: &hash,
                    fanout: &plan_bytes,
                    log: applied.log.as_deref(),
                    link: link.as_ref().map(|(h, _)| h),
                    joined: &joined,
                    prune: &dead,
                },
            )
            .await
        {
            Ok(()) => {}
            // The link was spent, revoked or expired meanwhile.
            Err(RegistryError::GroupLink) => return Outcome::NotFound,
            // Another node stored a commit from this epoch first.
            Err(RegistryError::StaleEpoch) => {
                let current = registry
                    .group(&group)
                    .await
                    .ok()
                    .flatten()
                    .map_or(from + 1, |r| r.epoch);
                return Outcome::Stale(current);
            }
            Err(_) => return Outcome::Unavailable,
        }
        self.finish(registry, m, &group, from, from + 1).await
    }

    /// Delivers the recorded fan-out of the commit from `from` (if still pending), forgets it,
    /// and answers with the group's epoch.
    async fn finish(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        group: &[u8; 16],
        from: u64,
        epoch: u64,
    ) -> Outcome {
        match self.deliver(registry, m, group, from).await {
            Ok(()) => Outcome::Json(serde_json::json!({ "epoch": epoch })),
            Err(()) => Outcome::Unavailable,
        }
    }

    /// Stores a recorded fan-out in every recipient's mailbox (idempotent), then deletes it.
    async fn deliver(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        group: &[u8; 16],
        from: u64,
    ) -> Result<(), ()> {
        let Some(bytes) = registry.group_fanout(group, from).await.map_err(|_| ())? else {
            return Ok(());
        };
        let plan: Plan = postcard::from_bytes(&bytes).map_err(|_| ())?;
        let members: Vec<DeviceId> = plan
            .members
            .iter()
            .map(|d| DeviceId::from_bytes(*d))
            .collect();
        m.path
            .shared(
                &members,
                Bytes::from(plan.commit),
                self.idempotency(b"commit", group, from),
            )
            .await
            .or_else(|e| stored_before(&e))?;
        if let Some(w) = plan.welcome {
            let added: Vec<DeviceId> = plan
                .added
                .iter()
                .map(|d| DeviceId::from_bytes(*d))
                .collect();
            m.path
                .shared(
                    &added,
                    Bytes::from(w),
                    self.idempotency(b"welcome", group, from),
                )
                .await
                .or_else(|e| stored_before(&e))?;
        }
        registry
            .group_fanout_done(group, from)
            .await
            .map_err(|_| ())
    }

    /// `POST /v1/groups/state`.
    pub(crate) async fn state(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        if !rest.is_empty() {
            return Outcome::Malformed("request");
        }
        let row = match registry.group(&group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::NotFound,
            Err(_) => return Outcome::Unavailable,
        };
        if !Self::may_read(m, &row.roster, &device) {
            return Outcome::NotFound;
        }
        let dir = m.path.directory();
        let accounts: Vec<(DeviceId, AccountId)> = devices_of(&row.roster)
            .into_iter()
            .filter_map(|d| dir.listed_account(&d).map(|a| (d, a)))
            .collect();
        state_blob(&row, &[], Some(&accounts)).await
    }

    /// `POST /v1/groups/ds` (D-0075): `version u8 (1)`. 200 with the delivery service's
    /// external-sender public key (32 bytes): every group's context must name it, with the
    /// basic credential `songbird.v1/ds`, as its only external sender.
    pub(crate) fn ds_key(&self, body: &[u8]) -> Outcome {
        if body != [VERSION] {
            return Outcome::Malformed("request");
        }
        Outcome::Blob(Bytes::copy_from_slice(&self.external_sender()))
    }

    /// Who may read a group's state and change log: a device in it, or a listed device of an
    /// account in it (to rejoin). Everyone else gets the answer for no group.
    pub(crate) fn may_read(m: &Messages, roster: &[u8], device: &DeviceId) -> bool {
        let dir = m.path.directory();
        let roster = devices_of(roster);
        roster.binary_search(device).is_ok()
            || dir
                .listed_account(device)
                .is_some_and(|a| roster.iter().any(|d| dir.listed_account(d) == Some(a)))
    }

    /// `POST /v1/groups/propose` (D-0070): a member's proposal to leave (`version | group |
    /// MLSMessage`). Queued in the group's state (compare-and-set on epoch and revision), then
    /// stored in every other member's mailbox; 204 once stored. A retried proposal is queued
    /// again harmlessly (same reference) and stored once (same key).
    pub(crate) async fn propose(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, msg)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let row = match registry.group(&group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::NotFound,
            Err(_) => return Outcome::Unavailable,
        };
        // Outsiders get the answer for no group, before any work (D-0074).
        if !Self::may_read(m, &row.roster, &device) {
            return Outcome::NotFound;
        }
        let Ok(record) = Record::decode(&row.state) else {
            return Outcome::Unavailable;
        };
        let msg_owned = msg.to_vec();
        let queued = tokio::task::spawn_blocking(move || {
            let (g, hash) = Group::load(record)?.apply_proposal(&device, &msg_owned)?;
            Ok::<_, GroupError>((g.record().encode()?, hash))
        })
        .await;
        let (state, hash) = match queued {
            Ok(Ok(v)) => v,
            Ok(Err(GroupError::StaleEpoch { current })) => return Outcome::Stale(current),
            Ok(Err(e)) => return Outcome::Malformed(reason(&e)),
            Err(_) => return Outcome::Unavailable,
        };
        match registry
            .propose_group(&group, row.epoch, row.rev, &state)
            .await
        {
            Ok(()) => {}
            // A commit came first: propose again from its epoch. Only another proposal came
            // first: the same request can simply be retried.
            Err(RegistryError::StaleEpoch) => {
                return match registry.group(&group).await {
                    Ok(Some(now)) if now.epoch != row.epoch => Outcome::Stale(now.epoch),
                    _ => Outcome::Unavailable,
                };
            }
            Err(_) => return Outcome::Unavailable,
        }
        let members: Vec<DeviceId> = devices_of(&row.roster)
            .into_iter()
            .filter(|d| *d != device)
            .collect();
        // One key per proposal: two members leaving in one epoch never share it.
        let key = self.idempotency_of(b"proposal", &group, &hash);
        match m
            .path
            .shared(&members, Bytes::copy_from_slice(msg), key)
            .await
        {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/groups/meta` (D-0070): replaces the encrypted metadata (`version | group |
    /// expected u64 | blob`, at most 16 KiB). Allowed to devices whose role reaches the policy's
    /// "edit info" role ("only admins can edit group info"). 200 `{"version": n}`; 409
    /// `{"error": "stale", "epoch": current}` (here the metadata's version); 403 `role`.
    pub(crate) async fn meta(
        &self,
        registry: &songbird_accounts::Registry,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Some((expected, blob)) = rest.split_at_checked(8) else {
            return Outcome::Malformed("request");
        };
        let expected = u64::from_be_bytes(expected.try_into().unwrap_or([0; 8]));
        if blob.is_empty() || blob.len() > songbird_accounts::groups::MAX_META {
            return Outcome::Malformed("size");
        }
        let row = match registry.group(&group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::NotFound,
            Err(_) => return Outcome::Unavailable,
        };
        let Some(roster) = roster_of(&row.roster) else {
            return Outcome::Unavailable;
        };
        let Some(role) = roster.role(&device) else {
            return Outcome::NotFound;
        };
        if role < roster.policy[POLICY_EDIT_INFO] {
            return Outcome::Forbidden("role");
        }
        match registry.put_group_meta(&group, expected, blob).await {
            Ok(v) => Outcome::Json(serde_json::json!({ "version": v })),
            Err(RegistryError::StaleEpoch) => Outcome::Stale(row.meta_v),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/groups/log` (D-0070): change-log entries after an epoch (`version | group |
    /// after u64`, `u64::MAX` for all), at most 200: `n u16 | n × (epoch u64 | len u16 |
    /// entry)`. For the same devices as the state.
    pub(crate) async fn log(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Ok(after) = <[u8; 8]>::try_from(rest) else {
            return Outcome::Malformed("request");
        };
        let after = u64::from_be_bytes(after);
        let row = match registry.group(&group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Outcome::NotFound,
            Err(_) => return Outcome::Unavailable,
        };
        if !Self::may_read(m, &row.roster, &device) {
            return Outcome::NotFound;
        }
        let after = (after != u64::MAX).then_some(after);
        let Ok(entries) = registry.group_log(&group, after, 200).await else {
            return Outcome::Unavailable;
        };
        let mut out = Vec::new();
        out.extend_from_slice(
            &u16::try_from(entries.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        for (e, entry) in &entries {
            out.extend_from_slice(&e.to_be_bytes());
            out.extend_from_slice(&u16::try_from(entry.len()).unwrap_or(u16::MAX).to_be_bytes());
            out.extend_from_slice(entry);
        }
        Outcome::Blob(Bytes::from(out))
    }
}

/// Finishes fan-outs a crash interrupted (every few seconds, on every `api` node; delivery is
/// idempotent, so two nodes re-driving one fan-out store it once).
pub async fn redrive(
    registry: Arc<songbird_accounts::Registry>,
    app: Arc<crate::App>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    let mut ticks: u64 = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.changed() => return,
        }
        let Some(m) = app.messages.as_ref() else {
            return;
        };
        let Some(g) = m.groups.as_ref() else { return };
        g.redrive_once(&registry, m, REDRIVE_AFTER_SECS).await;
        // Groups holding devices that left the directory (D-0075).
        g.prune_once(&registry, m).await;
        // Hourly: lapsed join requests and expired links (D-0071).
        if ticks.is_multiple_of(PURGE_EVERY_TICKS) && registry.purge_group_invites().await.is_err()
        {
            tracing::warn!("lapsed group invites could not be purged yet");
        }
        ticks = ticks.wrapping_add(1);
    }
}

impl Groups {
    /// One re-drive pass over fan-outs older than `age_secs`; returns how many it finished.
    pub async fn redrive_once(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        age_secs: u64,
    ) -> usize {
        let Ok(stale) = registry.stale_group_fanouts(age_secs, 64).await else {
            return 0;
        };
        let mut done = 0;
        for (group, from, _) in stale {
            if self.deliver(registry, m, &group, from).await.is_ok() {
                done += 1;
            } else {
                tracing::warn!("a group fan-out could not be re-driven yet");
            }
        }
        done
    }
}

/// What a validated commit leaves to store.
struct Validated {
    state: Vec<u8>,
    roster: Vec<DeviceId>,
    roster_bytes: Vec<u8>,
    applied: songbird_groups::Applied,
    /// Devices still in the group that left the directory: proposals do not outlive an
    /// epoch, so their removal is proposed again at the new one (D-0075).
    dead: Vec<[u8; 16]>,
}

/// Validates a commit (or, with `link`, a join by that invite link) against the stored record
/// (CPU work: run it off the async threads).
fn validate(
    record: Record,
    device: &DeviceId,
    bundle: &[u8],
    dir: &Listed,
    limits: &Limits,
    link: Option<[u8; 16]>,
) -> Result<Validated, GroupError> {
    let g = Group::load(record)?;
    // Devices a queued server removal targets: confirmed gone in the registry when proposed.
    let proposed = g.pending_server_removals()?;
    let (g, applied) = match link {
        Some(handle) => g.apply_join(device, bundle, dir, limits, &handle)?,
        None => g.apply_commit(device, bundle, dir, limits)?,
    };
    let roster: Vec<DeviceId> = g.roster().devices().collect();
    let dead = roster
        .iter()
        .filter(|d| {
            songbird_groups::Directory::account_of(dir, d).is_none() || proposed.contains(d)
        })
        .map(|d| *d.as_bytes())
        .collect();
    Ok(Validated {
        state: g.record().encode()?,
        roster_bytes: g.roster_bytes(dir)?,
        roster,
        applied,
        dead,
    })
}

/// The directory a prune decides with: this node's snapshot, overruled by the registry for
/// every device the prune looked up (D-0075). A stale snapshot can neither propose removing a
/// device that is still listed nor hide one that left.
struct Confirmed {
    snapshot: Arc<songbird_send::Directory>,
    looked_up: std::collections::HashMap<DeviceId, Option<AccountId>>,
}

impl songbird_groups::Directory for Confirmed {
    fn account_of(&self, device: &DeviceId) -> Option<AccountId> {
        self.looked_up
            .get(device)
            .copied()
            .unwrap_or_else(|| self.snapshot.listed_account(device))
    }
}

/// Most groups one prune pass handles.
const PRUNE_BATCH: u64 = 64;

/// Server-proposed removals (D-0075).
impl Groups {
    /// One pass over the groups marked for pruning (a device they hold left the directory);
    /// returns how many it finished. A group that could not be finished keeps its mark and is
    /// tried again next pass.
    pub async fn prune_once(&self, registry: &songbird_accounts::Registry, m: &Messages) -> usize {
        let Ok(marks) = registry.groups_to_prune(PRUNE_BATCH).await else {
            return 0;
        };
        let mut done = 0;
        for mark in marks {
            if self.prune(registry, m, &mark).await.is_ok() {
                done += 1;
            } else {
                tracing::warn!("a group's departed devices could not be proposed for removal yet");
                // To the back of the queue: a group that keeps failing holds up no other.
                let _ = registry.group_prune_later(&mark.group, mark.n).await;
            }
        }
        done
    }

    /// Proposes, as the group's external sender, removing every device it holds that left the
    /// directory; stores the proposals with the group's state (compare-and-set on epoch and
    /// revision), fans them out to the members, then forgets the mark.
    async fn prune(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        mark: &songbird_accounts::groups::PruneMark,
    ) -> Result<(), ()> {
        let group = mark.group;
        let Some(row) = registry.group(&group).await.map_err(|_| ())? else {
            return registry.group_pruned(&group, mark.n).await.map_err(|_| ());
        };
        let record = Record::decode(&row.state).map_err(|_| ())?;
        // Who left: the devices the mark names and those this node's snapshot no longer
        // lists, each confirmed in the registry (members refuse a removal of a listed device).
        let snapshot = m.path.directory();
        let roster = devices_of(&row.roster);
        let mut candidates: Vec<DeviceId> = roster
            .iter()
            .filter(|d| snapshot.listed_account(d).is_none() || mark.gone.contains(d))
            .copied()
            .collect();
        candidates.sort_unstable();
        candidates.dedup();
        let mut looked_up = std::collections::HashMap::with_capacity(candidates.len());
        for d in candidates {
            let listed = registry
                .device(&d)
                .await
                .map_err(|_| ())?
                .filter(|r| !r.disabled && !r.pending)
                .map(|r| r.account);
            looked_up.insert(d, listed);
        }
        if looked_up.values().all(Option::is_some) {
            return registry.group_pruned(&group, mark.n).await.map_err(|_| ());
        }
        let dir = Confirmed {
            snapshot,
            looked_up,
        };
        let ds = Arc::clone(&self.ds);
        let proposed = tokio::task::spawn_blocking(move || {
            let (g, p) = Group::load(record)?.propose_removals(&dir, &ds)?;
            let members: Vec<DeviceId> = g
                .roster()
                .devices()
                .filter(|d| songbird_groups::Directory::account_of(&dir, d).is_some())
                .collect();
            Ok::<_, GroupError>((g.record().encode()?, p, members))
        })
        .await;
        let Ok(Ok((state, p, members))) = proposed else {
            return Err(());
        };
        if p.queued > 0 {
            // A commit or another proposal came first: the next pass proposes at its epoch.
            registry
                .propose_group(&group, row.epoch, row.rev, &state)
                .await
                .map_err(|_| ())?;
        }
        for msg in p.messages {
            let key = self.idempotency_of(b"ds-remove", &group, &sha256(&msg));
            m.path
                .shared(&members, Bytes::from(msg), key)
                .await
                .or_else(|e| stored_before(&e))?;
        }
        registry.group_pruned(&group, mark.n).await.map_err(|_| ())
    }
}

/// `epoch u64 | info_len u32 | GroupInfo | tree_len u32 | ratchet tree | meta_v u64 | meta_len
/// u32 | meta`, after `prefix`.
async fn state_blob(
    row: &songbird_accounts::groups::GroupRow,
    prefix: &[u8],
    accounts: Option<&[(DeviceId, AccountId)]>,
) -> Outcome {
    let Ok(record) = Record::decode(&row.state) else {
        return Outcome::Unavailable;
    };
    let (meta_v, meta, prefix) = (row.meta_v, row.meta.clone(), prefix.to_vec());
    let accounts: Option<Vec<u8>> = accounts.map(|list| {
        let mut b = Vec::with_capacity(2 + list.len() * 32);
        b.extend_from_slice(&u16::try_from(list.len()).unwrap_or(u16::MAX).to_be_bytes());
        for (d, a) in list.iter().take(usize::from(u16::MAX)) {
            b.extend_from_slice(d.as_bytes());
            b.extend_from_slice(a.as_bytes());
        }
        b
    });
    let built = tokio::task::spawn_blocking(move || {
        let g = Group::load(record)?;
        let tree = g.ratchet_tree()?;
        let info = g.group_info().to_vec();
        let mut out = Vec::with_capacity(prefix.len() + 32 + info.len() + tree.len() + meta.len());
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&g.epoch().to_be_bytes());
        for part in [&info[..], &tree[..]] {
            out.extend_from_slice(&u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
            out.extend_from_slice(part);
        }
        put_meta(&mut out, meta_v, &meta);
        if let Some(a) = &accounts {
            out.extend_from_slice(a);
        }
        Ok::<_, GroupError>(out)
    })
    .await;
    match built {
        Ok(Ok(b)) => Outcome::Blob(Bytes::from(b)),
        _ => Outcome::Unavailable,
    }
}

pub(crate) fn put_meta(out: &mut Vec<u8>, meta_v: u64, meta: &[u8]) {
    out.extend_from_slice(&meta_v.to_be_bytes());
    out.extend_from_slice(&u32::try_from(meta.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(meta);
}

/// Policy bytes (`songbird_groups::roles::Policy::to_bytes`).
const POLICY_ADD: usize = 1;
const POLICY_LINKS: usize = 3;
const POLICY_FLAGS: usize = 6;
/// The longest a link may live: a year.
const MAX_LINK_SECS: u64 = 365 * 24 * 3600;

/// `open`'s first byte: the person may join now, or must ask.
const OPEN_JOIN: u8 = 0;
const OPEN_ASK: u8 = 1;

/// A checked invite link: its group, whether a join must be approved, whether it is usable.
struct Opened {
    group: [u8; 16],
    approval: bool,
    usable: bool,
    row: songbird_accounts::groups::GroupRow,
}

/// Invite links and join requests (Prompt 5, M3; D-0071).
///
/// A link is `handle [16]` and a client secret. The server holds `SHA-256(auth)`, where `auth`
/// is derived from the secret by the client (the rest of the secret, the key to the group's
/// metadata, never reaches the server). Every wrong, unknown, spent, revoked or expired link
/// gets one answer: 404.
impl Groups {
    /// The `valkey-hot` record of a device's read budget (state, change log, community state).
    pub(crate) fn read_record(&self, device: &DeviceId) -> String {
        let mut ctx = hmac::Context::with_key(&self.key);
        ctx.update(b"read");
        ctx.update(device.as_bytes());
        let tag = ctx.sign();
        format!(
            "gr:{}",
            hex::encode(tag.as_ref().get(..16).unwrap_or_default())
        )
    }

    /// The `valkey-hot` record of an account's link budget (opening, joining, asking).
    pub(crate) fn link_record(&self, account: &AccountId) -> String {
        let mut ctx = hmac::Context::with_key(&self.key);
        ctx.update(b"link");
        ctx.update(account.as_bytes());
        let tag = ctx.sign();
        format!(
            "gl:{}",
            hex::encode(tag.as_ref().get(..16).unwrap_or_default())
        )
    }

    /// The device's role, if at least policy byte `idx`'s.
    async fn require_role(
        registry: &songbird_accounts::Registry,
        group: &[u8; 16],
        device: &DeviceId,
        idx: usize,
    ) -> Result<songbird_send::Roster, Outcome> {
        // The roster only: never the (multi-MiB) state for a role check (D-0074).
        let (_, roster) = match registry.group_roster(group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Err(Outcome::NotFound),
            Err(_) => return Err(Outcome::Unavailable),
        };
        let roster = roster_of(&roster).ok_or(Outcome::Unavailable)?;
        let role = roster.role(device).ok_or(Outcome::NotFound)?;
        if roster.policy.get(idx).is_none_or(|need| role < *need) {
            return Err(Outcome::Forbidden("role"));
        }
        Ok(roster)
    }

    /// `POST /v1/groups/links`: `version | group | handle [16] | SHA-256(auth) [32] | ttl u64
    /// (seconds, 0: none) | max_uses u32 (0: any) | approval u8`. For roles reaching the
    /// policy's "links" role. 204; 409 `exists` (draw another handle) or `full` (16 links).
    pub(crate) async fn create_link(
        &self,
        registry: &songbird_accounts::Registry,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Ok(rest) = <&[u8; 61]>::try_from(rest) else {
            return Outcome::Malformed("request");
        };
        let (handle, rest) = rest.split_at(16);
        let (hash, rest) = rest.split_at(32);
        let (ttl, rest) = rest.split_at(8);
        let (max, approval) = rest.split_at(4);
        let ttl = u64::from_be_bytes(ttl.try_into().unwrap_or_default());
        let max = u32::from_be_bytes(max.try_into().unwrap_or_default());
        let approval = match approval {
            [0] => false,
            [1] => true,
            _ => return Outcome::Malformed("request"),
        };
        if ttl > MAX_LINK_SECS {
            return Outcome::Malformed("ttl");
        }
        if let Err(o) = Self::require_role(registry, &group, &device, POLICY_LINKS).await {
            return o;
        }
        let (Ok(handle), Ok(hash)) = (<[u8; 16]>::try_from(handle), <[u8; 32]>::try_from(hash))
        else {
            return Outcome::Malformed("request");
        };
        match registry
            .create_group_link(
                &handle,
                &group,
                &hash,
                (ttl > 0).then_some(ttl),
                max,
                approval,
            )
            .await
        {
            Ok(()) => Outcome::Done,
            Err(RegistryError::Exists) => Outcome::Conflict("exists"),
            Err(RegistryError::Full) => Outcome::Conflict("full"),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/groups/links/revoke`: `version | group | handle [16]`. Same role. 204.
    pub(crate) async fn revoke_link(
        &self,
        registry: &songbird_accounts::Registry,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Ok(handle) = <[u8; 16]>::try_from(rest) else {
            return Outcome::Malformed("request");
        };
        if let Err(o) = Self::require_role(registry, &group, &device, POLICY_LINKS).await {
            return o;
        }
        match registry.revoke_group_link(&group, &handle).await {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }

    /// Checks `handle` and `auth` (constant time), then loads the group.
    async fn open_link(
        registry: &songbird_accounts::Registry,
        handle: &[u8; 16],
        auth: &[u8],
    ) -> Result<Opened, Outcome> {
        if auth.len() != 32 {
            return Err(Outcome::Malformed("request"));
        }
        let presented = sha256(auth);
        let Ok(link) = registry.group_link(handle).await else {
            return Err(Outcome::Unavailable);
        };
        // Unknown and wrong get the same comparison and the same answer.
        let stored = link.as_ref().map_or([0u8; 32], |l| l.secret_hash);
        let matches =
            aws_lc_rs::constant_time::verify_slices_are_equal(&presented, &stored).is_ok();
        let Some(link) = link.filter(|_| matches) else {
            return Err(Outcome::NotFound);
        };
        let row = match registry.group(&link.group).await {
            Ok(Some(r)) => r,
            Ok(None) => return Err(Outcome::NotFound),
            Err(_) => return Err(Outcome::Unavailable),
        };
        let policy_approval = roster_of(&row.roster)
            .is_some_and(|r| r.policy[POLICY_FLAGS] & songbird_groups::roles::FLAG_APPROVAL != 0);
        Ok(Opened {
            group: link.group,
            approval: link.approval || policy_approval,
            usable: link.usable,
            row,
        })
    }

    /// `POST /v1/groups/links/open`: `version | handle [16] | auth [32]`. 200 with
    /// `0 | group [16] | state` (as `/v1/groups/state`) if the device may join now, or `1 |
    /// meta_v u64 | meta_len u32 | meta` if it must ask (the encrypted name and picture only,
    /// no members); 404 for every link that cannot be used.
    pub(crate) async fn open(
        &self,
        registry: &songbird_accounts::Registry,
        body: Bytes,
    ) -> Outcome {
        let Some((handle, auth)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let o = match Self::open_link(registry, &handle, auth).await {
            Ok(o) => o,
            Err(e) => return e,
        };
        if !o.usable {
            return Outcome::NotFound;
        }
        if o.approval {
            let mut out = vec![OPEN_ASK];
            put_meta(&mut out, o.row.meta_v, &o.row.meta);
            return Outcome::Blob(Bytes::from(out));
        }
        let mut prefix = vec![OPEN_JOIN];
        prefix.extend_from_slice(&o.group);
        // Not a member yet: no accounts.
        state_blob(&o.row, &prefix, None).await
    }

    /// `POST /v1/groups/join`: `version | handle [16] | auth [32] | bundle` (an external commit
    /// adding the device, with the default role). As `/v1/groups/commit`; 404 for a link that
    /// cannot be used; 403 `approval` if joins by this link must be approved.
    pub(crate) async fn join(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
        proofs: AddProofs,
    ) -> Outcome {
        let Some((handle, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Some((auth, bundle)) = rest.split_at_checked(32) else {
            return Outcome::Malformed("request");
        };
        let o = match Self::open_link(registry, &handle, auth).await {
            Ok(o) => o,
            Err(e) => return e,
        };
        if o.approval {
            return Outcome::Forbidden("approval");
        }
        self.store_commit(
            registry,
            m,
            device,
            o.group,
            bundle,
            Some((handle, o.usable)),
            proofs,
        )
        .await
    }

    /// `POST /v1/groups/requests`: `version | handle [16] | auth [32]`: the device's account
    /// asks to join (a link that needs approval). 204, also when asked before; 409 `open` (join
    /// directly), `member`, or `full` (256 pending).
    pub(crate) async fn request(
        &self,
        registry: &songbird_accounts::Registry,
        m: &Messages,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((handle, auth)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let o = match Self::open_link(registry, &handle, auth).await {
            Ok(o) => o,
            Err(e) => return e,
        };
        if !o.usable {
            return Outcome::NotFound;
        }
        if !o.approval {
            return Outcome::Conflict("open");
        }
        let dir = m.path.directory();
        let Some(account) = dir.listed_account(&device) else {
            return Outcome::Forbidden("pending");
        };
        if devices_of(&o.row.roster)
            .iter()
            .any(|d| dir.listed_account(d) == Some(account))
        {
            return Outcome::Conflict("member");
        }
        match registry
            .add_group_request(&o.group, account.as_bytes())
            .await
        {
            Ok(()) => Outcome::Done,
            Err(RegistryError::Full) => Outcome::Conflict("full"),
            Err(_) => Outcome::Unavailable,
        }
    }

    /// `POST /v1/groups/requests/list`: `version | group`. For roles reaching the policy's
    /// "add" role: `n u16 | n × (account [16] | age_secs u64)`, oldest first. Approving is
    /// adding the account's devices in a commit (which answers the request).
    pub(crate) async fn requests(
        &self,
        registry: &songbird_accounts::Registry,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        if !rest.is_empty() {
            return Outcome::Malformed("request");
        }
        if let Err(o) = Self::require_role(registry, &group, &device, POLICY_ADD).await {
            return o;
        }
        let Ok(list) = registry.group_requests(&group).await else {
            return Outcome::Unavailable;
        };
        let mut out = Vec::with_capacity(2 + list.len() * 24);
        out.extend_from_slice(&u16::try_from(list.len()).unwrap_or(u16::MAX).to_be_bytes());
        for (a, age) in &list {
            out.extend_from_slice(a);
            out.extend_from_slice(&age.to_be_bytes());
        }
        Outcome::Blob(Bytes::from(out))
    }

    /// `POST /v1/groups/requests/decline`: `version | group | account [16]`. Same role. 204.
    pub(crate) async fn decline(
        &self,
        registry: &songbird_accounts::Registry,
        device: DeviceId,
        body: Bytes,
    ) -> Outcome {
        let Some((group, rest)) = split(&body) else {
            return Outcome::Malformed("request");
        };
        let Ok(account) = <[u8; 16]>::try_from(rest) else {
            return Outcome::Malformed("request");
        };
        if let Err(o) = Self::require_role(registry, &group, &device, POLICY_ADD).await {
            return o;
        }
        match registry.delete_group_request(&group, &account).await {
            Ok(()) => Outcome::Done,
            Err(_) => Outcome::Unavailable,
        }
    }
}

/// A fan-out store that conflicts with an earlier store under the same key: that store
/// completed (stores are atomic), and only the recipients differ because a device left the
/// directory since. It counts as delivered, so the fan-out can finish (D-0074).
fn stored_before(e: &songbird_proto::frame::SendStatus) -> Result<(), ()> {
    match e {
        songbird_proto::frame::SendStatus::Rejected {
            reason: songbird_proto::frame::RejectReason::IdempotencyConflict,
            ..
        } => Ok(()),
        _ => Err(()),
    }
}

/// Who hears what: the commit goes to every device in the group before or after it (removed
/// devices learn they were removed) except the committer and the added devices, which get the
/// Welcome instead.
fn plan_of(
    old_roster: &[u8],
    roster: &[DeviceId],
    committer: DeviceId,
    parts: &Bundle<'_>,
    applied: &songbird_groups::Applied,
) -> Plan {
    let old = devices_of(old_roster);
    let mut members: Vec<DeviceId> = old.iter().chain(roster.iter()).copied().collect();
    members.sort_unstable();
    members.dedup();
    members.retain(|d| *d != committer && !applied.added.contains(d));
    Plan {
        commit: parts.commit.to_vec(),
        members: members.iter().map(|d| *d.as_bytes()).collect(),
        welcome: parts.welcome.map(<[u8]>::to_vec),
        added: applied.added.iter().map(|d| *d.as_bytes()).collect(),
    }
}

/// Accounts a commit brings in: their pending join requests are answered (D-0071).
fn joined_accounts(m: &Messages, added: &[DeviceId]) -> Vec<[u8; 16]> {
    let directory = m.path.directory();
    let mut joined: Vec<[u8; 16]> = added
        .iter()
        .filter_map(|d| directory.listed_account(d))
        .map(|a| *a.as_bytes())
        .collect();
    joined.sort_unstable();
    joined.dedup();
    joined
}
