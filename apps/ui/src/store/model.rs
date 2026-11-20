//! The data app-core hands the UI. Plain Rust, no NSH: these types are
//! the shape of the future UniFFI/Rust interface of `crates/app-core`
//! (A2). The UI only reads them and sends intents (`store::Store`).

use crate::ui::Tick;

pub type ChatId = u64;
pub type ContactId = u64;
pub type MsgId = u64;

/// Minutes since the mock epoch. `store::now()` is "now".
pub type Minutes = i64;

#[derive(Debug, Clone, PartialEq)]
pub enum Presence {
    Online,
    Away,
    Busy,
    Invisible,
    /// Offline, last seen `Minutes` (None: hidden by their privacy setting).
    LastSeen(Option<Minutes>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    pub id: ContactId,
    pub name: String,
    pub username: String,
    pub about: String,
    /// Yahoo-style custom status ("In a meeting until 3").
    pub status_line: Option<String>,
    pub presence: Presence,
    /// The user compared safety numbers with them.
    pub verified: bool,
    /// Their key changed since the user last looked (calm notice).
    pub key_changed: bool,
    /// 60 digits in 12 groups of 5.
    pub safety_number: String,
    pub blocked: bool,
    /// The user turned nudges off for them.
    pub nudges_off: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Owner => "Owner",
            Role::Admin => "Admin",
            Role::Member => "Member",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// `None`: the user.
    pub contact: Option<ContactId>,
    pub role: Role,
}

/// What a group's roles may do (owners and admins toggle these).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupPermissions {
    pub members_edit_info: bool,
    pub members_add: bool,
    pub approve_joins: bool,
    pub members_send: bool,
    pub members_pin: bool,
    pub members_call: bool,
    pub admins_delete_any: bool,
}

