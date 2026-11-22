//! Username links (PROTOCOL §4.3, D-0140): `https://songbird.chat/u#<base64url, no padding>` of
//! `handle [16] || key [32]`. The handle names the ciphertext the server keeps
//! (`GET /v1/username/link/<handle>`); the key, which never reaches a server (it is in the
//! fragment), opens it to the username. Resetting the link uploads a new ciphertext under a new
//! key and handle; the old link stops working.

use crate::invite_link::{decode, encode};

const PREFIX: &str = "https://songbird.chat/u#";

/// A parsed username link.
#[derive(Clone, PartialEq, Eq)]
pub struct UsernameLink {
    /// The server's handle for the ciphertext.
    pub handle: [u8; 16],
    /// The key that opens it.
    pub key: [u8; 32],
}

impl std::fmt::Debug for UsernameLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UsernameLink(..)")
    }
}

impl UsernameLink {
    /// The link.
    #[must_use]
    pub fn to_link(&self) -> String {
        let mut b = Vec::with_capacity(48);
        b.extend_from_slice(&self.handle);
        b.extend_from_slice(&self.key);
        format!("{PREFIX}{}", encode(&b))
    }

    /// Parses a link strictly (exact length, canonical encoding).
    #[must_use]
    pub fn parse(link: &str) -> Option<Self> {
        if link.len() > 128 {
            return None;
        }
        let b = decode(link.strip_prefix(PREFIX)?)?;
        let (handle, rest) = b.split_first_chunk::<16>()?;
        let (key, rest) = rest.split_first_chunk::<32>()?;
        rest.is_empty().then_some(Self {
            handle: *handle,
            key: *key,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn links_round_trip_strictly() {
        let l = UsernameLink {
            handle: [1; 16],
            key: [2; 32],
        };
        let s = l.to_link();
        assert!(s.starts_with(PREFIX));
        assert_eq!(UsernameLink::parse(&s).unwrap(), l);
        assert!(UsernameLink::parse(&format!("{s}A")).is_none());
        assert!(UsernameLink::parse("https://songbird.chat/i#AAAA").is_none());
        assert!(!format!("{l:?}").contains('2'));
    }
}
