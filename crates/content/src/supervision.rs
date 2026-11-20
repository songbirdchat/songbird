//! Parental controls, the apps' side (PROTOCOL §10.6; Prompt 9, M4, D-0105).
//!
//! **Linking happens in person.** The parent's device makes a fresh Ed25519 policy key for this
//! child and a 32-byte link secret, and shows them as a QR code; the child's device scans it and
//! shows its own (the child account). Both screens then show the same [`link_code`], which the
//! parent and child compare and confirm. The child's primary uploads the first policy, signed by
//! the parent (`PUT /v1/supervision`).
//!
//! **What the server sees** is the policy key (not the parent's account), the three rules it
//! enforces, a version and the unlink date. The rules only apps enforce ([`ClientRules`]: calls
//! from non-contacts, nudges, quiet hours, approving each new contact) travel encrypted under the
//! link secret, which only the two devices hold.
//!
//! **Unlinking** needs the parent's signature, or happens by itself on the date set at linking:
//! the child's device computes it from the age the child enters ([`unlink_at`]) and the country's
//! age of digital consent ([`consent_age`], GDPR Article 8); the birth date never leaves the
//! device. A supervised child may still delete their own account (the owner's decision, D-0101).
//!
//! **The parent never reads messages, sees calls or browses contacts**: the policy is the only
//! thing the parent writes, and the child's app shows every rule in force.

use aws_lc_rs::aead::{AES_256_GCM_SIV, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use aws_lc_rs::{digest, hkdf};
use prost::Message as _;
use zeroize::Zeroizing;

use crate::Error;

/// The age of digital consent (GDPR Article 8) in each EU member state, by ISO 3166-1 alpha-2
/// code (`docs/THREAT_MODEL.md` §5 lists their sources and which are still to confirm against the
/// national law). The Regulation's default is 16.
pub const CONSENT_AGES: [(&str, u8); 27] = [
    ("AT", 14),
    ("BE", 13),
    ("BG", 14),
    ("CY", 14),
    ("CZ", 15),
    ("DE", 16),
    ("DK", 13),
    ("EE", 13),
    ("ES", 14),
    ("FI", 13),
    ("FR", 15),
    ("GR", 15),
    ("HR", 16),
    ("HU", 16),
    ("IE", 16),
    ("IT", 14),
    ("LT", 14),
    ("LU", 16),
    ("LV", 13),
    ("MT", 13),
    ("NL", 16),
    ("PL", 16),
    ("PT", 13),
    ("RO", 16),
    ("SE", 13),
    ("SI", 15),
    ("SK", 16),
];

/// The age of digital consent for a country; 16 (the Regulation's default) for any other.
#[must_use]
pub fn consent_age(country: &str) -> u8 {
    CONSENT_AGES
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(country))
        .map_or(16, |(_, a)| *a)
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// When supervision ends by itself: the first day of the month after the child turns `age`,
/// from the birth year and month the child entered on their own device (seconds since the epoch,
/// UTC). `None` for a month out of range.
#[must_use]
pub fn unlink_at(birth_year: i32, birth_month: u32, age: u8) -> Option<u64> {
    if !(1..=12).contains(&birth_month) {
        return None;
    }
    let (y, m) = if birth_month == 12 {
        (i64::from(birth_year) + i64::from(age) + 1, 1)
    } else {
        (i64::from(birth_year) + i64::from(age), birth_month + 1)
    };
    u64::try_from(days_from_civil(y, m, 1) * 86_400).ok()
}

/// The rules only apps enforce, encrypted under the link secret.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClientRules {
    /// Every new contact needs the parent's approval (with the server's "no requests").
    #[prost(bool, tag = "1")]
    pub approved_contacts_only: bool,
    /// Calls from accounts that are not contacts are declined.
    #[prost(bool, tag = "2")]
    pub block_unknown_calls: bool,
    /// Nudges are off.
    #[prost(bool, tag = "3")]
    pub nudges_off: bool,
    /// Quiet hours: minutes after midnight, local time; equal means none.
    #[prost(uint32, tag = "4")]
    pub quiet_from_min: u32,
    /// End of quiet hours.
    #[prost(uint32, tag = "5")]
    pub quiet_to_min: u32,
}