impl Default for GroupPermissions {
    fn default() -> Self {
        Self {
            members_edit_info: false,
            members_add: false,
            approve_joins: true,
            members_send: true,
            members_pin: false,
            members_call: true,
            admins_delete_any: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatKind {
    Direct(ContactId),
    Group,
    /// A community's home; its groups point to it with `Chat::community`.
    Community,
    Channel {
        followers: u32,
        admin: bool,
    },
    /// A chat bridged to WhatsApp (A15); hides features WhatsApp lacks.
    WhatsApp(ContactId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disappear {
    Day,
    Week,
    FourWeeks,
}

impl Disappear {
    pub fn label(self) -> &'static str {
        match self {
            Disappear::Day => "24 hours",
            Disappear::Week => "7 days",
            Disappear::FourWeeks => "4 weeks",
        }
    }
}

/// A conversation's own look (REDESIGN.md "Per-chat themes"): it tints
/// the canvas and the user's bubbles. Synced with the chat as an
/// encrypted setting (A2); the colours live in the UI
/// (`features::chat::theme`), since they are presentation, not data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChatTheme {
    #[default]
    Dusk,
    Rose,
    Sea,
    Moss,
    Sand,
    Slate,
    Night,
}

impl ChatTheme {
    /// Every theme, in the picker's order: the default first, then warm
    /// to cool, the darkest last.
    pub const ALL: [ChatTheme; 7] = [
        ChatTheme::Dusk,
        ChatTheme::Rose,
        ChatTheme::Sand,
        ChatTheme::Moss,
        ChatTheme::Sea,
        ChatTheme::Slate,
        ChatTheme::Night,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChatTheme::Dusk => "Dusk",
            ChatTheme::Rose => "Rose",
            ChatTheme::Sea => "Sea",
            ChatTheme::Moss => "Moss",
            ChatTheme::Sand => "Sand",
            ChatTheme::Slate => "Slate",
            ChatTheme::Night => "Night",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chat {
    pub id: ChatId,
    pub kind: ChatKind,
    pub title: String,
    pub about: String,
    pub members: Vec<Member>,
    pub permissions: GroupPermissions,
    /// Admins only can send (announcement groups).
    pub announcement_only: bool,
    pub community: Option<ChatId>,
    pub pinned: bool,
    pub muted: bool,
    /// When a mute ends (unix ms); `u64::MAX` while muted always.
    pub muted_until: u64,
    pub archived: bool,
    pub favourite: bool,
    pub locked: bool,
    pub marked_unread: bool,
    pub unread: u32,
    pub mentions: u32,
    pub disappearing: Option<Disappear>,
    pub draft: String,
    pub typing: Option<String>,
    /// From someone not in the user's contacts: lives in Message requests.
    pub request: bool,
    /// Pending join requests (groups with approval on).
    pub join_requests: Vec<ContactId>,
    pub invite_link: String,
    /// Custom chat lists it belongs to ("Family", "Work").
    pub lists: Vec<String>,
    pub seed: u64,
    /// The conversation's canvas and bubble colours.
    pub theme: ChatTheme,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Text(String),
    Link {
        text: String,
        url: String,
        title: String,
        site: String,
    },
    Image {
        caption: String,
        alt: String,
        w: u32,
        h: u32,
        hd: bool,
    },
    Video {
        caption: String,
        secs: u32,
    },
    Voice {
        secs: u32,
        played: bool,
    },
    File {
        name: String,
        size: String,
        kind: String,
    },
    Poll {
        question: String,
        options: Vec<(String, u32)>,
        /// The options the user chose.
        mine: Vec<usize>,
        multiple: bool,
        /// Its author closed it: the votes are final.
        closed: bool,
    },
    Event {
        title: String,
        when: String,
        place: String,
        going: u32,
        mine: Option<bool>,
    },
    Contact {
        name: String,
        username: String,
    },
    Location {
        label: String,
        live: bool,
        /// Latitude and longitude, when the place is real (a map shows it).
        at: Option<(f64, f64)>,
    },
    Sticker {
        emoji: String,
        pack: String,
    },
    ViewOnce {
        photo: bool,
        opened: bool,
    },
    Nudge,
    Call {
        video: bool,
        missed: bool,
        secs: u32,
    },
    Deleted,
    /// Centred system notice ("Ana added Mihai").
    System(String),
}

impl Body {
    /// A one-line preview for the chat list and notifications.
    pub fn preview(&self) -> String {
        match self {
            Body::Text(t) => t.lines().next().unwrap_or_default().to_string(),
            Body::Link { text, .. } => text.clone(),
            Body::Image { caption, .. } if !caption.is_empty() => format!("Photo · {caption}"),
            Body::Image { .. } => "Photo".into(),
            Body::Video { caption, .. } if !caption.is_empty() => format!("Video · {caption}"),
            Body::Video { .. } => "Video".into(),
            Body::Voice { secs, .. } => format!("Voice message ({}:{:02})", secs / 60, secs % 60),
            Body::File { name, .. } => name.clone(),
            Body::Poll { question, .. } => format!("Poll: {question}"),
            Body::Event { title, .. } => format!("Event: {title}"),
            Body::Contact { name, .. } => format!("Contact: {name}"),
            Body::Location { label, live, .. } => {
                if *live {
                    format!("Live location · {label}")
                } else {
                    format!("Location · {label}")
                }
            }
            Body::Sticker { emoji, .. } => format!("{emoji} Sticker"),
            Body::ViewOnce { photo, .. } => {
                if *photo {
                    "View once photo".into()
                } else {
                    "View once video".into()
                }
            }
            Body::Nudge => "Nudge!".into(),
            Body::Call { video, missed, .. } => match (video, missed) {
                (true, true) => "Missed video call".into(),
                (false, true) => "Missed voice call".into(),
                (true, false) => "Video call".into(),
                (false, false) => "Voice call".into(),
            },
            Body::Deleted => "This message was deleted".into(),
            Body::System(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub emoji: String,
    /// Who reacted; `None` is the user.
    pub by: Vec<Option<ContactId>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: MsgId,
    pub chat: ChatId,
    /// `None`: sent by the user.
    pub from: Option<ContactId>,
    pub body: Body,
    pub at: Minutes,
    pub status: Tick,
    pub reply_to: Option<MsgId>,
    pub reactions: Vec<Reaction>,
    pub edited: bool,
    pub starred: bool,
    pub pinned: bool,
    pub forwarded: bool,
    /// Per-member delivery for Message info in groups: (member, delivered, read).
    pub receipts: Vec<(ContactId, Option<Minutes>, Option<Minutes>)>,
}

impl Message {
    pub fn mine(&self) -> bool {
        self.from.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDirection {
    Incoming,
    Outgoing,
    Missed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    pub id: u64,
    pub chat: ChatId,
    pub title: String,
    pub seed: u64,
    pub video: bool,
    pub group: bool,
    pub direction: CallDirection,
    pub at: Minutes,
    pub secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallLink {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub lobby: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Phone,
    Desktop,
    Tablet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: u64,
    pub name: String,
    pub kind: DeviceKind,
    pub last_active: String,
    pub this_device: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    pub username: String,
    pub about: String,
    pub status_line: Option<String>,
    pub presence: Presence,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Everyone,
    Contacts,
    Nobody,
}

impl Audience {
    pub fn label(self) -> &'static str {
        match self {
            Audience::Everyone => "Everyone",
            Audience::Contacts => "My contacts",
            Audience::Nobody => "Nobody",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
    HighContrast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Plus,
    Family,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupDestination {
    SongbirdServers,
    EncryptedFile,
    ICloudDrive,
    GoogleDrive,
    OneDrive,
    Dropbox,
    Nextcloud,
    S3,
}

impl BackupDestination {
    pub fn label(self) -> &'static str {
        match self {
            BackupDestination::SongbirdServers => "Songbird servers",
            BackupDestination::EncryptedFile => "Encrypted file",
            BackupDestination::ICloudDrive => "iCloud Drive",
            BackupDestination::GoogleDrive => "Google Drive",
            BackupDestination::OneDrive => "OneDrive",
            BackupDestination::Dropbox => "Dropbox",
            BackupDestination::Nextcloud => "Nextcloud (WebDAV)",
            BackupDestination::S3 => "S3 bucket",
        }
    }
}

/// Every user setting, synced across the user's devices as encrypted
/// records (A2). The UI writes it only through `Store::settings_update`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    // Privacy
    pub last_seen: Audience,
    pub online: Audience,
    pub photo: Audience,
    pub about: Audience,
    pub status: Audience,
    pub groups_add: Audience,
    pub read_receipts: bool,
    /// Typing indicators, sent and shown (Prompt 9).
    pub typing_indicators: bool,
    /// Found by username lookups (server-enforced, Prompt 9).
    pub discoverable: bool,
    /// Message requests from everyone (`false`: from nobody; server-enforced).
    pub message_requests: bool,
    /// The account is deleted after this many months unused: 6, 12 or 24
    /// (server-enforced).
    pub inactivity_months: u8,
    pub default_timer: Option<Disappear>,
    pub app_lock: bool,
    pub block_screenshots: bool,
    pub hide_in_switcher: bool,
    pub link_previews: bool,
    // Notifications
    pub notify_messages: bool,
    pub notify_groups: bool,
    pub notify_calls: bool,
    pub reactions_messages: bool,
    pub reactions_groups: bool,
    pub notify_sound: bool,
    pub hide_notification_content: bool,
    // Chats and appearance
    pub theme: ThemeChoice,
    pub chat_wallpaper: u8,
    pub animations: bool,
    pub default_reactions: Vec<String>,
    pub enter_sends: bool,
    pub keep_archived: bool,
    pub save_to_photos: bool,
    pub nudges: bool,
    pub sound_stickers_autoplay: bool,
    // Storage and data
    pub auto_photos: (bool, bool),
    pub auto_audio: (bool, bool),
    pub auto_video: (bool, bool),
    pub auto_docs: (bool, bool),
    pub hd_uploads: bool,
    pub low_data_calls: bool,
    pub proxy: Option<String>,
    // Backups
    pub backup_on: bool,
    pub backup_destination: BackupDestination,
    pub backup_include_media: bool,
    pub backup_daily: bool,
    pub backup_last: Option<String>,
    pub backup_size: String,
    // Account
    pub two_step_pin: bool,
    pub passkey: bool,
    pub plan: Plan,
    pub whatsapp_linked: bool,
    pub parental_controls: bool,
    pub crash_reports: bool,
    pub launch_at_login: bool,
}

/// The first-run state.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub onboarded: bool,
    pub recovery_key: String,
    pub invites_left: u32,
    pub invites_used: u32,
    pub offline: bool,
}
