//! The Rust types of `proto/content.proto`, field for field (`prost`). The `.proto` file is the
//! specification; the vectors in `vectors/` and `tools/content-vectors` prove these types encode
//! as it says.
//!
//! `Debug` prints type names only: message content, keys and IDs never reach a log (invariant 6
//! applies to apps as much as to the server).
#![allow(
    missing_docs,
    clippy::doc_markdown,
    reason = "every field is documented in proto/content.proto, the specification"
)]
#![allow(
    clippy::large_enum_variant,
    reason = "protobuf oneofs mirror the schema; a message is decoded once and never moved hot"
)]

use std::fmt;

macro_rules! redacted_debug {
    ($($t:ty),* $(,)?) => {
        $(impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($t), " { .. }"))
            }
        })*
    };
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Plaintext {
    #[prost(bytes = "vec", tag = "1")]
    pub content: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub franking_key: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub profile_key: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Content {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(uint32, tag = "2")]
    pub required_version: u32,
    #[prost(bytes = "vec", tag = "3")]
    pub message_id: Vec<u8>,
    #[prost(uint64, tag = "4")]
    pub sent_at_ms: u64,
    #[prost(oneof = "content::Body", tags = "10, 11, 12, 13, 14, 15, 16, 17, 18")]
    pub body: Option<content::Body>,
}

pub mod content {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[prost(skip_debug)]
    pub enum Body {
        #[prost(message, tag = "10")]
        Data(super::DataMessage),
        #[prost(message, tag = "11")]
        Receipt(super::ReceiptMessage),
        #[prost(message, tag = "12")]
        Typing(super::TypingMessage),
        #[prost(message, tag = "13")]
        Sync(super::SyncMessage),
        #[prost(message, tag = "14")]
        RequestResponse(super::RequestResponse),
        #[prost(message, tag = "15")]
        Call(super::CallMessage),
        #[prost(message, tag = "16")]
        ProfileChanged(crate::profile::ProfileChanged),
        #[prost(message, tag = "17")]
        Presence(crate::presence::Presence),
        #[prost(message, tag = "18")]
        Attention(crate::presence::Attention),
    }
}

#[derive(Clone, PartialEq, Eq, Hash, ::prost::Message)]
#[prost(skip_debug)]
pub struct MessageRef {
    #[prost(bytes = "vec", tag = "1")]
    pub author: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub message_id: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Hash, ::prost::Message)]
#[prost(skip_debug)]
pub struct ChatRef {
    #[prost(bytes = "vec", tag = "1")]
    pub account: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub group_id: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct DataMessage {
    #[prost(bytes = "vec", tag = "1")]
    pub group_id: Vec<u8>,
    #[prost(uint32, tag = "2")]
    pub expire_timer_s: u32,
    #[prost(
        oneof = "data_message::Kind",
        tags = "10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22"
    )]
    pub kind: Option<data_message::Kind>,
}

