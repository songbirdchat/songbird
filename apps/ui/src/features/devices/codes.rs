//! What the camera may read (D-0110): the link code a new device shows (PROTOCOL §4.6), a
//! contact's safety-number code, and the three codes of parental linking (PROTOCOL §10.6,
//! D-0107): the parent's, the child's, and the signed first policy. Each has its own scheme, so a code made to look like one is
//! never taken for the other, and anything else the camera sees is ignored.
//!
//! Codes are upper case so QR codes hold them in alphanumeric mode (smaller codes; version 5
//! for a link code). Parsing ignores case and is otherwise strict.

/// The scheme of a link code: `SONGBIRD:LINK:<channel hex>:<provisioning key hex>`.
pub const LINK: &str = "SONGBIRD:LINK:";
/// The scheme of a safety-number code: `SONGBIRD:SAFETY:<digits>`.
pub const SAFETY: &str = "SONGBIRD:SAFETY:";
/// The parent's first code: `SONGBIRD:SUPERVISE:<commitment hex>`, a commitment to the policy
/// key and the link secret, revealed later in the policy code (D-0108).
pub const SUPERVISE: &str = "SONGBIRD:SUPERVISE:";
/// The child's code: `SONGBIRD:CHILD:<account hex>:<unlink date, seconds>`.
pub const CHILD: &str = "SONGBIRD:CHILD:";
/// The parent's signed first policy and the link secret: `SONGBIRD:POLICY:<secret hex>:<policy
/// hex>` (the policy names the key).
pub const POLICY: &str = "SONGBIRD:POLICY:";
/// A child's request to approve a new contact (D-0166): `SONGBIRD:APPROVE:<contact account
/// hex>:<username>`, for the parent's phone to scan.
pub const APPROVE: &str = "SONGBIRD:APPROVE:";
/// The longest policy a code carries, in bytes (a policy with the most client rules).
pub const MAX_POLICY: usize = 1 + 16 + 32 + 8 + 1 + 8 + 2 + 512 + 64;
/// The longest channel ID a link code carries, in bytes (as the provisioning parser accepts).
pub const MAX_CHANNEL: usize = 64;

/// A code the camera read and the app understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scanned {
    /// A new device's link code: its channel and provisioning public key.
    Link {
        /// The provisioning channel ID.
        channel: Vec<u8>,
        /// The new device's X25519 provisioning public key.
        key: [u8; 32],
    },
    /// A contact's safety number, digits only.
    Safety(String),
    /// A parent's first code: the commitment to the policy key and the link secret.
    Supervise([u8; 32]),
    /// A child's code: their account and the date supervision ends (computed on their phone).
    Child {
        /// The child's account ID.
        account: [u8; 16],
        /// When supervision ends by itself (seconds since 1970).
        unlink_at_s: u64,
    },
    /// The parent's signed first policy, as bytes, and the link secret (both checked by the
    /// child's phone against the commitment it scanned first).
    Policy {
        /// The link secret.
        secret: [u8; 32],
        /// The encoded policy.
        policy: Vec<u8>,
    },
    /// A child's request to approve a contact: the contact's account and username.
    Approve {
        /// The contact's account ID.
        account: [u8; 16],
        /// The username the child typed.
        username: String,
    },
}

/// The code a child's phone shows to have a new contact approved.
#[must_use]
pub fn approve_code(account: &[u8; 16], username: &str) -> String {
    format!("{APPROVE}{}:{username}", hex_upper(account))
}

/// The link code a new device shows.
#[must_use]
pub fn link_code(channel: &[u8], key: &[u8; 32]) -> String {
    format!("{LINK}{}:{}", hex_upper(channel), hex_upper(key))
}

/// The code for a safety number (its digits; spaces and grouping dropped).
#[must_use]
pub fn safety_code(number: &str) -> String {
    let digits: String = number.chars().filter(char::is_ascii_digit).collect();
    format!("{SAFETY}{digits}")
}

/// The parent's first code (parental linking): the commitment only.
#[must_use]
pub fn supervise_code(commitment: &[u8; 32]) -> String {
    format!("{SUPERVISE}{}", hex_upper(commitment))
}

