//! Group invite links and join requests (PROTOCOL §6.8, D-0148).
//!
//! A link is a random handle and a 32-byte secret, both only in the link's fragment. From the
//! secret: `auth` (shown to the server to open or use the link; it keeps `SHA-256(auth)`) and
//! the `proof` key, whose public half the link's creator commits into the group's roles before
//! sharing the link. A joiner signs its external commit with the proof key; members and the
//! server refuse a join without a valid proof, so a server cannot admit someone who never held
//! a link the members made.

use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::*;
use songbird_groups::wire::Bundle;
use songbird_proto::group_link::GroupLink;
use songbird_types::DeviceId;
use zeroize::Zeroizing;

use super::crypto::GROUP_CIPHERSUITE;
use crate::crypto::{hkdf, random};
use crate::messenger::{Messenger, Net, Received, hex};
use crate::{Error, Result};

/// What opening a link found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// This device joined the group.
    Joined([u8; 16]),
    /// Joining needs an admin's approval: a request was sent.
    Requested,
}

fn derive(secret: &[u8; 32], info: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut out = Zeroizing::new([0u8; 32]);
    hkdf(b"songbird.v1/group-link", secret, &[info], out.as_mut())?;
    Ok(out)
}

fn proof_key(secret: &[u8; 32]) -> Result<aws_lc_rs::signature::Ed25519KeyPair> {
    aws_lc_rs::signature::Ed25519KeyPair::from_seed_unchecked(derive(secret, b"proof")?.as_ref())
        .map_err(|_| Error::Malformed)
}

fn link_head(l: &GroupLink) -> Result<Vec<u8>> {
    let mut b = vec![1];
    b.extend_from_slice(&l.handle);
    b.extend_from_slice(derive(&l.secret, b"auth")?.as_ref());
    Ok(b)
}

impl<N: Net> Messenger<N> {
    /// Makes an invite link for a group: publishes its proof key in the roles (a commit), then
    /// registers it (`POST /v1/groups/links`). `approval`: joins wait for an admin.
    pub async fn create_link(&mut self, gid: &[u8; 16], approval: bool) -> Result<String> {
        self.create_link_with(gid, approval, 0, 0).await
    }

    /// [`Self::create_link`] with limits the server enforces: the link stops working after
    /// `ttl_s` seconds (0: never, at most a year) or `max_uses` joins (0: any) (§6.8).
    pub async fn create_link_with(
        &mut self,
        gid: &[u8; 16],
        approval: bool,
        ttl_s: u64,
        max_uses: u32,
    ) -> Result<String> {
        use aws_lc_rs::signature::KeyPair as _;
        let link = GroupLink {
            handle: *random::<16>()?,
            secret: *random::<32>()?,
        };
        let pk: [u8; 32] = proof_key(&link.secret)?
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        let group = self.load_group(gid)?;
        let mut roles =
            songbird_groups::ds::roles_in(group.extensions()).map_err(|_| Error::Malformed)?;
        roles.links.insert(link.handle, pk);
        self.commit_roles(gid, &roles).await?;
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(&link.handle);
        body.extend_from_slice(&crate::crypto::sha256(&[
            derive(&link.secret, b"auth")?.as_ref()
        ]));
        body.extend_from_slice(&ttl_s.to_be_bytes());
        body.extend_from_slice(&max_uses.to_be_bytes());
        body.push(u8::from(approval));
        let (status, _) = self.net.api("POST", "/v1/groups/links", body).await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        let url = link.to_link();
        self.store
            .put_secret(&format!("group_link.{}", hex(gid)), url.as_bytes())?;
        // Its limits, to show them: when it expires (ms, 0: never), how many may join, approval.
        let expires = if ttl_s == 0 {
            0
        } else {
            crate::messenger::now_ms().saturating_add(ttl_s.saturating_mul(1000))
        };
        let mut opts = expires.to_be_bytes().to_vec();
        opts.extend_from_slice(&max_uses.to_be_bytes());
        opts.push(u8::from(approval));
        self.store
            .put_secret(&format!("group_link_opts.{}", hex(gid)), &opts)?;
        Ok(url)
    }

    /// The limits of the link this device made for a group: (expires at ms, 0: never; most
    /// joins, 0: any; joins wait for approval).
    pub fn group_link_limits(&self, gid: &[u8; 16]) -> Result<Option<(u64, u32, bool)>> {
        Ok(self
            .store
            .secret(&format!("group_link_opts.{}", hex(gid)))?
            .and_then(|v| {
                let e = u64::from_be_bytes(v.get(..8)?.try_into().ok()?);
                let n = u32::from_be_bytes(v.get(8..12)?.try_into().ok()?);
                Some((e, n, v.get(12) == Some(&1)))
            }))
    }

    /// The group's roles as this device sees them: its policy and every account whose role is
    /// not the default.
    pub fn group_roles(&self, gid: &[u8; 16]) -> Result<songbird_groups::roles::Roles> {
        let group = self.load_group(gid)?;
        songbird_groups::ds::roles_in(group.extensions()).map_err(|_| Error::Malformed)
    }