impl ClientRules {
    /// Whether `minute` (after midnight) is inside quiet hours.
    #[must_use]
    pub fn quiet(&self, minute: u32) -> bool {
        let (f, t) = (self.quiet_from_min, self.quiet_to_min);
        match f.cmp(&t) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Less => (f..t).contains(&minute),
            std::cmp::Ordering::Greater => minute >= f || minute < t,
        }
    }
}

fn rules_key(link_secret: &[u8; 32]) -> Result<LessSafeKey, Error> {
    let mut k = Zeroizing::new([0u8; 32]);
    hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird.v1/supervision")
        .extract(link_secret)
        .expand(&[b"client-rules"], hkdf::HKDF_SHA256)
        .and_then(|o| o.fill(k.as_mut()))
        .map_err(|_| Error::Crypto)?;
    Ok(LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM_SIV, k.as_ref()).map_err(|_| Error::Crypto)?,
    ))
}

fn rules_aad(child: &[u8; 16], version: u64) -> Vec<u8> {
    let mut a = b"songbird.v1/client-rules\0".to_vec();
    a.extend_from_slice(child);
    a.extend_from_slice(&version.to_be_bytes());
    a
}

/// Client rules are padded to this many bytes before encryption (`be16(len) ‖ protobuf ‖
/// zeros`), so the stored policy doesn't show how many rules are on (D-0108).
pub const RULES_PADDED: usize = 64;

/// Encrypts client rules for the policy `version` of `child`, padded to [`RULES_PADDED`].
///
/// # Errors
/// [`Error::Crypto`].
pub fn seal_rules(
    link_secret: &[u8; 32],
    child: &[u8; 16],
    version: u64,
    rules: &ClientRules,
) -> Result<Vec<u8>, Error> {
    let mut nonce = [0u8; NONCE_LEN];
    aws_lc_rs::rand::fill(&mut nonce).map_err(|_| Error::Crypto)?;
    let proto = rules.encode_to_vec();
    let len = u16::try_from(proto.len())
        .ok()
        .filter(|n| usize::from(*n) + 2 <= RULES_PADDED)
        .ok_or(Error::Crypto)?;
    let mut buf = Vec::with_capacity(RULES_PADDED + 16);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&proto);
    buf.resize(RULES_PADDED, 0);
    rules_key(link_secret)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(rules_aad(child, version)),
            &mut buf,
        )
        .map_err(|_| Error::Crypto)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&buf);
    Ok(out)
}

/// Opens client rules.
///
/// # Errors
/// [`Error::Crypto`] for another secret, child or version, or a changed byte.
pub fn open_rules(
    link_secret: &[u8; 32],
    child: &[u8; 16],
    version: u64,
    blob: &[u8],
) -> Result<ClientRules, Error> {
    let (nonce, ct) = blob.split_first_chunk::<NONCE_LEN>().ok_or(Error::Crypto)?;
    let mut buf = ct.to_vec();
    let plain = rules_key(link_secret)?
        .open_in_place(
            Nonce::assume_unique_for_key(*nonce),
            Aad::from(rules_aad(child, version)),
            &mut buf,
        )
        .map_err(|_| Error::Crypto)?;
    let (len, rest) = plain.split_first_chunk::<2>().ok_or(Error::Decode)?;
    let body = rest
        .get(..usize::from(u16::from_be_bytes(*len)))
        .ok_or(Error::Decode)?;
    ClientRules::decode(body).map_err(|_| Error::Decode)
}

/// The parent's policy key for one child.
#[derive(Debug)]
pub struct PolicyKey(Ed25519KeyPair);

impl PolicyKey {
    /// A fresh key.
    ///
    /// # Errors
    /// [`Error::Crypto`].
    pub fn generate() -> Result<Self, Error> {
        let mut seed = Zeroizing::new([0u8; 32]);
        aws_lc_rs::rand::fill(seed.as_mut()).map_err(|_| Error::Crypto)?;
        Self::from_seed(&seed)
    }

    /// From its seed (the parent's app keeps it).
    ///
    /// # Errors
    /// [`Error::Crypto`].
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, Error> {
        Ed25519KeyPair::from_seed_unchecked(seed)
            .map(Self)
            .map_err(|_| Error::Crypto)
    }

