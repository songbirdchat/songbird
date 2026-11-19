//! The MLS delivery service's core (Prompt 5, M1): pure, no I/O.
//!
//! OpenMLS's `PublicGroup` checks every handshake as RFC 9420 requires (signatures, tree and
//! path, proposals, transcript). Songbird adds, before anything is merged:
//!
//! - **the sender binding**: a member commit is signed by the leaf whose credential is the
//!   authenticated device; an external commit's new leaf is that device;
//! - **credentials** are basic, and their identity is a 16-byte device ID the directory knows;
//!   no update may change a leaf's identity;
//! - **proposals**: add, remove, update, self-remove and (in an external commit) external init
//!   only; pre-shared keys, re-init, group-context changes and custom proposals are refused
//!   until a milestone defines them (roles, M2);
//! - **external commits** rejoin a lost device only: the joining device's account already has a
//!   device in the group, and the commit may remove only that account's own leaves;
//! - **size**: at most `Limits::max_members` leaves (`max_channel_members` in a channel);
//! - **Welcomes**: exactly one encrypted secret per added KeyPackage, none for anyone else;
//! - **the `GroupInfo`** uploaded with the commit is the new epoch's, signed by the committer.
//!
//! A [`Group`] is consumed by [`Group::apply_commit`] and returned only on success, so state
//! that failed half-way can never be used: the caller reloads its stored record.

use std::collections::BTreeSet;

use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
use openmls::prelude::*;
use songbird_types::{AccountId, DeviceId};

use crate::crypto::{GROUP_CIPHERSUITES, VerifyOnlyCrypto};
use crate::roles::{self, Change, Denied, ROLES_EXTENSION, Role, Roles};
use crate::state::{GroupState, StateError, StateParts};
use crate::wire::{self, Bundle, GROUP_ID_LEN, WireError};

const CRYPTO: VerifyOnlyCrypto = VerifyOnlyCrypto;

/// The registry's roster format with roles (D-0070).
pub const ROSTER_V2: u8 = 2;
/// Largest proposal message accepted (a `SelfRemove` is a few hundred bytes).
pub const MAX_PROPOSAL: usize = 4096;

/// What the delivery service needs to know about devices.
pub trait Directory {
    /// The account a device belongs to, if the device exists and is listed.
    fn account_of(&self, device: &DeviceId) -> Option<AccountId>;
}

/// Limits on groups (`[api.groups]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most leaves (devices) in a group; Prompt 5 starts at 1,024.
    pub max_members: usize,
    /// Most leaves in a private channel (D-0072: a full-path X-Wing commit to 2,048 leaves of a
    /// fresh tree is 2.3 MiB, inside `max_bundle`; at 5,000 it is 5.6 MiB, outside).
    pub max_channel_members: usize,
    /// Largest commit bundle accepted, checked before parsing.
    pub max_bundle: usize,
    /// The delivery service's external-sender key (D-0075). When set, every group's context
    /// must name exactly this key (with [`crate::external::DS_IDENTITY`]) as its only external
    /// sender, from creation and after every commit, so the service can propose removing
    /// devices that left the directory. When unset (tests of other rules), a group may name no
    /// external sender at all. `api` always sets it.
    pub external_sender: Option<[u8; 32]>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_members: 1_024,
            max_channel_members: 2_048,
            max_bundle: 4 << 20,
            external_sender: None,
        }
    }
}

impl Limits {
    /// The size limit for a group with this policy.
    #[must_use]
    pub const fn cap(&self, policy: &roles::Policy) -> usize {
        if policy.is_channel() {
            self.max_channel_members
        } else {
            self.max_members
        }
    }
}

