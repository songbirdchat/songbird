//! Call links (PROTOCOL §8.4, D-0153): `https://songbird.chat/call#<base64url, no padding>` of
//! the link's 32-byte root. The root travels only in the fragment, which no server sees; the app
//! derives the link ID it sends and the key that seals cards from it.

use crate::invite_link::{decode, encode};

const PREFIX: &str = "https://songbird.chat/call#";

/// A parsed call link.
#[derive(Clone, PartialEq, Eq)]
pub struct CallLink {
    /// The link's root.
    pub root: [u8; 32],
}

impl std::fmt::Debug for CallLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CallLink(..)")
    }
}

impl CallLink {
    /// The link.
    #[must_use]
    pub fn to_link(&self) -> String {
        format!("{PREFIX}{}", encode(&self.root))
    }

    /// Parses a link strictly.
    #[must_use]
    pub fn parse(link: &str) -> Option<Self> {
        if link.len() > 96 {
            return None;
        }
        let b = decode(link.strip_prefix(PREFIX)?)?;
        let root: [u8; 32] = b.as_slice().try_into().ok()?;
        Some(Self { root })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn links_round_trip_strictly() {
        let l = CallLink { root: [7; 32] };
        let s = l.to_link();
        assert!(s.starts_with(PREFIX));
        assert_eq!(CallLink::parse(&s), Some(l));
        assert!(CallLink::parse(&format!("{s}A")).is_none());
        assert!(CallLink::parse("https://songbird.chat/g#AAAA").is_none());
        assert!(CallLink::parse(&s.replace("call#", "call#x")).is_none());
    }
}
