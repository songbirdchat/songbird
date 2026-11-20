//! A6 and A7 additions to the store: roles, invite link settings, the
//! member change log, leaving, communities, following channels, packs and
//! the media the user keeps.
//!
//! **Mock.** The state the store has no field for lives in one global
//! beside it ([`GroupsState`]); app-core (A2) will own it. Every intent is
//! the call app-core will take over, marked `// mock:` where it differs.

use std::{collections::HashSet, time::Duration};

use nsh_core::prelude::*;

use super::{Body, Chat, ChatId, ChatKind, ContactId, Member, Minutes, NOW, Role, Store, seed};

// ------------------------------------------------------------------ roles

/// The user's role in a chat; `None` when the user is not a member.
pub fn my_role(chat: &Chat) -> Option<Role> {
    chat.members
        .iter()
        .find(|m| m.contact.is_none())
        .map(|m| m.role)
}

/// Owners and admins manage a group (permissions, members, invites).
pub fn can_manage(chat: &Chat) -> bool {
    matches!(my_role(chat), Some(Role::Owner | Role::Admin))
}

/// Who a member asks for something only admins can do: the owner, else
/// the first admin. Never the user.
pub fn who_to_ask(chat: &Chat) -> Option<(ContactId, Role)> {
    let pick = |role: Role| {
        chat.members
            .iter()
            .find(|m| m.role == role)
            .and_then(|m| m.contact.map(|c| (c, role)))
    };
    pick(Role::Owner).or_else(|| pick(Role::Admin))
}

/// Who becomes the owner when the owner leaves: the first admin, else the
/// first member. `None` when nobody else is left.
pub fn successor(chat: &Chat) -> Option<ContactId> {
    let others = || chat.members.iter().filter(|m| m.contact.is_some());
    others()
        .find(|m| m.role == Role::Admin)
        .or_else(|| others().next())
        .and_then(|m| m.contact)
}

/// Members in the order the info page lists them: the user first, then
/// owners, admins and members, each by name.
pub fn sorted_members(chat: &Chat, name: impl Fn(Option<ContactId>) -> String) -> Vec<Member> {
    let rank = |r: Role| match r {
        Role::Owner => 0,
        Role::Admin => 1,
        Role::Member => 2,
    };
    let mut out = chat.members.clone();
    out.sort_by(|a, b| {
        b.contact
            .is_none()
            .cmp(&a.contact.is_none())
            .then(rank(a.role).cmp(&rank(b.role)))
            .then_with(|| {
                name(a.contact)
                    .to_lowercase()
                    .cmp(&name(b.contact).to_lowercase())
            })
    });
    out
}

// ------------------------------------------------------------------ invite links

/// How long an invite link works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteExpiry {
    #[default]
    Never,
    Day,
    Week,
}

impl InviteExpiry {
    pub fn label(self) -> &'static str {
        match self {
            InviteExpiry::Never => "Never",
            InviteExpiry::Day => "1 day",
            InviteExpiry::Week => "7 days",
        }
    }
}

/// How many people can join with one link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteUses {
    #[default]
    Unlimited,
    One,
    Ten,
    Hundred,
}

impl InviteUses {
    pub fn label(self) -> &'static str {
        match self {
            InviteUses::Unlimited => "No limit",
            InviteUses::One => "1",
            InviteUses::Ten => "10",
            InviteUses::Hundred => "100",
        }
    }
    pub fn limit(self) -> Option<u32> {
        match self {
            InviteUses::Unlimited => None,
            InviteUses::One => Some(1),
            InviteUses::Ten => Some(10),
            InviteUses::Hundred => Some(100),
        }
    }
}

/// A group's invite link settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InviteSettings {
    pub expiry: InviteExpiry,
    pub uses: InviteUses,
    /// People who joined with the current link.
    pub used: u32,
}

/// One line under the link: "Never expires · No limit".
pub fn invite_summary(s: &InviteSettings) -> String {
    let expiry = match s.expiry {
        InviteExpiry::Never => "Never expires".to_string(),
        e => format!("Expires in {}", e.label()),
    };
    let uses = match s.uses.limit() {
        None => "No limit on joins".to_string(),
        Some(n) => {
            let left = n.saturating_sub(s.used);
            if left == 1 {
                "1 join left".to_string()
            } else {
                format!("{left} joins left")
            }
        }
    };
    format!("{expiry} · {uses}")
}

