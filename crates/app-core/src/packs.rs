//! Sticker packs on the device (PROTOCOL §7.4, D-0159).
//!
//! - **Installing** from a link `https://songbird.chat/pack#<pack id, 32 hex>.<pack key,
//!   base64url>`: the manifest (object 0) and every asset are fetched from `GET /v1/packs/…`
//!   (no account), opened with keys derived from the pack key, each asset checked against the
//!   manifest's commitment, and kept in the encrypted store. The pack key never leaves the device
//!   except inside stickers sent (as the protocol has it).
//! - **Sending** a sticker from an installed pack carries its pack ID, key and emoji, and the
//!   asset itself as an attachment, so the receiver needs nothing installed.
//! - **The built-in pack** ("Songbird friends") is emoji drawn by each device's own font: its
//!   stickers carry [`BUILTIN_PACK_ID`], an all-zero key that hides nothing, and no data.

use songbird_content::pack::{Kind, Manifest, object_key};
use songbird_content::schema::{self, StickerRef};
use zeroize::Zeroizing;

use crate::messenger::{Messenger, Net, hex};
use crate::{Error, Result};

const LINK: &str = "https://songbird.chat/pack#";

/// The built-in pack's ID: every app ships it; its stickers are emoji.
pub const BUILTIN_PACK_ID: [u8; 16] = *b"songbird-friends";
/// The built-in pack's key: not a secret (the pack is public and has no assets).
pub const BUILTIN_PACK_KEY: [u8; 32] = [0; 32];

/// An installed pack, opened.
#[derive(Clone)]
pub struct Pack {
    pub id: [u8; 16],
    pub key: Zeroizing<[u8; 32]>,
    pub kind: Kind,
    pub title: String,
    pub author: String,
    /// Each asset: (object number, first emoji, media type).
    pub items: Vec<(u32, String, String)>,
}

impl std::fmt::Debug for Pack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pack")
            .field("title", &self.title)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// A pack link's (pack ID, pack key), strictly.
#[must_use]
pub fn parse_link(link: &str) -> Option<([u8; 16], Zeroizing<[u8; 32]>)> {
    let rest = link.trim().strip_prefix(LINK)?;
    let (id, key) = rest.split_once('.')?;
    let id: [u8; 16] = crate::messenger::unhex(id).ok()?;
    let key: [u8; 32] = b64url(key)?.try_into().ok()?;
    Some((id, Zeroizing::new(key)))
}

fn opened(id: [u8; 16], key: Zeroizing<[u8; 32]>, manifest: &Manifest) -> Pack {
    Pack {
        id,
        key,
        kind: Kind::try_from(manifest.kind).unwrap_or(Kind::Unspecified),
        title: manifest.title.clone(),
        author: manifest.author.clone(),
        items: manifest
            .items
            .iter()
            .map(|i| {
                (
                    i.object,
                    i.emoji.first().cloned().unwrap_or_default(),
                    i.content_type.clone(),
                )
            })
            .collect(),
    }
}

impl<N: Net> Messenger<N> {
    /// One pack object, by its signed media URL.
    async fn pack_object(&self, id: &[u8; 16], n: usize) -> Result<Vec<u8>> {
        let (status, body, location) = self
            .net
            .fetch(&format!("/v1/packs/{}/{n}", hex(id)))
            .await?;
        match (status, location) {
            (200, _) => Ok(body),
            (301..=308, Some(loc)) => match self.net.fetch(&loc).await? {
                (200, b, _) => Ok(b),
                (s, ..) => Err(Error::Server(s)),
            },
            (s, _) => Err(Error::Server(s)),
        }
    }

    /// Installs a pack from its link: fetched, every object opened and checked, and kept.
    pub async fn install_pack(&mut self, link: &str) -> Result<Pack> {
        use songbird_content::attachment;
        let (id, key) = parse_link(link).ok_or(Error::Malformed)?;
        let mct = self.pack_object(&id, 0).await?;
        let k0 = object_key(&key, 0).map_err(|_| Error::Malformed)?;
        let header = mct.get(..33).ok_or(Error::Malformed)?;
        let d0 = attachment::commitment(&k0, header).map_err(|_| Error::Verify)?;
        let padded = songbird_proto::media::padded_from_ciphertext(
            u64::try_from(mct.len()).map_err(|_| Error::Malformed)?,
        )
        .ok_or(Error::Malformed)?;
        let mut plain = Zeroizing::new(Vec::new());
        attachment::decrypt(&k0[..], &d0, padded, &mut &mct[..], &mut *plain)
            .map_err(|_| Error::Verify)?;
        let manifest = Manifest::open(&plain).map_err(|_| Error::Malformed)?;
        let mut items = Vec::with_capacity(manifest.items.len());
        for item in &manifest.items {
            let n = u16::try_from(item.object).map_err(|_| Error::Malformed)?;
            let ct = self.pack_object(&id, usize::from(n)).await?;
            let k = object_key(&key, n).map_err(|_| Error::Malformed)?;
            let mut out = Vec::new();
            attachment::decrypt(&k[..], &item.digest, item.size, &mut &ct[..], &mut out)
                .map_err(|_| Error::Verify)?;
            items.push((item.object, out));
        }
        let encoded = prost::Message::encode_to_vec(&manifest);
        self.store.put_pack(&id, &key, &encoded, &items)?;
        Ok(opened(id, key, &manifest))
    }

