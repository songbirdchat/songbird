//! Sticker packs through app-core (D-0159): installed from links, kept encrypted on the device,
//! shown in the picker and sent as stickers.

use std::{cell::RefCell, collections::HashMap};

use nsh_core::{element::ImageSrc, prelude::*};
use songbird_app_core::packs::Pack;
use songbird_content::pack::Kind;

use super::chat::core;
use crate::store::ext_groups::{self, PackKind, groups_state};
use crate::ui::toast;

thread_local! {
    /// Installed packs by their UI ID.
    static PACKS: RefCell<HashMap<u64, [u8; 16]>> = RefCell::new(HashMap::new());
    /// Assets already shown, by (pack, object).
    static IMAGES: RefCell<HashMap<(u64, u32), ImageSrc>> = RefCell::new(HashMap::new());
}

/// Whether packs are real here: a registered account on the real backend.
pub fn live() -> bool {
    !crate::store::demo() && super::is_live() && super::registered()
}

/// A pack's UI ID, from its pack ID.
fn ui_of(id: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&id[..8]);
    (u64::from_be_bytes(b) >> 2) | (1 << 61)
}

fn kind_of(k: Kind) -> PackKind {
    match k {
        Kind::Scene => PackKind::Scenes,
        Kind::Sounds => PackKind::SoundStickers,
        Kind::Emoticons => PackKind::Emoticons,
        _ => PackKind::Stickers,
    }
}

fn to_ui(p: &Pack) -> ext_groups::Pack {
    let id = ui_of(&p.id);
    PACKS.with_borrow_mut(|m| m.insert(id, p.id));
    ext_groups::Pack {
        id,
        kind: kind_of(p.kind),
        name: if p.title.is_empty() {
            "Untitled pack".into()
        } else {
            p.title.clone()
        },
        by: p.author.clone(),
        items: p
            .items
            .iter()
            .map(|(_, e, _)| {
                if e.is_empty() {
                    "🖼".into()
                } else {
                    e.clone()
                }
            })
            .collect(),
        objects: p.items.iter().map(|(o, ..)| *o).collect(),
    }
}

/// Shows the installed packs (called when app-core opens, with the core held).
pub fn show(m: &super::chat::Core) {
    if let Ok(packs) = m.packs() {
        let list: Vec<ext_groups::Pack> = packs.iter().map(to_ui).collect();
        groups_state().update(|g| g.packs = list);
    }
}

/// Installs a pack from its link, showing progress until it is in.
pub fn install(link: String) {
    groups_state().update(|g| {
        g.installing = Some(ext_groups::PackInstall {
            link: link.clone(),
            progress: 0.5,
        });
    });
    super::run(async move {
        let got = {
            let lock = core();
            let mut guard = lock.lock().await;
            let m = guard
                .as_mut()
                .ok_or_else(|| super::problem("Not connected yet."))?;
            m.install_pack(&link).await
        };
        let cancelled = groups_state().with_untracked(|g| g.installing.is_none());
        groups_state().update(|g| g.installing = None);
        match got {
            Ok(p) if !cancelled => {
                let ui = to_ui(&p);
                groups_state().update(|g| {
                    g.packs.retain(|x| x.id != ui.id);
                    g.packs.push(ui);
                });
                toast(format!("{} added", p.title));
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(_) => Err(super::problem(
                "The pack couldn't be added. The link may be wrong or the pack gone.",
            )),
        }
    });
}

/// Removes an installed pack.
pub fn remove(ui: u64) {
    let Some(id) = PACKS.with_borrow(|m| m.get(&ui).copied()) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let guard = lock.lock().await;
        if let Some(m) = guard.as_ref() {
            let _ = m.remove_pack(&id);
        }
        Ok(())
    });
}

/// Moves an installed pack up or down.
pub fn move_pack(ui: u64, up: bool) {
    let Some(id) = PACKS.with_borrow(|m| m.get(&ui).copied()) else {
        return;
    };
    super::run(async move {
        let lock = core();
        let mut guard = lock.lock().await;
        if let Some(m) = guard.as_mut() {
            let _ = m.move_pack(&id, up);
        }
        Ok(())
    });
}

/// An installed pack's asset as an image (read once, then kept for the picker).
pub fn image(ui: u64, object: u32) -> Option<ImageSrc> {
    if let Some(src) = IMAGES.with_borrow(|m| m.get(&(ui, object)).cloned()) {
        return Some(src);
    }
    let id = PACKS.with_borrow(|m| m.get(&ui).copied())?;
    let lock = core();
    let guard = lock.try_lock().ok()?;
    let bytes = guard.as_ref()?.pack_item(&id, object).ok()??;
    let src = ImageSrc::from(super::avatar::data_uri(&bytes));
    IMAGES.with_borrow_mut(|m| m.insert((ui, object), src.clone()));
    Some(src)
}

/// The pack ID of an installed pack's UI ID.
pub fn pack_id(ui: u64) -> Option<[u8; 16]> {
    PACKS.with_borrow(|m| m.get(&ui).copied())
}
