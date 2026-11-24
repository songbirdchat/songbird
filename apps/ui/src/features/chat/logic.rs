//! The conversation's plain logic: grouping, text spans, durations,
//! waveforms, the forward limit. No NSH here, so it is unit-tested.

use crate::{
    store::{Body, Message, clock, day_label, day_of},
    ui::Tick,
};

/// At most this many chats per forward (WhatsApp's limit against spam).
pub const FORWARD_LIMIT: usize = 5;

/// Messages closer than this (minutes) from one sender read as one run.
const GROUP_GAP: i64 = 10;

/// A pause this long (minutes) earns a time label: times show where the
/// conversation rests, never in every bubble (REDESIGN.md "Quiet
/// metadata"). An hour, as iMessage: at 15 minutes a family chat that
/// trickles through the day had a label between nearly every run.
pub const TIME_GAP: i64 = 60;

/// Photos and videos sent together are drawn as one album (WhatsApp's rule): at least this many
/// in a row from one sender, each within [`ALBUM_GAP`] of the one before, none captioned or
/// quoting. Each stays its own message (receipts, reactions, deletion); only the list draws them
/// together.
pub const ALBUM_MIN: usize = 4;

/// At most this long (minutes) between two of an album's items.
const ALBUM_GAP: i64 = 1;

fn album_item(m: &Message) -> bool {
    m.reply_to.is_none()
        && match &m.body {
            Body::Image { caption, .. } | Body::Video { caption, .. } => caption.is_empty(),
            _ => false,
        }
}

fn joins_album(a: &Message, b: &Message) -> bool {
    // Messages that say which album they are in are grouped by that (rows), never by time.
    a.album.is_none()
        && b.album.is_none()
        && album_item(a)
        && album_item(b)
        && a.from == b.from
        && a.forwarded == b.forwarded
        && day_of(a.at) == day_of(b.at)
        && (b.at - a.at).abs() <= ALBUM_GAP
}

/// How long (minutes) an incoming album keeps places for items still on their way: past it, an
/// item that never came (deleted on this device, or never sent) leaves no place behind.
pub const ALBUM_WAIT: i64 = 5;

/// One row of the message list: a message on its own, or an album drawn at its first item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The message the row stands at: an album's first item in the chat.
    pub at: usize,
    /// An album's items here, in album order; empty for a message on its own.
    pub items: Vec<usize>,
    /// The album's places: its items, and those still on their way (0 on its own).
    pub count: u32,
}

impl Row {
    fn single(at: usize) -> Self {
        Self {
            at,
            items: Vec::new(),
            count: 0,
        }
    }
}

/// The list's rows for the chat's messages `v` (oldest first) at `now`. Items that say which
/// album they are in (D-0186) make one row at the first of them, in album order, whatever order
/// they arrived in; an album of fewer than [`ALBUM_MIN`] stays as single messages (as
/// WhatsApp). Older messages without an album fall back to [`album_span`].
pub fn rows(v: &[Message], now: i64) -> Vec<Row> {
    use std::collections::HashMap;
    // Each tagged album's items, by (sender, album id).
    let mut albums: HashMap<(Option<u64>, [u8; 16]), Vec<usize>> = HashMap::new();
    // Every place an album's messages fill, deleted ones too: a deleted item leaves the album
    // without leaving a place to wait for.
    let mut seen: HashMap<(Option<u64>, [u8; 16]), std::collections::HashSet<u32>> = HashMap::new();
    for (i, m) in v.iter().enumerate() {
        let Some(a) = m.album else { continue };
        seen.entry((m.from, a.id)).or_default().insert(a.index);
        if a.count >= ALBUM_MIN as u32 && matches!(m.body, Body::Image { .. } | Body::Video { .. })
        {
            albums.entry((m.from, a.id)).or_default().push(i);
        }
    }
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while let Some(m) = v.get(i) {
        // Only the album's photos and videos: a deleted item keeps its own row.
        let tagged = m
            .album
            .and_then(|a| albums.get(&(m.from, a.id)))
            .filter(|members| members.contains(&i));
        if let Some(members) = tagged {
            if members.first() == Some(&i) {
                let mut items = members.clone();
                items.sort_by_key(|&k| v.get(k).and_then(|x| x.album).map_or(0, |a| a.index));
                items.dedup_by_key(|k| v.get(*k).and_then(|x| x.album).map_or(0, |a| a.index));
                let declared = m.album.map_or(0, |a| a.count);
                let present = items.len() as u32;
                let filled = m
                    .album
                    .and_then(|a| seen.get(&(m.from, a.id)))
                    .map_or(present, |s| s.len() as u32);
                let newest = items
                    .iter()
                    .filter_map(|&k| v.get(k))
                    .map(|x| x.at)
                    .max()
                    .unwrap_or(m.at);
                let waiting = !m.mine() && now - newest <= ALBUM_WAIT;
                let count = if waiting {
                    present + declared.saturating_sub(filled)
                } else {
                    present
                };
                if count >= ALBUM_MIN as u32 {
                    out.push(Row {
                        at: i,
                        items,
                        count,
                    });
                } else {
                    out.extend(items.into_iter().map(Row::single));
                }
            }
            i += 1;
            continue;
        }
        match album_span(v, i) {
            Some((a, b)) if a == i => {
                out.push(Row {
                    at: i,
                    items: (a..=b).collect(),
                    count: (b - a + 1) as u32,
                });
                i = b + 1;
            }
            _ => {
                out.push(Row::single(i));
                i += 1;
            }
        }
    }
    // Rows go in the order of their first message, so an album never moves once drawn.
    out.sort_by_key(|r| r.at);
    out
}

