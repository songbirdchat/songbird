//! Key transparency and security notices on the device (PROTOCOL §4.9, D-0168).
//!
//! - **The log's keys** (`GET /v1/kt/keys`) are pinned the first time this device reads them;
//!   other keys later are refused (and noticed as a fork).
//! - **Signed heads** are checked (Ed25519 by the log key over `"songbird.v1/kt-head\0" ‖ …`)
//!   and must only move forward: the same epoch with another root, or an older epoch than one
//!   already seen, is evidence the server shows different logs to different people.
//! - **Lookups**: whenever this device accepts a newer signed device list (a contact's or its
//!   own), it checks that the log publishes exactly that list: akd's `lookup_verify` against a
//!   checked head, and the value `HMAC(salt, "songbird.v1/kt-value\0" ‖ 2 ‖ SHA-256(list))`.
//!   A list the log hasn't published yet is checked again with the next one.
//! - **Notices**: new devices on a known account, a list the log contradicts, and a fork are
//!   queued here for the app to show ([`Messenger::take_notices`]).
//!
//! - **Audits** (D-0171): when the newest head moves forward, the device checks akd's
//!   append-only proof from the head it kept to the new one, with every epoch's root in between
//!   (`POST /v1/kt/roots`), whose two ends must be the signed heads' roots. A proof that fails is
//!   a fork.
//! - **The log's keys** come with release builds (`SONGBIRD_KT_LOG_KEY`, `SONGBIRD_KT_VRF_KEY`
//!   at build time); a development build without them trusts the first keys it reads.

use akd::proto::specs::types as pb;
use protobuf::Message as _;

use crate::messenger::{Messenger, Net, hex, json, unhex};
use crate::{Error, Result};

/// The log's keys this build trusts (hex), set at build time for releases.
const SHIPPED_LOG_KEY: Option<&str> = option_env!("SONGBIRD_KT_LOG_KEY");
const SHIPPED_VRF_KEY: Option<&str> = option_env!("SONGBIRD_KT_VRF_KEY");
/// The most epochs one audit request spans (the server's limit).
const AUDIT_SPAN: u64 = 1_000;

/// The directory's kind code for device lists.
const DEVICE_LIST: u8 = 2;
const NOTICES: &str = "security_notices";

/// Something the user should know about the people they talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// `account` (this device's own, or a contact's) has `count` new devices.
    NewDevices { account: [u8; 16], count: u32 },
    /// The log doesn't vouch for `account`'s device list as the server served it.
    ListNotInLog { account: [u8; 16] },
    /// Two signed heads that can't both be true: the server shows different logs.
    Fork,
}

impl Notice {
    fn encode(self) -> [u8; 21] {
        let mut b = [0u8; 21];
        match self {
            Self::NewDevices { account, count } => {
                b[0] = 1;
                b[1..17].copy_from_slice(&account);
                b[17..].copy_from_slice(&count.to_be_bytes());
            }
            Self::ListNotInLog { account } => {
                b[0] = 2;
                b[1..17].copy_from_slice(&account);
            }
            Self::Fork => b[0] = 3,
        }
        b
    }

    fn decode(b: &[u8; 21]) -> Option<Self> {
        let account: [u8; 16] = b[1..17].try_into().ok()?;
        let count = u32::from_be_bytes(b[17..].try_into().ok()?);
        match b[0] {
            1 => Some(Self::NewDevices { account, count }),
            2 => Some(Self::ListNotInLog { account }),
            3 => Some(Self::Fork),
            _ => None,
        }
    }
}

/// A signed head: `version u8 (1) | epoch u64 | root [32] | time_ms u64 | signature [64]`.
fn open_head(b: &[u8], log: &[u8; 32]) -> Result<(u64, [u8; 32])> {
    if b.len() != 1 + 8 + 32 + 8 + 64 || b.first() != Some(&1) {
        return Err(Error::Malformed);
    }
    let (signed, sig) = b.split_at(1 + 8 + 32 + 8);
    let sig: [u8; 64] = sig.try_into().map_err(|_| Error::Malformed)?;
    let mut msg = b"songbird.v1/kt-head\0".to_vec();
    msg.extend_from_slice(signed);
    if !songbird_keys::ed25519_verify(log, &msg, &sig) {
        return Err(Error::Verify);
    }
    let epoch = u64::from_be_bytes(
        signed
            .get(1..9)
            .and_then(|x| x.try_into().ok())
            .ok_or(Error::Malformed)?,
    );
    let root: [u8; 32] = signed
        .get(9..41)
        .and_then(|x| x.try_into().ok())
        .ok_or(Error::Malformed)?;
    Ok((epoch, root))
}

