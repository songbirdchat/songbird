//! The group client (A2 M5, D-0146): Songbird groups over the delivery service (PROTOCOL §6).
//!
//! - **KeyPackages** (§4.5): this device's, built with its group signing key and the
//!   capabilities every Songbird leaf has (the suite, basic credentials, the roles extension,
//!   self-remove), its credential the 16-byte device ID; one last-resort package.
//! - **Creating** (§6.2): a group at epoch 0 whose context names the roles (§6.6, this account
//!   the owner) and the delivery service as external sender (§6.11), exactly; then a commit adding
//!   the members' devices. Every fetched KeyPackage is validated in full (RFC 9420 §10.1), and its
//!   credential must be a device the account's signed list names.
//! - **Sending** (§6.5): `account [16] ‖ padded Content` as an MLS `PrivateMessage`, one shared
//!   body, franked. MLS names the sending device; the account rides inside, and receivers accept it
//!   only if that device is in the account's verified list.
//! - **Receiving**: a Welcome joins (the tree from `POST /v1/groups/state`); a commit is
//!   validated by OpenMLS and merged; a `PrivateMessage` is decrypted and stored under its group.
//!
//! Group state is saved to the encrypted store after every change.

use std::collections::{BTreeSet, HashMap};

use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::*;
use songbird_content::schema::{
    Content, DataMessage, Text, content::Body, data_message, group_update,
};
use songbird_groups::roles::{Policy, ROLES_EXTENSION, Roles};
use songbird_groups::wire::Bundle;
use songbird_proto::envelope::Stamp;
use songbird_proto::frame::{SendHints, SendStatus};
use songbird_types::AccountId;
use zeroize::Zeroizing;

use super::crypto::GROUP_CIPHERSUITE;
use crate::crypto::random;
use crate::messenger::{Messenger, Net, Received, data_row, hex, now_ms};
use crate::store::Status;
use crate::{Error, Result};

/// KeyPackages kept on the server (single use).
pub const KEY_PACKAGES: usize = 20;

pub(crate) fn caps() -> Capabilities {
    Capabilities::new(
        None,
        Some(&[GROUP_CIPHERSUITE]),
        Some(&[ExtensionType::Unknown(ROLES_EXTENSION)]),
        Some(&[ProposalType::SelfRemove]),
        Some(&[CredentialType::Basic]),
    )
}

pub(crate) fn context_extensions(roles: &Roles, ds: &[u8; 32]) -> Result<Extensions<GroupContext>> {
    Extensions::from_vec(vec![
        Extension::RequiredCapabilities(RequiredCapabilitiesExtension::new(
            &[ExtensionType::Unknown(ROLES_EXTENSION)],
            &[ProposalType::SelfRemove],
            &[CredentialType::Basic],
        )),
        Extension::Unknown(ROLES_EXTENSION, UnknownExtension(roles.encode())),
        Extension::ExternalSenders(songbird_groups::ds::ds_external_senders(ds)),
    ])
    .map_err(|_| Error::Malformed)
}

pub(crate) fn join_config() -> MlsGroupJoinConfig {
    MlsGroupJoinConfig::builder()
        .wire_format_policy(PURE_PLAINTEXT_WIRE_FORMAT_POLICY)
        .max_past_epochs(1)
        .build()
}

pub(crate) fn tls<T: tls_codec::Serialize>(v: &T) -> Result<Vec<u8>> {
    v.tls_serialize_detached().map_err(|_| Error::Malformed)
}

impl<N: Net> Messenger<N> {
    fn credential(&self) -> CredentialWithKey {
        CredentialWithKey {
            credential: BasicCredential::new(self.local.device.to_vec()).into(),
            signature_key: self.mls_signer.public().into(),
        }
    }

    pub(crate) fn save_mls(&self) -> Result<()> {
        self.store.put_secret("mls_state", &self.mls.snapshot()?)
    }

    fn key_package(&self, last_resort: bool) -> Result<Vec<u8>> {
        let mut b = KeyPackage::builder().leaf_node_capabilities(caps());
        if last_resort {
            b = b.mark_as_last_resort();
        }
        let kp = b
            .build(
                GROUP_CIPHERSUITE,
                &self.mls,
                &self.mls_signer,
                self.credential(),
            )
            .map_err(|_| Error::Malformed)?;
        tls(kp.key_package())
    }

    /// Publishes KeyPackages (`PUT /v1/keypackages`): up to [`KEY_PACKAGES`] single use and,
    /// with `last_resort`, the last-resort one.
    pub async fn publish_key_packages(&mut self, single: usize, last_resort: bool) -> Result<()> {
        let mut kps = Vec::new();
        for _ in 0..single.min(KEY_PACKAGES) {
            kps.push(self.key_package(false)?);
        }
        if last_resort {
            kps.push(self.key_package(true)?);
        }
        if kps.is_empty() {
            return Ok(());
        }
        let mut body = vec![1, u8::try_from(kps.len()).map_err(|_| Error::Malformed)?];
        for k in &kps {
            body.extend_from_slice(
                &u16::try_from(k.len())
                    .map_err(|_| Error::Malformed)?
                    .to_be_bytes(),
            );
            body.extend_from_slice(k);
        }
        // The private halves are saved before the server can hand out the public ones.
        self.save_mls()?;
        let (status, _) = self.net.api("PUT", "/v1/keypackages", body).await?;
        if status == 204 {
            Ok(())
        } else {
            Err(Error::Server(status))
        }
    }

    /// The delivery service's external-sender key (`POST /v1/groups/ds`), pinned on first use.
    pub(crate) async fn ds_key(&mut self) -> Result<[u8; 32]> {
        if let Some(k) = self.store.secret("ds_key")?
            && let Ok(k) = <[u8; 32]>::try_from(k.as_slice())
        {
            return Ok(k);
        }
        let (status, body) = self.net.api("POST", "/v1/groups/ds", vec![1]).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let k: [u8; 32] = body.as_slice().try_into().map_err(|_| Error::Malformed)?;
        if !songbird_keys::ed25519_public_key_ok(&k) {
            return Err(Error::Verify);
        }
        self.store.put_secret("ds_key", &k)?;
        Ok(k)
    }