/// The album the message at `index` belongs to, as its first and last index; `None` for a
/// message drawn on its own.
pub fn album_span(v: &[Message], index: usize) -> Option<(usize, usize)> {
    if !album_item(v.get(index)?) {
        return None;
    }
    let mut start = index;
    while let (Some(p), Some(c)) = (start.checked_sub(1).and_then(|i| v.get(i)), v.get(start)) {
        if !joins_album(p, c) {
            break;
        }
        start -= 1;
    }
    let mut end = index;
    while let (Some(c), Some(n)) = (v.get(end), v.get(end + 1)) {
        if !joins_album(c, n) {
            break;
        }
        end += 1;
    }
    (end - start + 1 >= ALBUM_MIN).then_some((start, end))
}

/// What a row shows around its message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Place {
    /// A new day starts here: its time label names the day too.
    pub day_break: bool,
    /// A time label goes above it (a new day, or a pause of
    /// [`TIME_GAP`] or more).
    pub time: bool,
    /// First of a run from one sender (name in groups, more space above).
    pub first: bool,
    /// Last of a run (the sender's avatar in groups; the run's joint
    /// corners end here).
    pub last: bool,
}

/// Whether a body is drawn as a sender's bubble (and so groups).
pub fn in_bubble(body: &Body) -> bool {
    !matches!(body, Body::System(_) | Body::Nudge)
}

fn same_run(a: &Message, b: &Message) -> bool {
    a.from == b.from
        && in_bubble(&a.body)
        && in_bubble(&b.body)
        && day_of(a.at) == day_of(b.at)
        && (b.at - a.at).abs() <= GROUP_GAP
}

/// The encryption notice a chat opens with: it heads the conversation,
/// above the first day's date (it belongs to no day).
pub fn opening_notice(m: &Message) -> bool {
    matches!(&m.body, Body::System(t) if t.contains("end-to-end encrypted"))
}

/// Where `cur` sits between its neighbours.
pub fn place(prev: Option<&Message>, cur: &Message, next: Option<&Message>) -> Place {
    let day_break = match prev {
        None => !opening_notice(cur),
        // The first message after the opening notice starts its day.
        Some(p) => day_of(p.at) != day_of(cur.at) || opening_notice(p),
    };
    let pause = prev.is_some_and(|p| cur.at - p.at >= TIME_GAP);
    Place {
        day_break,
        time: day_break || pause,
        first: prev.is_none_or(|p| !same_run(p, cur)),
        last: next.is_none_or(|n| !same_run(cur, n)),
    }
}

/// A time label's words: the clock ("09:24"), with the day at a day
/// break ("Today 09:24", "Yesterday 18:02", "7 Nov 09:24").
pub fn time_label(at: i64, day_break: bool) -> String {
    if day_break {
        format!("{} {}", day_label(at), clock(at))
    } else {
        clock(at)
    }
}