// ------------------------------------------------------------------ member changes

/// Real groups' change logs as read from app-core, newest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RealChanges(pub std::collections::HashMap<ChatId, Vec<MemberChange>>);

/// One line of a group's member change log.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberChange {
    pub at: Minutes,
    pub text: String,
}

// ------------------------------------------------------------------ channels

/// A channel in the directory (Find channels).
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryChannel {
    pub id: u64,
    pub name: &'static str,
    pub about: &'static str,
    pub followers: u32,
}

/// Mock: the directory app-core will fetch. Public channels are an owner
/// decision after legal review (DECISIONS.md, D-0065 §4), so these are
/// sample entries.
pub fn directory() -> Vec<DirectoryChannel> {
    // There is no public channel directory (public channels await the owner's legal review,
    // D-0065 §4): real accounts find channels by their invite links. The list is the demo's.
    if !crate::store::demo() {
        return Vec::new();
    }
    vec![
        DirectoryChannel {
            id: 9001,
            name: "Songbird News",
            about: "Product updates from the Songbird team.",
            followers: 18_420,
        },
        DirectoryChannel {
            id: 9002,
            name: "Cluj Events",
            about: "Concerts, talks and markets in Cluj-Napoca, weekly.",
            followers: 6_210,
        },
        DirectoryChannel {
            id: 9003,
            name: "Privacy Weekly",
            about: "One calm read about privacy every Friday.",
            followers: 41_870,
        },
        DirectoryChannel {
            id: 9004,
            name: "Romanian Recipes",
            about: "Sarmale, cozonac and everything in between.",
            followers: 12_905,
        },
        DirectoryChannel {
            id: 9005,
            name: "EU Digital Rights",
            about: "What new EU rules mean for you, in plain words.",
            followers: 3_340,
        },
    ]
}

/// "18,420 followers".
pub fn followers_label(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n == 1 {
        "1 follower".into()
    } else {
        format!("{out} followers")
    }
}

// ------------------------------------------------------------------ packs

/// What a pack holds. One service carries all four (D-0083).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Stickers,
    Scenes,
    SoundStickers,
    Emoticons,
}

impl PackKind {
    pub const ALL: [PackKind; 4] = [
        PackKind::Stickers,
        PackKind::Scenes,
        PackKind::SoundStickers,
        PackKind::Emoticons,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PackKind::Stickers => "Stickers",
            PackKind::Scenes => "Chat scenes",
            PackKind::SoundStickers => "Sound stickers",
            PackKind::Emoticons => "Emoticons",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub id: u64,
    pub kind: PackKind,
    pub name: String,
    pub by: String,
    /// Each item's emoji (a demo pack's stand-in for its artwork).
    pub items: Vec<String>,
    /// An installed (real) pack's asset numbers, item by item; empty for a demo pack.
    pub objects: Vec<u32>,
}

/// A pack being added from a link.
#[derive(Debug, Clone, PartialEq)]
pub struct PackInstall {
    pub link: String,
    pub progress: f32,
}

/// Checks a pack link: `https://songbird.chat/pack/<id>#<key>`. The key
/// after `#` never reaches the server (D-0083). Returns the pack id.
pub fn parse_pack_link(link: &str) -> Result<String, &'static str> {
    let rest = link
        .trim()
        .strip_prefix("https://songbird.chat/pack#")
        .ok_or("This isn't a Songbird pack link.")?;
    let (id, key) = rest
        .split_once('.')
        .ok_or("This link has no pack key. Ask for the full link.")?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("This isn't a Songbird pack link.");
    }
    if key.len() < 8 {
        return Err("This link has no pack key. Ask for the full link.");
    }
    Ok(id.to_string())
}

