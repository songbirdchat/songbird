//! Bounds and shapes after decoding (`proto/content.proto` writes each bound next to its field).
//!
//! Decoding is already bounded by the plaintext's size (checked first, [`crate::MAX_PADDED`]); these
//! checks keep every list, string and ID to what the specification allows, so app code never
//! meets a 10,000-option poll or a 3-byte account ID. Unknown enum values are accepted (a newer
//! writer may add one; apps show the unknown as plain), the "unspecified" zero is not.
//! Unknown `oneof` members (a newer writer's new kind) decode as `None` and are accepted: apps show
//! "update to see this" when `required_version` asks for it, and otherwise ignore them.

use crate::schema::{
    AttachmentPointer, CallEvent, ChatRef, ContactCard, Content, DataMessage, EventCreate,
    GroupUpdate, LinkPreview, LocationPin, Message, MessageRef, PollClose, PollCreate, PollVote,
    Quote, ReceiptMessage, RequestResponse, SentTranscript, StickerRef, SyncMessage, Text,
    content::Body, data_message::Kind as DataKind, span, sync_message::Kind as SyncKind,
};
use crate::{Error, ID_LEN, KEY_LEN, MAX_CONTENT};

/// Longest text body.
pub const MAX_TEXT: usize = 32_768;
/// Longest quoted text.
pub const MAX_QUOTE_TEXT: usize = 2_048;
/// Most formatting spans and mentions in one text.
pub const MAX_SPANS: usize = 256;
/// Most items in one album (D-0186).
pub const MAX_ALBUM: u32 = 100;

/// Most attachments in one message.
pub const MAX_ATTACHMENTS: usize = 32;
/// Most poll options.
pub const MAX_POLL_OPTIONS: usize = 12;
/// Most entries in one receipt, read sync, starred sync or chat-state sync.
pub const MAX_BATCH: usize = 512;
/// Most group members a message names (vote heads, group updates).
pub const MAX_MEMBERS: usize = 1_024;
/// Most accounts of a broadcast list (the server's default limit, PROTOCOL §5.10).
pub const MAX_BROADCAST: usize = 256;
/// Most entries in a block list sync, per kind.
pub const MAX_BLOCKED: usize = 1_024;
/// Longest disappearing timer: 4 weeks.
pub const MAX_TIMER_S: u32 = 28 * 24 * 3600;
/// Shortest disappearing timer.
pub const MIN_TIMER_S: u32 = 5;
/// The pin durations: 24 hours, 7 days, 30 days.
pub const PIN_DURATIONS_S: [u32; 3] = [86_400, 604_800, 2_592_000];

type R = Result<(), Error>;

fn check(ok: bool, field: &'static str) -> R {
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(field))
    }
}

fn id(b: &[u8], field: &'static str) -> R {
    check(b.len() == ID_LEN, field)
}

fn opt_id(b: &[u8], field: &'static str) -> R {
    check(b.is_empty() || b.len() == ID_LEN, field)
}

fn len(s: &str, max: usize, field: &'static str) -> R {
    check(s.len() <= max, field)
}

fn nonempty(s: &str, max: usize, field: &'static str) -> R {
    check(!s.is_empty() && s.len() <= max, field)
}

fn ids(v: &[Vec<u8>], max: usize, field: &'static str) -> R {
    check(v.len() <= max && v.iter().all(|b| b.len() == ID_LEN), field)
}

fn some<'a, T>(v: Option<&'a T>, field: &'static str) -> Result<&'a T, Error> {
    v.ok_or(Error::Invalid(field))
}

/// A whole [`Content`].
///
/// # Errors
/// [`Error::Invalid`] naming the first field out of bounds.
pub fn content(c: &Content) -> R {
    check(c.version >= 1, "content.version")?;
    id(&c.message_id, "content.message_id")?;
    match &c.body {
        None => Ok(()),
        Some(Body::Data(d)) => data(d),
        Some(Body::Receipt(r)) => receipt(r),
        Some(Body::Typing(t)) => opt_id(&t.group_id, "typing.group_id"),
        Some(Body::Sync(s)) => sync(s),
        Some(Body::RequestResponse(r)) => request_response(r),
        Some(Body::Call(c)) => crate::call::validate(c),
        Some(Body::ProfileChanged(p)) => check(p.version > 0, "profile_changed.version"),
        Some(Body::Presence(p)) => {
            let state = crate::presence::State::try_from(p.state).ok();
            check(
                state.is_some_and(|s| s != crate::presence::State::Unspecified),
                "presence.state",
            )?;
            check(
                p.last_seen_ms == 0 || state == Some(crate::presence::State::Offline),
                "presence.last_seen_ms",
            )
        }
        Some(Body::Attention(a)) => check(
            a.kind == i32::from(crate::presence::AttentionKind::Nudge),
            "attention.kind",
        ),
    }
}

