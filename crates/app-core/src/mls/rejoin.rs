//! A new device of a member account joins the account's groups by external commit (PROTOCOL
//! §6.3 rule 5, §6.4; D-0167).
//!
//! The server lets a listed device of an account in the group read the group's state and
//! commit itself in, removing nothing. Every member's device follows such a join only when the
//! joiner is on the signed device list of an account that already has a device in the group
//! (checked here, from lists this device verifies), so the server cannot slip a device in under
//! someone's account.

use std::collections::BTreeSet;

use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::*;
use songbird_groups::wire::Bundle;
use songbird_types::DeviceId;

use crate::messenger::{Messenger, Net};
use crate::{Error, Result};

impl<N: Net> Messenger<N> {
    /// Whether this device is in group `gid`.
    pub fn in_group(&self, gid: &[u8; 16]) -> Result<bool> {
        Ok(
            MlsGroup::load(self.mls.storage(), &GroupId::from_slice(gid))
                .map_err(|_| Error::Storage)?
                .is_some_and(|g| g.is_active()),
        )
    }

    /// Joins a group this device's account is in, by external commit. Nothing to do when this
    /// device is in it already.
    ///
    /// # Errors
    /// [`Error::Server`] with 404 when the account isn't in the group (or it doesn't exist);
    /// [`Error::Verify`] when the group doesn't name this device's delivery service.
    pub async fn join_own_group(&mut self, gid: &[u8; 16]) -> Result<()> {
        if self.in_group(gid)? {
            return Ok(());
        }
        let mut body = vec![1];
        body.extend_from_slice(gid);
        let (status, state) = self.net.api("POST", "/v1/groups/state", body).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let mut r = crate::crypto::Reader::new(&state);
        let _epoch = r.u64()?;
        let info = r.bytes32(4 << 20)?.to_vec();
        let tree = r.bytes32(4 << 20)?.to_vec();
        let MlsMessageBodyIn::GroupInfo(gi) = MlsMessageIn::tls_deserialize_exact(&info)
            .map_err(|_| Error::Malformed)?
            .extract()
        else {
            return Err(Error::Malformed);
        };
        if gi.group_id().as_slice() != gid.as_slice() {
            return Err(Error::Verify);
        }
        let tree = RatchetTreeIn::tls_deserialize_exact(&tree).map_err(|_| Error::Malformed)?;
        let cred = CredentialWithKey {
            credential: BasicCredential::new(self.local.device.to_vec()).into(),
            signature_key: self.mls_signer.public().into(),
        };
        let (mut group, bundle) = MlsGroup::external_commit_builder()
            .with_ratchet_tree(tree)
            .with_config(super::client::join_config())
            .build_group(&self.mls, gi, cred)
            .map_err(|_| Error::Verify)?
            .leaf_node_parameters(
                LeafNodeParameters::builder()
                    .with_capabilities(super::client::caps())
                    .build(),
            )
            .load_psks(self.mls.storage())
            .map_err(|_| Error::Malformed)?
            .create_group_info(true)
            .build(self.mls.rand(), self.mls.crypto(), &self.mls_signer, |_| {
                true
            })
            .map_err(|_| Error::Malformed)?
            .finalize(&self.mls)
            .map_err(|_| Error::Malformed)?;
        // The group must name exactly the delivery service this device pins.
        let ds = self.ds_key().await?;
        if !songbird_groups::ds::context_extensions_allowed(group.extensions(), Some(&ds)) {
            let _ = group.delete(self.mls.storage());
            return Err(Error::Verify);
        }
        let (commit, _, info) = bundle.into_messages();
        let commit = super::client::tls(&commit)?;
        let info = super::client::tls(&info.ok_or(Error::Malformed)?)?;
        let bundle = Bundle {
            commit: &commit,
            welcome: None,
            group_info: &info,
            log: None,
        }
        .encode();
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(&bundle);
        let (status, _) = self.net.api("POST", "/v1/groups/commit", body).await?;
        if status != 200 {
            let _ = group.delete(self.mls.storage());
            self.save_mls()?;
            return Err(Error::Server(status));
        }
        self.save_mls()?;
        self.store.ensure_group_chat(gid)?;
        self.warm_accounts(gid, &group).await;
        Ok(())
    }