fn seed_packs() -> Vec<Pack> {
    let pack = |id, kind, name: &str, by: &str, items: &[&str]| Pack {
        id,
        kind,
        name: name.into(),
        by: by.into(),
        items: items.iter().map(|s| s.to_string()).collect(),
        objects: Vec::new(),
    };
    vec![
        pack(
            1,
            PackKind::Stickers,
            "Night owls",
            "Songbird",
            &["🦉", "🌙", "⭐", "😴", "☕", "📚", "🕯️", "🌌"],
        ),
        pack(
            2,
            PackKind::Stickers,
            "Cheers",
            "Ioana Ștefănescu",
            &["🙌", "🥂", "🎉", "👏", "💐", "🎂", "🥳", "✨"],
        ),
        pack(
            3,
            PackKind::Scenes,
            "Snowy evening",
            "Songbird",
            &["❄️", "🏔️", "🌲", "🔥"],
        ),
        pack(
            4,
            PackKind::SoundStickers,
            "Birdsong",
            "Songbird",
            &["🐦", "🎶", "🔔", "🥁"],
        ),
        pack(
            5,
            PackKind::Emoticons,
            "Classic faces",
            "Songbird",
            &["🙂", "😉", "😂", "😮", "😢", "😡", "😎", "🤔"],
        ),
    ]
}

// ------------------------------------------------------------------ stored media

/// A kind of file the user keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Photo,
    Video,
    Voice,
    Document,
}

impl FileKind {
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Photo => "Photo",
            FileKind::Video => "Video",
            FileKind::Voice => "Voice message",
            FileKind::Document => "Document",
        }
    }
}

/// A file stored on this device for a chat.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredFile {
    pub id: u64,
    pub chat: ChatId,
    pub name: String,
    pub kind: FileKind,
    pub bytes: u64,
    pub at: Minutes,
}

/// Mock: the files app-core's storage index will list for a chat, made up
/// deterministically from the chat's id.
pub fn files_for(chat: &Chat) -> Vec<StoredFile> {
    if matches!(chat.kind, ChatKind::Community) {
        return Vec::new();
    }
    let mut x = chat.id.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let count = 3 + (next() % 9) as usize;
    (0..count)
        .map(|i| {
            let r = next();
            let kind = match r % 7 {
                0 | 1 | 2 => FileKind::Photo,
                3 | 4 => FileKind::Video,
                5 => FileKind::Voice,
                _ => FileKind::Document,
            };
            let bytes = match kind {
                FileKind::Photo => 900_000 + r % 3_200_000,
                FileKind::Video => 8_000_000 + r % 90_000_000,
                FileKind::Voice => 60_000 + r % 900_000,
                FileKind::Document => 120_000 + r % 6_000_000,
            };
            let name = match kind {
                FileKind::Photo => format!("IMG_{:04}.jpg", 2000 + r % 8000),
                FileKind::Video => format!("VID_{:04}.mp4", 1000 + r % 9000),
                FileKind::Voice => format!("Voice message {}", i + 1),
                FileKind::Document => format!("Document {}.pdf", i + 1),
            };
            StoredFile {
                id: chat.id * 1000 + i as u64,
                chat: chat.id,
                name,
                kind,
                bytes,
                at: -((r % (60 * 24 * 40)) as i64),
            }
        })
        .collect()
}

/// "2.4 MB", in decimal units as phones show storage.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// Each chat's total, largest first (ties by chat id).
pub fn chat_totals(files: &[StoredFile]) -> Vec<(ChatId, u64)> {
    let mut totals: Vec<(ChatId, u64)> = Vec::new();
    for f in files {
        match totals.iter_mut().find(|(c, _)| *c == f.chat) {
            Some((_, t)) => *t += f.bytes,
            None => totals.push((f.chat, f.bytes)),
        }
    }
    totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    totals
}

/// A chat's files, largest first.
pub fn largest_first(files: &[StoredFile], chat: ChatId) -> Vec<StoredFile> {
    let mut out: Vec<StoredFile> = files.iter().filter(|f| f.chat == chat).cloned().collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.id.cmp(&b.id)));
    out
}

/// Mock network statistics since the last reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkUsage {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub media_sent: u64,
    pub media_received: u64,
    pub calls_sent: u64,
    pub calls_received: u64,
    pub since: &'static str,
}

impl NetworkUsage {
    pub fn total(&self) -> u64 {
        self.messages_sent
            + self.messages_received
            + self.media_sent
            + self.media_received
            + self.calls_sent
            + self.calls_received
    }
}

// ------------------------------------------------------------------ the mock state

