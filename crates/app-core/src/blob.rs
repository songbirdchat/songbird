//! Small blobs sealed under a 32-byte key the server never holds (D-0140): avatars
//! (`PUT /v1/profile/avatar`, the key inside the encrypted profile) and the like.
//!
//! `blob = nonce [12] || AES-256-GCM(k, nonce, aad = context, plaintext)` with
//! `k = HKDF-SHA256(salt = context, ikm = key, info = "key")`; `context` separates uses.

use zeroize::Zeroizing;

use crate::crypto::{hkdf, open, random, seal};
use crate::{Error, Result};

fn derive(context: &[u8], key: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
    let mut k = Zeroizing::new([0u8; 32]);
    hkdf(context, key, &[b"key"], k.as_mut())?;
    Ok(k)
}

/// Seals `plaintext` under `key` for `context`.
pub fn seal_blob(context: &[u8], key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let nonce = *random::<12>()?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&seal(&*derive(context, key)?, &nonce, context, plaintext)?);
    Ok(out)
}

/// Opens what [`seal_blob`] made.
pub fn open_blob(context: &[u8], key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>> {
    let (nonce, body) = blob.split_first_chunk::<12>().ok_or(Error::Malformed)?;
    open(&*derive(context, key)?, nonce, context, body)
}

/// The avatar context.
pub const AVATAR: &[u8] = b"songbird.v1/avatar";

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn contexts_and_keys_separate() {
        let b = seal_blob(AVATAR, &[1; 32], b"jpeg").unwrap();
        assert_eq!(open_blob(AVATAR, &[1; 32], &b).unwrap(), b"jpeg");
        assert!(open_blob(AVATAR, &[2; 32], &b).is_err());
        assert!(open_blob(b"other", &[1; 32], &b).is_err());
    }
}