/// A message reference.
///
/// # Errors
/// [`Error::Invalid`].
pub fn message_ref(r: &MessageRef) -> R {
    id(&r.author, "ref.author")?;
    id(&r.message_id, "ref.message_id")
}

fn opt_ref(r: Option<&MessageRef>, field: &'static str) -> R {
    message_ref(some(r, field)?)
}

/// A chat: exactly one of an account or a group.
///
/// # Errors
/// [`Error::Invalid`].
pub fn chat_ref(c: &ChatRef) -> R {
    check(
        (c.account.len() == ID_LEN && c.group_id.is_empty())
            || (c.account.is_empty() && c.group_id.len() == ID_LEN),
        "chat",
    )
}

fn data(d: &DataMessage) -> R {
    opt_id(&d.group_id, "data.group_id")?;
    timer(d.expire_timer_s, "data.expire_timer_s")?;
    let Some(kind) = &d.kind else {
        return Ok(());
    };
    match kind {
        DataKind::Message(m) => message(m),
        DataKind::Reaction(r) => {
            opt_ref(r.target.as_ref(), "reaction.target")?;
            check(r.remove || !r.emoji.is_empty(), "reaction.emoji")?;
            len(&r.emoji, 32, "reaction.emoji")?;
            check(r.counter >= 1, "reaction.counter")
        }
        DataKind::Edit(e) => {
            id(&e.target_message_id, "edit.target_message_id")?;
            text(some(e.text.as_ref(), "edit.text")?, MAX_TEXT)?;
            check(e.revision >= 1, "edit.revision")
        }
        DataKind::Delete(x) => opt_ref(x.target.as_ref(), "delete.target"),
        DataKind::PollVote(v) => poll_vote(v),
        DataKind::PollClose(c) => poll_close(c),
        DataKind::Rsvp(r) => {
            opt_ref(r.event.as_ref(), "rsvp.event")?;
            check(r.answer != 0, "rsvp.answer")?;
            check(r.counter >= 1, "rsvp.counter")
        }
        DataKind::Pin(p) => {
            opt_ref(p.target.as_ref(), "pin.target")?;
            check(
                if p.unpin {
                    p.duration_s == 0
                } else {
                    PIN_DURATIONS_S.contains(&p.duration_s)
                },
                "pin.duration_s",
            )
        }
        DataKind::TimerChange(t) => timer(t.expire_timer_s, "timer.expire_timer_s"),
        DataKind::Nudge(_) | DataKind::ChatPolicy(_) => Ok(()),
        DataKind::CallEvent(c) => call(c),
        DataKind::GroupUpdate(g) => group_update(g),
    }
}

fn timer(s: u32, field: &'static str) -> R {
    check(s == 0 || (MIN_TIMER_S..=MAX_TIMER_S).contains(&s), field)
}

/// [`message`], for the crate's tests.
#[cfg(test)]
pub(crate) fn message_for_tests(m: &Message) -> R {
    message(m)
}

fn message(m: &Message) -> R {
    let visible = m.text.as_ref().is_some_and(|t| !t.body.is_empty())
        || !m.attachments.is_empty()
        || !m.contacts.is_empty()
        || m.location.is_some()
        || m.sticker.is_some()
        || m.poll.is_some()
        || m.event.is_some();
    check(visible, "message.empty")?;
    if let Some(t) = &m.text {
        text(t, MAX_TEXT)?;
    }
    check(
        m.attachments.len() <= MAX_ATTACHMENTS,
        "message.attachments",
    )?;
    for a in &m.attachments {
        attachment(a)?;
    }
    if let Some(q) = &m.quote {
        quote(q)?;
    }
    check(m.contacts.len() <= 8, "message.contacts")?;
    for c in &m.contacts {
        contact(c)?;
    }
    if let Some(l) = &m.location {
        location(l)?;
    }
    if let Some(s) = &m.sticker {
        sticker(s)?;
    }
    if let Some(p) = &m.poll {
        poll_create(p)?;
    }
    if let Some(e) = &m.event {
        event(e)?;
    }
    check(
        !m.view_once || !m.attachments.is_empty(),
        "message.view_once",
    )?;
    check(m.previews.len() <= 4, "message.previews")?;
    m.previews.iter().try_for_each(preview)?;
    if let Some(a) = &m.album {
        // An album item is a photo or video message, seen more than once.
        check(
            a.id.len() == 16
                && (2..=MAX_ALBUM).contains(&a.count)
                && a.index < a.count
                && !m.attachments.is_empty()
                && !m.view_once,
            "message.album",
        )?;
    }
    Ok(())
}