/// Why a handshake was refused. Nothing was merged.
#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error("malformed: {0}")]
    Wire(#[from] WireError),
    #[error("malformed MLS message")]
    Malformed,
    #[error("not a handshake this endpoint takes")]
    Kind,
    #[error("another group")]
    OtherGroup,
    #[error("stale epoch; the group is at {current}")]
    StaleEpoch { current: u64 },
    #[error("unsupported ciphersuite")]
    Ciphersuite,
    #[error("the sender is not the authenticated device")]
    Sender,
    #[error("a credential is not a known device")]
    Credential,
    #[error("a proposal Songbird does not allow")]
    Proposal,
    #[error("an external commit that is not a rejoin")]
    External,
    #[error("the device is already a member")]
    Duplicate,
    #[error("the group would exceed {max} members")]
    TooLarge { max: usize },
    #[error("the Welcome does not match the added devices")]
    Welcome,
    #[error("the GroupInfo is not the new epoch's, signed by the committer")]
    GroupInfo,
    #[error("rejected by MLS validation")]
    Invalid,
    #[error("the roles extension is missing or malformed")]
    Roles,
    #[error("not allowed by the group's roles: {0}")]
    Forbidden(Denied),
    #[error("the change-log entry is not signed by the committer")]
    LogSignature,
    #[error("the join carries no valid link proof: {0}")]
    LinkProof(crate::links::ProofError),
    #[error("state: {0}")]
    State(#[from] StateError),
}

/// The roles extension of a group context (D-0070).
pub fn roles_of(ctx: &GroupContext) -> Result<Roles, GroupError> {
    roles_in(ctx.extensions())
}

/// The roles extension among a group context's extensions (clients read them with
/// `MlsGroup::extensions`).
pub fn roles_in(exts: &Extensions<GroupContext>) -> Result<Roles, GroupError> {
    let ext = exts.unknown(ROLES_EXTENSION).ok_or(GroupError::Roles)?;
    Roles::decode(&ext.0).map_err(|_| GroupError::Roles)
}

/// A change-log entry (D-0070) is the committer's: signed by its leaf key, bound to this group
/// and epoch. The server cannot read it and cannot forge one.
fn verify_log(
    group: &[u8; GROUP_ID_LEN],
    from_epoch: u64,
    log: &[u8],
    key: &SignaturePublicKey,
) -> Result<(), GroupError> {
    let entry = wire::LogEntry::parse(log)?;
    let content = wire::log_signed_content(group, from_epoch, entry.ciphertext);
    CRYPTO
        .verify_signature(
            SignatureScheme::ED25519,
            &content,
            key.as_slice(),
            entry.signature,
        )
        .map_err(|_| GroupError::LogSignature)
}

/// The `external_senders` extension naming the delivery service's key (D-0075).
#[must_use]
pub fn ds_external_senders(key: &[u8; 32]) -> ExternalSendersExtension {
    vec![ExternalSender::new(
        SignaturePublicKey::from(&key[..]),
        BasicCredential::new(crate::external::DS_IDENTITY.to_vec()).into(),
    )]
}

/// Only the roles extension, `required_capabilities` and (exactly as `ds`, when set) the
/// delivery service's `external_senders` may be in a group's context.
#[must_use]
pub fn extensions_allowed(ctx: &GroupContext, ds: Option<&[u8; 32]>) -> bool {
    context_extensions_allowed(ctx.extensions(), ds)
}

/// [`extensions_allowed`] on the context's extensions (what a client holds for a group it is in).
#[must_use]
pub fn context_extensions_allowed(exts: &Extensions<GroupContext>, ds: Option<&[u8; 32]>) -> bool {
    let mut required = false;
    let mut external = false;
    exts.iter().all(|e| match e {
        Extension::Unknown(t, _) => *t == ROLES_EXTENSION,
        // Exactly Songbird's: every member supports the roles extension, `SelfRemove` and basic
        // credentials. A member cannot drop it to let in clients that ignore roles (D-0074).
        Extension::RequiredCapabilities(r) => {
            required = r.extension_types() == [ExtensionType::Unknown(ROLES_EXTENSION)]
                && r.proposal_types() == [ProposalType::SelfRemove]
                && r.credential_types() == [CredentialType::Basic];
            required
        }
        // Exactly the delivery service, alone: no member may add another external sender (who
        // could then propose), or drop the service (and keep departed devices' leaves).
        Extension::ExternalSenders(s) => {
            external = ds.is_some_and(|k| *s == ds_external_senders(k));
            external
        }
        _ => false,
    }) && required
        && external == ds.is_some()
}

/// The structural rule for an external commit (a join or a rejoin), which the server and every
/// app run alike (D-0078):
///
/// - its proposals are its own (`NewMemberCommit`), inline, and only `ExternalInit` and
///   `Remove`: nothing by reference, no pre-shared key, nothing custom;
/// - its new leaf is the joining device's;
/// - the only leaf it may remove is that device's own old one (a resync, RFC 9420 §12.4.3.2);
///   another member's leaf, even another device of the same account, is removed by a
///   member's commit;
/// - a device that still holds a leaf must remove it (no device twice).
///
/// `leaf_device` names the device of a leaf index in the group before the commit; `joiner_leaf`
/// is the joining device's leaf there, if any. Who may join at all (an account already present,
/// or a link and its proof) is decided on top of this. Returns the removed leaves.
///
/// # Errors
/// [`GroupError::Proposal`], [`GroupError::Sender`], [`GroupError::External`] or
/// [`GroupError::Duplicate`].
pub fn check_external_commit(
    staged: &StagedCommit,
    joiner: &DeviceId,
    leaf_device: impl Fn(u32) -> Option<DeviceId>,
    joiner_leaf: Option<u32>,
) -> Result<BTreeSet<u32>, GroupError> {
    for q in staged.queued_proposals() {
        match (q.sender(), q.proposal()) {
            (Sender::NewMemberCommit, Proposal::ExternalInit(_) | Proposal::Remove(_)) => {}
            _ => return Err(GroupError::Proposal),
        }
    }
    let leaf = staged.update_path_leaf_node().ok_or(GroupError::External)?;
    if device_of(leaf.credential())? != *joiner {
        return Err(GroupError::Sender);
    }
    let removed: BTreeSet<u32> = staged
        .remove_proposals()
        .map(|r| r.remove_proposal().removed().u32())
        .collect();
    if removed.iter().any(|i| leaf_device(*i) != Some(*joiner)) {
        return Err(GroupError::External);
    }
    if joiner_leaf.is_some_and(|i| !removed.contains(&i)) {
        return Err(GroupError::Duplicate);
    }
    Ok(removed)
}

/// One leaf of the roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Leaf {
    pub device: DeviceId,
    pub index: u32,
}

