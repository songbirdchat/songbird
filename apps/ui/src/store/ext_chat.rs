//! A5 conversation: reads by index (the message list binds each cell to
//! one row, never cloning the whole chat) and the few intents the
//! conversation needs beyond the shared store.
//!
//! **Mock.** As the rest of the store: app-core (A2) will own these.

use super::*;

impl Store {
    // ------------------------------------------------------------ reads

    /// How many messages the chat holds.
    pub fn message_count(&self, chat: ChatId) -> usize {
        self.messages.with(|m| m.get(&chat).map_or(0, Vec::len))
    }

    /// The message at `index` (oldest first), if there still is one.
    pub fn message_at(&self, chat: ChatId, index: usize) -> Option<Message> {
        self.messages
            .with(|m| m.get(&chat).and_then(|v| v.get(index)).cloned())
    }

    /// The messages around `index`: before, at, after (for date
    /// separators and sender grouping).
    pub fn message_window(
        &self,
        chat: ChatId,
        index: usize,
    ) -> (Option<Message>, Option<Message>, Option<Message>) {
        self.messages.with(|m| {
            let Some(v) = m.get(&chat) else {
                return (None, None, None);
            };
            let before = index.checked_sub(1).and_then(|i| v.get(i)).cloned();
            (before, v.get(index).cloned(), v.get(index + 1).cloned())
        })
    }

    /// Where message `id` is in the chat.
    pub fn index_of(&self, chat: ChatId, id: MsgId) -> Option<usize> {
        self.messages
            .with(|m| m.get(&chat).and_then(|v| v.iter().position(|x| x.id == id)))
    }

    /// The ids of the chat's messages, in order (the list's identities).
    pub fn message_ids(&self, chat: ChatId) -> Vec<MsgId> {
        self.messages.with(|m| {
            m.get(&chat)
                .map(|v| v.iter().map(|x| x.id).collect())
                .unwrap_or_default()
        })
    }

    /// The user's latest text message (Up arrow edits it).
    pub fn last_own_text(&self, chat: ChatId) -> Option<(MsgId, String)> {
        self.messages.with(|m| {
            m.get(&chat)?
                .iter()
                .rev()
                .find(|x| x.mine())
                .and_then(|x| match &x.body {
                    Body::Text(t) => Some((x.id, t.clone())),
                    Body::Link { text, .. } => Some((x.id, text.clone())),
                    _ => None,
                })
        })
    }

    /// The user's latest message that went out (not a system notice): the
    /// only one that shows its delivery status under it.
    pub fn latest_own(&self, chat: ChatId) -> Option<MsgId> {
        self.messages.with(|m| {
            m.get(&chat)?
                .iter()
                .rev()
                .find(|x| x.mine() && !matches!(x.body, Body::System(_)))
                .map(|x| x.id)
        })
    }

    /// How many people a group holds (the user included) and how many of
    /// the others are online now: the header's "5 people · 2 online".
    pub fn people_online(&self, chat: ChatId) -> (usize, usize) {
        let Some(c) = self.chat(chat) else {
            return (0, 0);
        };
        let online = self.contacts.with(|cs| {
            c.members
                .iter()
                .filter_map(|m| m.contact)
                .filter(|id| {
                    cs.iter()
                        .any(|x| x.id == *id && x.presence == Presence::Online)
                })
                .count()
        });
        (c.members.len(), online)
    }

    /// Messages of the chat matching `keep`, newest first.
    fn messages_where(&self, chat: ChatId, keep: impl Fn(&Message) -> bool) -> Vec<Message> {
        self.messages.with(|m| {
            m.get(&chat)
                .map(|v| v.iter().rev().filter(|x| keep(x)).cloned().collect())
                .unwrap_or_default()
        })
    }

    /// Photos and videos shared in the chat, newest first.
    pub fn media_of(&self, chat: ChatId) -> Vec<Message> {
        self.messages_where(chat, |m| {
            matches!(m.body, Body::Image { .. } | Body::Video { .. })
        })
    }

    /// Documents shared in the chat, newest first.
    pub fn docs_of(&self, chat: ChatId) -> Vec<Message> {
        self.messages_where(chat, |m| matches!(m.body, Body::File { .. }))
    }

    /// Links shared in the chat, newest first.
    pub fn links_of(&self, chat: ChatId) -> Vec<Message> {
        self.messages_where(chat, |m| matches!(m.body, Body::Link { .. }))
    }

    /// Starred messages of the chat, newest first.
    pub fn starred_of(&self, chat: ChatId) -> Vec<Message> {
        self.messages_where(chat, |m| m.starred)
    }

    /// Groups the user shares with `contact`.
    pub fn groups_in_common(&self, contact: ContactId) -> Vec<Chat> {
        self.chats.with(|cs| {
            cs.iter()
                .filter(|c| {
                    matches!(c.kind, ChatKind::Group)
                        && c.members.iter().any(|m| m.contact == Some(contact))
                })
                .cloned()
                .collect()
        })
    }

    // ------------------------------------------------------------ intents

    /// Gives the conversation another theme (its canvas and the user's
    /// bubbles). Only this device's view of the chat changes.
    // mock: app-core syncs it to the user's other devices as an encrypted
    // chat setting; the other people in the chat never see it.
    pub fn set_chat_theme(&self, chat: ChatId, theme: ChatTheme) {
        if self.chat(chat).is_some_and(|c| c.theme != theme) {
            self.edit_chat(chat, |c| c.theme = theme);
        }
    }

    /// Stars (or unstars) several messages at once (multi-select).
    pub fn star_messages(&self, chat: ChatId, ids: &[MsgId], starred: bool) {
        self.messages.update(|m| {
            if let Some(v) = m.get_mut(&chat) {
                for x in v.iter_mut().filter(|x| ids.contains(&x.id)) {
                    x.starred = starred;
                }
            }
        });
    }

    /// Deletes several messages from this device (multi-select).
    pub fn delete_messages_for_me(&self, chat: ChatId, ids: &[MsgId]) {
        self.messages.update(|m| {
            if let Some(v) = m.get_mut(&chat) {
                v.retain(|x| !ids.contains(&x.id));
            }
        });
    }

    /// Deletes several of the user's messages for everyone.
    pub fn delete_messages_for_everyone(&self, chat: ChatId, ids: &[MsgId]) {
        for &id in ids {
            if self.message(chat, id).is_some_and(|m| m.mine()) {
                self.delete_for_everyone(chat, id);
            }
        }
    }

    /// Edits the user's text, keeping a link's preview when it has one.
    // mock: app-core sends the edit as a new encrypted revision.
    pub fn edit_message_text(&self, chat: ChatId, id: MsgId, text: String) {
        self.edit_message(chat, id, |m| {
            if !m.mine() {
                return;
            }
            match &mut m.body {
                Body::Link { text: t, .. } => *t = text,
                _ => m.body = Body::Text(text),
            }
            m.edited = true;
        });
    }

    /// Nudges; in a demo chat the other person nudges back a moment later (a real chat's
    /// nudges arrive from the network, `backend::chat::nudged`).
    pub fn nudge_with_echo(&self, chat: ChatId) {
        self.nudge(chat);
        if crate::backend::chat::owns(chat) {
            return;
        }
        let store = *self;
        set_timeout(Duration::from_millis(2600), move || {
            if let Some(peer) = store.peer(chat).filter(|p| !p.nudges_off && !p.blocked) {
                store.push_message(chat, Some(peer.id), Body::Nudge);
            }
        });
    }
}