/// What app-core will keep that the store has no field for yet.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupsState {
    pub invites: Vec<(ChatId, InviteSettings)>,
    pub history: Vec<(ChatId, MemberChange)>,
    pub packs: Vec<Pack>,
    pub installing: Option<PackInstall>,
    /// Files the user deleted (by id) from Manage storage.
    pub deleted_files: HashSet<u64>,
    pub usage: NetworkUsage,
    next: u64,
}

impl Default for GroupsState {
    fn default() -> Self {
        Self {
            invites: Vec::new(),
            history: Vec::new(),
            // A real account starts with none installed; the demo with sample packs.
            packs: if crate::store::demo() {
                seed_packs()
            } else {
                Vec::new()
            },
            installing: None,
            deleted_files: HashSet::new(),
            usage: NetworkUsage {
                messages_sent: 18_400_000,
                messages_received: 42_900_000,
                media_sent: 612_000_000,
                media_received: 1_840_000_000,
                calls_sent: 311_000_000,
                calls_received: 298_000_000,
                since: "1 September 2026",
            },
            next: 500_000,
        }
    }
}

/// The groups and media state of this surface.
pub fn groups_state() -> Signal<GroupsState> {
    global::<GroupsState>()
}

impl Store {
    // -------------------------------------------------------------- members and roles

    /// The display name of a member (`None`: the user, "You").
    pub fn member_name(&self, contact: Option<ContactId>) -> String {
        self.contact_name(contact)
    }

    fn log_change(&self, chat: ChatId, text: String) {
        groups_state().update(|g| g.history.push((chat, MemberChange { at: NOW, text })));
    }

    /// Makes `contact` an admin or a member again, with a system line.
    pub fn change_role(&self, chat: ChatId, contact: ContactId, role: Role) {
        self.set_role(chat, contact, role);
        let name = self.contact_name(Some(contact));
        let line = match role {
            Role::Admin => format!("You made {name} an admin"),
            Role::Member => format!("You dismissed {name} as admin"),
            Role::Owner => format!("You made {name} the owner"),
        };
        self.push_message(chat, None, Body::System(line.clone()));
        self.log_change(chat, line);
    }

    /// Removes a member (the store adds the system line).
    pub fn remove_from_group(&self, chat: ChatId, contact: ContactId) {
        self.remove_member(chat, contact);
        let name = self.contact_name(Some(contact));
        self.log_change(chat, format!("You removed {name}"));
    }

    /// Adds people (the store adds the system lines).
    pub fn add_to_group(&self, chat: ChatId, contacts: Vec<ContactId>) {
        for &c in &contacts {
            let name = self.contact_name(Some(c));
            self.log_change(chat, format!("You added {name}"));
        }
        self.add_members(chat, contacts);
    }

    /// Approves or rejects a join request.
    pub fn answer_request(&self, chat: ChatId, contact: ContactId, approve: bool) {
        if crate::backend::chat::owns(chat) {
            crate::backend::chat::answer_request(chat, contact, approve);
        }
        let name = self.contact_name(Some(contact));
        self.answer_join_request(chat, contact, approve);
        if approve {
            self.log_change(chat, format!("You approved {name}'s request to join"));
        } else {
            self.log_change(chat, format!("You rejected {name}'s request to join"));
        }
    }

    /// The member change log, newest first. A real group's comes from its encrypted change
    /// log (D-0156, read when the page opens); a demo group's is made up from its members.
    pub fn member_history(&self, chat: ChatId) -> Vec<MemberChange> {
        if crate::backend::chat::owns(chat) {
            return global::<RealChanges>().with(|r| r.0.get(&chat).cloned().unwrap_or_default());
        }
        let Some(c) = self.chat(chat) else {
            return Vec::new();
        };
        let mut out: Vec<MemberChange> = groups_state().with(|g| {
            g.history
                .iter()
                .rev()
                .filter(|(id, _)| *id == chat)
                .map(|(_, m)| m.clone())
                .collect()
        });
        let creator = c
            .members
            .iter()
            .find(|m| m.role == Role::Owner)
            .map(|m| m.contact)
            .unwrap_or(None);
        let creator_name = self.contact_name(creator);
        let mut past = Vec::new();
        for (i, m) in c.members.iter().enumerate() {
            if m.contact == creator {
                continue;
            }
            let at = -60 * 24 * (30 - i as i64 * 3);
            // "You" starts a line; mid-sentence the user is "you".
            let subject = self.contact_name(m.contact);
            let name = if m.contact.is_none() {
                "you".to_string()
            } else {
                subject.clone()
            };
            if i % 2 == 0 {
                past.push(MemberChange {
                    at,
                    text: format!("{subject} joined using the invite link"),
                });
            } else {
                past.push(MemberChange {
                    at,
                    text: format!("{creator_name} added {name}"),
                });
            }
            if m.role == Role::Admin {
                past.push(MemberChange {
                    at: at + 60,
                    text: format!("{creator_name} made {name} an admin"),
                });
            }
        }
        past.push(MemberChange {
            at: -60 * 24 * 40,
            text: format!("{creator_name} created the group"),
        });
        past.sort_by(|a, b| b.at.cmp(&a.at));
        out.extend(past);
        out
    }