/// A text with its spans: offsets in UTF-8 bytes, on character boundaries, within the body.
///
/// # Errors
/// [`Error::Invalid`].
pub fn text(t: &Text, max: usize) -> R {
    len(&t.body, max, "text.body")?;
    check(t.spans.len() <= MAX_SPANS, "text.spans")?;
    for s in &t.spans {
        let start = usize::try_from(s.start).map_err(|_| Error::Invalid("span.start"))?;
        let length = usize::try_from(s.length).map_err(|_| Error::Invalid("span.length"))?;
        let end = start
            .checked_add(length)
            .ok_or(Error::Invalid("span.length"))?;
        check(
            length >= 1
                && end <= t.body.len()
                && t.body.is_char_boundary(start)
                && t.body.is_char_boundary(end),
            "span.range",
        )?;
        match &s.kind {
            None => return Err(Error::Invalid("span.kind")),
            Some(span::Kind::Style(st)) => check(*st != 0, "span.style")?,
            Some(span::Kind::Mention(a)) => id(a, "span.mention")?,
        }
    }
    Ok(())
}

fn quote(q: &Quote) -> R {
    opt_ref(q.target.as_ref(), "quote.target")?;
    if let Some(t) = &q.text {
        text(t, MAX_QUOTE_TEXT)?;
    }
    len(&q.attachment_type, 128, "quote.attachment_type")?;
    q.thumbnail.as_ref().map_or(Ok(()), attachment)
}

/// An attachment pointer.
///
/// # Errors
/// [`Error::Invalid`].
pub fn attachment(a: &AttachmentPointer) -> R {
    check(a.object_id.len() == KEY_LEN, "attachment.object_id")?;
    check(a.key.len() == 2 * KEY_LEN, "attachment.key")?;
    check(a.digest.len() == KEY_LEN, "attachment.digest")?;
    len(&a.content_type, 128, "attachment.content_type")?;
    len(&a.file_name, 256, "attachment.file_name")?;
    len(&a.blurhash, 128, "attachment.blurhash")?;
    check(a.waveform.len() <= 256, "attachment.waveform")?;
    len(&a.caption, 4_096, "attachment.caption")
}

fn contact(c: &ContactCard) -> R {
    len(&c.name, 256, "contact.name")?;
    opt_id(&c.account, "contact.account")?;
    len(&c.username, 64, "contact.username")?;
    check(
        c.phones.len() <= 8 && c.phones.iter().all(|p| p.len() <= 64),
        "contact.phones",
    )?;
    check(
        c.emails.len() <= 8 && c.emails.iter().all(|e| e.len() <= 256),
        "contact.emails",
    )?;
    len(&c.organization, 256, "contact.organization")?;
    c.avatar.as_ref().map_or(Ok(()), attachment)
}

fn location(l: &LocationPin) -> R {
    check(
        l.latitude_e7.unsigned_abs() <= 900_000_000
            && l.longitude_e7.unsigned_abs() <= 1_800_000_000,
        "location.coordinates",
    )?;
    len(&l.name, 256, "location.name")?;
    len(&l.address, 1_024, "location.address")
}

fn sticker(s: &StickerRef) -> R {
    id(&s.pack_id, "sticker.pack_id")?;
    check(s.pack_key.len() == KEY_LEN, "sticker.pack_key")?;
    len(&s.emoji, 32, "sticker.emoji")?;
    s.data.as_ref().map_or(Ok(()), attachment)
}

fn preview(p: &LinkPreview) -> R {
    nonempty(&p.url, 2_048, "preview.url")?;
    len(&p.title, 512, "preview.title")?;
    len(&p.description, 2_048, "preview.description")?;
    p.image.as_ref().map_or(Ok(()), attachment)
}

fn poll_create(p: &PollCreate) -> R {
    nonempty(&p.question, 512, "poll.question")?;
    check(
        (2..=MAX_POLL_OPTIONS).contains(&p.options.len())
            && p.options.iter().all(|o| !o.is_empty() && o.len() <= 256),
        "poll.options",
    )
}

fn poll_vote(v: &PollVote) -> R {
    opt_ref(v.poll.as_ref(), "vote.poll")?;
    check(v.counter >= 1, "vote.counter")?;
    check(v.options.len() <= MAX_POLL_OPTIONS, "vote.options")?;
    let mut seen = 0u32;
    for o in &v.options {
        // Indices below 12 fit the bit set; a repeat is refused.
        let bit = 1u32
            .checked_shl(*o)
            .filter(|_| usize::try_from(*o).is_ok_and(|i| i < MAX_POLL_OPTIONS));
        let Some(bit) = bit else {
            return Err(Error::Invalid("vote.options"));
        };
        check(seen & bit == 0, "vote.options")?;
        seen |= bit;
    }
    Ok(())
}

