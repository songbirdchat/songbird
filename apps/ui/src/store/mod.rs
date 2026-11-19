//! The UI's view of app-core: a `Copy` store of signals, made once at the
//! root and provided as context, and the intents that change it.
//!
//! **Mock.** app-core (A2) is not built yet, so this store keeps seed data
//! in memory and simulates the network: statuses advance on timers,
//! contacts "type" and reply. Every intent here is a call the real
//! app-core will take over; screens never write signals directly.

mod ext_chat;
mod ext_chats;
mod ext_contacts;
mod ext_extras;
pub mod ext_groups;
pub mod ext_onboarding;
mod ext_settings;
mod model;
pub(crate) mod seed;

use std::{collections::HashMap, time::Duration};

use nsh_core::{prelude::*, storage::Storage};

pub use ext_contacts::*;
pub use ext_extras::*;
pub use ext_settings::*;
pub use model::*;

/// The seeded data, for other modules' tests.
#[cfg(test)]
pub(crate) mod seed_for_tests {
    pub use super::seed::seed;
}

use crate::ui::Tick;

/// "Now" in mock minutes: Tuesday 17 Nov 2026, 10:24.
pub const NOW: Minutes = 0;

/// The time of day `at` shows as ("09:41").
/// A link's host without `www.` ("songbird.chat").
fn link_site(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.trim_start_matches("www.").to_string()
}

/// A plausible preview title made on the device from the URL alone (mock:
/// app-core reads the page's own title): the last path segment in words
/// ("…/blog/privacy-first" → "Privacy first"), else the site's name
/// ("songbird.chat" → "Songbird").
fn link_title(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let segment = path
        .split_once('/')
        .and_then(|(_, p)| p.split('/').rfind(|s| !s.is_empty()))
        .map(|s| s.rsplit_once('.').map_or(s, |(stem, _)| stem))
        .filter(|s| s.chars().any(char::is_alphabetic));
    let words = match segment {
        Some(s) => s.replace(['-', '_', '+'], " "),
        None => link_site(url).split('.').next().unwrap_or("").to_string(),
    };
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => link_site(url),
    }
}

pub fn clock(at: Minutes) -> String {
    let m = (10 * 60 + 24 + at).rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// The day `at` falls on, relative to today (0), as the list shows it.
pub fn day_of(at: Minutes) -> i64 {
    (10 * 60 + 24 + at).div_euclid(24 * 60)
}

/// The chat list's time: today's clock, "Yesterday", a weekday, a date.
pub fn when_label(at: Minutes) -> String {
    match day_of(at) {
        0 => clock(at),
        -1 => "Yesterday".into(),
        d if d > -7 => [
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
            "Monday",
            "Tuesday",
        ][(d.rem_euclid(7)) as usize]
            .into(),
        // Older: the day and month, as WhatsApp shows them ("7 Nov").
        d => {
            let day = 17 + d;
            if day >= 1 {
                format!("{day} Nov")
            } else {
                format!("{} Oct", 31 + day)
            }
        }
    }
}

/// A date separator's label.
pub fn day_label(at: Minutes) -> String {
    match day_of(at) {
        0 => "Today".into(),
        -1 => "Yesterday".into(),
        _ => when_label(at),
    }
}

/// "last seen …" for a presence.
pub fn presence_label(p: &Presence) -> String {
    match p {
        Presence::Online => "online".into(),
        Presence::Away => "away".into(),
        Presence::Busy => "busy".into(),
        Presence::Invisible => String::new(),
        Presence::LastSeen(Some(at)) => match day_of(*at) {
            0 => format!("last seen today at {}", clock(*at)),
            -1 => format!("last seen yesterday at {}", clock(*at)),
            _ => format!("last seen {}", when_label(*at)),
        },
        Presence::LastSeen(None) => String::new(),
    }
}

/// A message from someone else or an incoming call, for the notifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrival {
    Message {
        chat: ChatId,
        sender: Option<String>,
        preview: String,
    },
    Call {
        chat: ChatId,
        video: bool,
    },
}