    /// Leaves a group. An owner who leaves hands the group to
    /// [`successor`].
    pub fn exit_group(&self, chat: ChatId) {
        if crate::backend::chat::owns(chat) {
            crate::backend::chat::leave_group(chat);
        }
        let Some(c) = self.chat(chat) else { return };
        let was_owner = my_role(&c) == Some(Role::Owner);
        let heir = if was_owner { successor(&c) } else { None };
        self.update_chat(chat, |c| {
            c.members.retain(|m| m.contact.is_some());
            if let Some(h) = heir {
                if let Some(m) = c.members.iter_mut().find(|m| m.contact == Some(h)) {
                    m.role = Role::Owner;
                }
            }
        });
        self.push_message(chat, None, Body::System("You left".into()));
        self.log_change(chat, "You left".into());
    }

    // -------------------------------------------------------------- invite links

    pub fn invite_settings(&self, chat: ChatId) -> InviteSettings {
        groups_state().with(|g| {
            g.invites
                .iter()
                .find(|(c, _)| *c == chat)
                .map(|(_, s)| *s)
                .unwrap_or_default()
        })
    }

    pub fn set_invite_settings(&self, chat: ChatId, f: impl FnOnce(&mut InviteSettings)) {
        let mut s = self.invite_settings(chat);
        f(&mut s);
        groups_state().update(|g| {
            g.invites.retain(|(c, _)| *c != chat);
            g.invites.push((chat, s));
        });
    }

    /// A new link; the old one stops working at once.
    pub fn reset_invite(&self, chat: ChatId) {
        self.reset_invite_link(chat);
        self.set_invite_settings(chat, |s| s.used = 0);
        self.log_change(chat, "You reset the invite link".into());
    }

    // -------------------------------------------------------------- communities and channels

    /// Creates a community the user owns, with its announcement group.
    pub fn create_community(&self, title: String, about: String) -> ChatId {
        // A real account: a real community on the server (D-0149).
        if crate::backend::registered() && !crate::store::demo() {
            let id = crate::backend::chat::create_community(*self, title.clone());
            self.update_chat(id, |c| c.about = about.clone());
            return id;
        }
        let id = self.id();
        let mut c = seed::chat(id, ChatKind::Community, &title);
        c.about = about;
        c.members = vec![Member {
            contact: None,
            role: Role::Owner,
        }];
        let ann = self.id();
        let mut a = seed::chat(ann, ChatKind::Group, "Announcements");
        a.community = Some(id);
        a.announcement_only = true;
        a.members = vec![Member {
            contact: None,
            role: Role::Owner,
        }];
        self.chats.update(|cs| {
            cs.push(c);
            cs.push(a);
        });
        self.push_message(
            ann,
            None,
            Body::System(format!("You created the community “{title}”")),
        );
        id
    }

    /// The groups of a community: Announcements first, then by title.
    pub fn community_groups(&self, community: ChatId) -> Vec<Chat> {
        let mut gs: Vec<Chat> = self.chats.with(|cs| {
            cs.iter()
                .filter(|c| c.community == Some(community))
                .cloned()
                .collect()
        });
        gs.sort_by(|a, b| {
            b.announcement_only
                .cmp(&a.announcement_only)
                .then(a.title.cmp(&b.title))
        });
        gs
    }