fn value(salt: &[u8; 32], kind: u8, raw: &[u8; 32]) -> Vec<u8> {
    use aws_lc_rs::hmac;
    let k = hmac::Key::new(hmac::HMAC_SHA256, salt);
    let mut ctx = hmac::Context::with_key(&k);
    ctx.update(b"songbird.v1/kt-value\0");
    ctx.update(&[kind]);
    ctx.update(raw);
    ctx.sign().as_ref().to_vec()
}

impl<N: Net> Messenger<N> {
    /// Queues a notice for the app.
    pub(crate) fn notice(&self, n: Notice) -> Result<()> {
        let mut all = self
            .store
            .secret(NOTICES)?
            .map(|v| v.to_vec())
            .unwrap_or_default();
        // A bounded queue: the newest 256.
        if all.len() >= 21 * 256 {
            all.drain(..21);
        }
        all.extend_from_slice(&n.encode());
        self.store.put_secret(NOTICES, &all)
    }

    /// The notices queued since the last call, oldest first.
    pub fn take_notices(&self) -> Result<Vec<Notice>> {
        let all = self.store.secret(NOTICES)?;
        self.store.put_secret(NOTICES, &[])?;
        Ok(all
            .map(|v| {
                v.as_chunks::<21>()
                    .0
                    .iter()
                    .filter_map(Notice::decode)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The log's (VRF key, log key): the build's, or in a development build without them, the
    /// first ones read (pinned).
    async fn kt_keys(&mut self) -> Result<(Vec<u8>, [u8; 32])> {
        if let (Some(log), Some(vrf)) = (SHIPPED_LOG_KEY, SHIPPED_VRF_KEY) {
            return Ok((crate::messenger::unhex_vec(vrf)?, unhex(log)?));
        }
        if !cfg!(debug_assertions) {
            // A release build without its keys checks nothing rather than trusting the server.
            return Err(Error::Verify);
        }
        if let Some(k) = self.store.secret("kt_keys")?
            && k.len() > 32
        {
            let (log, vrf) = k.split_at(32);
            return Ok((vrf.to_vec(), log.try_into().map_err(|_| Error::Malformed)?));
        }
        let (status, b) = self.net.api("GET", "/v1/kt/keys", Vec::new()).await?;
        if status != 200 {
            return Err(Error::Server(status));
        }
        let v = json(&b)?;
        let s = |k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let log: [u8; 32] = unhex(s("log"))?;
        let vrf = crate::messenger::unhex_vec(s("vrf"))?;
        let mut keep = log.to_vec();
        keep.extend_from_slice(&vrf);
        self.store.put_secret("kt_keys", &keep)?;
        Ok((vrf, log))
    }

    /// Checks a head and keeps the newest: an older epoch, or the same epoch with another
    /// root, is a fork; a newer one must follow from the kept one by an audit proof.
    async fn accept_head(&mut self, epoch: u64, root: [u8; 32]) -> Result<()> {
        if let Some(h) = self.store.secret("kt_head")?
            && h.len() == 40
        {
            let seen = u64::from_be_bytes(
                h.get(..8)
                    .and_then(|x| x.try_into().ok())
                    .ok_or(Error::Malformed)?,
            );
            let seen_root: [u8; 32] = h
                .get(8..)
                .and_then(|x| x.try_into().ok())
                .ok_or(Error::Malformed)?;
            if epoch < seen || (epoch == seen && root != seen_root) {
                self.notice(Notice::Fork)?;
                return Err(Error::Verify);
            }
            if epoch == seen {
                return Ok(());
            }
            match self.audit(seen, seen_root, epoch, root).await {
                Ok(()) => {}
                Err(Error::Verify) => {
                    self.notice(Notice::Fork)?;
                    return Err(Error::Verify);
                }
                // Not checkable now (offline, throttled): the old head stays, checked next time.
                Err(e) => return Err(e),
            }
        }
        let mut b = epoch.to_be_bytes().to_vec();
        b.extend_from_slice(&root);
        self.store.put_secret("kt_head", &b)
    }

    /// Checks that epoch `to` (root `to_root`) extends epoch `from` (root `from_root`): akd's
    /// append-only proofs over every epoch's root, a span at a time.
    async fn audit(
        &mut self,
        from: u64,
        from_root: [u8; 32],
        to: u64,
        to_root: [u8; 32],
    ) -> Result<()> {
        let mut start = from;
        let mut start_root = from_root;
        while start < to {
            let end = to.min(start + AUDIT_SPAN);
            let body = serde_json::json!({ "from": start, "to": end })
                .to_string()
                .into_bytes();
            let (status, b) = self.net.api("POST", "/v1/kt/roots", body.clone()).await?;
            if status != 200 {
                return Err(Error::Server(status));
            }
            let v = json(&b)?;
            let roots: Vec<[u8; 32]> = v
                .get("roots")
                .and_then(serde_json::Value::as_array)
                .ok_or(Error::Malformed)?
                .iter()
                .map(|r| unhex::<32>(r.as_str().unwrap_or_default()))
                .collect::<Result<_>>()?;
            let want_end = if end == to { Some(to_root) } else { None };
            if roots.first() != Some(&start_root)
                || want_end.is_some_and(|r| roots.last() != Some(&r))
                || u64::try_from(roots.len()).ok() != Some(end - start + 1)
            {
                return Err(Error::Verify);
            }
            let (status, b) = self.net.api("POST", "/v1/kt/audit", body).await?;
            if status != 200 {
                return Err(Error::Server(status));
            }
            let v = json(&b)?;
            let proof = crate::messenger::unhex_vec(
                v.get("proof")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )?;
            if proof.len() > 64 << 20 {
                return Err(Error::Malformed);
            }
            let proof =
                pb::AppendOnlyProof::parse_from_bytes(&proof).map_err(|_| Error::Malformed)?;
            let proof = akd::AppendOnlyProof::try_from(&proof).map_err(|_| Error::Malformed)?;
            akd::auditor::audit_verify::<akd::WhatsAppV1Configuration>(roots.clone(), proof)
                .await
                .map_err(|_| Error::Verify)?;
            start_root = *roots.last().ok_or(Error::Malformed)?;
            start = end;
        }
        Ok(())
    }

    /// Checks that the log publishes exactly `list` (its signed bytes) as `account`'s device
    /// list. `Ok(false)`: not in the log yet (an epoch away); a contradiction is noticed.
    pub async fn kt_check_list(&mut self, account: &[u8; 16], list: &[u8]) -> Result<bool> {
        let (vrf, log) = self.kt_keys().await?;
        let body = serde_json::json!({ "account": hex(account), "kind": "device_list" })
            .to_string()
            .into_bytes();
        let (status, b) = self.net.api("POST", "/v1/kt/lookup", body).await?;
        if status == 404 {
            return Ok(false);
        }
        if status != 200 {
            return Err(Error::Server(status));
        }
        let v = json(&b)?;
        let s = |k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        };
        let salt: [u8; 32] = unhex(s("salt"))?;
        let head = crate::messenger::unhex_vec(s("head"))?;
        let proof = crate::messenger::unhex_vec(s("proof"))?;
        let (epoch, root) = open_head(&head, &log)?;
        self.accept_head(epoch, root).await?;
        if proof.len() > 4 << 20 {
            return Err(Error::Malformed);
        }
        let proof = pb::LookupProof::parse_from_bytes(&proof).map_err(|_| Error::Malformed)?;
        let proof = akd::LookupProof::try_from(&proof).map_err(|_| Error::Malformed)?;
        let mut label = vec![DEVICE_LIST];
        label.extend_from_slice(account);
        let r = akd::client::lookup_verify::<akd::WhatsAppV1Configuration>(
            &vrf,
            root,
            epoch,
            akd::AkdLabel(label),
            proof,
        )
        .map_err(|_| Error::Verify)?;
        let raw: [u8; 32] = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, list)
            .as_ref()
            .try_into()
            .map_err(|_| Error::Malformed)?;
        if r.value.0 == value(&salt, DEVICE_LIST, &raw) {
            return Ok(true);
        }
        // The log holds another list: either this one is newer than the log's epoch (checked
        // again next time), or the server served a list the log doesn't vouch for.
        let pending = self
            .store
            .secret(&format!("kt_pending.{}", hex(account)))?
            .is_some_and(|p| p.as_slice() == raw);
        if pending {
            self.notice(Notice::ListNotInLog { account: *account })?;
        } else {
            self.store
                .put_secret(&format!("kt_pending.{}", hex(account)), &raw)?;
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
    use super::*;

    #[test]
    fn notices_round_trip_and_heads_are_checked() {
        for n in [
            Notice::NewDevices {
                account: [7; 16],
                count: 2,
            },
            Notice::ListNotInLog { account: [8; 16] },
            Notice::Fork,
        ] {
            assert_eq!(Notice::decode(&n.encode()), Some(n));
        }
        let key = aws_lc_rs::signature::Ed25519KeyPair::generate().unwrap();
        let log: [u8; 32] = aws_lc_rs::signature::KeyPair::public_key(&key)
            .as_ref()
            .try_into()
            .unwrap();
        let mut head = vec![1];
        head.extend_from_slice(&5u64.to_be_bytes());
        head.extend_from_slice(&[9; 32]);
        head.extend_from_slice(&1u64.to_be_bytes());
        let mut msg = b"songbird.v1/kt-head\0".to_vec();
        msg.extend_from_slice(&head);
        head.extend_from_slice(key.sign(&msg).as_ref());
        assert_eq!(open_head(&head, &log).unwrap(), (5, [9; 32]));
        let mut bad = head.clone();
        bad[9] ^= 1;
        assert_eq!(open_head(&bad, &log), Err(Error::Verify));
    }
}
