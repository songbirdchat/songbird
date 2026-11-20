//! A5 chat list intents on top of the shared store: chat lists, bulk
//! read, and answering message requests.
//!
//! **Mock.** Each of these is a call app-core (A2) will take over; they
//! change the in-memory store only. List membership and names are synced
//! settings records in the real app.

use super::{ChatId, ChatKind, Store};

impl Store {
    /// Adds the chat to the custom list `name`, or takes it out.
    // mock: app-core `chats.set_lists(chat, lists)` (a synced record).
    pub fn toggle_in_list(&self, id: ChatId, name: &str) {
        self.edit_chat(id, |c| {
            if let Some(at) = c.lists.iter().position(|l| l == name) {
                c.lists.remove(at);
            } else {
                c.lists.push(name.to_string());
            }
        });
    }

    /// Marks every chat in the inbox read (the list's "Mark all as read").
    // mock: app-core `chats.mark_all_read()`, which also sends read
    // receipts where the user has them on.
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

    /// Blocks the sender of a message request and removes the request.
    // mock: app-core `requests.block(chat)`: the server stops delivering
    // their messages; they are not told.
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

    /// Reports a message request to Songbird, then blocks the sender.
    // mock: app-core `requests.report(chat)`: the reported messages are
    // sent to Songbird's trust and safety team, then the sender is blocked.
    pub fn report_request(&self, id: ChatId) {
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