    /// Creates a private channel the user posts in (real accounts; D-0149).
    pub fn create_channel(&self, title: String) -> Option<ChatId> {
        crate::backend::registered().then(|| crate::backend::chat::create_channel(*self, title))
    }

    /// Links a group the user admins into a community (both sides'
    /// admins agree, D-0072).
    pub fn link_group(&self, community: ChatId, group: ChatId) {
        if crate::backend::chat::owns(group) {
            crate::backend::chat::community_add_group(community, group);
        }
        self.update_chat(group, |c| c.community = Some(community));
        let name = self.chat(community).map(|c| c.title).unwrap_or_default();
        self.push_message(
            group,
            None,
            Body::System(format!("You added this group to the community “{name}”")),
        );
    }

    /// Follows a channel from the directory, or returns the one followed.
    pub fn follow_channel(&self, entry: &DirectoryChannel) -> ChatId {
        if let Some(id) = self.followed(entry.name) {
            return id;
        }
        let id = self.id();
        let mut c = seed::chat(
            id,
            ChatKind::Channel {
                followers: entry.followers + 1,
                admin: false,
            },
            entry.name,
        );
        c.about = entry.about.into();
        self.chats.update(|cs| cs.push(c));
        id
    }

    /// The followed channel with this name.
    pub fn followed(&self, name: &str) -> Option<ChatId> {
        self.chats.with(|cs| {
            cs.iter()
                .find(|c| matches!(c.kind, ChatKind::Channel { .. }) && c.title == name)
                .map(|c| c.id)
        })
    }

    pub fn unfollow(&self, chat: ChatId) {
        self.delete_chat(chat);
    }

    // -------------------------------------------------------------- packs

    pub fn move_pack(&self, id: u64, up: bool) {
        crate::backend::packs::move_pack(id, up);
        groups_state().update(|g| {
            let Some(i) = g.packs.iter().position(|p| p.id == id) else {
                return;
            };
            let kind = g.packs[i].kind;
            let same: Vec<usize> = g
                .packs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.kind == kind)
                .map(|(j, _)| j)
                .collect();
            let at = same.iter().position(|&j| j == i).unwrap_or(0);
            let other = if up { at.checked_sub(1) } else { Some(at + 1) };
            if let Some(&j) = other.and_then(|o| same.get(o)) {
                g.packs.swap(i, j);
            }
        });
    }

    pub fn remove_pack(&self, id: u64) {
        crate::backend::packs::remove(id);
        groups_state().update(|g| g.packs.retain(|p| p.id != id));
    }

    /// Adds a pack from its link: fetches and decrypts it, showing progress. Returns the
    /// link's problem, if any. A real account's app-core fetches the manifest and assets with no
    /// account (D-0083, D-0159); the demo walks its progress on a timer.
    pub fn install_pack(&self, link: String, kind: PackKind) -> Result<(), &'static str> {
        let id = parse_pack_link(&link)?;
        if crate::backend::packs::live() {
            crate::backend::packs::install(link);
            return Ok(());
        }
        let state = groups_state();
        state.update(|g| {
            g.installing = Some(PackInstall {
                link: link.clone(),
                progress: 0.0,
            })
        });
        for step in 1..=10_u32 {
            set_timeout(Duration::from_millis(160 * step as u64), move || {
                state.update(|g| {
                    if let Some(i) = g.installing.as_mut() {
                        i.progress = step as f32 / 10.0;
                    }
                });
            });
        }
        let name = format!("Pack {}", id.chars().take(6).collect::<String>());
        set_timeout(Duration::from_millis(1800), move || {
            state.update(|g| {
                if g.installing.take().is_some() {
                    g.next += 1;
                    let pack_id = g.next;
                    g.packs.push(Pack {
                        id: pack_id,
                        kind,
                        name: name.clone(),
                        by: "Shared link".into(),
                        items: ["🌻", "🌈", "🍀", "🐝", "🌊", "🍉"]
                            .iter()
                            .map(|s| s.to_string())
                            .collect(),
                        objects: Vec::new(),
                    });
                }
            });
        });
        Ok(())
    }

    /// Stops adding a pack.
    pub fn cancel_install(&self) {
        groups_state().update(|g| g.installing = None);
    }

    // -------------------------------------------------------------- storage

    /// Every file kept on this device.
    pub fn stored_files(&self) -> Vec<StoredFile> {
        let deleted = groups_state().with(|g| g.deleted_files.clone());
        self.chats.with(|cs| {
            cs.iter()
                .flat_map(files_for)
                .filter(|f| !deleted.contains(&f.id))
                .collect()
        })
    }

    /// Deletes files from this device (the messages stay).
    pub fn delete_files(&self, ids: &[u64]) {
        groups_state().update(|g| g.deleted_files.extend(ids.iter().copied()));
    }

    pub fn reset_network_usage(&self) {
        groups_state().update(|g| {
            g.usage = NetworkUsage {
                messages_sent: 0,
                messages_received: 0,
                media_sent: 0,
                media_received: 0,
                calls_sent: 0,
                calls_received: 0,
                since: "today",
            }
        });
    }
}