/// The store. `Copy`: closures and rows capture it like a signal.
#[derive(Clone, Copy)]
pub struct Store {
    pub me: Signal<Profile>,
    pub contacts: Signal<Vec<Contact>>,
    pub chats: Signal<Vec<Chat>>,
    /// Messages by chat, oldest first.
    pub messages: Signal<HashMap<ChatId, Vec<Message>>>,
    pub calls: Signal<Vec<CallRecord>>,
    pub call_links: Signal<Vec<CallLink>>,
    pub devices: Signal<Vec<Device>>,
    pub settings: Signal<Settings>,
    pub session: Signal<Session>,
    /// Who hears of messages from others and incoming calls (the notifier,
    /// D-0098): called at once, so it decides with the chat and settings as
    /// they are when the message arrives.
    on_arrival: Stored<Option<std::rc::Rc<dyn Fn(Store, Arrival)>>>,
    next_id: Stored<u64>,
}

const ONBOARDED: &str = "onboarded";

thread_local! {
    /// The root's store: one per app, alive as long as the app. Kept here
    /// as well as in context, so listeners and tasks (which have no
    /// component to look context up from) reach it too.
    static ROOT: std::cell::Cell<Option<Store>> = const { std::cell::Cell::new(None) };
}

/// The store the root provided. Works while building, in listeners and in
/// tasks.
pub fn store() -> Store {
    use_context::<Store>()
        .or_else(|| ROOT.with(|r| r.get()))
        .expect("the root provides the store")
}

/// The store, when the root has made one (the backend's tasks).
pub fn try_store() -> Option<Store> {
    use_context::<Store>().or_else(|| ROOT.with(|r| r.get()))
}

/// Makes `s` the app's store (the root, once).
pub fn install(s: Store) {
    ROOT.with(|r| r.set(Some(s)));
    provide_context(s);
}

/// Whether this run shows the demo data: only a preview build started as a
/// screenshot fixture (`SONGBIRD_ONBOARDED=1`, or the `songbird-onboarded`
/// marker in the data folder). Every other run is a real account on the
/// real backend, starting empty.
pub fn demo() -> bool {
    cfg!(test)
        || crate::PREVIEW_BUILD
            && (std::env::var_os("SONGBIRD_ONBOARDED").is_some()
                || data_dir().is_some_and(|d| d.join("songbird-onboarded").exists()))
}

impl Store {
    /// The store: empty for a real account, the demo data for fixtures.
    pub fn new() -> Self {
        let seed = if demo() { seed::seed() } else { seed::empty() };
        // Debug builds only: `SONGBIRD_ONBOARDED=1` skips the first run (for
        // screenshot tests of the rest of the app).
        // …or a `songbird-onboarded` file in the app's data folder (for
        // emulators and VMs, which don't pass the environment through).
        // Preview builds (`SONGBIRD_PREVIEW_BUILD=1` at compile time, for
        // screenshots on emulators and VMs) honour the marker as well.
        let preview_build = crate::PREVIEW_BUILD;
        let preview = preview_build
            && (std::env::var_os("SONGBIRD_ONBOARDED").is_some()
                || data_dir().is_some_and(|d| d.join("songbird-onboarded").exists()));

        let onboarded = preview || Storage::get_item(ONBOARDED).is_some();
        Self {
            me: signal(seed.me),
            contacts: signal(seed.contacts),
            chats: signal(seed.chats),
            messages: signal(seed.messages),
            calls: signal(seed.calls),
            call_links: signal(seed.call_links),
            devices: signal(seed.devices),
            settings: signal(seed.settings),
            session: signal(Session {
                onboarded,
                // The real recovery key (Prompt 10): made once, kept on this device.
                recovery_key: crate::backend::backup::shown_key(),
                ..seed.session
            }),
            on_arrival: stored(None),
            next_id: stored(100_000),
        }
    }

