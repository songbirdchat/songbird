//! A5 chat list intents on top of the shared store: chat lists, bulk
//! read, and answering message requests. Real chats reach app-core: list
//! membership is part of each chat's synced state record (`backend::chat_state`),
//! read marks and request answers go through the messenger (PROTOCOL §5.10).

use super::{ChatId, ChatKind, Store};

impl Store {
    /// Adds the chat to the custom list `name`, or takes it out. A real chat's lists are synced
    /// with its state record (`backend::chat_state` writes it when the chat changes).
    pub fn toggle_in_list(&self, id: ChatId, name: &str) {
        self.edit_chat(id, |c| {
            if let Some(at) = c.lists.iter().position(|l| l == name) {
                c.lists.remove(at);
            } else {
                c.lists.push(name.to_string());
            }
        });
    }

    /// Marks every chat in the inbox read (the list's "Mark all as read"); real chats send read
    /// receipts where the user has them on (message requests never do).
    pub fn mark_all_read(&self) {
        crate::backend::chat::mark_all_read(*self);
        self.chats.update(|cs| {
            for c in cs.iter_mut().filter(|c| !c.archived && !c.request) {
                c.unread = 0;
                c.mentions = 0;
                c.marked_unread = false;
            }
        });
    }

    /// Blocks the sender of a message request and deletes the request (on the user's other
    /// devices too); the sender is not told.
    pub fn block_request(&self, id: ChatId) {
        let sender = self.chat(id).and_then(|c| match c.kind {
            ChatKind::Direct(p) | ChatKind::WhatsApp(p) => Some(p),
            _ => None,
        });
        if let Some(p) = sender {
            self.block(p, true);
        }
        self.delete_chat(id);
    }

    /// Reports a message request to Songbird (its newest messages with their franking proofs,
    /// PROTOCOL §5.14), then blocks the sender and deletes the request. A real one does all
    /// three in that order in one task: the report needs the messages the deletion removes.
    pub fn report_request(&self, id: ChatId) {
        if crate::backend::chat::owns(id) {
            let sender = self.chat(id).and_then(|c| match c.kind {
                ChatKind::Direct(p) => Some(p),
                _ => None,
            });
            crate::backend::chat::report_request(*self, id);
            if let Some(p) = sender {
                self.contacts.update(|cs| {
                    if let Some(c) = cs.iter_mut().find(|c| c.id == p) {
                        c.blocked = true;
                    }
                });
            }
            self.forget_chat(id);
            return;
        }
        self.report(id);
        self.block_request(id);
    }
}

#[cfg(test)]
impl Store {
    /// The seed's chats, messages and contacts as plain data, for the
    /// chat list's unit tests (no NSH runtime needed).
    pub fn seed_parts() -> (
        Vec<super::Chat>,
        std::collections::HashMap<ChatId, Vec<super::Message>>,
        Vec<super::Contact>,
    ) {
        let s = super::seed::seed();
        (s.chats, s.messages, s.contacts)
    }
}