/// The child's code (parental linking).
#[must_use]
pub fn child_code(account: &[u8; 16], unlink_at_s: u64) -> String {
    format!("{CHILD}{}:{unlink_at_s}", hex_upper(account))
}

/// The code of a signed first policy and the link secret (parental linking).
#[must_use]
pub fn policy_code(secret: &[u8; 32], policy: &[u8]) -> String {
    format!("{POLICY}{}:{}", hex_upper(secret), hex_upper(policy))
}

/// What `text` is, if it is a code the app understands.
#[must_use]
pub fn parse(text: &str) -> Option<Scanned> {
    let text = text.trim();
    if let Some(rest) = strip_prefix_ignore_case(text, LINK) {
        let (channel, key) = rest.split_once(':')?;
        let channel = unhex(channel).filter(|c| !c.is_empty() && c.len() <= MAX_CHANNEL)?;
        let key: [u8; 32] = unhex(key)?.try_into().ok()?;
        return Some(Scanned::Link { channel, key });
    }
    if let Some(rest) = strip_prefix_ignore_case(text, SUPERVISE) {
        return Some(Scanned::Supervise(unhex(rest)?.try_into().ok()?));
    }
    if let Some(rest) = strip_prefix_ignore_case(text, CHILD) {
        let (account, date) = rest.split_once(':')?;
        let account: [u8; 16] = unhex(account)?.try_into().ok()?;
        if date.is_empty() || date.len() > 12 || !date.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        return Some(Scanned::Child {
            account,
            unlink_at_s: date.parse().ok()?,
        });
    }
    if let Some(rest) = strip_prefix_ignore_case(text, APPROVE) {
        let (account, username) = rest.split_once(':')?;
        let account: [u8; 16] = unhex(account)?.try_into().ok()?;
        let ok = !username.is_empty()
            && username.len() <= 64
            && username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
        return ok.then(|| Scanned::Approve {
            account,
            username: username.to_owned(),
        });
    }
    if let Some(rest) = strip_prefix_ignore_case(text, POLICY) {
        let (secret, policy) = rest.split_once(':')?;
        let secret: [u8; 32] = unhex(secret)?.try_into().ok()?;
        if policy.is_empty() || policy.len() > 2 * MAX_POLICY {
            return None;
        }
        return Some(Scanned::Policy {
            secret,
            policy: unhex_any(policy)?,
        });
    }
    if let Some(digits) = strip_prefix_ignore_case(text, SAFETY)
        && digits.len() == 60
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(Scanned::Safety(digits.to_owned()));
    }
    None
}

/// Whether a scanned safety code is the number shown for this contact.
#[must_use]
pub fn safety_matches(scanned: &str, number: &str) -> bool {
    let ours: String = number.chars().filter(char::is_ascii_digit).collect();
    !ours.is_empty() && scanned == ours
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() > 2 * MAX_CHANNEL.max(32) {
        return None;
    }
    unhex_any(text)
}