    /// The installed packs, in order.
    pub fn packs(&self) -> Result<Vec<Pack>> {
        Ok(self
            .store
            .packs()?
            .into_iter()
            .filter_map(|(id, key, m)| {
                let manifest: Manifest = prost::Message::decode(m.as_slice()).ok()?;
                Some(opened(id, key, &manifest))
            })
            .collect())
    }

    /// An installed pack's asset.
    pub fn pack_item(&self, id: &[u8; 16], object: u32) -> Result<Option<Zeroizing<Vec<u8>>>> {
        self.store.pack_item(id, object)
    }

    /// Removes an installed pack.
    pub fn remove_pack(&self, id: &[u8; 16]) -> Result<()> {
        self.store.remove_pack(id)
    }

    /// Moves an installed pack up or down the picker.
    pub fn move_pack(&mut self, id: &[u8; 16], up: bool) -> Result<()> {
        self.store.move_pack(id, up)
    }

    /// Sends a sticker: from the built-in pack (`pack` `None`: its `index`-th emoji `emoji`),
    /// or an installed pack's asset `object` (uploaded as an attachment, so the receiver needs
    /// nothing installed).
    pub async fn send_sticker(
        &mut self,
        chat: &[u8; 16],
        pack: Option<&[u8; 16]>,
        object: u32,
        emoji: &str,
    ) -> Result<[u8; 16]> {
        let sticker = match pack {
            None => StickerRef {
                pack_id: BUILTIN_PACK_ID.to_vec(),
                pack_key: BUILTIN_PACK_KEY.to_vec(),
                sticker_id: object,
                emoji: emoji.to_owned(),
                data: None,
            },
            Some(id) => {
                let p = self
                    .packs()?
                    .into_iter()
                    .find(|p| p.id == *id)
                    .ok_or(Error::Malformed)?;
                let bytes = self.store.pack_item(id, object)?.ok_or(Error::Malformed)?;
                let content_type = p
                    .items
                    .iter()
                    .find(|(o, ..)| *o == object)
                    .map_or_else(|| "image/webp".into(), |(_, _, t)| t.clone());
                let meta = crate::attachments::FileMeta {
                    content_type,
                    file_name: format!("sticker-{object}"),
                    flags: 2, // FLAG_BORDERLESS
                    ..Default::default()
                };
                let fetches = if self.is_group(chat) {
                    self.group_devices(chat)?.len().saturating_sub(1).max(1)
                } else {
                    let me = self.local.account;
                    self.devices_of(chat, false).await?.len()
                        + self.store.devices(&me)?.len().saturating_sub(1)
                };
                let (mut ptr, ct) = crate::attachments::encrypt(&bytes, &meta)?;
                let up = crate::attachments::upload(&self.net, &ct, fetches).await?;
                ptr.object_id = up.to_vec();
                ptr.uploaded_at_ms = crate::messenger::now_ms();
                StickerRef {
                    pack_id: id.to_vec(),
                    pack_key: p.key.to_vec(),
                    sticker_id: object,
                    emoji: emoji.chars().take(8).collect(),
                    data: Some(ptr),
                }
            }
        };
        let message = schema::Message {
            sticker: Some(sticker),
            ..Default::default()
        };
        let preview = format!("{emoji} Sticker");
        self.send_message(chat, message, &preview).await
    }
}

/// The sticker an encoded `Content` holds: (pack ID, emoji, has an image).
#[must_use]
pub fn sticker_in(content: &[u8]) -> Option<([u8; 16], String, bool)> {
    use prost::Message as _;
    use songbird_content::schema::{Content, content::Body, data_message};
    let c = Content::decode(content).ok()?;
    match c.body? {
        Body::Data(d) => match d.kind? {
            data_message::Kind::Message(m) => {
                let s = m.sticker?;
                Some((
                    s.pack_id.as_slice().try_into().ok()?,
                    s.emoji,
                    s.data.is_some(),
                ))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    #[test]
    fn links_parse_strictly() {
        let key = [9u8; 32];
        let enc = {
            const A: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut s = String::new();
            let (mut acc, mut bits) = (0u32, 0u32);
            for b in key {
                acc = (acc << 8) | u32::from(b);
                bits += 8;
                while bits >= 6 {
                    bits -= 6;
                    s.push(char::from(A[usize::try_from((acc >> bits) & 63).unwrap()]));
                }
            }
            if bits > 0 {
                s.push(char::from(
                    A[usize::try_from((acc << (6 - bits)) & 63).unwrap()],
                ));
            }
            s
        };
        let link = format!("{LINK}{}.{enc}", "ab".repeat(16));
        let (id, k) = parse_link(&link).unwrap();
        assert_eq!((id, *k), ([0xab; 16], key));
        assert!(parse_link("https://songbird.chat/pack#zz.AAAA").is_none());
        assert!(parse_link(&link.replace("pack#", "g#")).is_none());
    }
}
