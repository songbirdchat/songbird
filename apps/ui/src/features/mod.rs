//! The feature screens. Each module owns its screens and exposes the
//! entry points below; features call each other only through these.

pub mod backups;
pub mod calls;
pub mod chat;
pub mod chats;
pub mod contacts;
pub mod cover;
pub mod desktop;
pub mod devices;
pub mod gallery;
pub mod groups;
pub mod lock;
pub mod maps;
pub mod media;
pub mod notify;
pub mod onboarding;
pub mod plans;
pub mod server;
pub mod settings;
pub mod whatsapp;

/// Opens a Songbird link in the app instead of the browser (D-0183): a call link joins its
/// call, a group link asks to join the group, a username link opens a chat. `false` for any
/// other URL, which the caller opens in the browser.
pub fn open_songbird_link(url: &str) -> bool {
    let url = url.trim();
    if calls::open_link(url) {
        return true;
    }
    let ours = ["https://songbird.chat/g#", "https://songbird.chat/u#"]
        .iter()
        .any(|p| url.starts_with(p));
    if !ours || !(crate::backend::is_live() && crate::backend::registered()) {
        return false;
    }
    let url = url.to_owned();
    crate::backend::run(async move {
        match crate::backend::chat::start_with_username(&url).await {
            Ok(chat) => chat::open(chat),
            Err(p) => crate::ui::toast(p.0),
        }
        Ok(())
    });
    true
}

/// Opens `url`: in the app when it's a Songbird link, else in the browser.
pub fn open_link_or_url(url: &str) {
    if !open_songbird_link(url) {
        let _ = nsh_core::prelude::open_url(url.to_owned());
    }
}
