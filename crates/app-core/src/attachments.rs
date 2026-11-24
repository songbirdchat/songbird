//! Attachments (A2 M6, D-0141): files encrypted on the device in the format of PROTOCOL §7.1
//! (`songbird_content::attachment`: a streaming AEAD in 64 KiB chunks, key-committing, padded to
//! Padmé buckets), uploaded under a grant (§7.2), named by an `AttachmentPointer` inside the
//! end-to-end encrypted message, and fetched by object ID (§7.3), which is the only capability.
//!
//! - **Upload:** `POST /v1/attachments {size, fetches}` (fetches: every recipient device, this
//!   account's other devices included); one `PUT` when the grant has one part, else every part
//!   then `complete`. A part that fails is sent again; parts already stored are skipped
//!   (`GET …/parts`), so an interrupted upload resumes.
//! - **Download:** `GET /v1/attachments/<id>` answers a short-lived signed URL; the object is
//!   decrypted chunk by chunk, every chunk verified, and only a fully verified file is returned.
//!   Then `POST …/fetched` tells the server this device has it (the last device's confirmation
//!   deletes the object).

use songbird_content::attachment::{self, Suite};
use songbird_content::schema::AttachmentPointer;
use zeroize::Zeroizing;

use crate::messenger::Net;
use crate::{Error, Result};

/// Largest file the format and the server take (2 GiB by default); the app holds a file in
/// memory, so it caps lower.
pub const MAX_FILE: usize = 256 * 1024 * 1024;

/// What the sender knows about a file before it is encrypted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileMeta {
    /// Media type (`image/jpeg`).
    pub content_type: String,
    /// The file's name.
    pub file_name: String,
    /// Pixels, for images and videos.
    pub width: u32,
    /// Pixels.
    pub height: u32,
    /// For audio and video.
    pub duration_ms: u32,
    /// `AttachmentPointer.Flag` bits (voice note, GIF, …).
    pub flags: u32,
    /// Voice notes: amplitude samples.
    pub waveform: Vec<u8>,
    /// Caption.
    pub caption: String,
    /// The photo's blurred stand-in ([`crate::blurhash`]), at most 128 characters; empty for
    /// none.
    pub blurhash: String,
}

/// One of the photos and videos sent together (D-0186): the album's id, this item's place
/// from 0, and how many the album has (2 to 100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlbumRef {
    /// 16 random bytes, the same for every item.
    pub id: [u8; 16],
    /// This item's place, from 0.
    pub index: u32,
    /// How many items the album has.
    pub count: u32,
}

impl AlbumRef {
    fn schema(self) -> songbird_content::schema::Album {
        songbird_content::schema::Album {
            id: self.id.to_vec(),
            index: self.index,
            count: self.count,
        }
    }
}