/// When a message was sent, in full (the desktop tooltip, message info):
/// "Today, 09:24".
pub fn sent_at(at: i64) -> String {
    format!("{}, {}", day_label(at), clock(at))
}

/// The status line under the user's latest message: "Sent", "Delivered",
/// "Read", in groups "Read by 4" once anyone read it; a failed send says
/// "Not sent" (its Retry is a button beside it).
pub fn status_words(status: Tick, group: bool, read_by: usize) -> String {
    match status {
        Tick::Sending => "Sending…".into(),
        Tick::Sent => "Sent".into(),
        Tick::Delivered => "Delivered".into(),
        Tick::Read | Tick::Played if group && read_by > 0 => format!("Read by {read_by}"),
        Tick::Read => "Read".into(),
        Tick::Played => "Played".into(),
        Tick::Failed => "Not sent".into(),
    }
}

/// `m:ss` for a duration.
pub fn duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// A call's length in words ("21 min", "45 sec").
pub fn call_length(secs: u32) -> String {
    if secs >= 3600 {
        format!("{} hr {} min", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} sec")
    }
}

/// A voice note's bar heights (0.2–1.0), the same for one message every
/// time (mock: app-core gives the real waveform from the audio).
pub fn waveform(seed: u64, bars: usize) -> Vec<f32> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..bars)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let noise = (x % 1000) as f32 / 1000.0;
            // A speech-like envelope: louder in the middle of phrases.
            let phrase = ((i as f32 / bars as f32) * std::f32::consts::PI * 3.0)
                .sin()
                .abs();
            (0.2 + 0.5 * noise + 0.3 * phrase).min(1.0)
        })
        .collect()
}

/// A piece of message text and how it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: bool,
    /// An `@name` mention (drawn in the mention colour, semibold).
    pub mention: bool,
}

impl Span {
    fn plain(text: &str) -> Self {
        Span {
            text: text.into(),
            bold: false,
            italic: false,
            link: false,
            mention: false,
        }
    }
}

/// Splits text into spans, `@name` mentions apart: an `@` that starts a
/// word, followed by letters.
fn push_mentions(out: &mut Vec<Span>, text: &str, bold: bool, italic: bool) {
    let mut plain_from = 0;
    let mut i = 0;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    while i < bytes.len() {
        let (at, c) = bytes[i];
        let word_start = i == 0 || bytes[i - 1].1.is_whitespace();
        let named = bytes.get(i + 1).is_some_and(|(_, n)| n.is_alphabetic());
        if c == '@' && word_start && named {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].1.is_alphanumeric() || bytes[j].1 == '.' || bytes[j].1 == '_')
            {
                j += 1;
            }
            // A trailing full stop ends the sentence, not the name.
            while j > i + 1 && bytes[j - 1].1 == '.' {
                j -= 1;
            }
            let end = bytes.get(j).map_or(text.len(), |b| b.0);
            if at > plain_from {
                out.push(Span {
                    bold,
                    italic,
                    ..Span::plain(&text[plain_from..at])
                });
            }
            out.push(Span {
                bold,
                italic,
                mention: true,
                ..Span::plain(&text[at..end])
            });
            plain_from = end;
            i = j;
            continue;
        }
        i += 1;
    }
    if plain_from < text.len() {
        out.push(Span {
            bold,
            italic,
            ..Span::plain(&text[plain_from..])
        });
    }
}

/// Where a `*bold*` / `_italic_` run that opens at `open` closes: the
/// marker must touch a word on both sides, as WhatsApp's do.
fn closing(chars: &[char], open: usize, marker: char) -> Option<usize> {
    let starts_word = open == 0 || chars[open - 1].is_whitespace();
    let next = *chars.get(open + 1)?;
    if !starts_word || next.is_whitespace() || next == marker {
        return None;
    }
    (open + 2..chars.len()).find(|&j| {
        chars[j] == marker
            && !chars[j - 1].is_whitespace()
            && chars.get(j + 1).is_none_or(|c| !c.is_alphanumeric())
    })
}

