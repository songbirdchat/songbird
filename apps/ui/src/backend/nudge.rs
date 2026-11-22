//! Nudges on the real backend (Prompt 9, M6; PROTOCOL §10.5, D-0104).
//!
//! A nudge is `Content.attention {NUDGE}`, sent identified on the delivery
//! connection in the server's attention class (frame `0x04`), one device at
//! a time: `songbird_client::Client::attention(device, payload)`. The server
//! holds it to budgets per pair of accounts and per sender and drops it
//! silently when over a budget, blocked or offline: a nudge to an offline
//! contact is lost, as Yahoo's buzz was. It learns that a nudge happened
//! between two accounts, never what it is.
//!
//! The payload is sealed for each of the contact's devices over an end-to-end session by
//! app-core (`Messenger::nudge`); a nudge received is taken only from the attention class
//! (`Messenger::receive_attention`).
//!
//! **Never by surprise** (D-0101 question 9): the first nudge this device
//! ever sends is confirmed first; the answer is kept on this device.

use super::vault::Storage;
#[cfg(test)]
use songbird_content::{
    Content,
    presence::{Attention, AttentionKind},
    schema::content::Body,
};

const K_CONFIRMED: &str = "sb.nudge_confirmed";

/// Nudges the other person of a real chat (sealed for each of their devices in the attention
/// class; the server drops it silently when over a budget, blocked or offline).
pub fn send(chat: crate::store::ChatId) {
    if !(super::is_live() && super::registered()) {
        crate::ui::toast("Not connected: the nudge wasn't sent.");
        return;
    }
    super::chat::nudge(chat);
}

/// Whether the user already agreed to send nudges (asked once, ever).
pub fn confirmed() -> bool {
    Storage::get_item(K_CONFIRMED).is_some()
}

pub fn set_confirmed() {
    if let Err(e) = Storage::set_item(K_CONFIRMED, "1") {
        log::warn!("nudge storage: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nudge_is_an_attention_content() {
        let c = Content {
            version: songbird_content::VERSION,
            message_id: vec![1; 16],
            sent_at_ms: 1,
            body: Some(Body::Attention(Attention {
                kind: i32::from(AttentionKind::Nudge),
            })),
            ..Content::default()
        };
        let (bytes, _) = songbird_content::seal(&c, None).expect("encodes");
        let opened = songbird_content::open(&bytes).expect("opens");
        assert!(matches!(
            opened.content.body,
            Some(Body::Attention(Attention { kind })) if kind == i32::from(AttentionKind::Nudge)
        ));
    }
}