    fn id(&self) -> u64 {
        self.next_id.with(|n| {
            *n += 1;
            *n
        })
    }

    // ------------------------------------------------------------ reads

    pub fn contact(&self, id: ContactId) -> Option<Contact> {
        self.contacts
            .with(|cs| cs.iter().find(|c| c.id == id).cloned())
    }

    pub fn contact_name(&self, id: Option<ContactId>) -> String {
        match id {
            None => "You".into(),
            Some(id) => self
                .contact(id)
                .map(|c| c.name)
                .unwrap_or_else(|| "Unknown".into()),
        }
    }

    pub fn chat(&self, id: ChatId) -> Option<Chat> {
        self.chats
            .with(|cs| cs.iter().find(|c| c.id == id).cloned())
    }

    /// The other person of a direct (or WhatsApp) chat.
    pub fn peer(&self, chat: ChatId) -> Option<Contact> {
        match self.chat(chat)?.kind {
            ChatKind::Direct(c) | ChatKind::WhatsApp(c) => self.contact(c),
            _ => None,
        }
    }

    pub fn messages_of(&self, chat: ChatId) -> Vec<Message> {
        self.messages
            .with(|m| m.get(&chat).cloned().unwrap_or_default())
    }

    pub fn message(&self, chat: ChatId, id: MsgId) -> Option<Message> {
        self.messages.with(|m| {
            m.get(&chat)
                .and_then(|v| v.iter().find(|x| x.id == id).cloned())
        })
    }

    pub fn last_message(&self, chat: ChatId) -> Option<Message> {
        self.messages
            .with(|m| m.get(&chat).and_then(|v| v.last().cloned()))
    }