/// Splits plain text into spans, links apart (`https://` and `http://`).
fn push_links(out: &mut Vec<Span>, text: &str, bold: bool, italic: bool) {
    let mut rest = text;
    while !rest.is_empty() {
        let at = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(at) = at else {
            push_mentions(out, rest, bold, italic);
            return;
        };
        if at > 0 {
            push_mentions(out, &rest[..at], bold, italic);
        }
        let end = rest[at..]
            .find(char::is_whitespace)
            .map_or(rest.len(), |e| at + e);
        // A sentence's full stop or comma is not part of the link.
        let mut link_end = end;
        while link_end > at && rest[..link_end].ends_with(['.', ',', ')', '!', '?']) {
            link_end -= 1;
        }
        out.push(Span {
            text: rest[at..link_end].into(),
            bold,
            italic,
            link: true,
            mention: false,
        });
        rest = &rest[link_end..];
    }
}

/// A message's text as spans: `*bold*`, `_italic_` and links.
pub fn spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '*' || c == '_')
            && let Some(j) = closing(&chars, i, c)
        {
            push_links(&mut out, &std::mem::take(&mut plain), false, false);
            let inner: String = chars[i + 1..j].iter().collect();
            push_links(&mut out, &inner, c == '*', c == '_');
            i = j + 1;
            continue;
        }
        plain.push(c);
        i += 1;
    }
    push_links(&mut out, &plain, false, false);
    out.retain(|s| !s.text.is_empty());
    out
}

/// Whether the text needs spans at all (most messages are one plain run).
pub fn is_plain(text: &str) -> bool {
    !text.contains(['*', '_', '@']) && !text.contains("http")
}

/// Whether `text` matches a chat search (case- and accent-blind for the
/// common Romanian letters).
pub fn matches_search(text: &str, query: &str) -> bool {
    let fold = |s: &str| -> String {
        s.chars()
            .flat_map(char::to_lowercase)
            .map(|c| match c {
                'ă' | 'â' => 'a',
                'î' => 'i',
                'ș' | 'ş' => 's',
                'ț' | 'ţ' => 't',
                c => c,
            })
            .collect()
    };
    let q = fold(query.trim());
    !q.is_empty() && fold(text).contains(&q)
}

/// Whether the user may still add a chat to a forward of `chosen` chats.
pub fn can_forward_to_more(chosen: usize) -> bool {
    chosen < FORWARD_LIMIT
}

/// How long after sending the user may edit a message, in minutes.
pub const EDIT_WINDOW: i64 = 15;

/// Whether the user can edit `m`: their own text, sent within the last
/// [`EDIT_WINDOW`] minutes.
pub fn editable(m: &Message) -> bool {
    m.mine()
        && matches!(m.body, Body::Text(_) | Body::Link { .. })
        && crate::store::now() - m.at <= EDIT_WINDOW
}

/// The text Copy puts on the clipboard.
pub fn copy_text(m: &Message) -> String {
    match &m.body {
        Body::Text(t) => t.clone(),
        Body::Link { text, .. } => text.clone(),
        other => other.preview(),
    }
}

/// What a call row says.
pub fn call_title(video: bool, missed: bool, mine: bool) -> &'static str {
    match (video, missed, mine) {
        (true, true, _) => "Missed video call",
        (false, true, _) => "Missed voice call",
        (true, false, true) => "Outgoing video call",
        (false, false, true) => "Outgoing voice call",
        (true, false, false) => "Incoming video call",
        (false, false, false) => "Incoming voice call",
    }
}

