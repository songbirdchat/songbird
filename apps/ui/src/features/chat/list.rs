//! The messages: a virtualized list resting at its end (a chat), each
//! cell bound to the message at its position. Date separators and sender
//! runs come from the neighbours; per-message state (playing, selected,
//! highlighted) lives in `Conv`, keyed by id.

use std::time::Duration;

use nsh_core::prelude::*;

use super::{Conv, bubbles, logic};
use crate::ui::floating;
use crate::{
    features::devices,
    store::{Body, ChatKind, Message, MsgId},
    ui::*,
};

/// A row's message and where it sits among its neighbours.
#[derive(Clone, PartialEq)]
pub(crate) struct Item {
    pub m: Message,
    pub place: logic::Place,
    /// An album's places in album order (its items, then any still on their way); empty for
    /// a message on its own.
    pub album: Vec<Tile>,
}

impl Item {
    /// A message drawn on its own (outside the list: the lifted copy, message info).
    pub fn single(m: Message, place: logic::Place) -> Self {
        Self {
            m,
            place,
            album: Vec::new(),
        }
    }
}

/// One place of an album: a photo or video, or one still on its way (`id` 0).
#[derive(Clone, PartialEq)]
pub(crate) struct Tile {
    pub id: MsgId,
    pub alt: String,
    pub w: u32,
    pub h: u32,
    /// A video's length in seconds.
    pub video: Option<u32>,
    /// The user's own, and its sending failed (the album's Retry resends it).
    pub failed: bool,
    /// The user's own, still uploading (a spinner over it).
    pub sending: bool,
}

impl Tile {
    fn of(m: &Message) -> Self {
        let (alt, w, h, video) = match &m.body {
            Body::Image { alt, w, h, .. } => (alt.clone(), *w, *h, None),
            Body::Video { caption, secs } => (format!("video {caption}"), 16, 9, Some(*secs)),
            _ => (String::new(), 1, 1, None),
        };
        Self {
            id: m.id,
            alt,
            w,
            h,
            video,
            failed: m.mine() && m.status == crate::ui::Tick::Failed,
            sending: m.mine() && m.status == crate::ui::Tick::Sending,
        }
    }

    /// A place for an item still on its way.
    fn waiting() -> Self {
        Self {
            id: 0,
            alt: String::new(),
            w: 1,
            h: 1,
            video: None,
            failed: false,
            sending: false,
        }
    }
}

/// A cell's current row (a memo: bindings run only when it changes).
pub(crate) type ItemMemo = Memo<Option<Item>>;