/// Plain chats for other modules' tests.
#[cfg(test)]
pub mod tests_support {
    use super::*;

    pub fn group() -> Chat {
        seed::chat(1, ChatKind::Group, "Test group")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(id: ChatId) -> Chat {
        seed::seed().chats.into_iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn roles_in_the_seed() {
        assert_eq!(my_role(&seeded(2)), Some(Role::Owner));
        assert_eq!(my_role(&seeded(4)), Some(Role::Admin));
        assert_eq!(my_role(&seeded(9)), Some(Role::Member));
        assert!(can_manage(&seeded(4)));
        assert!(!can_manage(&seeded(9)));
    }

    #[test]
    fn a_member_is_told_whom_to_ask() {
        assert_eq!(who_to_ask(&seeded(4)), Some((1, Role::Owner)));
        assert_eq!(who_to_ask(&seeded(9)), Some((13, Role::Admin)));
    }

    #[test]
    fn the_first_admin_inherits_a_group() {
        assert_eq!(successor(&seeded(2)), Some(12));
        let mut lonely = seeded(2);
        lonely.members.retain(|m| m.contact.is_none());
        assert_eq!(successor(&lonely), None);
    }

    #[test]
    fn members_list_the_user_then_by_role() {
        let order = sorted_members(&seeded(4), |c| format!("{c:?}"));
        assert_eq!(order[0].contact, None);
        assert_eq!(order[1].role, Role::Owner);
        assert!(order[2..].iter().all(|m| m.role == Role::Member));
    }

    #[test]
    fn invite_summaries() {
        assert_eq!(
            invite_summary(&InviteSettings::default()),
            "Never expires · No limit on joins"
        );
        let s = InviteSettings {
            expiry: InviteExpiry::Week,
            uses: InviteUses::Ten,
            used: 9,
        };
        assert_eq!(invite_summary(&s), "Expires in 7 days · 1 join left");
    }

    #[test]
    fn pack_links() {
        assert_eq!(
            parse_pack_link("https://songbird.chat/pack#ab12.k3y-long-enough"),
            Ok("ab12".into())
        );
        assert!(parse_pack_link("https://songbird.chat/pack#ab12").is_err());
        assert!(parse_pack_link("https://example.com/pack#ab12.key12345").is_err());
        assert!(parse_pack_link("https://songbird.chat/pack#a/b.key12345").is_err());
    }

    #[test]
    fn bytes_and_followers() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(2_400_000), "2.4 MB");
        assert_eq!(format_bytes(184_000), "184 KB");
        assert_eq!(format_bytes(1_840_000_000), "1.8 GB");
        assert_eq!(followers_label(18_420), "18,420 followers");
        assert_eq!(followers_label(1), "1 follower");
        assert_eq!(followers_label(1_234_567), "1,234,567 followers");
    }

    #[test]
    fn storage_is_sorted_largest_first() {
        let files: Vec<StoredFile> = seed::seed().chats.iter().flat_map(files_for).collect();
        let totals = chat_totals(&files);
        assert!(totals.windows(2).all(|w| w[0].1 >= w[1].1));
        let (chat, total) = totals[0];
        let mine = largest_first(&files, chat);
        assert!(mine.windows(2).all(|w| w[0].bytes >= w[1].bytes));
        assert_eq!(mine.iter().map(|f| f.bytes).sum::<u64>(), total);
        // Deterministic: the same chat gives the same files.
        assert_eq!(files_for(&seeded(4)), files_for(&seeded(4)));
    }
}