    /// The public key (in the policy).
    #[must_use]
    pub fn public(&self) -> [u8; 32] {
        let mut k = [0u8; 32];
        k.copy_from_slice(self.0.public_key().as_ref());
        k
    }

    /// Signs a policy in place.
    pub fn sign(&self, p: &mut songbird_proto::supervision::Policy) {
        p.policy_key = self.public();
        let sig = self.0.sign(&p.signed_bytes());
        p.signature.copy_from_slice(sig.as_ref());
    }

    /// Approves adding `child` to `group` until `expires_s`: the `Group-Add-Approval` value
    /// (hex of these bytes).
    #[must_use]
    pub fn approve(&self, group: &[u8; 16], child: &[u8; 16], expires_s: u64) -> Vec<u8> {
        let child = songbird_types::AccountId::from_bytes(*child);
        let sig = self.0.sign(&songbird_proto::supervision::approval_bytes(
            group, &child, expires_s,
        ));
        let mut out = expires_s.to_be_bytes().to_vec();
        out.extend_from_slice(sig.as_ref());
        out
    }
}

/// The commitment the parent's first code carries (D-0108): its key and secret are revealed only
/// after the child's code is fixed, so nobody relaying the codes can pick values that make two
/// different links show the same six digits.
#[must_use]
pub fn link_commit(policy_key: &[u8; 32], link_secret: &[u8; 32]) -> [u8; 32] {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"songbird.v1/link-commit\0");
    ctx.update(policy_key);
    ctx.update(link_secret);
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

/// The six-digit code both devices show at linking, from both QR codes' contents: the parent
/// and child compare it, so nobody in between can swap a key.
#[must_use]
pub fn link_code(policy_key: &[u8; 32], link_secret: &[u8; 32], child: &[u8; 16]) -> String {
    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"songbird.v1/link-code\0");
    ctx.update(policy_key);
    ctx.update(link_secret);
    ctx.update(child);
    let d = ctx.finish();
    let mut n = [0u8; 4];
    n.copy_from_slice(d.as_ref().get(..4).unwrap_or(&[0; 4]));
    format!("{:06}", u32::from_be_bytes(n) % 1_000_000)
}

/// The eight digits a parent's device shows to approve one new contact for the child (D-0166):
/// an HMAC under a key from the link secret over the child and the contact, so only the two
/// linked devices can make or check it, and a code for one contact never approves another.
/// Shown in person; nothing goes through the server.
///
/// # Errors
/// [`Error::Crypto`] if the key can't be derived (it can).
pub fn approval_code(
    link_secret: &[u8; 32],
    child: &[u8; 16],
    contact: &[u8; 16],
) -> Result<String, Error> {
    use aws_lc_rs::hmac;
    let mut k = Zeroizing::new([0u8; 32]);
    hkdf::Salt::new(hkdf::HKDF_SHA256, b"songbird.v1/supervision")
        .extract(link_secret)
        .expand(&[b"contact-approval"], hkdf::HKDF_SHA256)
        .and_then(|o| o.fill(k.as_mut()))
        .map_err(|_| Error::Crypto)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, k.as_ref());
    let mut msg = b"songbird.v1/contact-approval\0".to_vec();
    msg.extend_from_slice(child);
    msg.extend_from_slice(contact);
    let tag = hmac::sign(&key, &msg);
    let mut n = [0u8; 8];
    n.copy_from_slice(tag.as_ref().get(..8).ok_or(Error::Crypto)?);
    Ok(format!("{:08}", u64::from_be_bytes(n) % 100_000_000))
}