/// One field of the row, as a memo of its own.
pub(crate) fn sel<T: PartialEq + Default + 'static>(item: ItemMemo, f: fn(&Item) -> T) -> Memo<T> {
    computed(move || item.with(|i| i.as_ref().map(f).unwrap_or_default()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    System,
    Nudge,
    Bubble,
}

/// How far from the end still counts as "at the end".
const END_SLOP: f32 = 80.0;

/// The room the jump-to-latest button takes at the list's end (a
/// capsule's height and its 8pt above the composer).
const JUMP_ROOM: f32 = 56.0;

/// An album's status: failed if any photo failed, else the least advanced one's.
fn album_status(all: impl Iterator<Item = crate::ui::Tick>) -> crate::ui::Tick {
    use crate::ui::Tick;
    let rank = |t: Tick| match t {
        Tick::Failed => 0,
        Tick::Sending => 1,
        Tick::Sent => 2,
        Tick::Delivered => 3,
        Tick::Read => 4,
        Tick::Played => 5,
    };
    all.min_by_key(|t| rank(*t)).unwrap_or(Tick::Sent)
}

/// The list of messages.
pub(crate) fn message_list(cv: Conv) -> impl IntoElement {
    let (s, chat) = (cv.s, cv.chat);
    // The composer's frame is measured after the list first rests at its
    // end, and it grows with multi-line drafts, replies and the keyboard:
    // more than the follow slop, so the newest message would end up under
    // it. While following, the list goes back to its end each time.
    effect(move || {
        let (_, _) = (cv.bottom_inset.get(), cv.top_inset.get());
        if cv.following.get_untracked() {
            cv.list.scroll_to_end(false);
        }
    });
    // Opening a chat rests at its end by estimated row heights; tall rows (an album, a photo)
    // measure larger as they appear and move the end past the follow slop (found on two
    // iPhones: chats opened short of the newest message). While the chat settles, it goes back
    // to the end, unless the user has scrolled away.
    // Measuring can itself report "not at the end" for a moment, so this doesn't ask; a jump
    // to a message (a highlight) wins.
    for ms in [60, 200, 450, 900] {
        set_timeout(Duration::from_millis(ms), move || {
            if cv.highlight.get_untracked().is_none() {
                cv.list.scroll_to_end(false);
                cv.following.set(true);
            }
        });
    }
    // One row per message, or per album at its first item (D-0186): an album's items join its
    // row as they arrive, so nothing above or below moves.
    virtual_list(
        move || cv.rows.with(Vec::len),
        move |at: Position| row(cv, at),
    )
    .keys(move || {
        s.messages.with(|m| {
            let v = m.get(&chat);
            cv.rows.with(|r| {
                r.iter()
                    .map(|row| v.and_then(|v| v.get(row.at)).map_or(0, |x| x.id))
                    .collect::<Vec<_>>()
            })
        })
    })
    .estimated_item_size(px(58.0))
    .alignment(ListAlignment::Bottom)
    .follow_end(px(END_SLOP))
    .initial_scroll_at_end()
    // The messages scroll under the floating header and composer:
    // their measured frames keep the first and last messages clear.
    .content_padding_top(move || px(cv.top_inset.get() + 8.0))
    // While scrolled up, the jump-to-latest button floats over the
    // list's end: room for it, so it never covers the status line.
    .content_padding_bottom(move || {
        px(cv.bottom_inset.get()
            + if cv.following.get() {
                8.0
            } else {
                8.0 + JUMP_ROOM
            })
    })
    .footer(typing_indicator(cv))
    .keyboard_dismiss_mode(KeyboardDismissMode::Interactive)
    .event_throttle(100.0)
    .on_scroll(move |e: &ScrollEvent| {
        let at_end = e.offset.y + e.size.height >= e.content_size.height - px(END_SLOP);
        if cv.following.get_untracked() != at_end {
            cv.following.set(at_end);
        }
        if at_end && cv.unseen.get_untracked() > 0 {
            cv.unseen.set(0);
            cv.s.mark_read(cv.chat);
        }
    })
    .flex_1()
    .element_ref(&cv.list)
    .label("Messages")
}

/// A sent message rising in from the composer.
fn rise() -> MotionSpec {
    MotionSpec::new(super::timing(motion::SLOW, Easing::EaseOut))
        .values(MotionChannel::Opacity, [0.0, 1.0])
        .values(MotionChannel::TranslateY, [8.0, 0.0])
}

/// The rhythm of a conversation (REDESIGN.md "Conversation details"):
/// bubbles of one run 2pt apart, runs 10pt apart.
pub(crate) const IN_RUN: f32 = 2.0;
pub(crate) const BETWEEN_RUNS: f32 = 10.0;

/// The messages' gutters: 12pt on phones, 20pt on wide layouts.
pub(crate) fn gutter(cv: Conv) -> f32 {
    if cv.wide { 20.0 } else { 12.0 }
}

/// One cell: made once, then bound to whichever message it shows.
fn row(cv: Conv, at: Position) -> View {
    let (s, chat) = (cv.s, cv.chat);
    let item: ItemMemo = computed(move || {
        let n = at.index();
        cv.rows.with(|rows| {
            let r = rows.get(n)?;
            s.messages.with(|all| {
                let v = all.get(&chat)?;
                // The row's newest message speaks for an album (its time, its status line),
                // placed among the rows around it.
                let last = r.items.iter().copied().max().unwrap_or(r.at);
                let mut m = v.get(last)?.clone();
                let prev = n
                    .checked_sub(1)
                    .and_then(|p| rows.get(p))
                    .and_then(|p| v.get(p.items.iter().copied().max().unwrap_or(p.at)));
                let next = rows.get(n + 1).and_then(|x| v.get(x.at));
                let first = v.get(r.at)?;
                let mut place = logic::place(prev, first, next);
                place.last = logic::place(prev, &m, next).last;
                let album: Vec<Tile> = if r.items.is_empty() {
                    Vec::new()
                } else {
                    let members: Vec<&Message> = r.items.iter().filter_map(|&k| v.get(k)).collect();
                    // The album's status line speaks for all of it: a failure anywhere, else
                    // the least advanced of its photos (never "Read" while one is sending).
                    m.status = album_status(members.iter().map(|x| x.status));
                    let mut tiles: Vec<Tile> = members.into_iter().map(Tile::of).collect();
                    let waiting = (r.count as usize).saturating_sub(tiles.len());
                    tiles.extend(std::iter::repeat_with(Tile::waiting).take(waiting));
                    tiles
                };
                Some(Item { m, place, album })
            })
        })
    });
    let kind = computed(move || {
        item.with(|i| {
            i.as_ref().map(|i| match i.m.body {
                Body::System(_) => RowKind::System,
                Body::Nudge => RowKind::Nudge,
                _ => RowKind::Bubble,
            })
        })
    });
    let id = sel(item, |i| i.m.id);
    let first = sel(item, |i| i.place.first);
    let time = sel(item, |i| i.place.time);
    let time_text = sel(item, |i| logic::time_label(i.m.at, i.place.day_break));
    // The message just sent rises in once, whichever cell shows it.
    let enter = motion();
    effect(move || {
        let id = id.get();
        if id != 0 && cv.fresh.get_untracked() == Some(id) {
            cv.fresh.set(None);
            let _ = enter.play(rise());
        }
    });
    let lit = move || id.get() != 0 && cv.highlight.get() == Some(id.get());
    view()
        .w_full()
        .items_center()
        .bind_motion(&enter)
        .bg(move || {
            if lit() {
                c::highlight().opacity(0.55)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .transition_colors()
        .duration(motion::SLOW)
        .child(
            view()
                .w_full()
                .max_w(px(760.0))
                .px(px(gutter(cv)))
                .pt(move || px(if first.get() { BETWEEN_RUNS } else { IN_RUN }))
                .child(when(move || time.get(), move || time_label(time_text)))
                .child(dynamic(move || {
                    let k = kind.get()?;
                    Some(untracked(|| match k {
                        RowKind::System => system_row(cv, item).into_element(),
                        RowKind::Nudge => nudge_row(cv, item).into_element(),
                        RowKind::Bubble => bubble_row(cv, item),
                    }))
                })),
        )
}

/// Where the conversation paused: "09:24", or "Today 09:24" at a day
/// break. Plain centred text, no pill (REDESIGN.md "Quiet metadata").
fn time_label(label: Memo<String>) -> View {
    // 16pt above the label (the run gap and 6), 8pt below: it belongs to
    // the run under it.
    view()
        .w_full()
        .items_center()
        .pt(px(6.0))
        .pb(px(8.0))
        .child(
            text(move || label.get())
                .text_size(px(font::CAPTION))
                .line_height(px(16.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(move || c::muted_fg())
                .role(AccessibilityRole::Header),
        )
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Notice {
    #[default]
    Plain,
    Encryption,
    KeyChange,
    Timer,
}

fn notice_of(t: &str) -> Notice {
    if t.contains("end-to-end encrypted") {
        Notice::Encryption
    } else if t.contains("safety number changed") {
        Notice::KeyChange
    } else if t.contains("isappearing messages") {
        Notice::Timer
    } else {
        Notice::Plain
    }
}

/// The system line's text and its leading glyph (14pt).
fn notice_line(
    glyph: impl Fn() -> Icon + Copy + 'static,
    words: impl Fn() -> String + 'static,
) -> Text {
    text("")
        .text_size(px(font::CAPTION))
        .line_height(px(17.0))
        .text_center()
        .text_color(move || c::muted_fg())
        .child(dynamic(move || {
            let g = glyph();
            Some(untracked(|| icon(g, 14.0)))
        }))
        .child("\u{2002}")
        .child(text(words))
}

/// A centred notice: the encryption line at the top (lock), a key change
/// (calm, with Verify), the timer, members coming and going. Plain muted
/// text with its icon, no pill.
fn system_row(cv: Conv, item: ItemMemo) -> View {
    let (s, chat) = (cv.s, cv.chat);
    let raw = sel(item, |i| match &i.m.body {
        Body::System(t) => t.clone(),
        _ => String::new(),
    });
    let notice = computed(move || raw.with(|t| notice_of(t)));
    let shown = move || raw.with(|t| t.trim_end_matches(" Tap to verify.").to_string());
    let glyph = move || match notice.get() {
        Notice::Encryption => Icon::Lock,
        Notice::KeyChange => Icon::ShieldCheck,
        Notice::Timer => Icon::Timer,
        Notice::Plain => Icon::Info,
    };
    let verify = move || {
        if let Some(peer) = s.peer(chat) {
            devices::safety_number(peer.id);
        } else {
            super::chat_info(chat);
        }
    };
    view().w_full().items_center().py_1().child(
        pressable()
            .on_press(move || {
                if matches!(
                    notice.get_untracked(),
                    Notice::Encryption | Notice::KeyChange
                ) {
                    verify();
                }
            })
            .accessibility_label(super::say(move || raw.get()))
            .max_w(px(480.0))
            .px_3()
            .py_1()
            .gap_1()
            .items_center()
            .rounded(move || radius::control())
            .focus_visible(|st| st.outline(px(2.0)).outline_color(move || c::ring()))
            .child(notice_line(glyph, shown))
            .child(when(
                move || notice.get() == Notice::KeyChange,
                move || btn("Verify", Variant::Ghost, BtnSize::Sm, verify),
            )),
    )
}

/// A nudge: a centred line with a zap (the conversation shakes when one
/// arrives), as quiet as the other notices.
fn nudge_row(cv: Conv, item: ItemMemo) -> View {
    let s = cv.s;
    let line = sel(item, |i| {
        if i.m.mine() {
            "You sent a nudge".to_string()
        } else {
            String::new()
        }
    });
    let from = sel(item, |i| i.m.from);
    let label = move || {
        let l = line.get();
        if l.is_empty() {
            format!("{} sent you a nudge", s.contact_name(from.get()))
        } else {
            l
        }
    };
    view()
        .w_full()
        .items_center()
        .py_1()
        .label(label)
        .role(AccessibilityRole::Text)
        .child(notice_line(|| Icon::Zap, label))
}

/// A sender's message: selection mark, then the column (the sender's
/// name and avatar in groups, the bubble, reactions, the status line);
/// swiped right on phones, it replies.
fn bubble_row(cv: Conv, item: ItemMemo) -> AnyElement {
    let s = cv.s;
    let id = sel(item, |i| i.m.id);
    // An album is selected and unselected whole.
    let ids = sel(item, |i| {
        if i.album.is_empty() {
            vec![i.m.id]
        } else {
            i.album.iter().map(|t| t.id).filter(|&id| id != 0).collect()
        }
    });
    let people = s
        .chat(cv.chat)
        .is_some_and(|c| matches!(c.kind, ChatKind::Group | ChatKind::Community));
    let selected = move || ids.with(|ids| cv.selected.with(|v| ids.iter().all(|x| v.contains(x))));
    // While selecting, the marks make a column of their own at the
    // leading edge (for both sides' messages), and the chosen rows are
    // tinted: shape and colour.
    let content = view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .w_full()
        .rounded(move || radius::control())
        .bg(move || {
            if cv.selecting.get() && selected() {
                c::primary().opacity(0.1)
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .transition_colors()
        .duration(motion::INSTANT)
        .child(when(cv.selecting, move || select_mark(selected)))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(bubbles::bubble_column(cv, item, people)),
        );
    // While selecting, the whole row toggles the message.
    let picker = when(cv.selecting, move || {
        pressable()
            .absolute()
            .inset_0()
            .selected(selected)
            .accessibility_label(super::say(move || {
                if selected() {
                    "Selected message"
                } else {
                    "Select message"
                }
            }))
            .on_press(move || cv.toggle_all(ids.get_untracked()))
    });
    if is_desktop() {
        return view().child(content).child(picker).into_element();
    }
    // Phones: swipe right to reply. The bubble follows the finger 1:1,
    // the reply mark fades in; past the threshold, release replies.
    let x = animated_value(0.0);
    const REACH: f32 = 64.0;
    view()
        .child(
            view()
                .absolute()
                .left(px(4.0))
                .top_0()
                .bottom_0()
                .justify_center()
                .bind_opacity(&x.interpolate(&[0.0, REACH], &[0.0, 1.0]))
                .accessibility_elements_hidden(true)
                .child(
                    view()
                        .size(px(30.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || c::card())
                        .text_color(move || c::muted_fg())
                        .child(icon(Icon::Reply, 16.0)),
                ),
        )
        .child(
            view()
                .bind_translate_x(&x)
                .claim_touches(TouchClaim::on_pan(PanActivation::horizontal(
                    px(14.0),
                    px(12.0),
                )))
                .on_responder_move(move |e: &ResponderEvent| {
                    if cv.selecting.get_untracked() {
                        return;
                    }
                    let dx = e.gesture.delta.x.0.max(0.0);
                    x.set(if dx > REACH {
                        REACH + (dx - REACH) * 0.2
                    } else {
                        dx
                    });
                })
                .on_responder_release(move |e: &ResponderEvent| {
                    if !cv.selecting.get_untracked() && e.gesture.delta.x.0 >= REACH {
                        cv.start_reply(id.get_untracked());
                    }
                    let _ = x.timing(0.0, motion::BASE, Easing::EaseOut);
                })
                .on_responder_terminate(move |_: &ResponderEvent| {
                    let _ = x.timing(0.0, motion::FAST, Easing::EaseOut);
                })
                .child(content),
        )
        .child(picker)
        .into_element()
}

/// The round selection mark: a filled check when chosen (shape, not just
/// colour).
fn select_mark(selected: impl Fn() -> bool + Copy + 'static) -> View {
    view()
        .size(px(22.0))
        .ml(px(4.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(move || {
            if selected() {
                c::primary()
            } else {
                c::muted_fg()
            }
        })
        .bg(move || {
            if selected() {
                c::primary()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .text_color(move || c::on_primary())
        .accessibility_elements_hidden(true)
        .child(icon(Icon::Check, 14.0).opacity(move || if selected() { 1.0_f32 } else { 0.0 }))
}

/// Three dots breathing in a bubble while the other person types.
fn typing_indicator(cv: Conv) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let typing = computed(move || s.chat(chat).and_then(|c| c.typing));
    let who = move || match s.peer(chat) {
        Some(p) => format!("{} is typing", p.name),
        None => "Someone is typing".into(),
    };
    when(
        move || typing.with(Option::is_some),
        move || {
            let dots: Vec<Motion> = (0..3)
                .map(|i| {
                    let m = motion();
                    let spec = MotionSpec::new(super::looping(1100.0, i as f32 * 160.0))
                        .values(MotionChannel::Opacity, [0.6, 1.0, 0.35, 0.6])
                        .values(MotionChannel::TranslateY, [0.0, -3.0, 0.0, 0.0]);
                    let _ = m.play(spec);
                    m
                })
                .collect();
            view().w_full().items_center().pt(px(BETWEEN_RUNS)).child(
                view().w_full().max_w(px(760.0)).px(px(gutter(cv))).child(
                    view()
                        .self_start()
                        .flex_row()
                        .gap(px(4.0))
                        .px_4()
                        .py(px(13.0))
                        .rounded(px(bubbles::RADIUS))
                        .border_curve(BorderCurve::Continuous)
                        .bg(move || c::bubble_in())
                        .label(who())
                        .role(AccessibilityRole::Text)
                        .children(dots.into_iter().map(|m| {
                            view()
                                .size(px(7.0))
                                .rounded_full()
                                .bg(move || c::bubble_meta())
                                .bind_motion(&m)
                        })),
                ),
            )
        },
    )
    .into_element()
}

/// The round "jump to latest" button, with how many arrived meanwhile: a
/// small capsule over the list's end, just above the composer.
pub(crate) fn jump_button(cv: Conv) -> AnyElement {
    when(
        move || !cv.following.get(),
        move || {
            let pop = motion().entering(
                MotionSpec::new(super::timing(motion::FAST, Easing::EaseOut))
                    .values(MotionChannel::Opacity, [0.0, 1.0])
                    .values(MotionChannel::Scale, [0.9, 1.0]),
            );
            let side = floating::capsule_height();
            view()
                .absolute()
                // On the messages' gutter (the outgoing bubbles' edge).
                .right(px(gutter(cv)))
                .bottom(move || px(cv.bottom_inset.get() + 8.0))
                .bind_motion(&pop)
                .child(floating::capsule(
                    pressable()
                        .on_press(move || cv.to_latest())
                        .accessibility_label(super::say(move || match cv.unseen.get() {
                            0 => "Scroll to latest message".to_string(),
                            n => format!("Scroll to latest message, {n} new"),
                        }))
                        .size(px(side))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .text_color(move || c::foreground())
                        .cursor_pointer()
                        .transition()
                        .duration(motion::FAST)
                        .hover(|st| st.bg(move || c::foreground().opacity(0.06)))
                        .active(|st| {
                            if motion::reduced() {
                                st.opacity(0.85)
                            } else {
                                st.scale(motion::ICON_PRESS_SCALE)
                            }
                        })
                        .focus_visible(|st| {
                            st.outline(px(2.0))
                                .outline_offset(px(2.0))
                                .outline_color(move || c::ring())
                        })
                        .child(icon(Icon::ChevronDown, 20.0)),
                ))
                .child(when(
                    move || cv.unseen.get() > 0,
                    move || {
                        view()
                            .absolute()
                            .top(px(-8.0))
                            .right(px(-4.0))
                            .child(unread_badge(move || cv.unseen.get(), || false))
                    },
                ))
        },
    )
    .into_element()
}

/// Clears the highlight a jump left (used by search).
pub(crate) fn flash(cv: Conv, id: crate::store::MsgId) {
    cv.highlight.set(Some(id));
    let h = cv.highlight;
    set_timeout(Duration::from_millis(1400), move || {
        if h.get_untracked() == Some(id) {
            h.set(None);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::album_status;
    use crate::ui::Tick;

    #[test]
    fn an_album_shows_its_worst_photo() {
        let s = |v: &[Tick]| album_status(v.iter().copied());
        assert_eq!(
            s(&[Tick::Read, Tick::Read, Tick::Delivered]),
            Tick::Delivered
        );
        assert_eq!(
            s(&[Tick::Failed, Tick::Read, Tick::Read, Tick::Read]),
            Tick::Failed
        );
        assert_eq!(s(&[Tick::Read, Tick::Sending, Tick::Sent]), Tick::Sending);
    }
}