    /// Every device of `account` as a validated KeyPackage, each device on its signed list.
    async fn key_packages_of(&mut self, account: &[u8; 16]) -> Result<Vec<KeyPackage>> {
        let listed = self.devices_of(account, false).await?;
        let body = serde_json::json!({ "account": hex(account) })
            .to_string()
            .into_bytes();
        let (status, answer) = self.net.api("POST", "/v1/keypackages/fetch", body).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let mut r = crate::crypto::Reader::new(&answer);
        if r.u8()? != 1 {
            return Err(Error::Malformed);
        }
        let n = r.u8()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let device: [u8; 16] = r.array()?;
            let _flags = r.u8()?;
            let len = usize::from(r.u16()?);
            let kp_in =
                KeyPackageIn::tls_deserialize_exact(r.take(len)?).map_err(|_| Error::Malformed)?;
            let kp = kp_in
                .validate(self.mls.crypto(), ProtocolVersion::Mls10)
                .map_err(|_| Error::Verify)?;
            let cred = kp.leaf_node().credential().serialized_content();
            if cred != device.as_slice() || !listed.iter().any(|(d, _)| *d == device) {
                return Err(Error::Verify);
            }
            out.push(kp);
        }
        r.finish()?;
        Ok(out)
    }

    /// Creates a group with `members` (other accounts), titled `title`. Answers the group ID.
    pub async fn create_group(&mut self, title: &str, members: &[[u8; 16]]) -> Result<[u8; 16]> {
        let gid = *random::<16>()?;
        self.create_group_with_id(gid, title, members).await
    }

    /// [`Self::create_group`] with an ID the caller drew (16 random bytes), so a UI can name the
    /// chat before the server answers.
    pub async fn create_group_with_id(
        &mut self,
        gid: [u8; 16],
        title: &str,
        members: &[[u8; 16]],
    ) -> Result<[u8; 16]> {
        self.create_with_policy(gid, title, members, Policy::GROUP)
            .await
    }

    /// Creates a private channel (§6.10): admins post; subscribers join read-only by link.
    pub async fn create_channel(&mut self, gid: [u8; 16], title: &str) -> Result<[u8; 16]> {
        self.create_with_policy(gid, title, &[], Policy::CHANNEL)
            .await
    }

    /// Creates a community (§6.9) around its announcement group, whose ID is the community's
    /// (D-0149): only admins post there; its members are the community's members.
    pub async fn create_community(
        &mut self,
        cid: [u8; 16],
        title: &str,
        members: &[[u8; 16]],
    ) -> Result<[u8; 16]> {
        let policy = Policy {
            send: songbird_groups::roles::Role::Admin,
            ..Policy::GROUP
        };
        self.create_with_policy(cid, title, members, policy).await?;
        let mut body = vec![1];
        body.extend_from_slice(&cid);
        body.extend_from_slice(&cid);
        let (status, _) = self.net.api("POST", "/v1/communities", body).await?;
        if status == 204 || status == 200 {
            Ok(cid)
        } else {
            Err(Error::Server(status))
        }
    }

    /// Puts a group this device's account admins into a community it admins.
    pub async fn community_add_group(&mut self, cid: &[u8; 16], gid: &[u8; 16]) -> Result<()> {
        let mut body = vec![1];
        body.extend_from_slice(cid);
        body.extend_from_slice(gid);
        let (status, _) = self
            .net
            .api("POST", "/v1/communities/groups/add", body)
            .await?;
        if status == 204 || status == 200 {
            Ok(())
        } else {
            Err(Error::Server(status))
        }
    }

    /// A community's groups, or `None` when `cid` is not a community this device can read.
    pub async fn community_groups(&mut self, cid: &[u8; 16]) -> Result<Option<Vec<[u8; 16]>>> {
        let mut body = vec![1];
        body.extend_from_slice(cid);
        let (status, answer) = self.net.api("POST", "/v1/communities/state", body).await?;
        if status == 404 {
            return Ok(None);
        }
        if status != 200 {
            return Err(Error::Server(status));
        }
        let mut r = crate::crypto::Reader::new(&answer);
        let _announcement: [u8; 16] = r.array()?;
        let _meta_v = r.u64()?;
        let _meta = r.bytes32(16 << 10)?;
        let n = r.u16()?;
        let mut out = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            out.push(r.array()?);
        }
        r.finish()?;
        Ok(Some(out))
    }

    /// What kind of group this is, from its roles: (a channel, an announcement group — only
    /// admins send — and this account's role).
    pub fn group_kind(&self, gid: &[u8; 16]) -> Result<(bool, bool, songbird_groups::roles::Role)> {
        let group = self.load_group(gid)?;
        let roles =
            songbird_groups::ds::roles_in(group.extensions()).map_err(|_| Error::Malformed)?;
        let channel = roles.policy.flags & songbird_groups::roles::FLAG_CHANNEL != 0;
        let announcement = !channel && roles.policy.send >= songbird_groups::roles::Role::Admin;
        Ok((
            channel,
            announcement,
            roles.role(&AccountId::from_bytes(self.local.account)),
        ))
    }

    async fn create_with_policy(
        &mut self,
        gid: [u8; 16],
        title: &str,
        members: &[[u8; 16]],
        policy: Policy,
    ) -> Result<[u8; 16]> {
        let ds = self.ds_key().await?;
        let roles = Roles::new(AccountId::from_bytes(self.local.account), policy);
        let cfg = MlsGroupCreateConfig::builder()
            .wire_format_policy(PURE_PLAINTEXT_WIRE_FORMAT_POLICY)
            .ciphersuite(GROUP_CIPHERSUITE)
            .max_past_epochs(1)
            .capabilities(caps())
            .with_group_context_extensions(context_extensions(&roles, &ds)?)
            .build();
        let group = MlsGroup::new_with_group_id(
            &self.mls,
            &self.mls_signer,
            &cfg,
            GroupId::from_slice(&gid),
            self.credential(),
        )
        .map_err(|_| Error::Malformed)?;
        let info = tls(&group
            .export_group_info(self.mls.crypto(), &self.mls_signer, false)
            .map_err(|_| Error::Malformed)?)?;
        let tree = tls(&group.export_ratchet_tree())?;
        let mut body = vec![1];
        body.extend_from_slice(&gid);
        crate::crypto::put_bytes32(&mut body, &info)?;
        crate::crypto::put_bytes32(&mut body, &tree)?;
        self.save_mls()?;
        let (status, _) = self.net.api("POST", "/v1/groups", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        self.store.ensure_group_chat(&gid)?;
        self.store
            .put_secret(&format!("group_title.{}", hex(&gid)), title.as_bytes())?;
        // The change log's key (D-0156): made here, handed to everyone added.
        self.store
            .put_secret(&format!("group_log_key.{}", hex(&gid)), &*random::<32>()?)?;
        if !members.is_empty() {
            // The "created" update below carries the key; no separate announcement.
            self.creating = true;
            let added = self.add_members(&gid, members).await;
            self.creating = false;
            added?;
        }
        // The title (and that the group exists) as a group update everyone reads.
        let update = songbird_content::schema::GroupUpdate {
            kind: group_update::Kind::Created.into(),
            actor: self.local.account.to_vec(),
            subjects: members.iter().map(|m| m.to_vec()).collect(),
            value: title.into(),
            // The change log's key, for everyone added (D-0156).
            log_key: self.log_key(&gid)?.map(|k| k.to_vec()).unwrap_or_default(),
        };
        let data = DataMessage {
            group_id: gid.to_vec(),
            kind: Some(data_message::Kind::GroupUpdate(update)),
            ..Default::default()
        };
        self.send_group_data(&gid, data, title).await?;
        Ok(gid)
    }

    /// Adds every device of `accounts` (a commit; the server fans out the Welcome).
    pub async fn add_members(&mut self, gid: &[u8; 16], accounts: &[[u8; 16]]) -> Result<()> {
        let mut kps = Vec::new();
        for a in accounts {
            kps.extend(self.key_packages_of(a).await?);
        }
        let line = LogLine::new(group_update::Kind::MembersAdded, accounts);
        self.commit(gid, kps, Vec::new(), Some(line)).await?;
        // Those added get the change log's key (inside the group, after the commit).
        if !self.creating && !accounts.is_empty() {
            self.announce_log_key(gid, group_update::Kind::MembersAdded, accounts)
                .await?;
        }
        Ok(())
    }

    /// Removes every device of `accounts` from a group (an admin who outranks them; the server
    /// checks the roles rule, §6.6).
    pub async fn remove_members(&mut self, gid: &[u8; 16], accounts: &[[u8; 16]]) -> Result<()> {
        let mut devices = Vec::new();
        for a in accounts {
            devices.extend(self.devices_of(a, false).await?.into_iter().map(|(d, _)| d));
        }
        let group = self.load_group(gid)?;
        let leaves: Vec<LeafNodeIndex> = group
            .members()
            .filter(|m| {
                devices
                    .iter()
                    .any(|d| d.as_slice() == m.credential.serialized_content())
            })
            .map(|m| m.index)
            .collect();
        if leaves.is_empty() {
            return Ok(());
        }
        let line = LogLine::new(group_update::Kind::MembersRemoved, accounts);
        self.commit(gid, Vec::new(), leaves, Some(line)).await
    }

    /// Commits proposals others queued (a member leaving): no role is needed (§6.7).
    pub async fn commit_pending(&mut self, gid: &[u8; 16]) -> Result<()> {
        let group = self.load_group(gid)?;
        if group.pending_proposals().next().is_none() {
            return Ok(());
        }
        // Who leaves: the senders of the queued `SelfRemove`s.
        let leaves: HashMap<u32, [u8; 16]> = group
            .members()
            .filter_map(|m| {
                Some((
                    m.index.u32(),
                    <[u8; 16]>::try_from(m.credential.serialized_content()).ok()?,
                ))
            })
            .collect();
        let leaving: BTreeSet<[u8; 16]> = group
            .pending_proposals()
            .filter_map(|q| match (q.proposal(), q.sender()) {
                (Proposal::SelfRemove, Sender::Member(i)) => leaves.get(&i.u32()).copied(),
                _ => None,
            })
            .collect();
        let accounts: Vec<[u8; 16]> = self
            .member_accounts(gid, &leaving)
            .await
            .unwrap_or_default()
            .into_values()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let line = LogLine::new(group_update::Kind::MemberLeft, &accounts);
        self.commit(gid, Vec::new(), Vec::new(), Some(line)).await
    }

    pub(crate) fn load_group(&self, gid: &[u8; 16]) -> Result<MlsGroup> {
        MlsGroup::load(self.mls.storage(), &GroupId::from_slice(gid))
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NoSession)
    }

    /// One commit with adds and removals (and any queued proposals), sent and merged on 200.
    pub(crate) async fn commit(
        &mut self,
        gid: &[u8; 16],
        adds: Vec<KeyPackage>,
        removes: Vec<LeafNodeIndex>,
        line: Option<LogLine>,
    ) -> Result<()> {
        self.commit_with(gid, adds, removes, None, line).await
    }

    /// [`Self::commit`], also changing the group's context extensions (roles, link keys).
    pub(crate) async fn commit_with(
        &mut self,
        gid: &[u8; 16],
        adds: Vec<KeyPackage>,
        removes: Vec<LeafNodeIndex>,
        exts: Option<Extensions<GroupContext>>,
        line: Option<LogLine>,
    ) -> Result<()> {
        let mut group = self.load_group(gid)?;
        let from_epoch = group.epoch().as_u64();
        let builder = group
            .commit_builder()
            .propose_adds(adds)
            .propose_removals(removes);
        let builder = match exts {
            Some(e) => builder
                .propose_group_context_extensions(e)
                .map_err(|_| Error::Malformed)?,
            None => builder,
        };
        let (commit, welcome, info) = builder
            .load_psks(self.mls.storage())
            .map_err(|_| Error::Malformed)?
            .create_group_info(true)
            .build(self.mls.rand(), self.mls.crypto(), &self.mls_signer, |_| {
                true
            })
            .map_err(|_| Error::Malformed)?
            .stage_commit(&self.mls)
            .map_err(|_| Error::Malformed)?
            .into_messages();
        let commit = tls(&commit)?;
        let welcome = welcome.map(|w| tls(&w)).transpose()?;
        let info = tls(&info.ok_or(Error::Malformed)?)?;
        // The change log's entry for this commit (D-0156), when this device holds the key.
        let log = match line {
            Some(line) => self.log_entry(gid, from_epoch, &line)?,
            None => None,
        };
        let bundle = Bundle {
            commit: &commit,
            welcome: welcome.as_deref(),
            group_info: &info,
            log: log.as_deref(),
        }
        .encode();
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(&bundle);
        self.save_mls()?;
        let (status, _) = self.net.api("POST", "/v1/groups/commit", body).await?;
        if status == 200 {
            group
                .merge_pending_commit(&self.mls)
                .map_err(|_| Error::Storage)?;
            self.save_mls()
        } else {
            let _ = group.clear_pending_commit(self.mls.storage());
            self.save_mls()?;
            Err(Error::Server(status))
        }
    }

    /// Leaves a group: proposes this device's own removal (`SelfRemove`,
    /// `POST /v1/groups/propose`); the next member to commit removes it (§6.7). The group is
    /// forgotten here at once.
    pub async fn leave_group(&mut self, gid: &[u8; 16]) -> Result<()> {
        let mut group = self.load_group(gid)?;
        let msg = group
            .leave_group_via_self_remove(&self.mls, &self.mls_signer)
            .map_err(|_| Error::Malformed)?;
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(&tls(&msg)?);
        let (status, _) = self.net.api("POST", "/v1/groups/propose", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        let _ = group.delete(self.mls.storage());
        self.save_mls()
    }

    /// Sends files to a group: each encrypted and uploaded once for every other device in it.
    pub async fn send_group_files(
        &mut self,
        gid: &[u8; 16],
        files: Vec<(Vec<u8>, crate::attachments::FileMeta)>,
        text: &str,
    ) -> Result<[u8; 16]> {
        let fetches = self.group_devices(gid)?.len().saturating_sub(1).max(1);
        let mut pointers = Vec::with_capacity(files.len());
        let mut preview = String::new();
        for (bytes, meta) in &files {
            let (mut p, ct) = crate::attachments::encrypt(bytes, meta)?;
            let id = crate::attachments::upload(&self.net, &ct, fetches).await?;
            p.object_id = id.to_vec();
            p.uploaded_at_ms = now_ms();
            if preview.is_empty() {
                preview = crate::messenger::attachment_label(&p);
            }
            pointers.push(p);
        }
        let message = songbird_content::schema::Message {
            text: (!text.is_empty()).then(|| Text {
                body: text.into(),
                spans: Vec::new(),
            }),
            attachments: pointers,
            ..Default::default()
        };
        let data = DataMessage {
            group_id: gid.to_vec(),
            kind: Some(data_message::Kind::Message(message)),
            ..Default::default()
        };
        let shown = if text.is_empty() {
            preview
        } else {
            text.to_owned()
        };
        let id = self.send_group_data(gid, data, &shown).await?;
        let me = self.local.account;
        for (i, (bytes, _)) in files.iter().enumerate() {
            self.store
                .put_file(&me, &id, u32::try_from(i).unwrap_or(u32::MAX), bytes)?;
        }
        Ok(id)
    }

    /// The accounts' devices in a group, as this device sees them.
    pub fn group_devices(&self, gid: &[u8; 16]) -> Result<Vec<[u8; 16]>> {
        let group = MlsGroup::load(self.mls.storage(), &GroupId::from_slice(gid))
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NoSession)?;
        Ok(group
            .members()
            .filter_map(|m| <[u8; 16]>::try_from(m.credential.serialized_content()).ok())
            .collect())
    }

    /// The title this device knows for a group.
    pub fn group_title(&self, gid: &[u8; 16]) -> Result<String> {
        Ok(self
            .store
            .secret(&format!("group_title.{}", hex(gid)))?
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .unwrap_or_default())
    }

    /// The description this device knows for a group (empty: none).
    pub fn group_about(&self, gid: &[u8; 16]) -> Result<String> {
        Ok(self
            .store
            .secret(&format!("group_about.{}", hex(gid)))?
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .unwrap_or_default())
    }

    /// Sends a text to a group.
    pub async fn send_group_text(&mut self, gid: &[u8; 16], text: &str) -> Result<[u8; 16]> {
        let message = songbird_content::schema::Message {
            text: Some(Text {
                body: text.into(),
                spans: Vec::new(),
            }),
            ..Default::default()
        };
        let data = DataMessage {
            group_id: gid.to_vec(),
            kind: Some(data_message::Kind::Message(message)),
            ..Default::default()
        };
        self.send_group_data(gid, data, text).await
    }

    pub(crate) async fn send_group_data(
        &mut self,
        gid: &[u8; 16],
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
        let franking = *random::<32>()?;
        let (padded, commitment) =
            songbird_content::seal(&content, Some(franking)).map_err(|_| Error::Malformed)?;
        let mut payload = Zeroizing::new(Vec::with_capacity(16 + padded.len()));
        payload.extend_from_slice(&self.local.account);
        payload.extend_from_slice(&padded);
        let mut group = MlsGroup::load(self.mls.storage(), &GroupId::from_slice(gid))
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NoSession)?;
        let msg = group
            .create_message(&self.mls, &self.mls_signer, &payload)
            .map_err(|_| Error::Malformed)?;
        self.save_mls()?;
        let body = tls(&msg)?;
        if matches!(
            &content.body,
            Some(Body::Data(DataMessage {
                kind: Some(data_message::Kind::Message(_)),
                ..
            }))
        ) {
            let row = data_row(
                &content,
                content_data(&content)?,
                *gid,
                self.local.account,
                Status::Sending,
            )?;
            let mut row = row;
            row.text = preview.into();
            self.store.insert_message(&row, false)?;
        }
        let hints = SendHints {
            group: Some(*gid),
            commitment,
            ..SendHints::default()
        };
        if self.net.send_group(*random::<16>()?, hints, body).await? == SendStatus::Accepted {
            self.store.advance(&self.local.account, &id, Status::Sent)?;
            Ok(id)
        } else {
            self.store
                .advance(&self.local.account, &id, Status::Failed)?;
            Err(Error::Server(0))
        }
    }

    /// Sends any content to a group (a call signal): one MLS message, not stored as a message.
    pub(crate) async fn send_group_body(&mut self, gid: &[u8; 16], content: Content) -> Result<()> {
        let (padded, _) = songbird_content::seal(&content, None).map_err(|_| Error::Malformed)?;
        let mut payload = Zeroizing::new(Vec::with_capacity(16 + padded.len()));
        payload.extend_from_slice(&self.local.account);
        payload.extend_from_slice(&padded);
        let mut group = self.load_group(gid)?;
        let msg = group
            .create_message(&self.mls, &self.mls_signer, &payload)
            .map_err(|_| Error::Malformed)?;
        self.save_mls()?;
        let hints = SendHints {
            group: Some(*gid),
            ..SendHints::default()
        };
        if self
            .net
            .send_group(*random::<16>()?, hints, tls(&msg)?)
            .await?
            == SendStatus::Accepted
        {
            Ok(())
        } else {
            Err(Error::Server(0))
        }
    }

    /// Handles an MLS envelope: a Welcome, a commit or a group message.
    pub(crate) async fn receive_mls(
        &mut self,
        bytes: &[u8],
        stamp: Option<Stamp>,
    ) -> Result<Received> {
        let Ok(msg) = MlsMessageIn::tls_deserialize_exact(bytes) else {
            return Ok(Received::Nothing);
        };
        match msg.extract() {
            MlsMessageBodyIn::Welcome(w) => self.join(w).await,
            MlsMessageBodyIn::PublicMessage(m) => self.process(m.into(), None).await,
            MlsMessageBodyIn::PrivateMessage(m) => self.process(m.into(), stamp).await,
            _ => Ok(Received::Nothing),
        }
    }

    async fn join(&mut self, welcome: Welcome) -> Result<Received> {
        let Ok(processed) = ProcessedWelcome::new_from_welcome(&self.mls, &join_config(), welcome)
        else {
            return Ok(Received::Nothing);
        };
        let gid: [u8; 16] = processed
            .unverified_group_info()
            .group_id()
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        let mut body = vec![1];
        body.extend_from_slice(&gid);
        let (status, state) = self.net.api("POST", "/v1/groups/state", body).await?;
        if status != 200 {
            return Ok(Received::Nothing);
        }
        let mut r = crate::crypto::Reader::new(&state);
        let _epoch = r.u64()?;
        let _info = r.bytes32(4 << 20)?;
        let tree = RatchetTreeIn::tls_deserialize_exact(r.bytes32(4 << 20)?)
            .map_err(|_| Error::Malformed)?;
        let Ok(staged) = processed.into_staged_welcome(&self.mls, Some(tree)) else {
            return Ok(Received::Nothing);
        };
        // The group must name exactly the delivery service this device pins.
        let ds = self.ds_key().await?;
        if !songbird_groups::ds::extensions_allowed(staged.group_context(), Some(&ds)) {
            return Ok(Received::Nothing);
        }
        let joined = staged.into_group(&self.mls).map_err(|_| Error::Storage)?;
        self.save_mls()?;
        self.store.ensure_group_chat(&gid)?;
        self.warm_accounts(&gid, &joined).await;
        Ok(Received::Joined { group: gid })
    }

    /// The accounts of `devices` in group `gid` (D-0155): from this device's cache, else from
    /// the server's group state; every pair checked against the account's signed device list
    /// (a device no longer listed has no account: it is dead). Devices the server leaves out are
    /// left out here.
    pub(crate) async fn member_accounts(
        &mut self,
        gid: &[u8; 16],
        devices: &BTreeSet<[u8; 16]>,
    ) -> Result<HashMap<[u8; 16], [u8; 16]>> {
        let key = format!("group_accounts.{}", hex(gid));
        let mut cache: HashMap<[u8; 16], [u8; 16]> = self
            .store
            .secret(&key)?
            .map(|b| {
                b.as_chunks::<32>()
                    .0
                    .iter()
                    .filter_map(|c| {
                        let (d, a) = c.split_first_chunk::<16>()?;
                        Some((*d, <[u8; 16]>::try_from(a).ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if devices.iter().any(|d| !cache.contains_key(d)) {
            let mut body = vec![1];
            body.extend_from_slice(gid);
            let (status, state) = self.net.api("POST", "/v1/groups/state", body).await?;
            if status == 200 {
                for (d, a) in state_accounts(&state)? {
                    if devices.contains(&d) && !cache.contains_key(&d) {
                        cache.insert(d, a);
                    }
                }
            }
        }
        let mut out = HashMap::new();
        for d in devices {
            let Some(a) = cache.get(d).copied() else {
                continue;
            };
            let mut listed = self.store.devices(&a)?.iter().any(|(x, _)| x == d);
            if !listed {
                listed = self
                    .devices_of(&a, true)
                    .await
                    .is_ok_and(|v| v.iter().any(|(x, _)| x == d));
            }
            if listed {
                out.insert(*d, a);
            } else {
                cache.remove(d);
            }
        }
        let mut b = Vec::with_capacity(cache.len() * 32);
        for (d, a) in &cache {
            b.extend_from_slice(d);
            b.extend_from_slice(a);
        }
        self.store.put_secret(&key, &b)?;
        Ok(out)
    }

    /// The group's change-log key (D-0156), if this device has it.
    pub fn log_key(&self, gid: &[u8; 16]) -> Result<Option<Zeroizing<[u8; 32]>>> {
        Ok(self
            .store
            .secret(&format!("group_log_key.{}", hex(gid)))?
            .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
            .map(Zeroizing::new))
    }

    /// Hands the change log's key to the group (those just added or joined read it).
    async fn announce_log_key(
        &mut self,
        gid: &[u8; 16],
        kind: group_update::Kind,
        subjects: &[[u8; 16]],
    ) -> Result<()> {
        let Some(key) = self.log_key(gid)? else {
            return Ok(());
        };
        let update = songbird_content::schema::GroupUpdate {
            kind: kind.into(),
            actor: self.local.account.to_vec(),
            subjects: subjects.iter().map(|s| s.to_vec()).collect(),
            value: String::new(),
            log_key: key.to_vec(),
        };
        let data = DataMessage {
            group_id: gid.to_vec(),
            kind: Some(data_message::Kind::GroupUpdate(update)),
            ..Default::default()
        };
        self.send_group_data(gid, data, "").await.map(|_| ())
    }

    /// This commit's change-log entry: `nonce [12] | ChaCha20-Poly1305(log key, nonce,
    /// 1 | at_ms u64 | GroupUpdate, aad = "songbird.v1/change-log-entry\0" | group |
    /// from_epoch)`, signed by this device's leaf key as §6.7 says.
    fn log_entry(
        &self,
        gid: &[u8; 16],
        from_epoch: u64,
        line: &LogLine,
    ) -> Result<Option<Vec<u8>>> {
        use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
        use openmls_traits::signatures::Signer as _;
        use prost::Message as _;
        let Some(key) = self.log_key(gid)? else {
            return Ok(None);
        };
        let update = songbird_content::schema::GroupUpdate {
            kind: line.kind.into(),
            actor: self.local.account.to_vec(),
            subjects: line.subjects.iter().map(|s| s.to_vec()).collect(),
            value: String::new(),
            log_key: Vec::new(),
        };
        let mut plain = vec![1];
        plain.extend_from_slice(&now_ms().to_be_bytes());
        plain.extend_from_slice(&update.encode_to_vec());
        let sealing = LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, &*key).map_err(|_| Error::Malformed)?,
        );
        let nonce = random::<12>()?;
        sealing
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(*nonce),
                Aad::from(log_aad(gid, from_epoch)),
                &mut plain,
            )
            .map_err(|_| Error::Malformed)?;
        let mut ciphertext = nonce.to_vec();
        ciphertext.extend_from_slice(&plain);
        if ciphertext.len() > songbird_groups::wire::MAX_LOG_CIPHERTEXT {
            // A change naming more people than an entry holds goes unlogged.
            return Ok(None);
        }
        let content = songbird_groups::wire::log_signed_content(gid, from_epoch, &ciphertext);
        let sig = self
            .mls_signer
            .sign(&content)
            .map_err(|_| Error::Malformed)?;
        let sig: [u8; 64] = sig.as_slice().try_into().map_err(|_| Error::Malformed)?;
        Ok(Some(songbird_groups::wire::LogEntry::encode(
            &ciphertext,
            &sig,
        )))
    }

    /// The group's change log ("Member changes", §6.7), oldest first: what this device can open
    /// with the log key (entries from before it had the key, or written without one, are left
    /// out).
    pub async fn change_log(&mut self, gid: &[u8; 16]) -> Result<Vec<LoggedChange>> {
        use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
        use prost::Message as _;
        let Some(key) = self.log_key(gid)? else {
            return Ok(Vec::new());
        };
        let opening = LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, &*key).map_err(|_| Error::Malformed)?,
        );
        let mut out = Vec::new();
        let mut after = u64::MAX;
        loop {
            let mut body = vec![1];
            body.extend_from_slice(gid);
            body.extend_from_slice(&after.to_be_bytes());
            let (status, answer) = self.net.api("POST", "/v1/groups/log", body).await?;
            if status != 200 {
                return Err(Error::Server(status));
            }
            let mut r = crate::crypto::Reader::new(&answer);
            let n = r.u16()?;
            let mut last = None;
            for _ in 0..n {
                let epoch = r.u64()?;
                let len = usize::from(r.u16()?);
                let entry = r.take(len)?;
                last = Some(epoch);
                let Ok(e) = songbird_groups::wire::LogEntry::parse(entry) else {
                    continue;
                };
                let Some((nonce, sealed)) = e.ciphertext.split_first_chunk::<12>() else {
                    continue;
                };
                let mut buf = sealed.to_vec();
                let Ok(plain) = opening.open_in_place(
                    Nonce::assume_unique_for_key(*nonce),
                    Aad::from(log_aad(gid, epoch)),
                    &mut buf,
                ) else {
                    continue;
                };
                let Some((&1, rest)) = plain.split_first() else {
                    continue;
                };
                let Some((at, proto)) = rest.split_first_chunk::<8>() else {
                    continue;
                };
                let Ok(u) = songbird_content::schema::GroupUpdate::decode(proto) else {
                    continue;
                };
                let (Ok(actor), Ok(kind)) = (
                    <[u8; 16]>::try_from(u.actor.as_slice()),
                    group_update::Kind::try_from(u.kind),
                ) else {
                    continue;
                };
                out.push(LoggedChange {
                    epoch,
                    at_ms: u64::from_be_bytes(*at),
                    kind,
                    actor,
                    subjects: u
                        .subjects
                        .iter()
                        .filter_map(|s| <[u8; 16]>::try_from(s.as_slice()).ok())
                        .collect(),
                });
            }
            r.finish()?;
            // The server answers at most 200 at a time, oldest first.
            match last {
                Some(e) if n == 200 => after = e,
                _ => break,
            }
        }
        Ok(out)
    }

    /// Learns (and verifies) the accounts of every member device now; failures leave the cache
    /// as it was.
    pub(crate) async fn warm_accounts(&mut self, gid: &[u8; 16], group: &MlsGroup) {
        let devices: BTreeSet<[u8; 16]> = group
            .members()
            .filter_map(|m| <[u8; 16]>::try_from(m.credential.serialized_content()).ok())
            .collect();
        let _ = self.member_accounts(gid, &devices).await;
    }

    /// Whether a member's commit is one `committer`'s role allows (`roles::authorize`, the rule
    /// the server ran), in accounts this device verified.
    async fn commit_allowed(
        &mut self,
        gid: &[u8; 16],
        group: &MlsGroup,
        staged: &StagedCommit,
        committer: &[u8; 16],
    ) -> Result<bool> {
        let device_of = |c: &Credential| <[u8; 16]>::try_from(c.serialized_content()).ok();
        let leaves: HashMap<u32, [u8; 16]> = group
            .members()
            .filter_map(|m| Some((m.index.u32(), device_of(&m.credential)?)))
            .collect();
        let mut self_removed = BTreeSet::new();
        let mut server_removed = BTreeSet::new();
        let mut removed = BTreeSet::new();
        for q in staged.queued_proposals() {
            match (q.proposal(), q.sender()) {
                (Proposal::SelfRemove, Sender::Member(i)) => {
                    self_removed.insert(i.u32());
                    removed.insert(i.u32());
                }
                (Proposal::Remove(r), Sender::External(_)) => {
                    server_removed.insert(r.removed().u32());
                    removed.insert(r.removed().u32());
                }
                (Proposal::Remove(r), _) => {
                    removed.insert(r.removed().u32());
                }
                _ => {}
            }
        }
        let added: Vec<[u8; 16]> = staged
            .add_proposals()
            .filter_map(|a| device_of(a.add_proposal().key_package().leaf_node().credential()))
            .collect();
        let mut wanted: BTreeSet<[u8; 16]> = leaves.values().copied().collect();
        wanted.extend(added.iter().copied());
        wanted.insert(*committer);
        let accounts = self.member_accounts(gid, &wanted).await?;
        let Some(actor) = accounts.get(committer).copied() else {
            return Ok(false);
        };
        let acct = |d: &[u8; 16]| accounts.get(d).copied().map(AccountId::from_bytes);
        let mut c = songbird_groups::roles::Change::default();
        let mut removed_devices = BTreeSet::new();
        for idx in &removed {
            let Some(d) = leaves.get(idx) else {
                return Ok(false);
            };
            removed_devices.insert(*d);
            if let Some(a) = acct(d) {
                if server_removed.contains(idx) {
                    c.external.insert(a);
                } else if self_removed.contains(idx) {
                    c.left.insert(a);
                } else {
                    c.lost.insert(a);
                }
            }
        }
        for d in leaves.values() {
            if let Some(a) = acct(d) {
                c.before.insert(a);
                if !removed_devices.contains(d) {
                    c.after.insert(a);
                }
            }
        }
        for d in &added {
            if let Some(a) = acct(d) {
                c.gained.insert(a);
                c.after.insert(a);
            }
        }
        let (Ok(old), Ok(new)) = (
            songbird_groups::ds::roles_in(group.extensions()),
            songbird_groups::ds::roles_in(staged.group_context().extensions()),
        ) else {
            return Ok(false);
        };
        Ok(
            songbird_groups::roles::authorize(&AccountId::from_bytes(actor), &old, &new, &c)
                .is_ok(),
        )
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one match over the MLS message kinds, read top to bottom"
    )]
    async fn process(&mut self, msg: ProtocolMessage, stamp: Option<Stamp>) -> Result<Received> {
        let gid: [u8; 16] = msg
            .group_id()
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        let Some(mut group) = MlsGroup::load(self.mls.storage(), &GroupId::from_slice(&gid))
            .map_err(|_| Error::Storage)?
        else {
            return Ok(Received::Nothing);
        };
        let from_epoch = group.epoch().as_u64();
        let Ok(processed) = group.process_message(&self.mls, msg) else {
            return Ok(Received::Nothing);
        };
        let sender_device = Some(processed.credential().serialized_content().to_vec());
        let join = matches!(processed.sender(), Sender::NewMemberCommit);
        let aad = processed.aad().to_vec();
        match processed.into_content() {
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                // A join by link (an external commit): the rule and the link proof (§6.8); or a
                // member account's new device (§6.3 rule 5, D-0167), checked against the signed
                // device lists of the accounts in the group.
                let by_link = join && Self::join_allowed(&group, &staged, &aad, from_epoch);
                let own_device = if join && !by_link {
                    self.own_account_join(&gid, &group, &staged).await
                } else {
                    None
                };
                if join && !by_link && own_device.is_none() {
                    return Ok(Received::Nothing);
                }
                let ds = self
                    .store
                    .secret("ds_key")?
                    .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok());
                if !songbird_groups::ds::extensions_allowed(staged.group_context(), ds.as_ref()) {
                    return Ok(Received::Nothing);
                }
                // A member's commit must be one its committer's role allows (§6.6): checked
                // here as the server checked it, from accounts this device verified itself.
                // (A commit removing this device ends the group here whatever it holds: the
                // server merged it, and this device can follow the group no further.)
                let me = group.own_leaf_index().u32();
                let removes_me = staged.remove_proposals().any(|r| r.remove_proposal().removed().u32() == me)
                    || staged.queued_proposals().any(|q| {
                        matches!((q.proposal(), q.sender()), (Proposal::SelfRemove, Sender::Member(i)) if i.u32() == me)
                    });
                if !join && !removes_me {
                    let committer: Option<[u8; 16]> =
                        sender_device.as_deref().and_then(|d| d.try_into().ok());
                    let allowed = match committer {
                        Some(c) => self.commit_allowed(&gid, &group, &staged, &c).await?,
                        None => false,
                    };
                    if !allowed {
                        return Ok(Received::GroupRulesBroken { group: gid });
                    }
                }
                group
                    .merge_staged_commit(&self.mls, *staged)
                    .map_err(|_| Error::Storage)?;
                self.save_mls()?;
                // A commit that removed this device: the group is over for it.
                if !group.is_active() {
                    let _ = group.delete(self.mls.storage());
                    self.save_mls()?;
                    return Ok(Received::RemovedFromGroup { group: gid });
                }
                // Who is who now, while this device may still ask (a later commit removing a
                // member is checked from it).
                self.warm_accounts(&gid, &group).await;
                if let Some(account) = own_device {
                    // A member's new device: the same person, no line in the chat. This
                    // account's own devices hand it what it can't read from the tree (D-0171).
                    if account == self.local.account {
                        self.after_own_join(&gid).await;
                    }
                    return Ok(Received::System);
                }
                if join {
                    // Someone joined by a link: they get the change log's key.
                    let _ = self
                        .announce_log_key(&gid, group_update::Kind::MemberJoinedByLink, &[])
                        .await;
                    return Ok(Received::GroupUpdated {
                        group: gid,
                        title: String::new(),
                    });
                }
                Ok(Received::System)
            }
            // A member's own `SelfRemove` (leaving): kept, and committed by this device now
            // (no role is needed). The delivery service's removals need each device's account,
            // which a device does not learn from the tree: left to other members and the
            // server for now (D-0147).
            ProcessedMessageContent::ProposalMessage(p) => {
                if matches!(p.proposal(), Proposal::SelfRemove) {
                    group
                        .store_pending_proposal(self.mls.storage(), *p)
                        .map_err(|_| Error::Storage)?;
                    self.save_mls()?;
                    let _ = self.commit_pending(&gid).await;
                    return Ok(Received::GroupUpdated {
                        group: gid,
                        title: String::new(),
                    });
                }
                Ok(Received::Nothing)
            }
            ProcessedMessageContent::ApplicationMessage(app) => {
                self.save_mls()?;
                let bytes = Zeroizing::new(app.into_bytes());
                let (account, padded) = bytes.split_first_chunk::<16>().ok_or(Error::Malformed)?;
                // The device MLS names must be on the claimed account's verified list.
                let device: [u8; 16] = sender_device
                    .as_deref()
                    .and_then(|d| d.try_into().ok())
                    .ok_or(Error::Malformed)?;
                let account = *account;
                let mut listed = self
                    .store
                    .devices(&account)?
                    .iter()
                    .any(|(d, _)| *d == device);
                if !listed {
                    listed = self
                        .devices_of(&account, true)
                        .await
                        .is_ok_and(|v| v.iter().any(|(d, _)| *d == device));
                }
                if !listed {
                    return Ok(Received::Nothing);
                }
                // Only those the group's roles let send (announcement groups, channels).
                if let Ok(roles) = songbird_groups::ds::roles_in(group.extensions())
                    && !roles.may_send(&AccountId::from_bytes(account))
                {
                    return Ok(Received::Nothing);
                }
                let account = &account;
                if self.store.is_blocked(account)? {
                    return Ok(Received::Nothing);
                }
                let Ok(opened) = songbird_content::open(padded) else {
                    return Ok(Received::Nothing);
                };
                let content = opened.content.clone();
                // Call signals are not visible messages: not franked (§5.3).
                if let Some(Body::Call(c)) = &content.body {
                    return Ok(
                        crate::calls::signal_of(c, content.sent_at_ms, now_ms()).map_or(
                            Received::Nothing,
                            |(call, signal)| Received::Call {
                                account: *account,
                                group: Some(gid),
                                call,
                                signal,
                            },
                        ),
                    );
                }
                // Visible messages must be franked, and the stamp must open (§5.3).
                if stamp.is_none_or(|s| !opened.franking_matches(&s.commitment)) {
                    return Ok(Received::Nothing);
                }
                let Ok(data) = content_data(&content) else {
                    return Ok(Received::Nothing);
                };
                if data.group_id.as_slice() != gid.as_slice() {
                    return Ok(Received::Nothing);
                }
                if !matches!(data.kind, Some(data_message::Kind::GroupUpdate(_)))
                    && let Some(r) = self.apply_event(&gid, account, data)?
                {
                    return Ok(r);
                }
                if let Some(data_message::Kind::GroupUpdate(u)) = &data.kind {
                    // The change log's key, from a member (kept once; never replaced).
                    if u.log_key.len() == 32 && self.log_key(&gid)?.is_none() {
                        self.store
                            .put_secret(&format!("group_log_key.{}", hex(&gid)), &u.log_key)?;
                    }
                    if u.kind == i32::from(group_update::Kind::MembersAdded)
                        || u.kind == i32::from(group_update::Kind::MemberJoinedByLink)
                    {
                        return Ok(Received::GroupUpdated {
                            group: gid,
                            title: String::new(),
                        });
                    }
                    let kind = group_update::Kind::try_from(u.kind)
                        .unwrap_or(group_update::Kind::Unspecified);
                    // A new name or description only from those the roles let edit the group's
                    // info ("only admins can edit group info", §6.6).
                    if matches!(
                        kind,
                        group_update::Kind::TitleChanged | group_update::Kind::DescriptionChanged
                    ) && songbird_groups::ds::roles_in(group.extensions())
                        .is_ok_and(|r| !r.may_edit_info(&AccountId::from_bytes(*account)))
                    {
                        return Ok(Received::Nothing);
                    }
                    if kind == group_update::Kind::DescriptionChanged {
                        self.store.put_secret(
                            &format!("group_about.{}", hex(&gid)),
                            u.value.as_bytes(),
                        )?;
                        return Ok(Received::GroupUpdated {
                            group: gid,
                            title: String::new(),
                        });
                    }
                    if !u.value.is_empty() {
                        self.store.put_secret(
                            &format!("group_title.{}", hex(&gid)),
                            u.value.as_bytes(),
                        )?;
                    }
                    return Ok(Received::GroupUpdated {
                        group: gid,
                        title: u.value.clone(),
                    });
                }
                let row = data_row(&content, data, gid, *account, Status::Delivered)?;
                if self.store.insert_message(&row, true)? {
                    // What reporting it needs, exactly as received: a group's shared body may be
                    // reported by any member (§5.14).
                    if let (Some(st), Some(fk)) = (stamp, opened.franking_key) {
                        self.store.put_frank(
                            account,
                            &row.id,
                            &crate::messenger::stamp_bytes(&st),
                            &fk,
                            &opened.content_bytes,
                        )?;
                    }
                    Ok(Received::Message(row))
                } else {
                    Ok(Received::Nothing)
                }
            }
            _ => Ok(Received::Nothing),
        }
    }
}

