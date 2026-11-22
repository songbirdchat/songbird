//! A7: media, attachments and packs — the media viewer, attachment send
//! progress, storage and data, and stickers and packs.

mod packs;
mod storage;
mod upload;
mod viewer;

pub use packs::packs;
pub use storage::{manage_storage, storage_and_data};
pub use upload::{Upload, ring_markup, upload_progress};
pub use viewer::{fit, media_ids, save_blocked, step};

use crate::{
    nav,
    store::{ChatId, MsgId},
};

/// What a view-once viewer shows: a photo from memory, or a video's playing file.
pub enum OnceSource {
    Photo(nsh_core::element::ImageSrc),
    Video(String),
}

/// Shows a view-once photo or video, once: closing the page leaves nothing (the file was
/// deleted from the store as it opened; a video's playing copy goes as the page closes).
pub fn view_once(src: OnceSource) {
    use nsh_core::prelude::*;
    let src = std::cell::RefCell::new(Some(src));
    nav::push("view-once", move || {
        let shown = src.borrow_mut().take();
        cleanup(crate::backend::chat::clear_media_cache);
        let body = match shown {
            Some(OnceSource::Photo(s)) => image(s)
                .w_full()
                .flex_1()
                .object_fit(ResizeMode::Contain)
                .into_element(),
            Some(OnceSource::Video(url)) => {
                video(url).controls(true).w_full().flex_1().into_element()
            }
            None => crate::ui::muted("Already viewed.").into_element(),
        };
        crate::ui::page(
            "View once",
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(move || crate::ui::c::call_bg())
                .child(body),
        )
    });
}

/// Opens the full-screen viewer on `msg`, with the chat's other photos
/// and videos a swipe or an arrow key away.
pub fn open_viewer(chat: ChatId, msg: MsgId) {
    nav::push(format!("viewer:{chat}"), move || {
        viewer::viewer_page(chat, msg)
    });
}