/// Whether `typed` (spaces ignored) is the approval code for this contact, in constant time.
#[must_use]
pub fn approval_matches(
    link_secret: &[u8; 32],
    child: &[u8; 16],
    contact: &[u8; 16],
    typed: &str,
) -> bool {
    let typed: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
    approval_code(link_secret, child, contact).is_ok_and(|want| {
        typed.len() == want.len()
            && aws_lc_rs::constant_time::verify_slices_are_equal(typed.as_bytes(), want.as_bytes())
                .is_ok()
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;
    use songbird_proto::supervision::{HIDDEN, Policy};

    #[test]
    fn an_approval_code_is_for_one_contact_and_one_link() {
        let (secret, child) = ([1u8; 32], [2u8; 16]);
        let code = approval_code(&secret, &child, &[3; 16]).unwrap();
        assert_eq!(code.len(), 8);
        assert!(approval_matches(
            &secret,
            &child,
            &[3; 16],
            &format!("{} {}", &code[..4], &code[4..])
        ));
        assert!(!approval_matches(&secret, &child, &[4; 16], &code));
        assert!(!approval_matches(&[9; 32], &child, &[3; 16], &code));
        assert!(!approval_matches(&secret, &child, &[3; 16], ""));
    }

    #[test]
    fn every_member_state_has_an_age_between_13_and_16() {
        assert_eq!(CONSENT_AGES.len(), 27);
        assert!(CONSENT_AGES.iter().all(|(_, a)| (13..=16).contains(a)));
        assert_eq!(consent_age("ro"), 16);
        assert_eq!(consent_age("XX"), 16);
    }

    #[test]
    fn supervision_ends_the_month_after_the_birthday() {
        // Born March 2014, age 16: ends 1 April 2030.
        assert_eq!(unlink_at(2014, 3, 16), Some(1_901_232_000));
        // Born December 2012, age 13: ends 1 January 2026.
        assert_eq!(unlink_at(2012, 12, 13), Some(1_767_225_600));
        assert_eq!(unlink_at(2012, 13, 13), None);
    }

    #[test]
    fn client_rules_open_only_for_their_child_and_version() {
        let r = ClientRules {
            approved_contacts_only: true,
            block_unknown_calls: true,
            nudges_off: true,
            quiet_from_min: 21 * 60,
            quiet_to_min: 7 * 60,
        };
        let b = seal_rules(&[1; 32], &[2; 16], 3, &r).unwrap();
        assert_eq!(open_rules(&[1; 32], &[2; 16], 3, &b).unwrap(), r);
        assert!(open_rules(&[1; 32], &[2; 16], 4, &b).is_err());
        assert!(open_rules(&[9; 32], &[2; 16], 3, &b).is_err());
        assert!(r.quiet(22 * 60) && r.quiet(60) && !r.quiet(12 * 60));
        // Padded: no rules on and every rule on seal to the same length.
        let none = seal_rules(&[1; 32], &[2; 16], 3, &ClientRules::default()).unwrap();
        assert_eq!(none.len(), b.len());
        assert_eq!(b.len(), 12 + RULES_PADDED + 16);
        assert_eq!(
            open_rules(&[1; 32], &[2; 16], 3, &none).unwrap(),
            ClientRules::default()
        );
    }

    #[test]
    fn the_commitment_binds_the_key_and_the_secret() {
        let c = link_commit(&[1; 32], &[2; 32]);
        assert_eq!(c, link_commit(&[1; 32], &[2; 32]));
        assert_ne!(c, link_commit(&[3; 32], &[2; 32]));
        assert_ne!(c, link_commit(&[1; 32], &[3; 32]));
    }

    #[test]
    fn policies_and_approvals_verify_against_the_policy_key() {
        let k = PolicyKey::generate().unwrap();
        let mut p = Policy {
            child: songbird_types::AccountId::from_bytes([3; 16]),
            policy_key: [0; 32],
            policy_version: 1,
            flags: HIDDEN,
            unlink_at_s: 2_000_000_000,
            client_rules: Vec::new(),
            signature: [0; 64],
        };
        k.sign(&mut p);
        assert!(songbird_keys::ed25519_verify(
            &k.public(),
            &p.signed_bytes(),
            &p.signature
        ));
        let a = k.approve(&[4; 16], &[3; 16], 99);
        let (e, sig) = songbird_proto::supervision::split_approval(&a).unwrap();
        assert!(songbird_keys::ed25519_verify(
            &k.public(),
            &songbird_proto::supervision::approval_bytes(
                &[4; 16],
                &songbird_types::AccountId::from_bytes([3; 16]),
                e
            ),
            &sig
        ));
        assert_eq!(link_code(&[1; 32], &[2; 32], &[3; 16]).len(), 6);
    }
}