    /// Gives `account` another role in a group (a commit every member checks against the
    /// roles rule, §6.6: an admin who outranks both roles, or an owner).
    pub async fn set_role(
        &mut self,
        gid: &[u8; 16],
        account: &[u8; 16],
        role: songbird_groups::roles::Role,
    ) -> Result<()> {
        let mut roles = self.group_roles(gid)?;
        let who = songbird_types::AccountId::from_bytes(*account);
        if role == roles.policy.default_role {
            roles.explicit.remove(&who);
        } else {
            roles.explicit.insert(who, role);
        }
        let line = super::client::LogLine::of(
            songbird_content::schema::group_update::Kind::RoleChanged,
            &[*account],
        );
        self.commit_roles_logged(gid, &roles, line).await
    }

    /// Changes a group's policy ("only admins can send", "only admins can edit group info",
    /// approving new members): a commit only admins may make (§6.6).
    pub async fn set_policy(
        &mut self,
        gid: &[u8; 16],
        change: impl FnOnce(&mut songbird_groups::roles::Policy),
    ) -> Result<()> {
        let mut roles = self.group_roles(gid)?;
        let before = roles.policy;
        change(&mut roles.policy);
        if roles.policy == before {
            return Ok(());
        }
        self.commit_roles(gid, &roles).await
    }

    /// Renames a group or changes its description: a group update every member shows, from
    /// those its roles let edit the info (checked here and by every receiver).
    pub async fn set_group_info(
        &mut self,
        gid: &[u8; 16],
        title: Option<&str>,
        about: Option<&str>,
    ) -> Result<()> {
        use songbird_content::schema::{DataMessage, GroupUpdate, data_message, group_update};
        let me = songbird_types::AccountId::from_bytes(self.local.account);
        if !self.group_roles(gid)?.may_edit_info(&me) {
            return Err(Error::Server(403));
        }
        let changes = [
            (title, group_update::Kind::TitleChanged, "group_title"),
            (about, group_update::Kind::DescriptionChanged, "group_about"),
        ];
        for (value, kind, key) in changes {
            let Some(value) = value else { continue };
            let update = GroupUpdate {
                kind: kind.into(),
                actor: self.local.account.to_vec(),
                subjects: Vec::new(),
                value: value.into(),
                log_key: Vec::new(),
            };
            let data = DataMessage {
                group_id: gid.to_vec(),
                kind: Some(data_message::Kind::GroupUpdate(update)),
                ..Default::default()
            };
            self.send_group_data(gid, data, "").await?;
            self.store
                .put_secret(&format!("{key}.{}", hex(gid)), value.as_bytes())?;
        }
        Ok(())
    }

    /// The link this device made for a group, if any.
    pub fn group_link(&self, gid: &[u8; 16]) -> Result<Option<String>> {
        Ok(self
            .store
            .secret(&format!("group_link.{}", hex(gid)))?
            .map(|v| String::from_utf8_lossy(&v).into_owned()))
    }

    /// Revokes a link: the server stops honouring it and its proof key leaves the roles.
    pub async fn revoke_link(&mut self, gid: &[u8; 16], url: &str) -> Result<()> {
        let link = GroupLink::parse(url).ok_or(Error::Malformed)?;
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(&link.handle);
        let (status, _) = self
            .net
            .api("POST", "/v1/groups/links/revoke", body)
            .await?;
        if status != 204 && status != 200 {
            return Err(Error::Server(status));
        }
        let group = self.load_group(gid)?;
        let mut roles =
            songbird_groups::ds::roles_in(group.extensions()).map_err(|_| Error::Malformed)?;
        if roles.links.remove(&link.handle).is_some() {
            self.commit_roles(gid, &roles).await?;
        }
        self.store
            .put_secret(&format!("group_link.{}", hex(gid)), b"")
    }

    async fn commit_roles(
        &mut self,
        gid: &[u8; 16],
        roles: &songbird_groups::roles::Roles,
    ) -> Result<()> {
        // A link's proof key or the policy in the roles: a settings change in the log.
        self.commit_roles_logged(gid, roles, super::client::LogLine::settings())
            .await
    }

    async fn commit_roles_logged(
        &mut self,
        gid: &[u8; 16],
        roles: &songbird_groups::roles::Roles,
        line: super::client::LogLine,
    ) -> Result<()> {
        let ds = self
            .store
            .secret("ds_key")?
            .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
            .ok_or(Error::NoSession)?;
        let exts = super::client::context_extensions(roles, &ds)?;
        self.commit_with(gid, Vec::new(), Vec::new(), Some(exts), Some(line))
            .await
    }

