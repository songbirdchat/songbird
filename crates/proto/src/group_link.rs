//! Group invite links (PROTOCOL §6.8, D-0148): `https://songbird.chat/g#<base64url, no padding>`
//! of `handle [16] || secret [32]`. Both travel only in the fragment, which no server sees; the
//! app derives the link's `auth`, info and proof keys from the secret.

use crate::invite_link::{decode, encode};

const PREFIX: &str = "https://songbird.chat/g#";

/// A parsed group link.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupLink {
    /// The server's handle for the link.
    pub handle: [u8; 16],
    /// The link secret.
    pub secret: [u8; 32],
}

impl std::fmt::Debug for GroupLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GroupLink(..)")
    }
}

impl GroupLink {
    /// The link.
    #[must_use]
    pub fn to_link(&self) -> String {
        let mut b = Vec::with_capacity(48);
        b.extend_from_slice(&self.handle);
        b.extend_from_slice(&self.secret);
        format!("{PREFIX}{}", encode(&b))
    }

    /// Parses a link strictly.
    #[must_use]
    pub fn parse(link: &str) -> Option<Self> {
        if link.len() > 128 {
            return None;
        }
        let b = decode(link.strip_prefix(PREFIX)?)?;
        let (handle, rest) = b.split_first_chunk::<16>()?;
        let (secret, rest) = rest.split_first_chunk::<32>()?;
        rest.is_empty().then_some(Self {
            handle: *handle,
            secret: *secret,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn links_round_trip_strictly() {
        let l = GroupLink {
            handle: [1; 16],
            secret: [2; 32],
        };
        let s = l.to_link();
        assert_eq!(GroupLink::parse(&s).unwrap(), l);
        assert!(GroupLink::parse(&format!("{s}A")).is_none());
        assert!(GroupLink::parse("https://songbird.chat/u#AAAA").is_none());
    }
}