/// The roster the server authorizes and fans out with: one entry per leaf, sorted by device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Roster {
    leaves: Vec<Leaf>,
}

impl Roster {
    fn of(public: &PublicGroup) -> Result<Self, GroupError> {
        let mut leaves = public
            .members()
            .map(|m| {
                Ok(Leaf {
                    device: device_of(&m.credential)?,
                    index: m.index.u32(),
                })
            })
            .collect::<Result<Vec<_>, GroupError>>()?;
        leaves.sort_unstable();
        // Two leaves for one device would make the device's sends ambiguous.
        if leaves
            .windows(2)
            .any(|w| matches!(w, [a, b] if a.device == b.device))
        {
            return Err(GroupError::Duplicate);
        }
        Ok(Self { leaves })
    }

    /// The device's leaf, if it is a member.
    #[must_use]
    pub fn leaf(&self, device: &DeviceId) -> Option<u32> {
        self.leaves
            .binary_search_by(|l| l.device.cmp(device))
            .ok()
            .and_then(|i| self.leaves.get(i))
            .map(|l| l.index)
    }

    /// Every member device, sorted.
    pub fn devices(&self) -> impl Iterator<Item = DeviceId> + '_ {
        self.leaves.iter().map(|l| l.device)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }
}

/// What a commit changed, for the caller to persist and fan out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The epoch the commit was made from; the group is now at `from_epoch + 1`.
    pub from_epoch: u64,
    /// Devices added (each gets the Welcome).
    pub added: Vec<DeviceId>,
    /// Devices no longer in the group (each still gets the commit, to learn it).
    pub removed: Vec<DeviceId>,
    /// SHA-256 of the commit message, to recognise a retried commit.
    pub commit_hash: [u8; 32],
    /// The committer's change-log entry (`wire::LogEntry` bytes), its signature verified.
    pub log: Option<Vec<u8>>,
    /// Each added KeyPackage's reference, with its device: the caller checks that the device
    /// published it (D-0074), before storing the commit.
    pub key_packages: Vec<(DeviceId, [u8; 32])>,
}

/// What [`Group::propose_removals`] did (D-0075).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerProposals {
    /// Every server removal queued at this epoch (`MLSMessage` bytes), to fan out.
    pub messages: Vec<Vec<u8>>,
    /// How many of them this call queued: the state changed (store it) only if not zero.
    pub queued: usize,
}

/// The stored form of a group: what the registry keeps under the group ID, compared and set
/// by epoch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub group: [u8; GROUP_ID_LEN],
    pub epoch: u64,
    pub state: StateParts,
    /// The current epoch's `GroupInfo` (`MLSMessage`), for devices that join by external
    /// commit.
    pub group_info: Vec<u8>,
}