    /// Whether an external commit is a member account's new device joining (§6.3 rule 5): the
    /// structure the server checks (it adds only itself, removes nothing), and the joiner on
    /// the signed device list of an account that already has a device in the group.
    pub(crate) async fn own_account_join(
        &mut self,
        gid: &[u8; 16],
        group: &MlsGroup,
        staged: &StagedCommit,
    ) -> Option<[u8; 16]> {
        let leaf = staged.update_path_leaf_node()?;
        let Ok(device) = DeviceId::from_slice(leaf.credential().serialized_content()) else {
            return None;
        };
        let in_group = group
            .members()
            .any(|m| m.credential.serialized_content() == device.as_bytes());
        let removed = songbird_groups::ds::check_external_commit(
            staged,
            &device,
            |i| {
                group
                    .member(LeafNodeIndex::new(i))
                    .and_then(|c| DeviceId::from_slice(c.serialized_content()).ok())
            },
            None,
        )
        .ok();
        if in_group || removed.is_none_or(|r| !r.is_empty()) {
            return None;
        }
        let devices: BTreeSet<[u8; 16]> = group
            .members()
            .filter_map(|m| <[u8; 16]>::try_from(m.credential.serialized_content()).ok())
            .collect();
        let Ok(accounts) = self.member_accounts(gid, &devices).await else {
            return None;
        };
        let accounts: BTreeSet<[u8; 16]> = accounts.into_values().collect();
        for a in accounts {
            if self
                .devices_of(&a, true)
                .await
                .is_ok_and(|v| v.iter().any(|(d, _)| d == device.as_bytes()))
            {
                return Some(a);
            }
        }
        None
    }

    /// After this account's new device joined: hands it the change log's key and the group's
    /// title (a member's device has both; the tree carries neither), then removes this
    /// account's leaves whose devices it no longer lists (a reinstalled phone's old leaf).
    pub(crate) async fn after_own_join(&mut self, gid: &[u8; 16]) {
        use songbird_content::schema::{DataMessage, GroupUpdate, data_message, group_update};
        if let Ok(Some(key)) = self.log_key(gid) {
            let update = GroupUpdate {
                kind: group_update::Kind::DeviceJoined.into(),
                actor: self.local.account.to_vec(),
                subjects: Vec::new(),
                value: self.group_title(gid).unwrap_or_default(),
                log_key: key.to_vec(),
            };
            let data = DataMessage {
                group_id: gid.to_vec(),
                kind: Some(data_message::Kind::GroupUpdate(update)),
                ..Default::default()
            };
            let _ = self.send_group_data(gid, data, "").await;
        }
        let _ = self.prune_own_dead_leaves(gid).await;
    }

    /// Removes the leaves in `gid` whose devices no member account's signed device list names
    /// any more (a reinstalled or unlinked device's old leaf), this account's own list read
    /// fresh. Only lists this device verifies count, so the server can't make it remove a
    /// device someone still lists. Answers how many went.
    ///
    /// # Errors
    /// The server or the commit refused.
    pub async fn prune_own_dead_leaves(&mut self, gid: &[u8; 16]) -> Result<usize> {
        let me = self.local.account;
        let mut listed: BTreeSet<[u8; 16]> = self
            .devices_of(&me, true)
            .await?
            .into_iter()
            .map(|(d, _)| d)
            .collect();
        let group = self.load_group(gid)?;
        let devices: BTreeSet<[u8; 16]> = group
            .members()
            .filter_map(|m| <[u8; 16]>::try_from(m.credential.serialized_content()).ok())
            .collect();
        let accounts: BTreeSet<[u8; 16]> = self
            .member_accounts(gid, &devices)
            .await?
            .into_values()
            .filter(|a| *a != me)
            .collect();
        for a in accounts {
            // A fresh read: a device another account just linked must not look dead.
            for (d, _) in self.devices_of(&a, true).await? {
                listed.insert(d);
            }
        }
        let own = self.local.device;
        let dead: Vec<LeafNodeIndex> = group
            .members()
            .filter(|m| {
                <[u8; 16]>::try_from(m.credential.serialized_content())
                    .is_ok_and(|d| !listed.contains(&d) && d != own)
            })
            .map(|m| m.index)
            .collect();
        if dead.is_empty() {
            return Ok(0);
        }
        let n = dead.len();
        self.commit(gid, Vec::new(), dead, None).await?;
        Ok(n)
    }
}