fn poll_close(c: &PollClose) -> R {
    opt_ref(c.poll.as_ref(), "close.poll")?;
    check(c.heads.len() <= MAX_MEMBERS, "close.heads")?;
    let mut voters: Vec<&[u8]> = Vec::with_capacity(c.heads.len());
    for h in &c.heads {
        id(&h.voter, "close.voter")?;
        check(h.counter >= 1, "close.counter")?;
        voters.push(&h.voter);
    }
    voters.sort_unstable();
    check(
        voters.windows(2).all(|w| w.first() != w.get(1)),
        "close.voter",
    )
}

fn event(e: &EventCreate) -> R {
    nonempty(&e.title, 512, "event.title")?;
    len(&e.description, 4_096, "event.description")?;
    check(e.end_ms == 0 || e.end_ms >= e.start_ms, "event.end_ms")?;
    e.location.as_ref().map_or(Ok(()), location)
}

fn call(c: &CallEvent) -> R {
    id(&c.call_id, "call.call_id")?;
    check(c.kind != 0, "call.kind")?;
    check(c.outcome != 0, "call.outcome")?;
    opt_id(&c.peer, "call.peer")?;
    opt_id(&c.group_id, "call.group_id")
}

fn group_update(g: &GroupUpdate) -> R {
    check(g.kind != 0, "group_update.kind")?;
    id(&g.actor, "group_update.actor")?;
    ids(&g.subjects, MAX_MEMBERS, "group_update.subjects")?;
    len(&g.value, 4_096, "group_update.value")?;
    check(
        g.log_key.is_empty() || g.log_key.len() == 32,
        "group_update.log_key",
    )
}

fn receipt(r: &ReceiptMessage) -> R {
    check(r.kind != 0, "receipt.kind")?;
    opt_id(&r.group_id, "receipt.group_id")?;
    ids(&r.message_ids, MAX_BATCH, "receipt.message_ids")?;
    opt_id(&r.up_to, "receipt.up_to")?;
    check(
        !r.message_ids.is_empty() || !r.up_to.is_empty(),
        "receipt.empty",
    )
}

fn request_response(r: &RequestResponse) -> R {
    check(r.action != 0, "request_response.action")?;
    chat_ref(some(r.chat.as_ref(), "request_response.chat")?)
}

fn sync(s: &SyncMessage) -> R {
    let Some(kind) = &s.kind else {
        return Ok(());
    };
    match kind {
        SyncKind::Sent(t) => sent(t),
        SyncKind::Read(r) | SyncKind::Viewed(r) => {
            check(r.marks.len() <= MAX_BATCH, "read.marks")?;
            r.marks
                .iter()
                .try_for_each(|m| opt_ref(m.message.as_ref(), "read.message"))
        }
        SyncKind::Starred(s) => {
            check(
                s.added.len() <= MAX_BATCH && s.removed.len() <= MAX_BATCH,
                "starred",
            )?;
            s.added.iter().chain(&s.removed).try_for_each(message_ref)
        }
        SyncKind::ChatState(c) => {
            check(c.chats.len() <= MAX_BATCH, "chat_state.chats")?;
            c.chats
                .iter()
                .try_for_each(|c| chat_ref(some(c.chat.as_ref(), "chat_state.chat")?))
        }
        SyncKind::Settings(s) => s
            .default_expire_timer_s
            .map_or(Ok(()), |t| timer(t, "settings.default_expire_timer_s")),
        SyncKind::Blocked(b) => {
            ids(&b.accounts, MAX_BLOCKED, "blocked.accounts")?;
            ids(&b.group_ids, MAX_BLOCKED, "blocked.group_ids")
        }
        SyncKind::CallHistory(h) => {
            check(h.calls.len() <= 256, "call_history.calls")?;
            h.calls.iter().try_for_each(call)
        }
        SyncKind::RequestResponse(r) => request_response(r),
        SyncKind::Storage(st) => check(st.key.is_empty() || st.key.len() == KEY_LEN, "storage.key"),
    }
}

/// A sent transcript carries one chat message, never another sync message (so nesting stops at
/// one level).
fn sent(t: &SentTranscript) -> R {
    match &t.chat {
        Some(c) => {
            chat_ref(c)?;
            check(t.broadcast_to.is_empty(), "sent.broadcast_to")?;
        }
        None => check(!t.broadcast_to.is_empty(), "sent.chat")?,
    }
    ids(&t.broadcast_to, MAX_BROADCAST, "sent.broadcast_to")?;
    check(
        t.franking_key.is_empty() || t.franking_key.len() == KEY_LEN,
        "sent.franking_key",
    )?;
    check(t.content.len() <= MAX_CONTENT, "sent.content")?;
    let inner = <Content as prost::Message>::decode(t.content.as_slice())
        .map_err(|_| Error::Invalid("sent.content"))?;
    check(matches!(inner.body, Some(Body::Data(_))), "sent.content")?;
    content(&inner)
}