    /// Opens a link someone shared: joins (external commit with the link proof) or, where joins
    /// need approval, asks to join.
    #[allow(
        clippy::too_many_lines,
        reason = "one join, step by step as §6.8 lists it"
    )]
    pub async fn open_link(&mut self, url: &str) -> Result<Opened> {
        let link = GroupLink::parse(url.trim()).ok_or(Error::Malformed)?;
        let (status, answer) = self
            .net
            .api("POST", "/v1/groups/links/open", link_head(&link)?)
            .await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let mut r = crate::crypto::Reader::new(&answer);
        match r.u8()? {
            1 => {
                let (status, _) = self
                    .net
                    .api("POST", "/v1/groups/requests", link_head(&link)?)
                    .await?;
                return if status == 204 {
                    Ok(Opened::Requested)
                } else {
                    Err(Error::Server(status))
                };
            }
            0 => {}
            _ => return Err(Error::Malformed),
        }
        let gid: [u8; 16] = r.array()?;
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
        let epoch = gi.epoch().as_u64();
        let key = proof_key(&link.secret)?;
        let content = songbird_groups::links::proof_content(
            &gid,
            epoch,
            &link.handle,
            &DeviceId::from_bytes(self.local.device),
            self.mls_signer.public(),
        );
        let sig: [u8; 64] = key
            .sign(&content)
            .as_ref()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        let aad = songbird_groups::links::LinkProof::encode(&link.handle, &sig);
        let tree = RatchetTreeIn::tls_deserialize_exact(&tree).map_err(|_| Error::Malformed)?;
        let cred = CredentialWithKey {
            credential: BasicCredential::new(self.local.device.to_vec()).into(),
            signature_key: self.mls_signer.public().into(),
        };
        let (mut group, bundle) = MlsGroup::external_commit_builder()
            .with_aad(aad)
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
        let mut body = link_head(&link)?;
        body.extend_from_slice(&bundle);
        let (status, _) = self.net.api("POST", "/v1/groups/join", body).await?;
        if status != 200 {
            let _ = group.delete(self.mls.storage());
            self.save_mls()?;
            return Err(Error::Server(status));
        }
        self.save_mls()?;
        self.store.ensure_group_chat(&gid)?;
        Ok(Opened::Joined(gid))
    }

    /// Join requests waiting for a group (admins): (account, seconds waiting), oldest first.
    pub async fn join_requests(&mut self, gid: &[u8; 16]) -> Result<Vec<([u8; 16], u64)>> {
        let mut body = vec![1];
        body.extend_from_slice(gid);
        let (status, answer) = self
            .net
            .api("POST", "/v1/groups/requests/list", body)
            .await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let mut r = crate::crypto::Reader::new(&answer);
        let n = r.u16()?;
        let mut out = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            out.push((r.array()?, r.u64()?));
        }
        r.finish()?;
        Ok(out)
    }

    /// Approves a request: adds the account's devices (the commit removes the request).
    pub async fn approve_request(&mut self, gid: &[u8; 16], account: &[u8; 16]) -> Result<()> {
        self.add_members(gid, &[*account]).await
    }

    /// Declines a request.
    pub async fn decline_request(&mut self, gid: &[u8; 16], account: &[u8; 16]) -> Result<()> {
        let mut body = vec![1];
        body.extend_from_slice(gid);
        body.extend_from_slice(account);
        let (status, _) = self
            .net
            .api("POST", "/v1/groups/requests/decline", body)
            .await?;
        if status == 204 || status == 200 {
            Ok(())
        } else {
            Err(Error::Server(status))
        }
    }

    /// Checks another device's join (an external commit) before following it (§6.8): the
    /// structural rule the server applies (D-0078), and a valid link proof under a key the group
    /// published, unless the joiner only replaces its own old leaf.
    pub(crate) fn join_allowed(
        group: &MlsGroup,
        staged: &StagedCommit,
        aad: &[u8],
        from_epoch: u64,
    ) -> bool {
        let Some(leaf) = staged.update_path_leaf_node() else {
            return false;
        };
        let Ok(device) = DeviceId::from_slice(leaf.credential().serialized_content()) else {
            return false;
        };
        let device_at = |i: u32| {
            group
                .member(LeafNodeIndex::new(i))
                .and_then(|c| DeviceId::from_slice(c.serialized_content()).ok())
        };
        let joiner_leaf = group
            .members()
            .find(|m| m.credential.serialized_content() == device.as_bytes())
            .map(|m| m.index.u32());
        if songbird_groups::ds::check_external_commit(staged, &device, device_at, joiner_leaf)
            .is_err()
        {
            return false;
        }
        if joiner_leaf.is_some() {
            return true;
        }
        let (Ok(roles), Ok(gid)) = (
            songbird_groups::ds::roles_in(group.extensions()),
            <[u8; 16]>::try_from(group.group_id().as_slice()),
        ) else {
            return false;
        };
        songbird_groups::links::verify(
            &roles,
            &gid,
            from_epoch,
            aad,
            &device,
            leaf.signature_key().as_slice(),
            None,
        )
        .is_ok()
    }
}

#[allow(dead_code, reason = "the suite is named for the reader")]
const SUITE: Ciphersuite = GROUP_CIPHERSUITE;

/// What `Received` a link join produces for members.
#[must_use]
pub fn joined_by_link(group: [u8; 16]) -> Received {
    Received::GroupUpdated {
        group,
        title: String::new(),
    }
}