impl Record {
    /// Postcard bytes of the record.
    pub fn encode(&self) -> Result<Vec<u8>, GroupError> {
        postcard::to_stdvec(self).map_err(|_| GroupError::State(StateError::Encoding))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, GroupError> {
        postcard::from_bytes(bytes).map_err(|_| GroupError::State(StateError::Encoding))
    }
}

/// A group as the delivery service tracks it.
#[derive(Debug)]
pub struct Group {
    id: [u8; GROUP_ID_LEN],
    public: PublicGroup,
    state: GroupState,
    roster: Roster,
    info: Vec<u8>,
}

fn device_of(credential: &Credential) -> Result<DeviceId, GroupError> {
    if credential.credential_type() != CredentialType::Basic {
        return Err(GroupError::Credential);
    }
    let basic =
        BasicCredential::try_from(credential.clone()).map_err(|_| GroupError::Credential)?;
    DeviceId::from_slice(basic.identity()).map_err(|_| GroupError::Credential)
}

fn mls_message(bytes: &[u8]) -> Result<MlsMessageBodyIn, GroupError> {
    let msg = MlsMessageIn::tls_deserialize_exact(bytes).map_err(|_| GroupError::Malformed)?;
    Ok(msg.extract())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let d = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

/// Checks a `GroupInfo` is for `context` and signed by `leaf`'s key.
fn check_group_info(
    bytes: &[u8],
    context: &GroupContext,
    leaf: u32,
    signature_key: &SignaturePublicKey,
) -> Result<(), GroupError> {
    if wire::wire_format(bytes)? != wire::WIRE_GROUP_INFO {
        return Err(GroupError::GroupInfo);
    }
    if wire::group_info_signer(bytes)? != leaf {
        return Err(GroupError::GroupInfo);
    }
    let MlsMessageBodyIn::GroupInfo(gi) = mls_message(bytes)? else {
        return Err(GroupError::GroupInfo);
    };
    if gi.group_context() != context {
        return Err(GroupError::GroupInfo);
    }
    let key = OpenMlsSignaturePublicKey::from_signature_key(
        signature_key.clone(),
        context.ciphersuite().signature_algorithm(),
    );
    gi.verify_no_out(&CRYPTO, &key)
        .map_err(|_| GroupError::GroupInfo)?;
    Ok(())
}

impl Group {
    /// Starts tracking a group its creator just made: epoch 0, the creator as the only leaf, a
    /// Songbird ciphersuite, the requested ID, and (when `limits` names one) the delivery
    /// service as its only external sender.
    pub fn create(
        id: [u8; GROUP_ID_LEN],
        creator: &DeviceId,
        group_info: &[u8],
        ratchet_tree: &[u8],
        dir: &impl Directory,
        limits: &Limits,
    ) -> Result<Self, GroupError> {
        let Some(creator_account) = dir.account_of(creator) else {
            return Err(GroupError::Credential);
        };
        let MlsMessageBodyIn::GroupInfo(gi) = mls_message(group_info)? else {
            return Err(GroupError::Kind);
        };
        let ctx = gi.group_context();
        if ctx.group_id().as_slice() != id {
            return Err(GroupError::OtherGroup);
        }
        if !GROUP_CIPHERSUITES.contains(&ctx.ciphersuite()) {
            return Err(GroupError::Ciphersuite);
        }
        if ctx.epoch().as_u64() != 0 || !extensions_allowed(ctx, limits.external_sender.as_ref()) {
            return Err(GroupError::Kind);
        }
        // The creator's account is the only owner, and the only entry.
        let roles = roles_of(ctx)?;
        if roles.explicit.len() != 1 || roles.explicit.get(&creator_account) != Some(&Role::Owner) {
            return Err(GroupError::Roles);
        }
        if wire::group_info_signer(group_info)? != 0 {
            return Err(GroupError::GroupInfo);
        }
        let tree = RatchetTreeIn::tls_deserialize_exact(ratchet_tree)
            .map_err(|_| GroupError::Malformed)?;
        let state = GroupState::empty();
        let (public, _) =
            PublicGroup::from_external(&CRYPTO, &state, tree, gi, ProposalStore::new())
                .map_err(|_| GroupError::Invalid)?;
        let roster = Roster::of(&public)?;
        if roster.len() != 1 || roster.leaf(creator) != Some(0) {
            return Err(GroupError::Sender);
        }
        Ok(Self {
            id,
            public,
            state,
            roster,
            info: group_info.to_vec(),
        })
    }

    /// Loads a stored record.
    pub fn load(record: Record) -> Result<Self, GroupError> {
        let state = GroupState::from_parts(record.state)?;
        let gid = GroupId::from_slice(&record.group);
        let public = PublicGroup::load(&state, &gid)?.ok_or(StateError::Encoding)?;
        if public.group_context().epoch().as_u64() != record.epoch {
            return Err(GroupError::State(StateError::Encoding));
        }
        let roster = Roster::of(&public)?;
        Ok(Self {
            id: record.group,
            public,
            state,
            roster,
            info: record.group_info,
        })
    }

    /// The record to store.
    #[must_use]
    pub fn record(&self) -> Record {
        Record {
            group: self.id,
            epoch: self.epoch(),
            state: self.state.parts(),
            group_info: self.info.clone(),
        }
    }

    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.public.group_context().epoch().as_u64()
    }

    #[must_use]
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// The current `GroupInfo` (`MLSMessage`).
    #[must_use]
    pub fn group_info(&self) -> &[u8] {
        &self.info
    }

    /// The current ratchet tree (TLS), for a device joining by Welcome or external commit.
    pub fn ratchet_tree(&self) -> Result<Vec<u8>, GroupError> {
        self.public
            .export_ratchet_tree()
            .tls_serialize_detached()
            .map_err(|_| GroupError::Malformed)
    }

    /// The accounts a commit adds and removes devices for (D-0070). Devices the directory no
    /// longer lists are dead: anyone may remove them, so they are left out of `lost`.
    fn change(
        &self,
        staged: &StagedCommit,
        removed_leaves: &BTreeSet<u32>,
        added: &[DeviceId],
        dir: &impl Directory,
    ) -> Result<Change, GroupError> {
        // Leaves removed by their own proposal (leaving): the leaver authorized it.
        let self_removed: BTreeSet<u32> = staged
            .queued_proposals()
            .filter_map(|q| match (q.proposal(), q.sender()) {
                (Proposal::SelfRemove, Sender::Member(idx)) => Some(idx.u32()),
                _ => None,
            })
            .collect();
        // Leaves removed by the delivery service's proposal (D-0075): only devices that left
        // the directory, whoever commits it.
        let server_removed: BTreeSet<u32> = staged
            .queued_proposals()
            .filter_map(|q| match (q.proposal(), q.sender()) {
                (Proposal::Remove(r), Sender::External(_)) => Some(r.removed().u32()),
                _ => None,
            })
            .collect();
        let mut c = Change::default();
        let mut removed_devices = BTreeSet::new();
        for idx in removed_leaves {
            let leaf = self
                .public
                .leaf(LeafNodeIndex::new(*idx))
                .ok_or(GroupError::Proposal)?;
            let device = device_of(leaf.credential())?;
            removed_devices.insert(device);
            if let Some(a) = dir.account_of(&device) {
                if server_removed.contains(idx) {
                    c.external.insert(a);
                } else if self_removed.contains(idx) {
                    c.left.insert(a);
                } else {
                    c.lost.insert(a);
                }
            }
        }
        for d in self.roster.devices() {
            if let Some(a) = dir.account_of(&d) {
                c.before.insert(a);
                if !removed_devices.contains(&d) {
                    c.after.insert(a);
                }
            }
        }
        for d in added {
            if let Some(a) = dir.account_of(d) {
                c.gained.insert(a);
                c.after.insert(a);
            }
        }
        Ok(c)
    }

    /// The group's roles (its context's roles extension).
    pub fn roles(&self) -> Result<Roles, GroupError> {
        roles_of(self.public.group_context())
    }

    /// The roster as the registry keeps it for the send path and `api` (D-0070):
    /// `version u8 (2) | policy [7] | n × (device [16] | role u8)`, devices ascending. A device
    /// the directory no longer lists has the role `ReadOnly` (it cannot connect anyway).
    pub fn roster_bytes(&self, dir: &impl Directory) -> Result<Vec<u8>, GroupError> {
        let roles = self.roles()?;
        let mut out = Vec::with_capacity(8 + self.roster.len() * 17);
        out.push(ROSTER_V2);
        out.extend_from_slice(&roles.policy.to_bytes());
        for d in self.roster.devices() {
            let role = dir
                .account_of(&d)
                .map_or(Role::ReadOnly, |a| roles.role(&a));
            out.extend_from_slice(d.as_bytes());
            out.push(role.as_u8());
        }
        Ok(out)
    }

    /// Proposal kinds, identities of new and updated leaves, and the external-commit rule.
    /// Returns the leaves the commit removes.
    fn check_leaves(
        &self,
        staged: &StagedCommit,
        sender: &DeviceId,
        sender_account: AccountId,
        external: bool,
        by_link: bool,
        dir: &impl Directory,
    ) -> Result<BTreeSet<u32>, GroupError> {
        if external {
            let removed = check_external_commit(
                staged,
                sender,
                |i| {
                    self.public
                        .leaf(LeafNodeIndex::new(i))
                        .and_then(|l| device_of(l.credential()).ok())
                },
                self.roster.leaf(sender),
            )?;
            // A rejoin: the account already has a device here (or a join by link).
            let account_present = self
                .roster
                .devices()
                .any(|d| d != *sender && dir.account_of(&d) == Some(sender_account))
                || self.roster.leaf(sender).is_some();
            if !account_present && !by_link {
                return Err(GroupError::External);
            }
            return Ok(removed);
        }
        for q in staged.queued_proposals() {
            // Who proposed it: the committer (inline) or a member (by reference), or the
            // delivery service (by reference, D-0075), which may only propose removals. No
            // other outsider proposes (no `NewMemberProposal`), and an external commit
            // carries nothing by reference.
            match q.sender() {
                Sender::Member(_) | Sender::NewMemberCommit => {}
                Sender::External(_) if !external && matches!(q.proposal(), Proposal::Remove(_)) => {
                }
                _ => return Err(GroupError::Proposal),
            }
            match q.proposal() {
                Proposal::Add(_)
                | Proposal::Remove(_)
                | Proposal::Update(_)
                | Proposal::SelfRemove => {}
                Proposal::ExternalInit(_) if external => {}
                // Roles and policy changes (M2), authorized by `roles::authorize`.
                Proposal::GroupContextExtensions(_) if !external => {}
                _ => return Err(GroupError::Proposal),
            }
        }

        // The committer's (or joiner's) new leaf keeps the authenticated device's identity.
        if let Some(leaf) = staged.update_path_leaf_node() {
            if device_of(leaf.credential())? != *sender {
                return Err(GroupError::Sender);
            }
        } else if external {
            return Err(GroupError::External);
        }
        // Updates never change a leaf's identity.
        for up in staged.update_proposals() {
            let Sender::Member(idx) = up.sender() else {
                return Err(GroupError::Proposal);
            };
            let old = self.public.leaf(*idx).ok_or(GroupError::Proposal)?;
            if device_of(up.update_proposal().leaf_node().credential())?
                != device_of(old.credential())?
            {
                return Err(GroupError::Credential);
            }
        }

        // Removed leaves: the commit's Remove proposals, and the self-removals it commits (a
        // leaver's SelfRemove, by reference: queued by `apply_proposal`).
        let mut removed: BTreeSet<u32> = staged
            .remove_proposals()
            .map(|r| r.remove_proposal().removed().u32())
            .collect();
        for q in staged.queued_proposals() {
            if let (Proposal::SelfRemove, Sender::Member(idx)) = (q.proposal(), q.sender()) {
                removed.insert(idx.u32());
            }
        }
        Ok(removed)
    }

    /// Added devices: known, not already present (unless their old leaf goes), and the Welcome
    /// holds exactly one secret per added KeyPackage. Returns the added devices with their
    /// KeyPackages' references.
    fn check_adds(
        &self,
        staged: &StagedCommit,
        welcome: Option<&[u8]>,
        removed: &BTreeSet<u32>,
        dir: &impl Directory,
    ) -> Result<Vec<(DeviceId, [u8; 32])>, GroupError> {
        let mut added = Vec::new();
        let mut added_refs = BTreeSet::new();
        for a in staged.add_proposals() {
            let kp = a.add_proposal().key_package();
            let device = device_of(kp.leaf_node().credential())?;
            if dir.account_of(&device).is_none() {
                return Err(GroupError::Credential);
            }
            if self
                .roster
                .leaf(&device)
                .is_some_and(|i| !removed.contains(&i))
                || added.iter().any(|(d, _)| *d == device)
            {
                return Err(GroupError::Duplicate);
            }
            let r = kp.hash_ref(&CRYPTO).map_err(|_| GroupError::Invalid)?;
            let r: [u8; 32] = r.as_slice().try_into().map_err(|_| GroupError::Invalid)?;
            added_refs.insert(r.to_vec());
            added.push((device, r));
        }
        match (welcome, added.is_empty()) {
            (None, true) => {}
            (Some(w), false) => {
                if wire::wire_format(w)? != wire::WIRE_WELCOME {
                    return Err(GroupError::Welcome);
                }
                let MlsMessageBodyIn::Welcome(welcome) = mls_message(w)? else {
                    return Err(GroupError::Welcome);
                };
                if welcome.ciphersuite() != self.public.ciphersuite() {
                    return Err(GroupError::Welcome);
                }
                let refs: BTreeSet<Vec<u8>> = welcome
                    .secrets()
                    .iter()
                    .map(|s| s.new_member().as_slice().to_vec())
                    .collect();
                if refs.len() != welcome.secrets().len() || refs != added_refs {
                    return Err(GroupError::Welcome);
                }
            }
            _ => return Err(GroupError::Welcome),
        }
        Ok(added)
    }

    /// Queues a member's proposal to leave (D-0070): an MLS `SelfRemove` in a `PublicMessage`,
    /// at the current epoch, signed by the leaf of `sender`. The next commit includes it by
    /// reference; its committer needs no role to do so. Every other proposal is refused: the
    /// rest of a group's changes travel inline in commits.
    ///
    /// Returns the message's SHA-256 (to recognise a retry). On error the group is dropped, as
    /// for a commit.
    pub fn apply_proposal(
        mut self,
        sender: &DeviceId,
        msg: &[u8],
    ) -> Result<(Self, [u8; 32]), GroupError> {
        if msg.len() > MAX_PROPOSAL {
            return Err(GroupError::Wire(WireError::TooLarge));
        }
        if wire::wire_format(msg)? != wire::WIRE_PUBLIC {
            return Err(GroupError::Kind);
        }
        let MlsMessageBodyIn::PublicMessage(pm) = mls_message(msg)? else {
            return Err(GroupError::Kind);
        };
        if pm.content_type() != ContentType::Proposal {
            return Err(GroupError::Kind);
        }
        if pm.group_id().as_slice() != self.id {
            return Err(GroupError::OtherGroup);
        }
        let current = self.epoch();
        if pm.epoch().as_u64() != current {
            return Err(GroupError::StaleEpoch { current });
        }
        let Sender::Member(leaf) = *pm.sender() else {
            return Err(GroupError::Sender);
        };
        if self.roster.leaf(sender) != Some(leaf.u32()) {
            return Err(GroupError::Sender);
        }
        let processed = self
            .public
            .process_message(&CRYPTO, pm)
            .map_err(|_| GroupError::Invalid)?;
        let ProcessedMessageContent::ProposalMessage(q) = processed.into_content() else {
            return Err(GroupError::Kind);
        };
        if !matches!(q.proposal(), Proposal::SelfRemove) {
            return Err(GroupError::Proposal);
        }
        self.public
            .add_proposal(&self.state, *q)
            .map_err(GroupError::State)?;
        Ok((self, sha256(msg)))
    }

    /// The devices queued server removals target (D-0075). A commit clears the queue whether it
    /// includes them or not, so the caller proposes again for those still in the group: they
    /// were confirmed gone in the registry, whatever a cached directory says.
    pub fn pending_server_removals(&self) -> Result<Vec<DeviceId>, GroupError> {
        let mut out = Vec::new();
        for (_, q) in self.public.queued_proposals(&self.state)? {
            if let (Proposal::Remove(r), Sender::External(_)) = (q.proposal(), q.sender())
                && let Some(leaf) = self.public.leaf(r.removed())
            {
                out.push(device_of(leaf.credential())?);
            }
        }
        Ok(out)
    }

    /// The delivery service proposes removing every leaf whose device left the directory
    /// (D-0075): an external `Remove` per leaf, signed by `signer`, checked by this group's own
    /// validation (the signature against the context's `external_senders`) and queued like a
    /// member's proposal. Leaves a queued proposal already removes are skipped, so calling it
    /// again queues nothing new. A group that does not name `signer` gets nothing; a full
    /// proposal queue stops early (the rest follow the next commit).
    ///
    /// The answer lists every server removal queued at this epoch, new or not, as the bytes to
    /// fan out: signing is deterministic, so a proposal is the same bytes each time (one
    /// idempotency key), and a fan-out that failed after the state was stored is finished by
    /// the next call.
    ///
    /// `dir` must answer from the registry, not a cached snapshot: members refuse, and the
    /// server refuses to commit, a server removal of a device that is still listed.
    ///
    /// On error the group is dropped, as for a commit.
    pub fn propose_removals(
        mut self,
        dir: &impl Directory,
        signer: &crate::external::DsSigner,
    ) -> Result<(Self, ServerProposals), GroupError> {
        let mut out = ServerProposals::default();
        let named = self
            .public
            .group_context()
            .extensions()
            .external_senders()
            .is_some_and(|s| *s == ds_external_senders(&signer.public_key()));
        if !named {
            return Ok((self, out));
        }
        let epoch = self.epoch();
        let mut pending = BTreeSet::new();
        for (_, q) in self.public.queued_proposals(&self.state)? {
            match (q.proposal(), q.sender()) {
                (Proposal::Remove(r), Sender::External(_)) => {
                    let leaf = r.removed().u32();
                    pending.insert(leaf);
                    out.messages.push(crate::external::remove_proposal(
                        &self.id, epoch, leaf, signer,
                    ));
                }
                (Proposal::Remove(r), _) => {
                    pending.insert(r.removed().u32());
                }
                (Proposal::SelfRemove, Sender::Member(i)) => {
                    pending.insert(i.u32());
                }
                _ => {}
            }
        }
        let dead: Vec<u32> = self
            .roster
            .leaves
            .iter()
            .filter(|l| dir.account_of(&l.device).is_none() && !pending.contains(&l.index))
            .map(|l| l.index)
            .collect();
        for leaf in dead {
            let msg = crate::external::remove_proposal(&self.id, epoch, leaf, signer);
            let MlsMessageBodyIn::PublicMessage(pm) = mls_message(&msg)? else {
                return Err(GroupError::Kind);
            };
            let processed = self
                .public
                .process_message(&CRYPTO, pm)
                .map_err(|_| GroupError::Invalid)?;
            let ProcessedMessageContent::ProposalMessage(q) = processed.into_content() else {
                return Err(GroupError::Kind);
            };
            match self.public.add_proposal(&self.state, *q) {
                Ok(()) => {
                    out.messages.push(msg);
                    out.queued += 1;
                }
                Err(StateError::TooManyProposals) => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok((self, out))
    }

    /// Validates, authorizes and merges a commit bundle sent by `sender`. On success the group
    /// is at the next epoch; on error it is dropped, and the caller reloads its record.
    pub fn apply_commit(
        self,
        sender: &DeviceId,
        bundle: &[u8],
        dir: &impl Directory,
        limits: &Limits,
    ) -> Result<(Self, Applied), GroupError> {
        self.apply(sender, bundle, dir, limits, None)
    }

    /// A person joining by invite link `link` (its handle; M3, D-0071): an external commit from
    /// a device whose account need not be in the group yet. Call it only once the link is
    /// checked; the link's use is counted with the commit's store. It joins with the default
    /// role, and must carry the link's proof (D-0076, `crate::links`): a signature by the key
    /// the members published for that link, which the server cannot make.
    pub fn apply_join(
        self,
        sender: &DeviceId,
        bundle: &[u8],
        dir: &impl Directory,
        limits: &Limits,
        link: &[u8; 16],
    ) -> Result<(Self, Applied), GroupError> {
        self.apply(sender, bundle, dir, limits, Some(link))
    }

    /// A commit's `PublicMessage`, for this group at its current epoch.
    fn commit_message(&self, commit: &[u8]) -> Result<PublicMessageIn, GroupError> {
        if wire::wire_format(commit)? != wire::WIRE_PUBLIC {
            return Err(GroupError::Kind);
        }
        let MlsMessageBodyIn::PublicMessage(pm) = mls_message(commit)? else {
            return Err(GroupError::Kind);
        };
        if pm.content_type() != ContentType::Commit {
            return Err(GroupError::Kind);
        }
        if pm.group_id().as_slice() != self.id {
            return Err(GroupError::OtherGroup);
        }
        let current = self.epoch();
        if pm.epoch().as_u64() != current {
            return Err(GroupError::StaleEpoch { current });
        }
        Ok(pm)
    }

    /// A join by link proves the joiner held the link (D-0076): a signature by the key the
    /// members published for `handle` (the link the join is counted against), over this group,
    /// the current epoch, the joining device and its new leaf's key.
    fn check_link_proof(
        &self,
        staged: &StagedCommit,
        sender: &DeviceId,
        aad: &[u8],
        handle: &[u8; 16],
    ) -> Result<(), GroupError> {
        let leaf = staged.update_path_leaf_node().ok_or(GroupError::External)?;
        crate::links::verify(
            &roles_of(self.public.group_context())?,
            &self.id,
            self.epoch(),
            aad,
            sender,
            leaf.signature_key().as_slice(),
            Some(handle),
        )
        .map(|_| ())
        .map_err(GroupError::LinkProof)
    }

    fn apply(
        mut self,
        sender: &DeviceId,
        bundle: &[u8],
        dir: &impl Directory,
        limits: &Limits,
        link: Option<&[u8; 16]>,
    ) -> Result<(Self, Applied), GroupError> {
        let by_link = link.is_some();
        let b = Bundle::parse(bundle, limits.max_bundle)?;
        let pm = self.commit_message(b.commit)?;
        let from_epoch = self.epoch();
        let sender_account = dir.account_of(sender).ok_or(GroupError::Sender)?;
        let external = match pm.sender() {
            // A join by link is an external commit.
            Sender::Member(_) if by_link => return Err(GroupError::Kind),
            Sender::Member(leaf) => {
                if self.roster.leaf(sender) != Some(leaf.u32()) {
                    return Err(GroupError::Sender);
                }
                false
            }
            Sender::NewMemberCommit => true,
            _ => return Err(GroupError::Sender),
        };

        let processed = self
            .public
            .process_message(&CRYPTO, pm)
            .map_err(|_| GroupError::Invalid)?;
        let aad = processed.aad().to_vec();
        let ProcessedMessageContent::StagedCommitMessage(staged) = processed.into_content() else {
            return Err(GroupError::Kind);
        };

        let removed_leaves =
            self.check_leaves(&staged, sender, sender_account, external, by_link, dir)?;
        if let Some(handle) = link {
            self.check_link_proof(&staged, sender, &aad, handle)?;
        }
        let key_packages = self.check_adds(&staged, b.welcome, &removed_leaves, dir)?;
        let added: Vec<DeviceId> = key_packages.iter().map(|(d, _)| *d).collect();

        // Roles (D-0070): the commit's new roles extension must be valid and the change allowed
        // for the committer's role. External commits cannot change extensions, and only rejoin.
        let new_context = staged.group_context().clone();
        if !extensions_allowed(&new_context, limits.external_sender.as_ref()) {
            return Err(GroupError::Proposal);
        }
        let new_roles = roles_of(&new_context)?;
        // A channel has its own size limit (D-0072), by the policy the commit leaves.
        let max = limits.cap(&new_roles.policy);
        let new_size = self.roster.len() + added.len() + usize::from(external)
            - removed_leaves.len().min(self.roster.len());
        if new_size > max {
            return Err(GroupError::TooLarge { max });
        }
        if !external {
            let old_roles = roles_of(self.public.group_context())?;
            let change = self.change(&staged, &removed_leaves, &added, dir)?;
            roles::authorize(&sender_account, &old_roles, &new_roles, &change)
                .map_err(GroupError::Forbidden)?;
        }

        let old = self.roster.clone();
        self.public
            .merge_commit(&self.state, *staged)
            .map_err(|_| GroupError::Invalid)?;
        self.roster = Roster::of(&self.public)?;

        // The uploaded GroupInfo describes the new epoch and is signed by the committer's
        // (or joiner's) leaf as it is now.
        let committer = self.roster.leaf(sender).ok_or(GroupError::Sender)?;
        let committer_leaf = self
            .public
            .leaf(LeafNodeIndex::new(committer))
            .ok_or(GroupError::Sender)?;
        if &new_context != self.public.group_context() {
            return Err(GroupError::Invalid);
        }
        check_group_info(
            b.group_info,
            &new_context,
            committer,
            committer_leaf.signature_key(),
        )?;
        if self.roster.len() > max {
            return Err(GroupError::TooLarge { max });
        }
        self.info = b.group_info.to_vec();

        if let Some(log) = b.log {
            verify_log(&self.id, from_epoch, log, committer_leaf.signature_key())?;
        }

        let removed = old
            .devices()
            .filter(|d| self.roster.leaf(d).is_none())
            .collect();
        let applied = Applied {
            from_epoch,
            added,
            removed,
            commit_hash: sha256(b.commit),
            log: b.log.map(<[u8]>::to_vec),
            key_packages,
        };
        Ok((self, applied))
    }
}
