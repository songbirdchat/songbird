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

use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::*;
use songbird_content::schema::{Content, DataMessage, Text, content::Body, data_message};
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

fn caps() -> Capabilities {
    Capabilities::new(
        None,
        Some(&[GROUP_CIPHERSUITE]),
        Some(&[ExtensionType::Unknown(ROLES_EXTENSION)]),
        Some(&[ProposalType::SelfRemove]),
        Some(&[CredentialType::Basic]),
    )
}

fn context_extensions(roles: &Roles, ds: &[u8; 32]) -> Result<Extensions<GroupContext>> {
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

fn join_config() -> MlsGroupJoinConfig {
    MlsGroupJoinConfig::builder()
        .wire_format_policy(PURE_PLAINTEXT_WIRE_FORMAT_POLICY)
        .max_past_epochs(1)
        .build()
}

fn tls<T: tls_codec::Serialize>(v: &T) -> Result<Vec<u8>> {
    v.tls_serialize_detached().map_err(|_| Error::Malformed)
}

impl<N: Net> Messenger<N> {
    fn credential(&self) -> CredentialWithKey {
        CredentialWithKey {
            credential: BasicCredential::new(self.local.device.to_vec()).into(),
            signature_key: self.mls_signer.public().into(),
        }
    }

    fn save_mls(&self) -> Result<()> {
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
    async fn ds_key(&mut self) -> Result<[u8; 32]> {
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
        let ds = self.ds_key().await?;
        let roles = Roles::new(AccountId::from_bytes(self.local.account), Policy::GROUP);
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
        if !members.is_empty() {
            self.add_members(&gid, members).await?;
        }
        // The title (and that the group exists) as a group update everyone reads.
        let update = songbird_content::schema::GroupUpdate {
            kind: songbird_content::schema::group_update::Kind::Created.into(),
            actor: self.local.account.to_vec(),
            subjects: members.iter().map(|m| m.to_vec()).collect(),
            value: title.into(),
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
        self.commit(gid, kps, Vec::new()).await
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
        self.commit(gid, Vec::new(), leaves).await
    }

    /// Commits proposals others queued (a member leaving): no role is needed (§6.7).
    pub async fn commit_pending(&mut self, gid: &[u8; 16]) -> Result<()> {
        let group = self.load_group(gid)?;
        if group.pending_proposals().next().is_none() {
            return Ok(());
        }
        self.commit(gid, Vec::new(), Vec::new()).await
    }

    fn load_group(&self, gid: &[u8; 16]) -> Result<MlsGroup> {
        MlsGroup::load(self.mls.storage(), &GroupId::from_slice(gid))
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NoSession)
    }

    /// One commit with adds and removals (and any queued proposals), sent and merged on 200.
    async fn commit(
        &mut self,
        gid: &[u8; 16],
        adds: Vec<KeyPackage>,
        removes: Vec<LeafNodeIndex>,
    ) -> Result<()> {
        let mut group = self.load_group(gid)?;
        let (commit, welcome, info) = group
            .commit_builder()
            .propose_adds(adds)
            .propose_removals(removes)
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
        let bundle = Bundle {
            commit: &commit,
            welcome: welcome.as_deref(),
            group_info: &info,
            log: None,
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

    async fn send_group_data(
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
        staged.into_group(&self.mls).map_err(|_| Error::Storage)?;
        self.save_mls()?;
        self.store.ensure_group_chat(&gid)?;
        Ok(Received::Joined { group: gid })
    }

    #[allow(clippy::too_many_lines, reason = "one match over the MLS message kinds, read top to bottom")]
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
        let Ok(processed) = group.process_message(&self.mls, msg) else {
            return Ok(Received::Nothing);
        };
        let sender_device = Some(processed.credential().serialized_content().to_vec());
        match processed.into_content() {
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                let ds = self
                    .store
                    .secret("ds_key")?
                    .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok());
                if !songbird_groups::ds::extensions_allowed(staged.group_context(), ds.as_ref()) {
                    return Ok(Received::Nothing);
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
                let account = &account;
                if self.store.is_blocked(account)? {
                    return Ok(Received::Nothing);
                }
                let Ok(opened) = songbird_content::open(padded) else {
                    return Ok(Received::Nothing);
                };
                if stamp.is_none_or(|s| !opened.franking_matches(&s.commitment)) {
                    return Ok(Received::Nothing);
                }
                let content = opened.content.clone();
                let Ok(data) = content_data(&content) else {
                    return Ok(Received::Nothing);
                };
                if data.group_id.as_slice() != gid.as_slice() {
                    return Ok(Received::Nothing);
                }
                if let Some(data_message::Kind::GroupUpdate(u)) = &data.kind {
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