    /// Chats for the list: not archived, not requests; pinned first, then
    /// by the latest message.
    pub fn inbox(&self) -> Vec<Chat> {
        let mut chats: Vec<Chat> = self.chats.with(|cs| {
            cs.iter()
                .filter(|c| !c.archived && !c.request && !matches!(c.kind, ChatKind::Community))
                .cloned()
                .collect()
        });
        let last = |c: &Chat| self.last_message(c.id).map(|m| m.at).unwrap_or(i64::MIN);
        chats.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(last(b).cmp(&last(a))));
        chats
    }

    pub fn total_unread(&self) -> u32 {
        self.chats.with(|cs| {
            cs.iter()
                .filter(|c| !c.archived && !c.request && !c.muted)
                .map(|c| c.unread)
                .sum()
        })
    }

    // ------------------------------------------------------------ chat intents

    pub(crate) fn edit_chat(&self, id: ChatId, f: impl FnOnce(&mut Chat)) {
        self.chats.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == id) {
                f(c);
            }
        });
    }

    pub fn mark_read(&self, id: ChatId) {
        if crate::backend::chat::owns(id) && self.chat(id).is_some_and(|c| c.unread > 0) {
            crate::backend::chat::mark_read(id);
        }
        if self
            .chat(id)
            .is_some_and(|c| c.unread > 0 || c.marked_unread || c.mentions > 0)
        {
            self.edit_chat(id, |c| {
                c.unread = 0;
                c.mentions = 0;
                c.marked_unread = false;
            });
        }
    }
    pub fn mark_unread(&self, id: ChatId) {
        self.edit_chat(id, |c| c.marked_unread = true);
    }
    pub fn toggle_pin(&self, id: ChatId) {
        self.edit_chat(id, |c| c.pinned = !c.pinned);
    }
    pub fn toggle_mute(&self, id: ChatId) {
        self.edit_chat(id, |c| c.muted = !c.muted);
    }
    pub fn toggle_archive(&self, id: ChatId) {
        self.edit_chat(id, |c| c.archived = !c.archived);
    }
    pub fn toggle_favourite(&self, id: ChatId) {
        self.edit_chat(id, |c| c.favourite = !c.favourite);
    }
    pub fn toggle_lock(&self, id: ChatId) {
        self.edit_chat(id, |c| c.locked = !c.locked);
    }
    pub fn set_disappearing(&self, id: ChatId, timer: Option<Disappear>) {
        self.edit_chat(id, |c| c.disappearing = timer);
        let label = match timer {
            Some(t) => format!(
                "You turned on disappearing messages. New messages will disappear after {}.",
                t.label()
            ),
            None => "You turned off disappearing messages.".into(),
        };
        self.push_message(id, None, Body::System(label));
    }
    pub fn set_draft(&self, id: ChatId, draft: String) {
        if self.chat(id).is_some_and(|c| c.draft != draft) {
            self.edit_chat(id, |c| c.draft = draft);
        }
    }
    pub fn clear_chat(&self, id: ChatId) {
        self.messages.update(|m| {
            m.insert(id, Vec::new());
        });
    }
    pub fn delete_chat(&self, id: ChatId) {
        self.chats.update(|cs| cs.retain(|c| c.id != id));
        self.messages.update(|m| {
            m.remove(&id);
        });
    }
    pub fn accept_request(&self, id: ChatId) {
        self.edit_chat(id, |c| c.request = false);
    }
    pub fn block(&self, contact: ContactId, blocked: bool) {
        self.contacts.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
                c.blocked = blocked;
            }
        });
    }
    /// Reports to Songbird: the last five messages of the chat are sent,
    /// as the report flow says (mock: nothing leaves the device).
    pub fn report(&self, chat: ChatId) {
        let _ = chat;
    }
    pub fn verify(&self, contact: ContactId, verified: bool) {
        self.contacts.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
                c.verified = verified;
                c.key_changed = false;
            }
        });
    }

    // ------------------------------------------------------------ messages

    /// Appends a message and returns its id. Mine start as `Sending`.
    pub fn push_message(&self, chat: ChatId, from: Option<ContactId>, body: Body) -> MsgId {
        let id = self.id();
        let at = self
            .last_message(chat)
            .map(|m| m.at.max(NOW))
            .unwrap_or(NOW);
        let system = matches!(body, Body::System(_));
        let msg = Message {
            id,
            chat,
            from,
            body,
            at,
            status: if from.is_none() && !system {
                Tick::Sending
            } else {
                Tick::Read
            },
            reply_to: None,
            reactions: Vec::new(),
            edited: false,
            starred: false,
            pinned: false,
            forwarded: false,
            receipts: Vec::new(),
        };
        let incoming = (from.is_some() && !system).then(|| (msg.body.preview(), from));
        self.messages
            .update(|m| m.entry(chat).or_default().push(msg));
        if incoming.is_some() {
            self.on_incoming(chat);
        }
        if let Some((preview, Some(from))) = incoming {
            let sender = self.contact(from).map(|c| c.name);
            self.arrive(Arrival::Message {
                chat,
                sender,
                preview,
            });
        }
        id
    }

    /// An incoming message brings an archived chat back to the list unless
    /// "Keep chats archived" is on (Settings → Chats).
    pub fn on_incoming(&self, chat: ChatId) {
        let keep = self.settings.with_untracked(|x| x.keep_archived);
        if !keep && self.chat(chat).is_some_and(|c| c.archived) {
            self.edit_chat(chat, |c| c.archived = false);
        }
    }

    /// Tells the notifier, if one listens.
    pub fn arrive(&self, a: Arrival) {
        if let Some(f) = self.on_arrival.with(|f| f.clone()) {
            f(*self, a);
        }
    }

    /// Sets who hears of arrivals (one listener: the notifier).
    pub fn on_arrival(&self, f: impl Fn(Store, Arrival) + 'static) {
        self.on_arrival
            .with(|slot| *slot = Some(std::rc::Rc::new(f)));
    }

    fn edit_message(&self, chat: ChatId, id: MsgId, f: impl FnOnce(&mut Message)) {
        self.messages.update(|m| {
            if let Some(msg) = m
                .get_mut(&chat)
                .and_then(|v| v.iter_mut().find(|x| x.id == id))
            {
                f(msg);
            }
        });
    }

    /// Sends text (optimistic: it shows at once as Sending), then walks it
    /// through Sent → Delivered → Read and, in direct chats, has the
    /// contact type and answer.
    pub fn send_text(&self, chat: ChatId, text: String, reply_to: Option<MsgId>) -> MsgId {
        let link = text
            .split_whitespace()
            .find(|w| w.starts_with("https://"))
            .map(str::to_owned);
        let previews = self.settings.with_untracked(|s| s.link_previews);
        let body = match link {
            Some(url) if previews => {
                // The link as the sender typed it, without the punctuation
                // that ends its sentence.
                let url = url
                    .trim_end_matches(['.', ',', ';', ':', '!', '?', ')'])
                    .to_string();
                Body::Link {
                    title: link_title(&url),
                    site: link_site(&url),
                    url,
                    text,
                }
            }
            _ => Body::Text(text),
        };
        self.send(chat, body, reply_to)
    }

    /// Sends any body (attachments, polls, stickers…).
    pub fn send(&self, chat: ChatId, body: Body, reply_to: Option<MsgId>) -> MsgId {
        let id = self.push_message(chat, None, body);
        if reply_to.is_some() {
            self.edit_message(chat, id, |m| m.reply_to = reply_to);
        }
        self.set_draft(chat, String::new());
        if crate::backend::chat::owns(chat) {
            // A real chat: app-core encrypts and sends it; ticks follow the
            // server's answer and the recipient's receipts (D-0139).
            let body = self.message(chat, id).map(|m| m.body);
            if let Some(body) = body {
                crate::backend::chat::send(*self, chat, id, &body);
            }
        } else {
            self.simulate_delivery(chat, id);
        }
        id
    }

    /// Marks one of the user's messages as failed to send.
    pub fn fail(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| m.status = Tick::Failed);
    }

    fn simulate_delivery(&self, chat: ChatId, id: MsgId) {
        let store = *self;
        let offline = self.session.with_untracked(|s| s.offline);
        if offline {
            return;
        }
        set_timeout(Duration::from_millis(350), move || {
            store.advance(chat, id, Tick::Sent)
        });
        set_timeout(Duration::from_millis(1000), move || {
            store.advance(chat, id, Tick::Delivered)
        });
        let direct = matches!(
            store.chat(chat).map(|c| c.kind),
            Some(ChatKind::Direct(_) | ChatKind::WhatsApp(_))
        );
        if !direct {
            return;
        }
        set_timeout(Duration::from_millis(2200), move || {
            store.advance(chat, id, Tick::Read);
            store.edit_chat(chat, |c| c.typing = Some("typing…".into()));
        });
        set_timeout(Duration::from_millis(4200), move || {
            store.edit_chat(chat, |c| c.typing = None);
            if let Some(peer) = store.peer(chat) {
                let reply = seed::reply_for(id);
                store.push_message(chat, Some(peer.id), Body::Text(reply.into()));
            }
        });
    }

    /// Moves a status forward (never back), as app-core's state machine.
    pub fn advance(&self, chat: ChatId, id: MsgId, to: Tick) {
        self.edit_message(chat, id, |m| {
            if m.status != Tick::Failed && to > m.status {
                m.status = to;
            }
        });
    }

    pub fn retry(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| m.status = Tick::Sending);
        self.simulate_delivery(chat, id);
    }

    /// Adds, changes or removes the user's reaction (one per message).
    pub fn react(&self, chat: ChatId, id: MsgId, emoji: &str) {
        let emoji = emoji.to_string();
        self.edit_message(chat, id, |m| {
            let had = m
                .reactions
                .iter()
                .any(|r| r.emoji == emoji && r.by.contains(&None));
            for r in &mut m.reactions {
                r.by.retain(|b| b.is_some());
            }
            m.reactions.retain(|r| !r.by.is_empty());
            if !had {
                match m.reactions.iter_mut().find(|r| r.emoji == emoji) {
                    Some(r) => r.by.push(None),
                    None => m.reactions.push(Reaction {
                        emoji,
                        by: vec![None],
                    }),
                }
            }
        });
    }

    /// Edits the user's own text within the edit window (15 minutes).
    pub fn edit_text(&self, chat: ChatId, id: MsgId, text: String) {
        self.edit_message(chat, id, |m| {
            if m.mine() {
                m.body = Body::Text(text);
                m.edited = true;
            }
        });
    }

    pub fn delete_for_everyone(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| {
            m.body = Body::Deleted;
            m.reactions.clear();
        });
    }

    pub fn delete_for_me(&self, chat: ChatId, id: MsgId) {
        self.messages.update(|m| {
            if let Some(v) = m.get_mut(&chat) {
                v.retain(|x| x.id != id);
            }
        });
    }

    pub fn toggle_star(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| m.starred = !m.starred);
    }

    pub fn toggle_pin_message(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| m.pinned = !m.pinned);
    }

    pub fn vote(&self, chat: ChatId, id: MsgId, option: usize) {
        self.edit_message(chat, id, |m| {
            if let Body::Poll { options, mine, .. } = &mut m.body {
                if let Some(old) = mine.take() {
                    options[old].1 -= 1;
                }
                if *mine != Some(option) {
                    options[option].1 += 1;
                    *mine = Some(option);
                }
            }
        });
    }

    pub fn rsvp(&self, chat: ChatId, id: MsgId, going: bool) {
        self.edit_message(chat, id, |m| {
            if let Body::Event {
                going: count, mine, ..
            } = &mut m.body
            {
                if *mine == Some(true) {
                    *count -= 1;
                }
                if going {
                    *count += 1;
                }
                *mine = Some(going);
            }
        });
    }

    pub fn open_view_once(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| {
            if let Body::ViewOnce { opened, .. } = &mut m.body {
                *opened = true;
            }
        });
    }

    pub fn mark_played(&self, chat: ChatId, id: MsgId) {
        self.edit_message(chat, id, |m| {
            if let Body::Voice { played, .. } = &mut m.body {
                *played = true;
            }
        });
    }

    /// Forwards to `to`, marked as forwarded.
    pub fn forward(&self, from_chat: ChatId, id: MsgId, to: ChatId) {
        if let Some(msg) = self.message(from_chat, id) {
            let new = self.send(to, msg.body, None);
            self.edit_message(to, new, |m| m.forwarded = true);
        }
    }

    /// Nudges the other person of a direct chat. The real path: when the
    /// account is live, `Content.attention {NUDGE}` to their devices
    /// (`backend::nudge`), which sends nothing until app-core knows them.
    /// The chat shows it either way. mock: the line in the chat is the
    /// mock's; a real nudge is not a stored message (PROTOCOL §10.5).
    pub fn nudge(&self, chat: ChatId) {
        if let Some(peer) = self.peer(chat) {
            let _ = crate::backend::nudge::send(
                crate::backend::presence::peer_of(*self, peer.id).as_ref(),
            );
        }
        self.send(chat, Body::Nudge, None);
    }

    // ------------------------------------------------------------ new chats and groups

    /// The direct chat with `contact`, made if there is none.
    pub fn direct_chat(&self, contact: ContactId) -> ChatId {
        if let Some(id) = self.chats.with(|cs| {
            cs.iter()
                .find(|c| c.kind == ChatKind::Direct(contact))
                .map(|c| c.id)
        }) {
            return id;
        }
        let name = self.contact_name(Some(contact));
        let id = self.id();
        self.chats
            .update(|cs| cs.push(seed::chat(id, ChatKind::Direct(contact), &name)));
        id
    }

    /// Creates a group the user owns and returns it.
    pub fn create_group(&self, title: String, members: Vec<ContactId>) -> ChatId {
        let id = self.id();
        let mut chat = seed::chat(id, ChatKind::Group, &title);
        chat.members = std::iter::once(Member {
            contact: None,
            role: Role::Owner,
        })
        .chain(members.iter().map(|&c| Member {
            contact: Some(c),
            role: Role::Member,
        }))
        .collect();
        self.chats.update(|cs| cs.push(chat));
        self.push_message(
            id,
            None,
            Body::System(format!("You created the group “{title}”")),
        );
        self.push_message(
            id,
            None,
            Body::System("Only people in this group can read its messages. Messages and calls are end-to-end encrypted.".into()),
        );
        id
    }

    pub fn set_role(&self, chat: ChatId, contact: ContactId, role: Role) {
        self.edit_chat(chat, |c| {
            if let Some(m) = c.members.iter_mut().find(|m| m.contact == Some(contact)) {
                m.role = role;
            }
        });
    }

    pub fn remove_member(&self, chat: ChatId, contact: ContactId) {
        self.edit_chat(chat, |c| c.members.retain(|m| m.contact != Some(contact)));
        let name = self.contact_name(Some(contact));
        self.push_message(chat, None, Body::System(format!("You removed {name}")));
    }

    pub fn add_members(&self, chat: ChatId, contacts: Vec<ContactId>) {
        for &contact in &contacts {
            self.edit_chat(chat, |c| {
                c.members.push(Member {
                    contact: Some(contact),
                    role: Role::Member,
                })
            });
            let name = self.contact_name(Some(contact));
            self.push_message(chat, None, Body::System(format!("You added {name}")));
        }
    }

    pub fn answer_join_request(&self, chat: ChatId, contact: ContactId, approve: bool) {
        self.edit_chat(chat, |c| c.join_requests.retain(|&x| x != contact));
        if approve {
            self.add_members(chat, vec![contact]);
        }
    }

    pub fn update_chat(&self, chat: ChatId, f: impl FnOnce(&mut Chat)) {
        self.edit_chat(chat, f);
    }

    pub fn reset_invite_link(&self, chat: ChatId) {
        let token = self.id();
        self.edit_chat(chat, |c| {
            c.invite_link = format!("https://songbird.chat/g/{token:x}")
        });
    }

    // ------------------------------------------------------------ calls

    pub fn log_call(&self, chat: ChatId, video: bool, direction: CallDirection, secs: u32) {
        let Some(c) = self.chat(chat) else { return };
        let group = !matches!(c.kind, ChatKind::Direct(_) | ChatKind::WhatsApp(_));
        let record = CallRecord {
            id: self.id(),
            chat,
            title: c.title.clone(),
            seed: c.seed,
            video,
            group,
            direction,
            at: NOW,
            secs,
        };
        self.calls.update(|cs| cs.insert(0, record));
    }

    pub fn create_call_link(&self, name: String, lobby: bool) -> CallLink {
        let id = self.id();
        let link = CallLink {
            id,
            name,
            url: format!("https://songbird.chat/call/{id:x}"),
            lobby,
        };
        self.call_links.update(|l| l.insert(0, link.clone()));
        link
    }

    // ------------------------------------------------------------ account and settings

    pub fn settings_update(&self, f: impl FnOnce(&mut Settings)) {
        self.settings.update(f);
        // Synced settings become encrypted records; device ones are saved here (Prompt 9).
        crate::backend::records::changed(*self);
    }

    pub fn profile_update(&self, f: impl FnOnce(&mut Profile)) {
        self.me.update(f);
    }

    pub fn remove_device(&self, id: u64) {
        if crate::backend::registered() {
            crate::backend::remove_device(*self, id);
        }
        self.devices.update(|d| d.retain(|x| x.id != id));
    }

    pub fn link_device(&self, name: String, kind: DeviceKind) {
        let id = self.id();
        self.devices.update(|d| {
            d.push(Device {
                id,
                name,
                kind,
                last_active: "Active now".into(),
                this_device: false,
            })
        });
    }

    pub fn finish_onboarding(&self) {
        if let Err(e) = Storage::set_item(ONBOARDED, "1") {
            log::warn!("{e}");
        }
        self.session.update(|s| s.onboarded = true);
    }

    /// Starts the first run again (Help → Replay welcome).
    pub fn replay_onboarding(&self) {
        Storage::remove_item(ONBOARDED);
        self.session.update(|s| s.onboarded = false);
    }

    pub fn set_offline(&self, offline: bool) {
        self.session.update(|s| s.offline = offline);
    }

    /// Mock: the username check app-core will ask the server for.
    pub fn username_available(&self, name: &str) -> bool {
        let taken = ["ana", "mihai", "admin", "songbird", "support"];
        name.len() >= 3 && !taken.contains(&name.to_lowercase().as_str())
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_days() {
        assert_eq!(clock(0), "10:24");
        assert_eq!(clock(-24), "10:00");
        assert_eq!(day_of(0), 0);
        assert_eq!(day_of(-60 * 11), -1);
        assert_eq!(when_label(-60 * 11), "Yesterday");
        assert_eq!(day_label(0), "Today");
    }

    #[test]
    fn link_previews_get_a_title_from_the_url() {
        assert_eq!(link_title("https://songbird.chat"), "Songbird");
        assert_eq!(link_title("https://www.songbird.chat/"), "Songbird");
        assert_eq!(
            link_title("https://songbird.chat/blog/privacy-first"),
            "Privacy first"
        );
        assert_eq!(
            link_title("https://example.com/docs/setup.html?x=1"),
            "Setup"
        );
        assert_eq!(link_site("https://www.example.com/a"), "example.com");
    }

    #[test]
    fn ticks_only_move_forward() {
        assert!(Tick::Read > Tick::Delivered);
        assert!(Tick::Delivered > Tick::Sent);
        assert!(Tick::Sent > Tick::Sending);
    }

    #[test]
    fn seed_is_consistent() {
        let s = seed::seed();
        // Every chat's messages belong to it, ids are unique per chat.
        for (chat, msgs) in &s.messages {
            assert!(msgs.iter().all(|m| m.chat == *chat));
            let mut ids: Vec<_> = msgs.iter().map(|m| m.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), msgs.len());
        }
        // The performance chat has 10,000 messages.
        assert_eq!(s.messages[&16].len(), 10_000);
        // Every direct chat's peer exists.
        for c in &s.chats {
            if let ChatKind::Direct(p) | ChatKind::WhatsApp(p) = c.kind {
                assert!(s.contacts.iter().any(|x| x.id == p), "chat {} peer", c.id);
            }
        }
    }

    #[test]
    fn chats_keep_their_themes() {
        let s = seed::seed();
        let theme = |id: ChatId| s.chats.iter().find(|c| c.id == id).map(|c| c.theme);
        assert_eq!(theme(2), Some(ChatTheme::Rose), "Familia");
        assert_eq!(theme(4), Some(ChatTheme::Slate), "Design team");
        assert_eq!(theme(9), Some(ChatTheme::Moss), "Weekend trips");
        assert_eq!(theme(1), Some(ChatTheme::Dusk), "everything else");
        assert_eq!(
            seed::chat(99, ChatKind::Group, "New").theme,
            ChatTheme::Dusk
        );
    }

    #[test]
    fn previews_are_one_line() {
        assert_eq!(Body::Text("a\nb".into()).preview(), "a");
        assert_eq!(
            Body::Voice {
                secs: 75,
                played: false
            }
            .preview(),
            "Voice message (1:15)"
        );
    }
}
