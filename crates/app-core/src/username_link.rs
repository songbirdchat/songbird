//! Username link encryption (PROTOCOL §4.3, D-0140): the username sealed under a key that only
//! travels in the link, so the server stores bytes it cannot read.
//!
//! A [`crate::blob`] with context `"songbird.v1/username-link"` under the link key.
//! At most 256 bytes (the server's bound): a 32-byte username gives 60.

use crate::blob::{open_blob, seal_blob};
use crate::crypto::random;
use crate::{Error, Result};

const CONTEXT: &[u8] = b"songbird.v1/username-link";

/// A fresh link key and the ciphertext to upload for `username`.
pub fn seal_username(username: &str) -> Result<([u8; 32], Vec<u8>)> {
    let link_key = *random::<32>()?;
    Ok((
        link_key,
        seal_blob(CONTEXT, &link_key, username.as_bytes())?,
    ))
}

/// Opens a ciphertext fetched for a link.
pub fn open_username(link_key: &[u8; 32], ciphertext: &[u8]) -> Result<String> {
    String::from_utf8(open_blob(CONTEXT, link_key, ciphertext)?).map_err(|_| Error::Malformed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn only_the_link_key_opens_it() {
        let (k, ct) = seal_username("Radu.Diaconu").unwrap();
        assert!(ct.len() <= 256);
        assert_eq!(open_username(&k, &ct).unwrap(), "Radu.Diaconu");
        assert!(open_username(&[0; 32], &ct).is_err());
    }
}