fn content_data(c: &Content) -> Result<&DataMessage> {
    match &c.body {
        Some(Body::Data(d)) => Ok(d),
        _ => Err(Error::Malformed),
    }
}

/// What a commit changed, for its change-log entry.
pub(crate) struct LogLine {
    kind: group_update::Kind,
    subjects: Vec<[u8; 16]>,
}

impl LogLine {
    pub(crate) fn settings() -> Self {
        Self::new(group_update::Kind::SettingsChanged, &[])
    }

    pub(crate) fn of(kind: group_update::Kind, subjects: &[[u8; 16]]) -> Self {
        Self::new(kind, subjects)
    }

    fn new(kind: group_update::Kind, subjects: &[[u8; 16]]) -> Self {
        Self {
            kind,
            subjects: subjects.to_vec(),
        }
    }
}

/// One change in a group's change log, opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedChange {
    /// The epoch the commit was made from.
    pub epoch: u64,
    /// When the committer made it (its clock).
    pub at_ms: u64,
    /// What changed.
    pub kind: group_update::Kind,
    /// Who changed it.
    pub actor: [u8; 16],
    /// Whom it concerned.
    pub subjects: Vec<[u8; 16]>,
}

/// The associated data of a change-log entry's sealing.
fn log_aad(gid: &[u8; 16], from_epoch: u64) -> Vec<u8> {
    let label = b"songbird.v1/change-log-entry\0";
    let mut a = Vec::with_capacity(label.len() + 24);
    a.extend_from_slice(label);
    a.extend_from_slice(gid);
    a.extend_from_slice(&from_epoch.to_be_bytes());
    a
}

/// The member accounts a `/v1/groups/state` answer ends with (D-0155): after `epoch | info |
/// tree | meta_v | meta`, `n u16 | n × (device | account)`. An answer without them names none.
pub(crate) fn state_accounts(state: &[u8]) -> Result<Vec<([u8; 16], [u8; 16])>> {
    let mut r = crate::crypto::Reader::new(state);
    let _epoch = r.u64()?;
    let _info = r.bytes32(4 << 20)?;
    let _tree = r.bytes32(4 << 20)?;
    let _meta_v = r.u64()?;
    let _meta = r.bytes32(16 << 10)?;
    let Ok(n) = r.u16() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        out.push((r.array()?, r.array()?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capabilities_name_the_roles_extension() {
        let c = caps();
        assert!(
            c.extensions()
                .contains(&ExtensionType::Unknown(ROLES_EXTENSION))
        );
        assert!(c.ciphersuites().contains(&GROUP_CIPHERSUITE.into()));
    }
}
