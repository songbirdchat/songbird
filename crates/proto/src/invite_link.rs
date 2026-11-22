//! The "invite a friend" link (Prompt 11, M6; `docs/PROTOCOL.md` §14.9; D-0126 §12, D-0132).
//!
//! `https://songbird.chat/i#<base64url, no padding>` of:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 1 | version (1) |
//! | 1 | flags: bit 0, a registration code follows |
//! | 0 or 15 | the registration code (invite-only mode) |
//! | 16 | the inviter's account ID |
//! | 32 | the inviter's profile key |
//! | 32 | the fingerprint of the inviter's identity key |
//!
//! Everything is in the **fragment**, which browsers never send: opening the link tells no
//! server who invited whom. After sign-up the friend's app fetches the inviter's profile and
//! keys like any contact's, checks the fingerprint against what key transparency returns, and
//! opens the chat; the server sees an ordinary first message. The inviter's app counts its own
//! invites used (`POST /v1/invites/status`, by code hash): the server keeps no per-inviter count.

use songbird_types::AccountId;

use crate::registration::INVITE_LEN;

/// The link's prefix.
pub const PREFIX: &str = "https://songbird.chat/i#";
/// The format's version.
pub const VERSION: u8 = 1;
const HAS_CODE: u8 = 1;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// What an invite link carries.
#[derive(Clone, PartialEq, Eq)]
pub struct InviteLink {
    /// The registration code, in invite-only mode.
    pub code: Option<[u8; INVITE_LEN]>,
    /// The inviter.
    pub inviter: AccountId,
    /// The inviter's profile key (decrypts their name and avatar).
    pub profile_key: [u8; 32],
    /// The fingerprint of the inviter's identity key (checked against key transparency).
    pub fingerprint: [u8; 32],
}

impl std::fmt::Debug for InviteLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The code and the profile key are secrets.
        f.debug_struct("InviteLink")
            .field("has_code", &self.code.is_some())
            .finish_non_exhaustive()
    }
}

pub(crate) fn encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for chunk in b.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &x)| n | (u32::from(x) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            let v = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
            out.push(char::from(B64.get(v).copied().unwrap_or(b'A')));
        }
    }
    out
}

pub(crate) fn decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| {
        B64.iter()
            .position(|&a| a == c)
            .and_then(|p| u32::try_from(p).ok())
    };
    let s = s.as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = chunk.len() - 1;
        for i in 0..bytes {
            out.push(u8::try_from((n >> (16 - 8 * i)) & 0xff).ok()?);
        }
        // Canonical: the unused low bits of a short last chunk are zero.
        let unused = 24 - 8 * bytes;
        if chunk.len() < 4 && n & ((1 << unused) - 1) != 0 {
            return None;
        }
    }
    Some(out)
}

impl InviteLink {
    /// The link.
    #[must_use]
    pub fn to_link(&self) -> String {
        let mut b = Vec::with_capacity(2 + INVITE_LEN + 16 + 64);
        b.push(VERSION);
        b.push(if self.code.is_some() { HAS_CODE } else { 0 });
        if let Some(c) = &self.code {
            b.extend_from_slice(c);
        }
        b.extend_from_slice(self.inviter.as_bytes());
        b.extend_from_slice(&self.profile_key);
        b.extend_from_slice(&self.fingerprint);
        format!("{PREFIX}{}", encode(&b))
    }

    /// Parses a link, strictly (one version, known flags, exact length, canonical encoding).
    #[must_use]
    pub fn parse(link: &str) -> Option<Self> {
        if link.len() > 256 {
            return None;
        }
        let b = decode(link.strip_prefix(PREFIX)?)?;
        let (&[version, flags], rest) = b.split_first_chunk::<2>()?;
        if version != VERSION || flags & !HAS_CODE != 0 {
            return None;
        }
        let (code, rest) = if flags & HAS_CODE != 0 {
            let (c, r) = rest.split_first_chunk::<INVITE_LEN>()?;
            (Some(*c), r)
        } else {
            (None, rest)
        };
        let (inviter, rest) = rest.split_first_chunk::<16>()?;
        let (profile_key, rest) = rest.split_first_chunk::<32>()?;
        let (fingerprint, rest) = rest.split_first_chunk::<32>()?;
        if !rest.is_empty() {
            return None;
        }
        Some(Self {
            code,
            inviter: AccountId::from_bytes(*inviter),
            profile_key: *profile_key,
            fingerprint: *fingerprint,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    fn link(code: Option<[u8; INVITE_LEN]>) -> InviteLink {
        InviteLink {
            code,
            inviter: AccountId::random().unwrap(),
            profile_key: [7; 32],
            fingerprint: [9; 32],
        }
    }

    #[test]
    fn links_round_trip_and_keep_everything_in_the_fragment() {
        for l in [link(None), link(Some([3; INVITE_LEN]))] {
            let s = l.to_link();
            assert!(s.starts_with(PREFIX));
            let (before, after) = s.split_once('#').unwrap();
            assert_eq!(before, "https://songbird.chat/i");
            assert!(
                after
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            );
            assert_eq!(InviteLink::parse(&s).unwrap(), l);
            assert!(!format!("{l:?}").contains("7, 7"));
        }
    }

    #[test]
    fn base64url_matches_rfc_4648() {
        // RFC 4648 §10, unpadded, URL-safe alphabet.
        for (plain, enc) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), enc);
            assert_eq!(decode(enc).unwrap(), plain.as_bytes());
        }
        assert_eq!(encode(&[0xfb, 0xff]), "-_8");
        // Non-canonical trailing bits, a lone symbol, a foreign character.
        assert!(decode("Zh").is_none());
        assert!(decode("Z").is_none());
        assert!(decode("Zm9v+").is_none());
    }

    #[test]
    fn anything_else_is_refused() {
        let good = link(Some([1; INVITE_LEN])).to_link();
        let body = good.strip_prefix(PREFIX).unwrap();
        let mut raw = decode(body).unwrap();
        let reencode = |b: &[u8]| format!("{PREFIX}{}", encode(b));
        assert!(InviteLink::parse(&reencode(&raw)).is_some());
        let mut v2 = raw.clone();
        v2[0] = 2;
        let mut flag = raw.clone();
        flag[1] = 3;
        let mut nocode = raw.clone();
        nocode[1] = 0;
        let short = raw[..raw.len() - 1].to_vec();
        raw.push(0);
        for bad in [
            String::new(),
            format!("https://songbird.chat/i?{body}"),
            format!("http://songbird.chat/i#{body}"),
            reencode(&v2),
            reencode(&flag),
            reencode(&nocode),
            reencode(&short),
            reencode(&raw),
            format!("{good}{}", "A".repeat(300)),
        ] {
            assert!(InviteLink::parse(&bad).is_none(), "{bad}");
        }
    }
}
