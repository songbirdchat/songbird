//! Profile photos (PROTOCOL §4.8, D-0140): picked with the system's picker,
//! encrypted on this device under a fresh 32-byte key
//! (`songbird_app_core::blob`, context `songbird.v1/avatar`), uploaded as an
//! opaque object (`PUT /v1/profile/avatar`), and named inside the encrypted
//! profile (`Avatar { id, key }`), so only people holding the profile key
//! (contacts) can find and open it. Contacts' photos come back the same way
//! (`GET /v1/profile/avatar/<id>`).
//!
//! What avatars show: [`photo`] for a seed, set from these photos; the
//! illustrated stand-ins of `ui::portrait` only in the demo fixtures.

use std::collections::HashMap;

use super::vault::Storage;
use nsh_core::{element::ImageSrc, picker::PickRequest, prelude::*};
use songbird_app_core::blob::{AVATAR, open_blob, seal_blob};

use super::{Problem, problem, wire};

const K_AVATAR: &str = "sb.avatar";
const FILE: &str = "songbird-avatar.img";
/// The server's bound on an avatar object.
const MAX: usize = 512 * 1024;

/// Real photos by avatar seed (the user's own under `me.seed`, a contact's
/// under its ID).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Photos(pub HashMap<u64, ImageSrc>);

fn photos() -> Signal<Photos> {
    global::<Photos>()
}

/// The real photo for `seed`, if one is known (tracked).
pub fn photo(seed: u64) -> Option<ImageSrc> {
    photos().with(|p| p.0.get(&seed).cloned())
}

fn set_photo(seed: u64, bytes: &[u8]) {
    let src = ImageSrc::from(data_uri(bytes));
    photos().update(|p| {
        p.0.insert(seed, src);
    });
}

/// `data:` URI of image bytes (the type sniffed).
pub(crate) fn data_uri(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mime = nsh_core::picker::sniff(bytes).unwrap_or("image/jpeg");
    let mut s = String::with_capacity(bytes.len() * 4 / 3 + 32);
    s.push_str("data:");
    s.push_str(mime);
    s.push_str(";base64,");
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// This account's avatar object and key, once uploaded.
pub fn mine() -> Option<([u8; 32], [u8; 32])> {
    let b = wire::unhex(&Storage::get_item(K_AVATAR)?)?;
    let (id, key) = b.split_first_chunk::<32>()?;
    Some((*id, (*key).try_into().ok()?))
}

/// Shows the user's own photo from this device (at start).
pub fn restore(me_seed: u64) {
    if let Some(bytes) = data_dir().and_then(|d| std::fs::read(d.join(FILE)).ok()) {
        set_photo(me_seed, &bytes);
    }
}

/// Picks a photo, uploads it encrypted and republishes the profile.
pub fn choose(store: crate::store::Store) {
    super::run(async move {
        let mut request = PickRequest::new(false, 1);
        request.max_dimension = Some(DevicePixels(640));
        let Some(file) = pick_images(request).await.into_iter().next() else {
            return Ok(());
        };
        let bytes = file.data.bytes().to_vec();
        if bytes.is_empty() {
            return Err(problem("That photo couldn't be read."));
        }
        upload(&bytes).await?;
        if let Some(d) = data_dir() {
            let _ = std::fs::write(d.join(FILE), &bytes);
        }
        set_photo(store.me.with_untracked(|m| m.seed), &bytes);
        super::profile::publish(store);
        Ok(())
    });
}

async fn upload(bytes: &[u8]) -> Result<(), Problem> {
    let key: [u8; 32] = wire::random();
    let blob =
        seal_blob(AVATAR, &key, bytes).map_err(|_| problem("The photo couldn't be encrypted."))?;
    if blob.len() > MAX {
        return Err(problem("That photo is too large. Pick a smaller one."));
    }
    let (status, body) = super::signed("PUT", "/v1/profile/avatar", Some(blob), false).await?;
    let v = super::ok_json(status, &body)?;
    let id: [u8; 32] = v["avatar"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let mut both = id.to_vec();
    both.extend_from_slice(&key);
    if let Err(e) = Storage::set_item(K_AVATAR, wire::hex(&both)) {
        log::warn!("avatar storage: {e}");
    }
    Ok(())
}

/// Removes the photo (the profile is republished without it).
pub fn remove(store: crate::store::Store) {
    Storage::remove_item(K_AVATAR);
    if let Some(d) = data_dir() {
        let _ = std::fs::remove_file(d.join(FILE));
    }
    let seed = store.me.with_untracked(|m| m.seed);
    photos().update(|p| {
        p.0.remove(&seed);
    });
    super::profile::publish(store);
}

/// Fetches and opens a contact's photo named in their profile.
pub async fn fetch(seed: u64, id: &[u8], key: &[u8]) -> Result<(), Problem> {
    let key: [u8; 32] = key.try_into().map_err(|_| problem("bad avatar key"))?;
    let path = format!("/v1/profile/avatar/{}", wire::hex(id));
    let (status, body) = super::signed("GET", &path, None, false).await?;
    if status != 200 {
        return Err(super::explain(status, &body));
    }
    let bytes =
        open_blob(AVATAR, &key, &body).map_err(|_| problem("A photo couldn't be opened."))?;
    set_photo(seed, &bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::data_uri;

    #[test]
    fn data_uris_are_standard_base64() {
        assert_eq!(data_uri(b"Man").split(',').nth(1), Some("TWFu"));
        assert_eq!(data_uri(b"Ma").split(',').nth(1), Some("TWE="));
        assert_eq!(data_uri(b"M").split(',').nth(1), Some("TQ=="));
    }
}