/// The album fields of a message about to go: none for a single photo.
#[must_use]
pub fn album_field(album: Option<AlbumRef>) -> Option<songbird_content::schema::Album> {
    album.map(AlbumRef::schema)
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

/// Encrypts `plaintext`: the pointer (without its object ID) and the ciphertext to upload.
pub fn encrypt(plaintext: &[u8], meta: &FileMeta) -> Result<(AttachmentPointer, Vec<u8>)> {
    if plaintext.len() > MAX_FILE {
        return Err(Error::Malformed);
    }
    let (key, salt) = attachment::fresh().map_err(|_| Error::Random)?;
    let len = u64::try_from(plaintext.len()).map_err(|_| Error::Malformed)?;
    let mut out = Vec::with_capacity(plaintext.len() + plaintext.len() / 64 + 4096);
    let digest = attachment::encrypt(
        Suite::DEFAULT,
        &key,
        &salt,
        len,
        &mut &plaintext[..],
        &mut out,
    )
    .map_err(|_| Error::Malformed)?;
    let pointer = AttachmentPointer {
        object_id: Vec::new(),
        key: key.to_vec(),
        digest: digest.to_vec(),
        size: len,
        content_type: meta.content_type.clone(),
        file_name: meta.file_name.clone(),
        width: meta.width,
        height: meta.height,
        // A longer one than the schema allows is left out, not sent (it would be refused).
        blurhash: if meta.blurhash.len() <= 128 {
            meta.blurhash.clone()
        } else {
            String::new()
        },
        duration_ms: meta.duration_ms,
        flags: meta.flags,
        waveform: meta.waveform.clone(),
        caption: meta.caption.clone(),
        uploaded_at_ms: 0,
    };
    Ok((pointer, out))
}

/// Decrypts and verifies a downloaded object against its pointer.
pub fn decrypt(pointer: &AttachmentPointer, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let len = usize::try_from(pointer.size).map_err(|_| Error::Malformed)?;
    if len > MAX_FILE {
        return Err(Error::Malformed);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(len));
    attachment::decrypt(
        &pointer.key,
        &pointer.digest,
        pointer.size,
        &mut &ciphertext[..],
        &mut *out,
    )
    .map_err(|_| Error::Verify)?;
    Ok(out)
}

/// Uploads a ciphertext for `fetches` recipient devices: answers the object ID.
pub async fn upload<N: Net>(net: &N, ciphertext: &[u8], fetches: usize) -> Result<[u8; 32]> {
    let body = serde_json::json!({ "size": ciphertext.len(), "fetches": fetches.clamp(1, 1024) })
        .to_string()
        .into_bytes();
    let (status, answer) = net.api("POST", "/v1/attachments", body).await?;
    if status != 201 {
        return Err(Error::Server(status));
    }
    let v: serde_json::Value = serde_json::from_slice(&answer).map_err(|_| Error::Malformed)?;
    let id_hex = v
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Malformed)?;
    let mut id = [0u8; 32];
    for (o, i) in id.iter_mut().zip((0..id_hex.len()).step_by(2)) {
        *o = u8::from_str_radix(id_hex.get(i..i + 2).ok_or(Error::Malformed)?, 16)
            .map_err(|_| Error::Malformed)?;
    }
    let part_size = v
        .get("part_size")
        .and_then(serde_json::Value::as_u64)
        .ok_or(Error::Malformed)?;
    let parts = v
        .get("parts")
        .and_then(serde_json::Value::as_u64)
        .ok_or(Error::Malformed)?;
    let base = format!("/v1/attachments/{}", hex(&id));
    if parts <= 1 {
        let (status, _) = net.upload("PUT", &base, ciphertext.to_vec()).await?;
        return if status == 201 {
            Ok(id)
        } else {
            Err(Error::Server(status))
        };
    }
    let part = usize::try_from(part_size).map_err(|_| Error::Malformed)?;
    for n in 1..=parts {
        let i = usize::try_from(n - 1).map_err(|_| Error::Malformed)?;
        let start = i * part;
        let chunk = ciphertext
            .get(start..(start + part).min(ciphertext.len()))
            .ok_or(Error::Malformed)?;
        // One retry per part; parts are idempotent.
        let mut ok = false;
        for _ in 0..2 {
            let (status, _) = net
                .upload("PUT", &format!("{base}/parts/{n}"), chunk.to_vec())
                .await?;
            if status == 204 {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(Error::Network);
        }
    }
    let (status, _) = net
        .api("POST", &format!("{base}/complete"), b"{}".to_vec())
        .await?;
    if status == 201 {
        Ok(id)
    } else {
        Err(Error::Server(status))
    }
}

/// Downloads, verifies and decrypts an attachment, then confirms it (the last recipient's
/// confirmation lets the server delete the object).
pub async fn download<N: Net>(net: &N, pointer: &AttachmentPointer) -> Result<Zeroizing<Vec<u8>>> {
    let id = hex(&pointer.object_id);
    if id.len() != 64 {
        return Err(Error::Malformed);
    }
    let (status, body, location) = net.fetch(&format!("/v1/attachments/{id}")).await?;
    let ciphertext = match (status, location) {
        (200, _) => body,
        (301..=308, Some(loc)) => {
            let (status, body, _) = net.fetch(&loc).await?;
            if status != 200 {
                return Err(Error::Server(status));
            }
            body
        }
        (s, _) => return Err(Error::Server(s)),
    };
    let plain = decrypt(pointer, &ciphertext)?;
    let _ = net
        .fetch_post(&format!("/v1/attachments/{id}/fetched"))
        .await;
    Ok(plain)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn files_round_trip_and_tampering_is_caught() {
        for len in [0usize, 1, 65_536, 200_000] {
            let data: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
            let (p, ct) = encrypt(&data, &FileMeta::default()).unwrap();
            assert!(ct.len() >= data.len());
            assert_eq!(&*decrypt(&p, &ct).unwrap(), &data);
            let mut bad = ct.clone();
            if let Some(b) = bad.last_mut() {
                *b ^= 1;
            }
            assert!(decrypt(&p, &bad).is_err());
        }
    }
}