/// The next voice note to play after `index` (consecutive notes play on).
pub fn next_voice(messages: &[Message], index: usize) -> Option<usize> {
    let next = messages.get(index + 1)?;
    matches!(next.body, Body::Voice { .. }).then_some(index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: u64, from: Option<u64>, at: i64, body: Body) -> Message {
        Message {
            id,
            chat: 1,
            from,
            body,
            at,
            status: Tick::Read,
            reply_to: None,
            reactions: Vec::new(),
            edited: false,
            starred: false,
            pinned: false,
            forwarded: false,
            receipts: Vec::new(),
            album: None,
        }
    }

    fn text(id: u64, from: Option<u64>, at: i64) -> Message {
        msg(id, from, at, Body::Text("hi".into()))
    }

    #[test]
    fn runs_group_by_sender_time_and_day() {
        let a = text(1, Some(2), -30);
        let b = text(2, Some(2), -28);
        let c = text(3, None, -27);
        assert_eq!(
            place(None, &a, Some(&b)),
            Place {
                day_break: true,
                time: true,
                first: true,
                last: false
            }
        );
        assert_eq!(
            place(Some(&a), &b, Some(&c)),
            Place {
                day_break: false,
                time: false,
                first: false,
                last: true
            }
        );
        assert_eq!(
            place(Some(&b), &c, None),
            Place {
                day_break: false,
                time: false,
                first: true,
                last: true
            }
        );
        // A long gap starts a new run.
        let late = text(4, Some(2), -5);
        assert!(place(Some(&b), &late, None).first);
        // A new day (23:59 → 00:04) breaks the run and adds a separator.
        let yesterday = text(5, Some(2), -625);
        let today = text(6, Some(2), -620);
        let p = place(Some(&yesterday), &today, None);
        assert!(p.day_break && p.first);
    }

    #[test]
    fn times_show_where_the_conversation_pauses() {
        let a = text(1, Some(2), -200);
        let soon = text(2, None, -141);
        let later = text(3, None, -81);
        assert!(!place(Some(&a), &soon, None).time, "59 minutes: no label");
        assert!(place(Some(&soon), &later, None).time, "an hour: a label");
        assert_eq!(time_label(-60, false), "09:24");
        assert_eq!(time_label(-60, true), "Today 09:24");
        assert_eq!(time_label(-60 * 11, true), "Yesterday 23:24");
        // A day break always has its label, even after a short pause.
        let yesterday = text(4, Some(2), -625);
        let today = text(5, Some(2), -620);
        assert!(place(Some(&yesterday), &today, None).time);
    }

    #[test]
    fn status_words_stay_quiet() {
        assert_eq!(status_words(Tick::Delivered, false, 0), "Delivered");
        assert_eq!(status_words(Tick::Read, false, 0), "Read");
        assert_eq!(status_words(Tick::Read, true, 4), "Read by 4");
        assert_eq!(status_words(Tick::Read, true, 0), "Read");
        assert_eq!(status_words(Tick::Failed, true, 0), "Not sent");
    }

    #[test]
    fn system_lines_do_not_group() {
        let s = msg(1, None, -10, Body::System("x".into()));
        let t = text(2, None, -9);
        assert!(place(Some(&s), &t, None).first);
        assert!(place(None, &s, Some(&t)).last);
    }

    #[test]
    fn spans_find_bold_italic_and_links() {
        let s = spans("Hey *you*, see https://songbird.chat/blog. _soon_");
        assert_eq!(s[0], Span::plain("Hey "));
        assert_eq!(
            s[1],
            Span {
                bold: true,
                ..Span::plain("you")
            }
        );
        assert_eq!(s[2], Span::plain(", see "));
        assert_eq!(
            s[3],
            Span {
                link: true,
                ..Span::plain("https://songbird.chat/blog")
            }
        );
        assert_eq!(s[4], Span::plain(". "));
        assert_eq!(
            s[5],
            Span {
                italic: true,
                ..Span::plain("soon")
            }
        );
    }

    #[test]
    fn mentions_are_their_own_spans() {
        let s = spans("@Radu can you bring the wine? mail me@x.y");
        assert_eq!(
            s[0],
            Span {
                mention: true,
                ..Span::plain("@Radu")
            }
        );
        assert_eq!(s[1], Span::plain(" can you bring the wine? mail me@x.y"));
        assert!(!is_plain("@Radu hi"));
    }

    #[test]
    fn the_encryption_notice_heads_the_chat() {
        let notice = msg(
            1,
            None,
            -60 * 24 * 3,
            Body::System("Messages are end-to-end encrypted.".into()),
        );
        let first = text(2, Some(2), -60 * 24 * 2);
        assert!(!place(None, &notice, Some(&first)).day_break);
        assert!(place(Some(&notice), &first, None).day_break);
        // Same day: the first message still gets its date.
        let same = text(3, Some(2), -60 * 24 * 3 + 5);
        assert!(place(Some(&notice), &same, None).day_break);
    }

    #[test]
    fn markers_inside_words_stay_text() {
        assert_eq!(
            spans("snake_case_name"),
            vec![Span::plain("snake_case_name")]
        );
        assert_eq!(spans("2 * 3 * 4"), vec![Span::plain("2 * 3 * 4")]);
        assert_eq!(spans("*unclosed"), vec![Span::plain("*unclosed")]);
        assert!(is_plain("Ce mai faci?"));
        assert!(!is_plain("see https://x.y"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(42), "0:42");
        assert_eq!(duration(95), "1:35");
        assert_eq!(call_length(1260), "21 min");
        assert_eq!(call_length(45), "45 sec");
        assert_eq!(call_length(3720), "1 hr 2 min");
    }

    #[test]
    fn waveform_is_stable_and_bounded() {
        let a = waveform(7, 30);
        assert_eq!(a, waveform(7, 30));
        assert_ne!(a, waveform(8, 30));
        assert!(a.iter().all(|h| (0.2..=1.0).contains(h)));
    }

    #[test]
    fn search_folds_case_and_diacritics() {
        assert!(matches_search("Ce mai faci? Totul bine?", "TOTUL"));
        assert!(matches_search("Ioana Ștefănescu", "stefanescu"));
        assert!(!matches_search("anything", "  "));
    }

    #[test]
    fn forward_stops_at_five() {
        assert!(can_forward_to_more(4));
        assert!(!can_forward_to_more(5));
    }

    #[test]
    fn only_own_text_is_editable() {
        assert!(editable(&text(1, None, 0)));
        assert!(!editable(&text(1, Some(3), 0)));
        // Within the window, not after it.
        assert!(editable(&text(1, None, -EDIT_WINDOW)));
        assert!(!editable(&text(1, None, -EDIT_WINDOW - 1)));
        assert!(!editable(&msg(
            1,
            None,
            0,
            Body::Voice {
                secs: 3,
                played: false
            }
        )));
    }

    #[test]
    fn voice_notes_play_on() {
        let v = |id| {
            msg(
                id,
                Some(2),
                0,
                Body::Voice {
                    secs: 3,
                    played: false,
                },
            )
        };
        let list = vec![v(1), v(2), text(3, Some(2), 0)];
        assert_eq!(next_voice(&list, 0), Some(1));
        assert_eq!(next_voice(&list, 1), None);
        assert_eq!(next_voice(&list, 2), None);
    }

    fn tagged(id: u64, from: Option<u64>, at: i64, album: u8, index: u32, count: u32) -> Message {
        let mut m = photo(id, from, at);
        m.album = Some(crate::store::AlbumTag {
            id: [album; 16],
            index,
            count,
        });
        m
    }

    fn shape(rows: &[Row]) -> Vec<(usize, Vec<usize>, u32)> {
        rows.iter()
            .map(|r| (r.at, r.items.clone(), r.count))
            .collect()
    }

    #[test]
    fn an_album_is_one_row_from_its_first_item() {
        let v: Vec<Message> = (0..5)
            .map(|i| tagged(i, Some(2), 0, 1, i as u32, 5))
            .collect();
        assert_eq!(shape(&rows(&v, 0)), vec![(0, vec![0, 1, 2, 3, 4], 5)]);
    }

    #[test]
    fn items_out_of_order_are_put_in_album_order_and_the_rest_wait() {
        // Places 2, 0 and 4 of 5 arrived (in that order); 1 and 3 are on their way.
        let v = vec![
            tagged(10, Some(2), 0, 1, 2, 5),
            tagged(11, Some(2), 0, 1, 0, 5),
            tagged(12, Some(2), 0, 1, 4, 5),
        ];
        assert_eq!(shape(&rows(&v, 1)), vec![(0, vec![1, 0, 2], 5)]);
        // Long after, nothing is waited for: three photos are not an album.
        assert_eq!(
            shape(&rows(&v, 60)),
            vec![(0, vec![], 0), (1, vec![], 0), (2, vec![], 0)]
        );
    }

    #[test]
    fn the_users_own_album_never_waits() {
        let v: Vec<Message> = [0, 1, 2, 4]
            .iter()
            .enumerate()
            .map(|(k, &i)| tagged(k as u64, None, 0, 1, i, 5))
            .collect();
        assert_eq!(shape(&rows(&v, 0)), vec![(0, vec![0, 1, 2, 3], 4)]);
    }

    #[test]
    fn small_albums_stay_single_photos() {
        let v: Vec<Message> = (0..3)
            .map(|i| tagged(i, Some(2), 0, 1, i as u32, 3))
            .collect();
        assert_eq!(
            shape(&rows(&v, 0)),
            vec![(0, vec![], 0), (1, vec![], 0), (2, vec![], 0)]
        );
    }

    #[test]
    fn a_message_in_between_keeps_its_row_and_the_album_stays_whole() {
        let mut v: Vec<Message> = (0..2)
            .map(|i| tagged(i, Some(2), 0, 1, i as u32, 4))
            .collect();
        v.push(text(9, None, 0));
        v.extend((2..4).map(|i| tagged(i, Some(2), 0, 1, i as u32, 4)));
        assert_eq!(
            shape(&rows(&v, 0)),
            vec![(0, vec![0, 1, 3, 4], 4), (2, vec![], 0)]
        );
    }

    #[test]
    fn a_deleted_item_leaves_the_album_without_a_place_to_wait_for() {
        let mut v: Vec<Message> = (0..5)
            .map(|i| tagged(i, Some(2), 0, 1, i as u32, 5))
            .collect();
        v[2].body = Body::Deleted;
        assert_eq!(
            shape(&rows(&v, 0)),
            vec![(0, vec![0, 1, 3, 4], 4), (2, vec![], 0)]
        );
    }

    #[test]
    fn albums_are_told_apart_by_sender_and_id_and_duplicates_count_once() {
        let mut v: Vec<Message> = (0..4)
            .map(|i| tagged(i, Some(2), 0, 1, i as u32, 4))
            .collect();
        v.extend((0..4).map(|i| tagged(10 + i, Some(3), 0, 1, i as u32, 4)));
        v.push(tagged(20, Some(2), 0, 1, 3, 4));
        let r = shape(&rows(&v, 0));
        assert_eq!(r[0], (0, vec![0, 1, 2, 3], 4));
        assert_eq!(r[1], (4, vec![4, 5, 6, 7], 4));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn untagged_photos_still_group_by_time() {
        let v: Vec<Message> = (0..4).map(|i| photo(i, Some(2), 0)).collect();
        assert_eq!(shape(&rows(&v, 0)), vec![(0, vec![0, 1, 2, 3], 4)]);
    }

    fn photo(id: u64, from: Option<u64>, at: i64) -> Message {
        msg(
            id,
            from,
            at,
            Body::Image {
                caption: String::new(),
                alt: String::new(),
                w: 4,
                h: 3,
                hd: false,
            },
        )
    }

    #[test]
    fn four_photos_sent_together_make_an_album() {
        let v: Vec<Message> = (0..5).map(|i| photo(i, Some(2), -10)).collect();
        for i in 0..5 {
            assert_eq!(album_span(&v, i as usize), Some((0, 4)));
        }
        // Three are not enough.
        assert_eq!(album_span(&v[..3], 1), None);
    }

    #[test]
    fn albums_break_on_sender_pause_caption_quote_and_other_bodies() {
        let mut v: Vec<Message> = (0..4).map(|i| photo(i, Some(2), -10)).collect();
        v.push(photo(4, None, -10)); // the user's own, after theirs
        v.extend((5..9).map(|i| photo(i, None, -10)));
        assert_eq!(album_span(&v, 0), Some((0, 3)));
        assert_eq!(album_span(&v, 4), Some((4, 8)));
        // A pause of more than a minute splits.
        let mut w: Vec<Message> = (0..4)
            .map(|i| photo(i, Some(2), -10 + i as i64 * 2))
            .collect();
        assert_eq!(album_span(&w, 1), None);
        // A caption or a quote takes a photo out; so does a text in between.
        w = (0..5).map(|i| photo(i, Some(2), -10)).collect();
        if let Body::Image { caption, .. } = &mut w[2].body {
            *caption = "look".into();
        }
        assert_eq!(album_span(&w, 0), None);
        w = (0..5).map(|i| photo(i, Some(2), -10)).collect();
        w[2].reply_to = Some(1);
        assert_eq!(album_span(&w, 4), None);
        w = (0..5).map(|i| photo(i, Some(2), -10)).collect();
        w.insert(2, text(9, Some(2), -10));
        assert_eq!(album_span(&w, 0), None);
    }
}