/// Hex of any length (callers bound it).
fn unhex_any(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16);
    let (pairs, _) = text.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .map(|&[hi, lo]| u8::try_from(digit(hi)? * 16 + digit(lo)?).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_code_round_trips_in_either_case() {
        let channel = [0xAB; 16];
        let key = [0x3C; 32];
        let code = link_code(&channel, &key);
        assert_eq!(
            code,
            format!("SONGBIRD:LINK:{}:{}", "AB".repeat(16), "3C".repeat(32))
        );
        let want = Some(Scanned::Link {
            channel: channel.to_vec(),
            key,
        });
        assert_eq!(parse(&code), want);
        assert_eq!(parse(&code.to_lowercase()), want);
        assert_eq!(parse(&format!("  {code}\n")), want);
    }

    #[test]
    fn anything_else_is_not_a_link() {
        let key = "3C".repeat(32);
        for bad in [
            format!("SONGBIRD:LINK::{key}"),                    // no channel
            format!("SONGBIRD:LINK:ABC:{key}"),                 // odd hex
            format!("SONGBIRD:LINK:AB:{}", "3C".repeat(31)),    // short key
            format!("SONGBIRD:LINK:AB:{key}00"),                // long key
            format!("SONGBIRD:LINK:{}:{key}", "AB".repeat(65)), // long channel
            format!("SONGBIRD:LINK:AB:{key}:extra"),            // a third field
            format!("SONGBIRD:LINK:ZZ:{key}"),                  // not hex
            format!("songbird-link:ab:{}", key.to_lowercase()), // the old scheme
            format!("https://songbird.chat/g/abc#SONGBIRD:LINK:AB:{key}"), // a link around it
            "SONGBIRD:LINK:".to_string(),
            "SONGBIRD:LIN".to_string(),
            "ſONGBIRD:LINK:AB:CD".to_string(), // a non-ASCII look-alike
        ] {
            assert_eq!(parse(&bad), None, "{bad}");
        }
    }

    #[test]
    fn safety_codes_carry_sixty_digits() {
        let number = "05219 47384 60128 57391 02647 38190 57382 91046 38572 91055 12345 67890";
        let code = safety_code(number);
        let Some(Scanned::Safety(digits)) = parse(&code) else {
            panic!("{code}")
        };
        assert!(safety_matches(&digits, number));
        assert!(!safety_matches(&digits, "1"));
        assert_eq!(parse("SONGBIRD:SAFETY:123"), None);
        assert_eq!(parse(&format!("{SAFETY}{}x", "1".repeat(59))), None);
    }

    #[test]
    fn parental_codes_round_trip_and_stay_apart() {
        let code = supervise_code(&[1; 32]);
        assert_eq!(
            parse(&code.to_lowercase()),
            Some(Scanned::Supervise([1; 32]))
        );
        let child = child_code(&[3; 16], 1_900_000_000);
        assert_eq!(
            parse(&child),
            Some(Scanned::Child {
                account: [3; 16],
                unlink_at_s: 1_900_000_000
            })
        );
        let policy = policy_code(&[2; 32], &[6; 200]);
        assert_eq!(
            parse(&policy),
            Some(Scanned::Policy {
                secret: [2; 32],
                policy: vec![6; 200]
            })
        );
        // None is ever read as a link code, nor the other way round.
        for code in [&code, &child, &policy] {
            assert!(!matches!(parse(code), Some(Scanned::Link { .. })), "{code}");
        }
        let link = link_code(&[4; 16], &[5; 32]);
        assert!(matches!(parse(&link), Some(Scanned::Link { .. })));
        for bad in [
            format!("{SUPERVISE}{}", "01".repeat(31)),
            format!("{SUPERVISE}{}:{}", "01".repeat(32), "02".repeat(32)),
            format!("{CHILD}{}:-1", "03".repeat(16)),
            format!("{CHILD}{}:1:x", "03".repeat(16)),
            format!("{CHILD}{}:", "03".repeat(16)),
            format!("{CHILD}{}:1234567890123", "03".repeat(16)),
            format!("{POLICY}"),
            format!("{POLICY}{}:", "02".repeat(32)),
            format!("{POLICY}{}:ABC", "02".repeat(32)),
            format!("{POLICY}{}", "00".repeat(40)),
            format!(
                "{POLICY}{}:{}",
                "02".repeat(32),
                "00".repeat(MAX_POLICY + 1)
            ),
        ] {
            assert_eq!(parse(&bad), None, "{bad}");
        }
    }

    #[test]
    fn approval_requests_parse_strictly() {
        let code = approve_code(&[0xab; 16], "ana.b_1");
        assert_eq!(
            parse(&code),
            Some(Scanned::Approve {
                account: [0xab; 16],
                username: "ana.b_1".into()
            })
        );
        assert_eq!(parse(&approve_code(&[0xab; 16], "a b")), None);
        assert_eq!(parse(&approve_code(&[0xab; 16], "")), None);
        assert_eq!(parse("SONGBIRD:APPROVE:ABCD:ana"), None);
    }
}