pub mod data_message {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[prost(skip_debug)]
    pub enum Kind {
        #[prost(message, tag = "10")]
        Message(super::Message),
        #[prost(message, tag = "11")]
        Reaction(super::Reaction),
        #[prost(message, tag = "12")]
        Edit(super::Edit),
        #[prost(message, tag = "13")]
        Delete(super::Delete),
        #[prost(message, tag = "14")]
        PollVote(super::PollVote),
        #[prost(message, tag = "15")]
        PollClose(super::PollClose),
        #[prost(message, tag = "16")]
        Rsvp(super::Rsvp),
        #[prost(message, tag = "17")]
        Pin(super::Pin),
        #[prost(message, tag = "18")]
        TimerChange(super::TimerChange),
        #[prost(message, tag = "19")]
        Nudge(super::Nudge),
        #[prost(message, tag = "20")]
        CallEvent(super::CallEvent),
        #[prost(message, tag = "21")]
        GroupUpdate(super::GroupUpdate),
        #[prost(message, tag = "22")]
        ChatPolicy(super::ChatPolicy),
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Message {
    #[prost(message, optional, tag = "1")]
    pub text: Option<Text>,
    #[prost(message, repeated, tag = "2")]
    pub attachments: Vec<AttachmentPointer>,
    #[prost(message, optional, tag = "3")]
    pub quote: Option<Quote>,
    #[prost(uint32, tag = "4")]
    pub forward_count: u32,
    #[prost(message, repeated, tag = "5")]
    pub contacts: Vec<ContactCard>,
    #[prost(message, optional, tag = "6")]
    pub location: Option<LocationPin>,
    #[prost(message, optional, tag = "7")]
    pub sticker: Option<StickerRef>,
    #[prost(message, optional, tag = "8")]
    pub poll: Option<PollCreate>,
    #[prost(message, optional, tag = "9")]
    pub event: Option<EventCreate>,
    #[prost(bool, tag = "10")]
    pub view_once: bool,
    #[prost(message, repeated, tag = "11")]
    pub previews: Vec<LinkPreview>,
    #[prost(message, optional, tag = "12")]
    pub album: Option<Album>,
}

/// One of the photos and videos sent together, each its own message (D-0186).
#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Album {
    /// 16 random bytes, the same for every item of the album.
    #[prost(bytes = "vec", tag = "1")]
    pub id: Vec<u8>,
    /// This item's place, from 0.
    #[prost(uint32, tag = "2")]
    pub index: u32,
    /// How many items the album has, 2 to [`crate::validate::MAX_ALBUM`].
    #[prost(uint32, tag = "3")]
    pub count: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Text {
    #[prost(string, tag = "1")]
    pub body: String,
    #[prost(message, repeated, tag = "2")]
    pub spans: Vec<Span>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Span {
    #[prost(uint32, tag = "1")]
    pub start: u32,
    #[prost(uint32, tag = "2")]
    pub length: u32,
    #[prost(oneof = "span::Kind", tags = "3, 4")]
    pub kind: Option<span::Kind>,
}

pub mod span {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Style {
        Unspecified = 0,
        Bold = 1,
        Italic = 2,
        Strikethrough = 3,
        Monospace = 4,
        Spoiler = 5,
    }

    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[prost(skip_debug)]
    pub enum Kind {
        #[prost(enumeration = "Style", tag = "3")]
        Style(i32),
        #[prost(bytes = "vec", tag = "4")]
        Mention(Vec<u8>),
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Quote {
    #[prost(message, optional, tag = "1")]
    pub target: Option<MessageRef>,
    #[prost(message, optional, tag = "2")]
    pub text: Option<Text>,
    #[prost(string, tag = "3")]
    pub attachment_type: String,
    #[prost(message, optional, tag = "4")]
    pub thumbnail: Option<AttachmentPointer>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct AttachmentPointer {
    #[prost(bytes = "vec", tag = "1")]
    pub object_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub key: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub digest: Vec<u8>,
    #[prost(uint64, tag = "4")]
    pub size: u64,
    #[prost(string, tag = "5")]
    pub content_type: String,
    #[prost(string, tag = "6")]
    pub file_name: String,
    #[prost(uint32, tag = "7")]
    pub width: u32,
    #[prost(uint32, tag = "8")]
    pub height: u32,
    #[prost(string, tag = "9")]
    pub blurhash: String,
    #[prost(uint32, tag = "10")]
    pub duration_ms: u32,
    #[prost(uint32, tag = "11")]
    pub flags: u32,
    #[prost(bytes = "vec", tag = "12")]
    pub waveform: Vec<u8>,
    #[prost(string, tag = "13")]
    pub caption: String,
    #[prost(uint64, tag = "14")]
    pub uploaded_at_ms: u64,
}

pub mod attachment_pointer {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Flag {
        Unspecified = 0,
        VoiceNote = 1,
        Borderless = 2,
        Gif = 4,
        VideoNote = 8,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ContactCard {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(bytes = "vec", tag = "2")]
    pub account: Vec<u8>,
    #[prost(string, tag = "3")]
    pub username: String,
    #[prost(string, repeated, tag = "4")]
    pub phones: Vec<String>,
    #[prost(string, repeated, tag = "5")]
    pub emails: Vec<String>,
    #[prost(string, tag = "6")]
    pub organization: String,
    #[prost(message, optional, tag = "7")]
    pub avatar: Option<AttachmentPointer>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct LocationPin {
    #[prost(sint32, tag = "1")]
    pub latitude_e7: i32,
    #[prost(sint32, tag = "2")]
    pub longitude_e7: i32,
    #[prost(string, tag = "3")]
    pub name: String,
    #[prost(string, tag = "4")]
    pub address: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct StickerRef {
    #[prost(bytes = "vec", tag = "1")]
    pub pack_id: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub pack_key: Vec<u8>,
    #[prost(uint32, tag = "3")]
    pub sticker_id: u32,
    #[prost(string, tag = "4")]
    pub emoji: String,
    #[prost(message, optional, tag = "5")]
    pub data: Option<AttachmentPointer>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct LinkPreview {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub title: String,
    #[prost(string, tag = "3")]
    pub description: String,
    #[prost(message, optional, tag = "4")]
    pub image: Option<AttachmentPointer>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Reaction {
    #[prost(message, optional, tag = "1")]
    pub target: Option<MessageRef>,
    #[prost(string, tag = "2")]
    pub emoji: String,
    #[prost(bool, tag = "3")]
    pub remove: bool,
    #[prost(uint64, tag = "4")]
    pub counter: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Edit {
    #[prost(bytes = "vec", tag = "1")]
    pub target_message_id: Vec<u8>,
    #[prost(message, optional, tag = "2")]
    pub text: Option<Text>,
    #[prost(uint32, tag = "3")]
    pub revision: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Delete {
    #[prost(message, optional, tag = "1")]
    pub target: Option<MessageRef>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct PollCreate {
    #[prost(string, tag = "1")]
    pub question: String,
    #[prost(string, repeated, tag = "2")]
    pub options: Vec<String>,
    #[prost(bool, tag = "3")]
    pub multiple_choice: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct PollVote {
    #[prost(message, optional, tag = "1")]
    pub poll: Option<MessageRef>,
    #[prost(uint32, repeated, tag = "2")]
    pub options: Vec<u32>,
    #[prost(uint64, tag = "3")]
    pub counter: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct PollClose {
    #[prost(message, optional, tag = "1")]
    pub poll: Option<MessageRef>,
    #[prost(message, repeated, tag = "2")]
    pub heads: Vec<VoteHead>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct VoteHead {
    #[prost(bytes = "vec", tag = "1")]
    pub voter: Vec<u8>,
    #[prost(uint64, tag = "2")]
    pub counter: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct EventCreate {
    #[prost(string, tag = "1")]
    pub title: String,
    #[prost(string, tag = "2")]
    pub description: String,
    #[prost(uint64, tag = "3")]
    pub start_ms: u64,
    #[prost(uint64, tag = "4")]
    pub end_ms: u64,
    #[prost(message, optional, tag = "5")]
    pub location: Option<LocationPin>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Rsvp {
    #[prost(message, optional, tag = "1")]
    pub event: Option<MessageRef>,
    #[prost(enumeration = "rsvp::Answer", tag = "2")]
    pub answer: i32,
    #[prost(uint64, tag = "3")]
    pub counter: u64,
}

pub mod rsvp {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Answer {
        Unspecified = 0,
        Going = 1,
        Maybe = 2,
        Declined = 3,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Pin {
    #[prost(message, optional, tag = "1")]
    pub target: Option<MessageRef>,
    #[prost(bool, tag = "2")]
    pub unpin: bool,
    #[prost(uint32, tag = "3")]
    pub duration_s: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct TimerChange {
    #[prost(uint32, tag = "1")]
    pub expire_timer_s: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct Nudge {}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallEvent {
    #[prost(bytes = "vec", tag = "1")]
    pub call_id: Vec<u8>,
    #[prost(enumeration = "call_event::Kind", tag = "2")]
    pub kind: i32,
    #[prost(enumeration = "call_event::Outcome", tag = "3")]
    pub outcome: i32,
    #[prost(bool, tag = "4")]
    pub outgoing: bool,
    #[prost(uint64, tag = "5")]
    pub started_at_ms: u64,
    #[prost(uint32, tag = "6")]
    pub duration_s: u32,
    #[prost(bytes = "vec", tag = "7")]
    pub peer: Vec<u8>,
    #[prost(bytes = "vec", tag = "8")]
    pub group_id: Vec<u8>,
}

pub mod call_event {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Kind {
        Unspecified = 0,
        Audio = 1,
        Video = 2,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Outcome {
        Unspecified = 0,
        Accepted = 1,
        Missed = 2,
        Declined = 3,
        Cancelled = 4,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct GroupUpdate {
    #[prost(enumeration = "group_update::Kind", tag = "1")]
    pub kind: i32,
    #[prost(bytes = "vec", tag = "2")]
    pub actor: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "3")]
    pub subjects: Vec<Vec<u8>>,
    #[prost(string, tag = "4")]
    pub value: String,
    #[prost(bytes = "vec", tag = "5")]
    pub log_key: Vec<u8>,
}

pub mod group_update {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Kind {
        Unspecified = 0,
        Created = 1,
        MembersAdded = 2,
        MembersRemoved = 3,
        MemberLeft = 4,
        MemberJoinedByLink = 5,
        RoleChanged = 6,
        TitleChanged = 7,
        DescriptionChanged = 8,
        AvatarChanged = 9,
        SettingsChanged = 10,
        ChannelCreated = 11,
        ChannelSubscribersChanged = 12,
        DeviceJoined = 13,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ChatPolicy {
    #[prost(bool, tag = "1")]
    pub restrict_export: bool,
    #[prost(bool, tag = "2")]
    pub restrict_auto_save: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ReceiptMessage {
    #[prost(enumeration = "receipt_message::Kind", tag = "1")]
    pub kind: i32,
    #[prost(bytes = "vec", tag = "2")]
    pub group_id: Vec<u8>,
    #[prost(bytes = "vec", repeated, tag = "3")]
    pub message_ids: Vec<Vec<u8>>,
    #[prost(bytes = "vec", tag = "4")]
    pub up_to: Vec<u8>,
}

pub mod receipt_message {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Kind {
        Unspecified = 0,
        Delivered = 1,
        Read = 2,
        Played = 3,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct TypingMessage {
    #[prost(bool, tag = "1")]
    pub started: bool,
    #[prost(bytes = "vec", tag = "2")]
    pub group_id: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct RequestResponse {
    #[prost(enumeration = "request_response::Action", tag = "1")]
    pub action: i32,
    #[prost(message, optional, tag = "2")]
    pub chat: Option<ChatRef>,
}

pub mod request_response {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Action {
        Unspecified = 0,
        Accept = 1,
        Delete = 2,
        Block = 3,
        Report = 4,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct SyncMessage {
    #[prost(oneof = "sync_message::Kind", tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10")]
    pub kind: Option<sync_message::Kind>,
}

pub mod sync_message {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[prost(skip_debug)]
    pub enum Kind {
        #[prost(message, tag = "1")]
        Sent(super::SentTranscript),
        #[prost(message, tag = "2")]
        Read(super::ReadSync),
        #[prost(message, tag = "3")]
        Viewed(super::ReadSync),
        #[prost(message, tag = "4")]
        Starred(super::StarredSync),
        #[prost(message, tag = "5")]
        ChatState(super::ChatStateSync),
        #[prost(message, tag = "6")]
        Settings(super::SettingsSync),
        #[prost(message, tag = "7")]
        Blocked(super::BlockListSync),
        #[prost(message, tag = "8")]
        CallHistory(super::CallHistorySync),
        #[prost(message, tag = "9")]
        RequestResponse(super::RequestResponse),
        #[prost(message, tag = "10")]
        Storage(super::StorageSync),
    }
}

/// The account record store (Prompt 9): a new storage key after a rotation, or "fetch now".
#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct StorageSync {
    #[prost(bytes = "vec", tag = "1")]
    pub key: Vec<u8>,
    #[prost(bool, tag = "2")]
    pub changed: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct SentTranscript {
    #[prost(message, optional, tag = "1")]
    pub chat: Option<ChatRef>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    pub broadcast_to: Vec<Vec<u8>>,
    #[prost(bytes = "vec", tag = "3")]
    pub content: Vec<u8>,
    #[prost(bytes = "vec", tag = "4")]
    pub franking_key: Vec<u8>,
    #[prost(uint64, tag = "5")]
    pub stored_at_ms: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ReadSync {
    #[prost(message, repeated, tag = "1")]
    pub marks: Vec<ReadMark>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ReadMark {
    #[prost(message, optional, tag = "1")]
    pub message: Option<MessageRef>,
    #[prost(uint64, tag = "2")]
    pub at_ms: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct StarredSync {
    #[prost(message, repeated, tag = "1")]
    pub added: Vec<MessageRef>,
    #[prost(message, repeated, tag = "2")]
    pub removed: Vec<MessageRef>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ChatStateSync {
    #[prost(message, repeated, tag = "1")]
    pub chats: Vec<ChatState>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct ChatState {
    #[prost(message, optional, tag = "1")]
    pub chat: Option<ChatRef>,
    #[prost(bool, tag = "2")]
    pub archived: bool,
    #[prost(uint64, tag = "3")]
    pub muted_until_ms: u64,
    #[prost(bool, tag = "4")]
    pub pinned: bool,
    #[prost(bool, tag = "5")]
    pub marked_unread: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct SettingsSync {
    #[prost(bool, optional, tag = "1")]
    pub read_receipts: Option<bool>,
    #[prost(bool, optional, tag = "2")]
    pub typing_indicators: Option<bool>,
    #[prost(bool, optional, tag = "3")]
    pub link_previews: Option<bool>,
    #[prost(uint32, optional, tag = "4")]
    pub default_expire_timer_s: Option<u32>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct BlockListSync {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub accounts: Vec<Vec<u8>>,
    #[prost(bytes = "vec", repeated, tag = "2")]
    pub group_ids: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallHistorySync {
    #[prost(message, repeated, tag = "1")]
    pub calls: Vec<CallEvent>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallMessage {
    #[prost(bytes = "vec", tag = "1")]
    pub call_id: Vec<u8>,
    #[prost(oneof = "call_message::Kind", tags = "10, 11, 12, 13, 14, 15")]
    pub kind: Option<call_message::Kind>,
}

pub mod call_message {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[prost(skip_debug)]
    pub enum Kind {
        #[prost(message, tag = "10")]
        Offer(super::CallOffer),
        #[prost(message, tag = "11")]
        Answer(super::CallAnswer),
        #[prost(message, tag = "12")]
        Decline(super::CallDecline),
        #[prost(message, tag = "13")]
        Busy(super::CallBusy),
        #[prost(message, tag = "14")]
        Hangup(super::CallHangup),
        #[prost(message, tag = "15")]
        SenderKey(super::CallSenderKey),
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallOffer {
    #[prost(enumeration = "call_event::Kind", tag = "1")]
    pub media: i32,
    #[prost(bytes = "vec", tag = "2")]
    pub call_secret: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub group_id: Vec<u8>,
    #[prost(uint64, tag = "4")]
    pub ring_until_ms: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallAnswer {}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallDecline {}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallBusy {}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallHangup {
    #[prost(enumeration = "call_hangup::Reason", tag = "1")]
    pub reason: i32,
}

pub mod call_hangup {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Reason {
        Unspecified = 0,
        Normal = 1,
        AnsweredElsewhere = 2,
        DeclinedElsewhere = 3,
        BusyElsewhere = 4,
        Failed = 5,
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallSenderKey {
    #[prost(string, tag = "1")]
    pub identity: String,
    #[prost(uint32, tag = "2")]
    pub key_index: u32,
    #[prost(bytes = "vec", tag = "3")]
    pub key: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
#[prost(skip_debug)]
pub struct CallLinkCard {
    #[prost(bytes = "vec", tag = "1")]
    pub account: Vec<u8>,
    #[prost(bytes = "vec", tag = "2")]
    pub device: Vec<u8>,
    #[prost(string, tag = "3")]
    pub identity: String,
    #[prost(string, tag = "4")]
    pub name: String,
}

redacted_debug!(
    Plaintext,
    Album,
    Content,
    content::Body,
    MessageRef,
    ChatRef,
    DataMessage,
    data_message::Kind,
    Message,
    Text,
    Span,
    span::Kind,
    Quote,
    AttachmentPointer,
    ContactCard,
    LocationPin,
    StickerRef,
    LinkPreview,
    Reaction,
    Edit,
    Delete,
    PollCreate,
    PollVote,
    PollClose,
    VoteHead,
    EventCreate,
    Rsvp,
    Pin,
    TimerChange,
    Nudge,
    CallEvent,
    GroupUpdate,
    ChatPolicy,
    ReceiptMessage,
    TypingMessage,
    RequestResponse,
    SyncMessage,
    sync_message::Kind,
    SentTranscript,
    ReadSync,
    ReadMark,
    StarredSync,
    ChatStateSync,
    ChatState,
    SettingsSync,
    StorageSync,
    BlockListSync,
    CallHistorySync,
    CallMessage,
    call_message::Kind,
    CallOffer,
    CallAnswer,
    CallDecline,
    CallBusy,
    CallHangup,
    CallSenderKey,
    CallLinkCard,
);
