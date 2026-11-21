//! The bubbles: one look per `Body` variant, inside the shared frame
//! (forwarded label, reply quote, marks), and around it the sender's name
//! and avatar (groups), reactions and the status line. Every
//! part binds to the cell's current row, so a recycled cell shows the
//! next message without being made again unless its kind changes.

use nsh_core::{element::ImageSrc, prelude::*};

use super::{
    Conv, MenuAt,
    list::{ItemMemo, sel},
    logic,
    theme::{self, Ink},
};
use crate::{
    features::{calls, media},
    store::Body,
    ui::*,
};

/// What kind of body a bubble draws (its content is made per kind).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Text,
    Link,
    Image,
    Video,
    Voice,
    File,
    Poll,
    Event,
    Contact,
    Location,
    Sticker,
    ViewOnce,
    Call,
    Deleted,
}

impl Kind {
    fn of(b: &Body) -> Kind {
        match b {
            Body::Text(_) | Body::System(_) | Body::Nudge => Kind::Text,
            Body::Link { .. } => Kind::Link,
            Body::Image { .. } => Kind::Image,
            Body::Video { .. } => Kind::Video,
            Body::Voice { .. } => Kind::Voice,
            Body::File { .. } => Kind::File,
            Body::Poll { .. } => Kind::Poll,
            Body::Event { .. } => Kind::Event,
            Body::Contact { .. } => Kind::Contact,
            Body::Location { .. } => Kind::Location,
            Body::Sticker { .. } => Kind::Sticker,
            Body::ViewOnce { .. } => Kind::ViewOnce,
            Body::Call { .. } => Kind::Call,
            Body::Deleted => Kind::Deleted,
        }
    }
    /// Drawn edge to edge in the bubble (media).
    fn media(self) -> bool {
        matches!(self, Kind::Image | Kind::Video | Kind::Location)
    }
}

/// Whether the bubble ends in a paragraph its marks (starred, pinned,
/// edited) can sit inside (the marks take the last line's end when they
/// fit): text, links, and captioned photos and videos.
fn inline_meta(b: &Body) -> bool {
    match b {
        Body::Text(_) | Body::Link { .. } => true,
        Body::Image { caption, .. } | Body::Video { caption, .. } => !caption.is_empty(),
        _ => false,
    }
}

/// The bubble's padding: 12pt at the sides, 8pt above and below text.
/// With the body's line heights (20pt for 14pt text, 22pt for 16pt) the
/// cap height sits within half a point of the line box's centre, so 8pt
/// above and below put 13pt between the bubble's edge and the caps on
/// both sides: about the 12pt at the sides, an optically even margin.
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 8.0;
/// Media, cards and quotes sit this far in from the bubble's edge; their
/// radius is the bubble's minus this inset (concentric: 20 − 4 = 16).
const INSET: f32 = 4.0;
/// The bubble's corner (REDESIGN.md "Conversation details": 20, no
/// tails), drawn continuous (squircle) where the platform can.
pub(crate) const RADIUS: f32 = 20.0;
/// The corners a run's bubbles turn to each other on the sender's side.
const JOINED: f32 = 6.0;
/// An inner surface's corner (a quote, a photo, a card): concentric with
/// the bubble's.
const INNER: f32 = RADIUS - INSET;
/// The marks' text (starred, pinned, "Edited"), the status line's.
const META_SIZE: f32 = 11.0;
const META_LINE: f32 = 14.0;
/// The avatar beside a group's runs, and its gap to the bubbles.
const AVATAR: f32 = 28.0;
const AVATAR_GAP: f32 = 8.0;
/// At most this share of the row (REDESIGN.md: 72%).
const MAX_SHARE: f32 = 0.72;

/// A paragraph's line height (whole points: no blur on 1x screens).
fn line() -> f32 {
    if is_desktop() { 20.0 } else { 22.0 }
}

/// Cards carry the app's own controls (buttons, choices), drawn for the
/// app's surfaces: the user's cards sit on the theme's soft tint.
fn card_kind(b: &Body) -> bool {
    matches!(
        b,
        Body::Poll { .. }
            | Body::Event { .. }
            | Body::Contact { .. }
            | Body::Call { .. }
            | Body::File { .. }
            | Body::ViewOnce { .. }
    )
}

/// The colours inside the row's bubble, following the chat's look.
fn ink_of(cv: Conv, item: ItemMemo) -> impl Fn() -> Ink + Copy + 'static {
    let mine = sel(item, |i| i.m.mine());
    let card = sel(item, |i| card_kind(&i.m.body));
    move || Ink::of(cv.look.get(), mine.get(), card.get())
}

/// Opens the message menu on the bubble `frame` draws: measured first, so
/// the menu can lift a copy of it in place and sit around it.
fn open_menu(cv: Conv, item: ItemMemo, frame: ElementRef) {
    let Some((id, place)) = item.with_untracked(|i| i.as_ref().map(|i| (i.m.id, i.place))) else {
        return;
    };
    if cv.selecting.get_untracked() || id == 0 {
        return;
    }
    spawn(async move {
        let Some(m) = frame.measure().await else {
            return;
        };
        let (w, h) = (m.frame.size.width.0, m.frame.size.height.0);
        cv.menu.set(Some(MenuAt {
            id,
            x: m.page.x.0,
            y: m.page.y.0,
            w,
            h,
            place,
        }));
    });
}

/// The bubble and what hangs around it: in groups the sender's name over
/// the run's first bubble and their avatar beside its last; reactions on
/// the bubble's bottom edge; the status line under the user's latest
/// message (or under any that failed).
///
/// Edges: the name starts where the bubble's text starts (the avatar's
/// column, its gap and the bubble's padding in), and the avatar's bottom
/// is the bubble's bottom (they share a row aligned to its end).
pub(crate) fn bubble_column(cv: Conv, item: ItemMemo, people: bool) -> View {
    let s = cv.s;
    let mine = sel(item, |i| i.m.mine());
    let last = sel(item, |i| i.place.last);
    let from = sel(item, |i| i.m.from);
    let named = sel(item, |i| {
        i.place.first && !i.m.mine() && logic::in_bubble(&i.m.body)
    });
    let lead = if people { AVATAR + AVATAR_GAP } else { 0.0 };
    view()
        .w_full()
        .style_when(move || mine.get(), |s| s.items_end())
        .style_when(move || !mine.get(), |s| s.items_start())
        .child(people.then(|| {
            when(
                move || named.get(),
                move || {
                    text(move || s.contact_name(from.get()))
                        .pl(px(lead + PAD_X))
                        .pr(px(PAD_X))
                        .pb(px(4.0))
                        .number_of_lines(1)
                        .text_size(px(font::CAPTION))
                        .line_height(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme::sender_ink(from.get().unwrap_or(0), cv.canvas()))
                },
            )
        }))
        .child(
            view()
                .w_full()
                .flex_row()
                .items_end()
                .style_when(move || mine.get(), |s| s.justify_end())
                .child(people.then(|| {
                    view()
                        .w(px(AVATAR))
                        .mr(px(AVATAR_GAP))
                        .style_when(move || mine.get(), |st| st.hidden())
                        .accessibility_elements_hidden(true)
                        .child(when(
                            move || last.get() && !mine.get(),
                            move || {
                                dynamic(move || {
                                    let from = from.get();
                                    Some(untracked(|| {
                                        avatar(s.contact_name(from), from.unwrap_or(0), AVATAR)
                                    }))
                                })
                            },
                        ))
                }))
                .child(
                    view()
                        .max_w(relative(MAX_SHARE))
                        .style_when(move || mine.get(), |s| s.items_end())
                        .style_when(move || !mine.get(), |s| s.items_start())
                        .child(bubble(cv, item)),
                ),
        )
        .child(reactions(cv, item, lead))
        .child(status(cv, item))
}

pub(crate) fn bubble(cv: Conv, item: ItemMemo) -> View {
    let mine = sel(item, |i| i.m.mine());
    let first = sel(item, |i| i.place.first);
    let last = sel(item, |i| i.place.last);
    let id = sel(item, |i| i.m.id);
    let at = sel(item, |i| i.m.at);
    let kind = computed(move || {
        item.with(|i| i.as_ref().map(|i| Kind::of(&i.m.body)).unwrap_or_default())
    });
    let bare = move || kind.get() == Kind::Sticker;
    let inline = sel(item, |i| inline_meta(&i.m.body));
    let marked = sel(item, |i| {
        i.m.starred || i.m.pinned || (i.m.edited && !matches!(i.m.body, Body::Deleted))
    });
    let ink = ink_of(cv, item);
    // Runs: on the sender's side the corners that face the run's other
    // bubbles are small (the top one unless it is the run's first, the
    // bottom one unless it is its last); the far side keeps the full
    // radius. No tails: a single bubble is round all over.
    let top = move |me: bool| {
        move || {
            px(if !bare() && mine.get() == me && !first.get() {
                JOINED
            } else {
                RADIUS
            })
        }
    };
    let bottom = move |me: bool| {
        move || {
            px(if !bare() && mine.get() == me && !last.get() {
                JOINED
            } else {
                RADIUS
            })
        }
    };
    let frame = element_ref();
    let reduced = motion::reduced();
    let picked = move || cv.selecting.get() && cv.selected.with(|v| v.contains(&id.get()));
    let bubble = view()
        .element_ref(&frame)
        .min_w(px(44.0))
        .rounded_tr(top(true))
        .rounded_tl(top(false))
        .rounded_br(bottom(true))
        .rounded_bl(bottom(false))
        .border_curve(BorderCurve::Continuous)
        .overflow_hidden()
        .bg(move || {
            if bare() {
                AdaptiveColor::TRANSPARENT
            } else {
                ink().fill
            }
        })
        // No colour transition here: a recycled cell changes message (and
        // side) as the list scrolls, and must not fade between them. The
        // canvas cross-fades on a theme change; the bubbles follow at once.
        .text_color(move || ink().text)
        // Chosen while selecting: a ring 2pt outside keeps the bubble apart
        // from the row's selection wash (dark incoming bubbles are close to
        // it in tone, and the user's are the accent's own colour). An
        // outline, so the bubble's size and padding never move.
        .outline(px(2.0))
        .outline_offset(px(2.0))
        .outline_color(move || {
            if picked() {
                c::ring()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        // Pressed (the hold that opens the menu): it dims and settles in a
        // little, so the finger feels it land.
        .active(move |st| {
            if reduced {
                st.opacity(0.85)
            } else {
                st.opacity(0.9).scale(0.98)
            }
        })
        .group()
        .on_long_press(move |_: &PressEvent| open_menu(cv, item, frame))
        .on_context_menu(move |_: &ContextMenuEvent| open_menu(cv, item, frame))
        .child(forwarded(cv, item))
        .child(quote(cv, item))
        .child(dynamic(move || {
            let k = kind.get();
            Some(untracked(|| body(cv, item, k)))
        }))
        .child(when(
            move || marked.get(),
            move || marks(cv, item, bare, move || inline.get()),
        ));
    if !is_desktop() {
        return bubble;
    }
    // Desktop: the time it was sent shows when the pointer rests on the
    // bubble (the platform's half second), and an actions button appears
    // on hover or focus, a shortcut to the menu a right click opens.
    bubble
        .tooltip(
            // `ui::hint`'s look; built here because its words follow the
            // recycled cell's message (`hint` takes fixed text).
            view()
                .px_2()
                .py_1()
                .rounded(move || radius::control())
                .bg(move || c::foreground())
                .shadow_md()
                .child(
                    text(move || logic::sent_at(at.get()))
                        .number_of_lines(1)
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(move || c::background()),
                )
                .into_element(),
        )
        .child(
            pressable()
                .absolute()
                .top(px(INSET))
                .right(px(INSET))
                .size(px(24.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || ink().fill)
                .text_color(move || ink().quiet)
                .opacity(0.0)
                .group_hover(|s| s.opacity(1.0))
                .focus_visible(|s| {
                    s.opacity(1.0)
                        .outline(px(2.0))
                        .outline_color(move || c::ring())
                })
                .transition_opacity()
                .duration(motion::INSTANT)
                .accessibility_label("Message actions")
                .on_press(move || open_menu(cv, item, frame))
                .child(icon(Icon::ChevronDown, 16.0)),
        )
}

fn forwarded(cv: Conv, item: ItemMemo) -> AnyElement {
    let on = sel(item, |i| i.m.forwarded);
    let ink = ink_of(cv, item);
    when(
        move || on.get(),
        move || {
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .px(px(PAD_X))
                .pt(px(PAD_Y))
                .mb(px(-4.0))
                .text_color(move || ink().quiet)
                .child(icon(Icon::Forward, 12.0))
                .child(text("Forwarded").italic().text_size(px(font::CAPTION)))
        },
    )
    .into_element()
}

/// The quoted message a reply answers; pressing it scrolls there.
fn quote(cv: Conv, item: ItemMemo) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let reply = sel(item, |i| i.m.reply_to);
    let ink = ink_of(cv, item);
    let mine = sel(item, |i| i.m.mine());
    when(
        move || reply.get().is_some(),
        move || {
            let quoted = computed(move || reply.get().and_then(|r| s.message(chat, r)));
            let who = move || {
                quoted.with(|q| {
                    q.as_ref()
                        .map(|q| s.contact_name(q.from))
                        .unwrap_or_default()
                })
            };
            let what = move || {
                quoted.with(|q| {
                    q.as_ref()
                        .map(|q| q.body.preview())
                        .unwrap_or_else(|| "Message not found".into())
                })
            };
            // In the user's full-colour bubble everything is its text colour;
            // elsewhere the quoted sender's hue, made legible on the bubble.
            let hue = move || {
                if mine.get() && cv.look.get().confident() {
                    return ink().text;
                }
                match quoted.with(|q| q.as_ref().and_then(|q| q.from)) {
                    Some(id) => theme::sender_ink(id, ink().fill),
                    None => c::primary(),
                }
            };
            pressable()
                .on_press(move || {
                    if let Some(r) = reply.get_untracked() {
                        cv.jump_to(r);
                    }
                })
                .accessibility_label(super::say(move || {
                    format!("Reply to {}: {}", who(), what())
                }))
                .mx(px(INSET))
                .mt(px(INSET))
                .flex_row()
                .rounded(px(INNER))
                .border_curve(BorderCurve::Continuous)
                .overflow_hidden()
                .bg(move || ink().wash)
                .focus_visible(|st| st.outline(px(2.0)).outline_color(move || c::ring()))
                .child(view().w(px(3.0)).bg(hue))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .px(px(PAD_X - INSET - 3.0))
                        .py(px(6.0))
                        .gap(px(2.0))
                        .child(
                            text(who)
                                .number_of_lines(1)
                                .text_size(px(font::SMALL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hue),
                        )
                        .child(
                            text(what)
                                .number_of_lines(2)
                                .text_size(px(font::SMALL))
                                .text_color(move || ink().text),
                        ),
                )
        },
    )
    .into_element()
}

/// What the marks hold, as text of the marks' own size, for the paragraph
/// to keep room for them on its last line. The glyphs (star, pin) are
/// stood in for by digits of about their width; the whole run is drawn
/// transparent and never wraps inside. Empty when there are no marks (no
/// time sits in the bubble any more).
fn meta_room(m: &crate::store::Message) -> String {
    const NB: char = '\u{00A0}';
    let deleted = matches!(m.body, Body::Deleted);
    if !(m.starred || m.pinned || (m.edited && !deleted)) {
        return String::new();
    }
    let mut room = String::from("  ");
    if m.starred {
        room.push_str("00");
    }
    if m.pinned {
        room.push_str("00");
    }
    if m.edited && !deleted {
        room.push_str("Edited");
        room.push(NB);
    }
    room
}

/// The marks a message carries (starred, pinned, edited) at the bubble's
/// end: inside the text's last line for text bubbles (`inline`), else a
/// line of its own under the content. Only when there are any.
fn marks(
    cv: Conv,
    item: ItemMemo,
    bare: impl Fn() -> bool + Copy + 'static,
    inline: impl Fn() -> bool + Copy + 'static,
) -> View {
    let starred = sel(item, |i| i.m.starred);
    let pinned = sel(item, |i| i.m.pinned);
    let edited = sel(item, |i| i.m.edited && !matches!(i.m.body, Body::Deleted));
    let ink = ink_of(cv, item);
    // Baselines: the marks' 11pt text in a 14pt line, the body's in a
    // 20pt (22pt) line ending 8pt above the bubble's edge: 10pt from the
    // bottom puts both on one baseline.
    view()
        .self_end()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .h(px(16.0))
        .px(px(PAD_X))
        .mb(px(6.0))
        .style_when(inline, |s| {
            s.absolute()
                .right(px(10.0))
                .bottom(px(9.0))
                .px(px(0.0))
                .mb(px(0.0))
        })
        .style_when(bare, |s| {
            s.mt_1()
                .mb(px(0.0))
                .px_2()
                .h(px(20.0))
                .rounded_full()
                .bg(c::card())
        })
        .text_color(move || ink().quiet)
        .child(when(
            move || starred.get(),
            || icon(Icon::Star, 11.0).label("Starred"),
        ))
        .child(when(
            move || pinned.get(),
            || icon(Icon::Pin, 11.0).label("Pinned"),
        ))
        .child(when(
            move || edited.get(),
            || {
                text("Edited")
                    .text_size(px(META_SIZE))
                    .line_height(px(META_LINE))
            },
        ))
}

/// Reactions: a small capsule on the bubble's bottom edge, half over it,
/// ringed in the canvas's colour so it reads as lifted off both. It
/// starts at the bubble's text edge (others') or ends there (the user's).
fn reactions(cv: Conv, item: ItemMemo, lead: f32) -> AnyElement {
    let id = sel(item, |i| i.m.id);
    let mine = sel(item, |i| i.m.mine());
    let list = computed(move || {
        item.with(|i| {
            i.as_ref()
                .map(|i| {
                    i.m.reactions
                        .iter()
                        .map(|r| (r.emoji.clone(), r.by.len(), r.by.contains(&None)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    when(
        move || list.with(|l| !l.is_empty()),
        move || {
            view()
                .flex_row()
                .gap_1()
                // Half the chip's 24pt over the bubble's edge.
                .mt(px(-12.0))
                .style_when(move || mine.get(), |s| s.pr(px(PAD_X - 2.0)))
                .style_when(move || !mine.get(), move |s| s.pl(px(lead + PAD_X - 2.0)))
                .child(each(
                    move || list.get(),
                    |r: &(String, usize, bool)| r.0.clone(),
                    move |r, _| {
                        // Pops in when the user just reacted (not on reuse).
                        let pop = if cv.popped.get_untracked() == Some(id.get_untracked()) {
                            motion().entering(MotionPreset::PopIn)
                        } else {
                            motion()
                        };
                        let yours = move || r.with(|r| r.2);
                        pressable()
                            .bind_motion(&pop)
                            .on_press(move || {
                                cv.react(id.get_untracked(), &r.with_untracked(|r| r.0.clone()))
                            })
                            .accessibility_label(super::say(move || {
                                r.with(|(e, n, me)| {
                                    if *me {
                                        format!("{e} {n}, including you")
                                    } else {
                                        format!("{e} {n}")
                                    }
                                })
                            }))
                            .flex_row()
                            .items_center()
                            .gap(px(3.0))
                            .px(px(7.0))
                            .h(px(24.0))
                            .rounded_full()
                            .border_2()
                            .border_color(move || cv.canvas())
                            .bg(move || if yours() { c::accent() } else { c::card() })
                            .cursor_pointer()
                            .active(|st| {
                                if motion::reduced() {
                                    st.opacity(0.85)
                                } else {
                                    st.scale(motion::ICON_PRESS_SCALE)
                                }
                            })
                            .focus_visible(|st| {
                                st.outline(px(2.0)).outline_color(move || c::ring())
                            })
                            .child(text(move || r.with(|r| r.0.clone())).text_size(px(font::SMALL)))
                            .child(when(
                                move || r.with(|r| r.1 > 1),
                                move || {
                                    text(move || r.with(|r| r.1.to_string()))
                                        .text_size(px(META_SIZE))
                                        .tabular_nums()
                                        .text_color(move || {
                                            if yours() {
                                                c::on_accent()
                                            } else {
                                                c::muted_fg()
                                            }
                                        })
                                },
                            ))
                    },
                ))
        },
    )
    .into_element()
}

/// The status under the user's latest message only: Signal-style receipt
/// circles (`receipt_mark`), spoken as "Sent", "Delivered", "Read" or, in
/// groups, "Read by 4"; and under any message that failed, the words "Not
/// sent · Retry". A new status fades in softly as it changes.
fn status(cv: Conv, item: ItemMemo) -> AnyElement {
    let group = cv.is_group();
    let id = sel(item, |i| i.m.id);
    let failed = sel(item, |i| i.m.mine() && i.m.status == Tick::Failed);
    let shown = move || {
        let id = id.get();
        failed.get() || (id != 0 && cv.latest_own.get() == Some(id))
    };
    let tick =
        computed(move || item.with(|i| i.as_ref().map(|i| i.m.status).unwrap_or(Tick::Sent)));
    let read_by = sel(item, |i| {
        i.m.receipts.iter().filter(|r| r.2.is_some()).count()
    });
    let words = computed(move || logic::status_words(tick.get(), group, read_by.get()));
    when(shown, move || {
        view()
            .flex_row()
            .items_center()
            .h(px(16.0))
            .mt(px(4.0))
            .pr(px(4.0))
            .gap_1()
            .child(dynamic(move || {
                let w = words.get();
                let tk = tick.get();
                // A failure needs words and a Retry; every other status is
                // Signal-style circles, its words kept for screen readers.
                Some(untracked(move || {
                    if failed.get_untracked() {
                        status_words(cv, w, failed, id)
                    } else {
                        receipt_mark(cv, tk, w)
                    }
                }))
            }))
    })
    .into_element()
}

/// One receipt circle's side and the overlap of a pair (points).
const MARK: f32 = 12.0;
const MARK_OVERLAP: f32 = 5.0;
/// The canvas ring cut around the front circle of a pair.
const MARK_CUT: f32 = 1.5;

/// Signal-style receipt circles (not WhatsApp's ticks): a faint ring while
/// sending, a ring with a check once sent, two overlapping rings once
/// delivered, and both filled in the chat theme's bubble colour once read
/// (or played). The front circle is cut out of the canvas so the overlap
/// reads cleanly on every theme.
fn receipt_mark(cv: Conv, tick: Tick, words: String) -> View {
    let soft = if motion::reduced() {
        motion().entering(
            MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                .values(MotionChannel::Opacity, [0.0, 1.0]),
        )
    } else {
        motion().entering(
            MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                .values(MotionChannel::Opacity, [0.0, 1.0])
                .values(MotionChannel::Scale, [0.8, 1.0]),
        )
    };
    let read = matches!(tick, Tick::Read | Tick::Played);
    let pair = matches!(tick, Tick::Delivered | Tick::Read | Tick::Played);
    let checked = !matches!(tick, Tick::Sending);
    // A circle: filled (read) or a ring on the canvas, with or without a check.
    let circle = move |check: bool| {
        let c = view()
            .size(px(MARK))
            .flex_shrink(0.0)
            .rounded_full()
            .items_center()
            .justify_center()
            .border_1();
        let c = if read {
            c.bg(move || cv.look.get().bubble())
                .border_color(move || cv.look.get().bubble())
        } else {
            c.bg(move || cv.canvas())
                .border_color(move || c::muted_fg())
        };
        let c = if matches!(tick, Tick::Sending) {
            c.opacity(0.5)
        } else {
            c
        };
        if check {
            c.child(icon(Icon::Check, 8.0).text_color(move || {
                if read {
                    cv.look.get().on_bubble()
                } else {
                    c::muted_fg()
                }
            }))
        } else {
            c
        }
    };
    let mark = view().flex_row().items_center().bind_motion(&soft);
    let mark = if pair {
        // The back circle first; the front one overlaps it from the right,
        // inside a 1.5pt ring of the canvas, so two filled circles read as a
        // pair rather than one blob. Only the front one carries the check: a
        // sliver of a hidden one reads as a glitch at this size.
        mark.child(circle(false)).child(
            view()
                .ml(px(-MARK_OVERLAP - MARK_CUT))
                .p(px(MARK_CUT))
                .rounded_full()
                .bg(move || cv.canvas())
                .child(circle(true)),
        )
    } else {
        mark.child(circle(checked))
    };
    mark.role(AccessibilityRole::Text).label(words)
}

fn status_words(cv: Conv, words: String, failed: Memo<bool>, id: Memo<u64>) -> View {
    let soft = if motion::reduced() {
        motion().entering(
            MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                .values(MotionChannel::Opacity, [0.0, 1.0]),
        )
    } else {
        motion().entering(
            MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                .values(MotionChannel::Opacity, [0.0, 1.0])
                .values(MotionChannel::TranslateY, [2.0, 0.0]),
        )
    };
    let say = words.clone();
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .bind_motion(&soft)
        .role(AccessibilityRole::Text)
        .label(say)
        .child(
            text(words)
                .text_size(px(META_SIZE))
                .line_height(px(META_LINE))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(move || {
                    if failed.get() {
                        c::destructive()
                    } else {
                        c::muted_fg()
                    }
                }),
        )
        .child(when(
            move || failed.get(),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        text("·")
                            .text_size(px(META_SIZE))
                            .text_color(move || c::destructive()),
                    )
                    .child(
                        pressable()
                            .on_press(move || cv.s.retry(cv.chat, id.get_untracked()))
                            .accessibility_label("Retry sending")
                            .px_1()
                            .rounded(move || radius::sm())
                            .cursor_pointer()
                            .hover(|st| st.bg(move || c::destructive().opacity(0.08)))
                            .focus_visible(|st| {
                                st.outline(px(2.0)).outline_color(move || c::ring())
                            })
                            .child(
                                text("Retry")
                                    .text_size(px(META_SIZE))
                                    .line_height(px(META_LINE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || c::destructive()),
                            ),
                    )
            },
        ))
}

// ---------------------------------------------------------------- bodies

fn body(cv: Conv, item: ItemMemo, kind: Kind) -> AnyElement {
    let el = match kind {
        Kind::Text => text_body(cv, item),
        Kind::Link => link_body(cv, item),
        Kind::Image => image_body(cv, item),
        Kind::Video => video_body(cv, item),
        Kind::Voice => voice_body(cv, item),
        Kind::File => file_body(cv, item),
        Kind::Poll => poll_body(cv, item),
        Kind::Event => event_body(cv, item),
        Kind::Contact => contact_body(cv, item),
        Kind::Location => location_body(cv, item),
        Kind::Sticker => sticker_body(cv, item),
        Kind::ViewOnce => view_once_body(cv, item),
        Kind::Call => call_body(cv, item),
        Kind::Deleted => deleted_body(cv, item),
    };
    if kind.media() {
        view().p(px(INSET)).child(el).into_element()
    } else {
        el
    }
}

/// The transparent run that keeps the meta's place at the paragraph's
/// end (see [`meta_room`]).
fn room_run(room: Memo<String>) -> Text {
    text(move || room.get())
        .text_size(px(META_SIZE))
        .tabular_nums()
        .text_color(AdaptiveColor::TRANSPARENT)
}

/// Message text with `*bold*`, `_italic_` and links (most messages are
/// one plain run, bound in place). With `room`, the paragraph ends with
/// the meta's room so the time can sit on its last line.
pub(crate) fn rich_text(
    t: Memo<String>,
    room: Option<Memo<String>>,
    ink: impl Fn() -> Ink + Copy + 'static,
) -> AnyElement {
    let plain = computed(move || t.with(|t| logic::is_plain(t)));
    let size = font::body();
    when(
        move || plain.get(),
        move || {
            text("")
                .text_size(px(size))
                .line_height(px(line()))
                // Screen readers read the message, not the room run.
                .label(move || t.get())
                .child(text(move || t.get()))
                .child(room.map(room_run))
        },
    )
    .otherwise(move || {
        dynamic(move || {
            let spans = t.with(|t| logic::spans(t));
            Some(untracked(|| {
                text("")
                    .text_size(px(size))
                    .line_height(px(line()))
                    .children(spans.into_iter().map(|sp| {
                        let mut el = text(sp.text.clone());
                        if sp.bold {
                            el = el.font_weight(FontWeight::BOLD);
                        }
                        if sp.italic {
                            el = el.italic();
                        }
                        if sp.mention {
                            el = el
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(move || ink().mention);
                        }
                        if sp.link {
                            let url = sp.text.clone();
                            el = el
                                .text_color(move || ink().link)
                                .underline()
                                .role(AccessibilityRole::Link)
                                .on_press(move || {
                                    crate::features::open_link_or_url(&url);
                                });
                        }
                        el
                    }))
                    .child(room.map(room_run))
            }))
        })
    })
    .into_element()
}

fn text_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let t = sel(item, |i| match &i.m.body {
        Body::Text(t) => t.clone(),
        _ => String::new(),
    });
    let room = sel(item, |i| meta_room(&i.m));
    view()
        .px(px(PAD_X))
        .py(px(PAD_Y))
        .child(rich_text(t, Some(room), ink_of(cv, item)))
        .into_element()
}

/// A link with its on-device preview (made by the sender's device, never
/// fetched by the server).
fn link_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let ink = ink_of(cv, item);
    let t = sel(item, |i| match &i.m.body {
        Body::Link { text, .. } => text.clone(),
        _ => String::new(),
    });
    let card = sel(item, |i| match &i.m.body {
        Body::Link {
            url, title, site, ..
        } => (url.clone(), title.clone(), site.clone()),
        _ => Default::default(),
    });
    let room = sel(item, |i| meta_room(&i.m));
    view()
        .px(px(INSET))
        .pt(px(INSET))
        .child(
            pressable()
                .on_press(move || {
                    crate::features::open_link_or_url(&card.with_untracked(|c| c.0.clone()));
                })
                .accessibility_label(super::say(move || {
                    card.with(|c| format!("Link preview: {}, {}", c.1, c.2))
                }))
                .flex_row()
                .rounded(px(INNER))
                .border_curve(BorderCurve::Continuous)
                .overflow_hidden()
                .bg(move || ink().wash)
                .focus_visible(|st| st.outline(px(2.0)).outline_color(move || c::ring()))
                .child(view().w(px(3.0)).bg(move || ink().link))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .px_2()
                        .py(px(6.0))
                        .gap(px(2.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .text_color(move || ink().quiet)
                                .child(icon(Icon::Globe, 11.0))
                                .child(
                                    text(move || card.with(|c| c.2.clone()))
                                        .text_size(px(font::CAPTION)),
                                ),
                        )
                        .child(
                            text(move || card.with(|c| c.1.clone()))
                                .number_of_lines(2)
                                .text_size(px(font::SMALL + 1.0))
                                .font_weight(FontWeight::SEMIBOLD),
                        ),
                ),
        )
        // The paragraph lines up with plain text bubbles' (12pt in).
        .child(
            view()
                .px(px(PAD_X - INSET))
                .py(px(PAD_Y))
                .child(rich_text(t, Some(room), ink)),
        )
        .into_element()
}

/// A photo or video frame at its aspect ratio: the message's (mock)
/// decrypted thumbnail, the same picture as in the viewer, chat info and
/// the media grid (`portrait::photo`, seeded by the message id). The
/// description is what screen readers say.
fn photo_frame(shot: Memo<(String, u64, u32, u32)>, label: impl Fn() -> String + 'static) -> View {
    let base = if is_desktop() { 280.0 } else { 240.0 };
    let ratio = move || shot.with(|s| (s.2.max(1) as f32 / s.3.max(1) as f32).clamp(0.6, 1.9));
    view()
        // Portrait media narrows, so it never towers over the chat.
        .w(move || px((base * ratio().clamp(0.75, 1.0)).round()))
        .aspect_ratio(ratio)
        .rounded(px(INNER))
        .border_curve(BorderCurve::Continuous)
        .overflow_hidden()
        .bg(move || c::foreground().opacity(0.07))
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Image)
        .label(label)
        .child(
            image(move || {
                shot.with(|s| {
                    // A real photo: its bytes once this device has them (tracked, so a photo
                    // still sending or downloading appears when it can; D-0183: a sent photo
                    // stayed blank until tapped); the illustrations only in the demo.
                    if let Some(src) = crate::backend::chat::media_src(s.1) {
                        src
                    } else if crate::backend::chat::is_media(s.1) || !crate::store::demo() {
                        ImageSrc::from("")
                    } else {
                        portrait::photo(&s.0, s.1, s.2.max(1), s.3.max(1))
                    }
                })
            })
            .absolute()
            .inset_0()
            .object_fit(ResizeMode::Cover),
        )
        .child(when(
            move || {
                shot.with(|s| {
                    crate::backend::chat::is_media(s.1)
                        && crate::backend::chat::media_src(s.1).is_none()
                })
            },
            move || {
                let msg = shot.with_untracked(|s| s.1);
                // A failed download says so and offers to try again.
                dynamic(move || {
                    Some(if crate::backend::chat::photo_failed(msg) {
                        icon_btn(Icon::Refresh, "Couldn't download the photo. Try again", move || {
                            crate::backend::chat::retry_photo(msg);
                        })
                    } else {
                        icon_btn(Icon::Download, "Download photo", move || {
                            crate::backend::chat::retry_photo(msg);
                        })
                    })
                })
            },
        ))
}

/// A small label over a photo ("HD", a video's length): a dark scrim pill
/// with light text, the same in both themes (it sits on the picture).
fn photo_badge(content: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .px(px(6.0))
        .h(px(20.0))
        .rounded_full()
        .bg(AdaptiveColor::rgb(0x0F0D1A, 0x0F0D1A).opacity(0.62))
        .text_color(AdaptiveColor::rgb(0xFFFFFF, 0xFFFFFF))
        .child(content)
}

fn caption(
    text_of: Memo<String>,
    room: Memo<String>,
    ink: impl Fn() -> Ink + Copy + 'static,
) -> AnyElement {
    // Under media (inset 4pt), the caption's text lines up with text
    // bubbles' (12pt from the edge and from the bottom), the meta on its
    // last line.
    when(
        move || text_of.with(|t| !t.is_empty()),
        move || {
            view()
                .px(px(PAD_X - INSET))
                .pt(px(6.0))
                .pb(px(PAD_Y - INSET))
                .child(rich_text(text_of, Some(room), ink))
        },
    )
    .into_element()
}

fn image_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let id = sel(item, |i| i.m.id);
    let info = sel(item, |i| match &i.m.body {
        Body::Image { alt, hd, .. } => (alt.clone(), *hd),
        _ => (String::new(), false),
    });
    let shot = sel(item, |i| match &i.m.body {
        Body::Image { alt, w, h, .. } => (alt.clone(), i.m.id, *w, *h),
        _ => (String::new(), 0, 1, 1),
    });
    let cap = sel(item, |i| match &i.m.body {
        Body::Image { caption, .. } => caption.clone(),
        _ => String::new(),
    });
    view()
        .child(
            pressable()
                .on_press(move || media::open_viewer(cv.chat, id.get_untracked()))
                .accessibility_label(super::say(move || info.with(|i| format!("Photo: {}", i.0))))
                .child(photo_frame(shot, move || info.with(|i| i.0.clone())))
                .child(when(
                    move || info.with(|i| i.1),
                    || {
                        view()
                            .absolute()
                            .top(px(8.0))
                            .left(px(8.0))
                            .child(photo_badge(
                                text("HD").text_size(px(11.0)).font_weight(FontWeight::BOLD),
                            ))
                    },
                )),
        )
        .child(caption(
            cap,
            sel(item, |i| meta_room(&i.m)),
            ink_of(cv, item),
        ))
        .into_element()
}

fn video_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let id = sel(item, |i| i.m.id);
    let secs = sel(item, |i| match &i.m.body {
        Body::Video { secs, .. } => *secs,
        _ => 0,
    });
    let cap = sel(item, |i| match &i.m.body {
        Body::Video { caption, .. } => caption.clone(),
        _ => String::new(),
    });
    // A video's poster frame: the scene its caption describes.
    let shot = sel(item, |i| match &i.m.body {
        Body::Video { caption, .. } => (format!("video {caption}"), i.m.id, 16, 9),
        _ => (String::new(), 0, 16, 9),
    });
    view()
        .child(
            pressable()
                .on_press(move || media::open_viewer(cv.chat, id.get_untracked()))
                .accessibility_label(super::say(move || {
                    format!("Video, {}", logic::duration(secs.get()))
                }))
                .child(
                    photo_frame(shot, || "Video".into()).child(
                        view()
                            .absolute()
                            .size(px(48.0))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(move || c::overlay())
                            .text_color(move || c::on_avatar())
                            .child(icon(Icon::Play, 22.0)),
                    ),
                )
                .child(
                    view().absolute().left(px(8.0)).bottom(px(8.0)).child(
                        photo_badge(icon(Icon::Video, 11.0)).child(
                            text(move || logic::duration(secs.get()))
                                .text_size(px(11.0))
                                .tabular_nums(),
                        ),
                    ),
                ),
        )
        .child(caption(
            cap,
            sel(item, |i| meta_room(&i.m)),
            ink_of(cv, item),
        ))
        .into_element()
}

/// A voice note: play/pause (a glyph swap), the waveform filling as it
/// plays, time left, the speed while playing; a dot until it is played.
fn voice_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let player = cv.voice;
    let ink = ink_of(cv, item);
    let id = sel(item, |i| i.m.id);
    let secs = sel(item, |i| match &i.m.body {
        Body::Voice { secs, .. } => *secs,
        _ => 0,
    });
    let unplayed = sel(item, |i| {
        matches!(i.m.body, Body::Voice { played: false, .. }) && !i.m.mine()
    });
    let playing = move || player.playing.get() == Some(id.get());
    let started = move || player.pos.with(|p| p.contains_key(&id.get()));
    let reduced = motion::reduced();
    let glyph = move |g: Icon, shown: bool| {
        icon(g, 20.0)
            .absolute()
            .opacity(if shown { 1.0_f32 } else { 0.0 })
            .transition_all()
            .duration(motion::BASE)
    };
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .w(px(if is_desktop() { 280.0 } else { 250.0 }))
        .px(px(8.0))
        .pt(px(8.0))
        .child(
            pressable()
                .on_press(move || player.toggle(cv, id.get_untracked()))
                .accessibility_label(super::say(move || {
                    if playing() {
                        "Pause voice message"
                    } else {
                        "Play voice message"
                    }
                }))
                .size(px(40.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || ink().accent)
                .text_color(move || ink().on_accent)
                .cursor_pointer()
                .active(move |st| {
                    if reduced {
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
                .child(
                    view()
                        .size(px(20.0))
                        .items_center()
                        .justify_center()
                        .child(
                            glyph(Icon::Play, true)
                                .opacity(move || if playing() { 0.0_f32 } else { 1.0 })
                                .scale(move || if playing() && !reduced { 0.6_f32 } else { 1.0 }),
                        )
                        .child(
                            glyph(Icon::Pause, false)
                                .opacity(move || if playing() { 1.0_f32 } else { 0.0 })
                                .scale(move || if playing() || reduced { 1.0_f32 } else { 0.6 }),
                        ),
                ),
        )
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(super::voice::waveform(
                    cv,
                    move || id.get(),
                    move || secs.get(),
                    ink,
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(move || {
                                let left = if started() {
                                    player.remaining(id.get(), secs.get())
                                } else {
                                    secs.get()
                                };
                                logic::duration(left)
                            })
                            .text_size(px(META_SIZE))
                            .tabular_nums()
                            .text_color(move || ink().quiet),
                        )
                        .child(when(
                            move || unplayed.get(),
                            move || {
                                view()
                                    .size(px(7.0))
                                    .rounded_full()
                                    .bg(move || ink().accent)
                                    .label("Not played yet")
                            },
                        ))
                        .child(when(playing, move || {
                            pressable()
                                .on_press(move || player.cycle_speed())
                                .accessibility_label(super::say(move || {
                                    format!("Playback speed {}", player.speed_label())
                                }))
                                .px(px(6.0))
                                .h(px(18.0))
                                .justify_center()
                                .rounded_full()
                                .bg(move || ink().wash)
                                .child(
                                    text(move || player.speed_label())
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .tabular_nums(),
                                )
                        })),
                ),
        )
        .into_element()
}

fn file_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let ink = ink_of(cv, item);
    let id = sel(item, |i| i.m.id);
    let info = sel(item, |i| match &i.m.body {
        Body::File { name, size, kind } => (name.clone(), size.clone(), kind.clone()),
        _ => Default::default(),
    });
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .m(px(INSET))
        .p_2()
        .w(px(260.0))
        .rounded(px(INNER))
        .border_curve(BorderCurve::Continuous)
        .bg(move || ink().wash)
        .child(
            view()
                .size(px(40.0))
                .rounded(move || radius::sm())
                .items_center()
                .justify_center()
                .bg(move || c::card())
                .text_color(move || c::primary())
                .child(icon(Icon::File, 20.0)),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(move || info.with(|i| i.0.clone()))
                        .number_of_lines(1)
                        .text_size(px(font::SMALL + 1.0))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    text(move || info.with(|i| format!("{} · {}", i.2, i.1)))
                        .text_size(px(font::CAPTION))
                        .text_color(move || ink().quiet),
                ),
        )
        .child(icon_btn(Icon::Download, "Download", move || {
            // Downloaded, verified and decrypted by app-core, then saved
            // where the user chooses.
            let name = info.with_untracked(|i| i.0.clone());
            let msg = id.get_untracked();
            if crate::backend::chat::is_media(msg) {
                crate::backend::chat::save_media(msg, name);
            } else {
                toast("This file is only in the demo.");
            }
        }))
        .into_element()
}

/// A poll: options with vote bars; pressing one votes (again: takes it
/// back).
fn poll_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let id = sel(item, |i| i.m.id);
    let question = sel(item, |i| match &i.m.body {
        Body::Poll { question, .. } => question.clone(),
        _ => String::new(),
    });
    let multiple = sel(item, |i| {
        matches!(i.m.body, Body::Poll { multiple: true, .. })
    });
    let options = computed(move || {
        item.with(|i| match i.as_ref().map(|i| &i.m.body) {
            Some(Body::Poll { options, mine, .. }) => {
                let total: u32 = options.iter().map(|o| o.1).sum::<u32>().max(1);
                options
                    .iter()
                    .enumerate()
                    .map(|(n, (label, votes))| {
                        (
                            n,
                            label.clone(),
                            *votes,
                            *votes as f32 / total as f32,
                            mine.contains(&n),
                        )
                    })
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
    });
    let total = computed(move || options.with(|o| o.iter().map(|o| o.2).sum::<u32>()));
    let closed = sel(item, |i| {
        matches!(i.m.body, Body::Poll { closed: true, .. })
    });
    let own = sel(item, |i| i.m.mine());
    view()
        .w(px(270.0))
        .px(px(PAD_X))
        .pt(px(8.0))
        .gap_2()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(icon(Icon::Poll, 16.0).text_color(move || c::primary()))
                .child(
                    text(move || question.get())
                        .flex_1()
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(
            text(move || {
                if multiple.get() {
                    "Select one or more"
                } else {
                    "Select one"
                }
            })
            .text_size(px(font::CAPTION))
            .text_color(move || c::bubble_meta()),
        )
        .child(each(
            move || options.get(),
            |o: &(usize, String, u32, f32, bool)| o.0,
            move |o, _| {
                let chosen = move || o.with(|o| o.4);
                pressable()
                    .on_press(move || {
                        if !closed.get_untracked() {
                            cv.s.vote(cv.chat, id.get_untracked(), o.with_untracked(|o| o.0));
                        }
                    })
                    .selected(chosen)
                    .accessibility_label(super::say(move || {
                        o.with(|o| {
                            format!(
                                "{}, {} votes{}",
                                o.1,
                                o.2,
                                if o.4 { ", your vote" } else { "" }
                            )
                        })
                    }))
                    .gap(px(5.0))
                    .py(px(3.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                view()
                                    .size(px(18.0))
                                    .rounded_full()
                                    .border_2()
                                    .items_center()
                                    .justify_center()
                                    .border_color(move || {
                                        if chosen() {
                                            c::primary()
                                        } else {
                                            c::bubble_meta()
                                        }
                                    })
                                    .bg(move || {
                                        if chosen() {
                                            c::primary()
                                        } else {
                                            AdaptiveColor::TRANSPARENT
                                        }
                                    })
                                    .text_color(move || c::on_primary())
                                    .child(
                                        icon(Icon::Check, 12.0)
                                            .opacity(move || if chosen() { 1.0_f32 } else { 0.0 }),
                                    ),
                            )
                            .child(
                                text(move || o.with(|o| o.1.clone()))
                                    .flex_1()
                                    .text_size(px(font::body())),
                            )
                            .child(
                                text(move || o.with(|o| o.2.to_string()))
                                    .text_size(px(font::SMALL))
                                    .tabular_nums()
                                    .text_color(move || c::bubble_meta()),
                            ),
                    )
                    .child(
                        view()
                            .ml(px(26.0))
                            .h(px(5.0))
                            .rounded_full()
                            .bg(move || c::foreground().opacity(0.1))
                            .child(
                                view()
                                    .h_full()
                                    .rounded_full()
                                    .bg(move || c::primary())
                                    .w(move || relative(o.with(|o| o.3)))
                                    .transition_all()
                                    .duration(motion::BASE),
                            ),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || {
                        let n = match total.get() {
                            1 => "1 vote".to_string(),
                            n => format!("{n} votes"),
                        };
                        if closed.get() {
                            format!("{n} · Final results")
                        } else {
                            n
                        }
                    })
                    .text_size(px(font::CAPTION))
                    .text_color(move || c::bubble_meta()),
                )
                .child(when(
                    move || own.get() && !closed.get(),
                    move || {
                        btn("Close poll", Variant::Ghost, BtnSize::Sm, move || {
                            cv.s.close_poll(cv.chat, id.get_untracked());
                        })
                        .into_element()
                    },
                )),
        )
        .into_element()
}

/// An event: title, when, where, who's going, and the user's answer.
fn event_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let id = sel(item, |i| i.m.id);
    let info = sel(item, |i| match &i.m.body {
        Body::Event {
            title,
            when,
            place,
            going,
            ..
        } => (title.clone(), when.clone(), place.clone(), *going),
        _ => Default::default(),
    });
    let mine = computed(move || {
        item.with(|i| match i.as_ref().map(|i| &i.m.body) {
            Some(Body::Event { mine, .. }) => *mine,
            _ => None,
        })
    });
    let answer = move |going: bool, label: &'static str| {
        let on = move || mine.get() == Some(going);
        pressable()
            .flex_1()
            .on_press(move || cv.s.rsvp(cv.chat, id.get_untracked(), going))
            .selected(on)
            .accessibility_label(label)
            .h(px(if is_desktop() { 30.0 } else { 38.0 }))
            .items_center()
            .justify_center()
            .rounded(move || radius::control())
            .border_1()
            .border_color(move || if on() { c::primary() } else { c::border() })
            .bg(move || {
                if on() {
                    c::primary()
                } else {
                    AdaptiveColor::TRANSPARENT
                }
            })
            .child(
                text(label)
                    .text_size(px(font::SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || if on() { c::on_primary() } else { c::primary() }),
            )
    };
    let line = |glyph: Icon, f: fn(&(String, String, String, u32)) -> String| {
        view()
            .flex_row()
            .items_center()
            .gap_2()
            .text_color(move || c::bubble_meta())
            .child(icon(glyph, 13.0))
            .child(
                text(move || info.with(f))
                    .flex_1()
                    .text_size(px(font::SMALL)),
            )
    };
    view()
        .w(px(270.0))
        .px(px(PAD_X))
        .pt(px(8.0))
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    view()
                        .size(px(36.0))
                        .rounded(move || radius::sm())
                        .items_center()
                        .justify_center()
                        // A tonal tile, visible on both bubbles in both
                        // themes (a card tile vanished on light bubbles).
                        .bg(move || c::primary().opacity(0.12))
                        .text_color(move || c::primary())
                        .child(icon(Icon::Calendar, 18.0)),
                )
                .child(
                    text(move || info.with(|i| i.0.clone()))
                        .flex_1()
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD),
                ),
        )
        .child(line(Icon::Clock, |i| i.1.clone()))
        .child(line(Icon::MapPin, |i| i.2.clone()))
        .child(line(Icon::Users, |i| format!("{} going", i.3)))
        .child(
            view()
                .flex_row()
                .gap_2()
                .pt_1()
                .child(answer(true, "Going"))
                .child(answer(false, "Not going")),
        )
        .into_element()
}

/// A shared contact: Message opens (or makes) the chat, Add saves them.
/// A contact the user knows shows their own photo (seeded by contact id,
/// as everywhere else).
fn contact_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let s = cv.s;
    let info = sel(item, |i| match &i.m.body {
        Body::Contact { name, username } => (name.clone(), username.clone()),
        _ => Default::default(),
    });
    let known = move || {
        let user = info.with_untracked(|i| i.1.clone());
        s.contacts
            .with_untracked(|cs| cs.iter().find(|c| c.username == user).map(|c| c.id))
    };
    let rule = move || c::foreground().opacity(0.1);
    view()
        .w(px(260.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px(px(PAD_X))
                .pt(px(10.0))
                .child(dynamic(move || {
                    let (name, user) = info.get();
                    let seed = s
                        .contacts
                        .with_untracked(|cs| cs.iter().find(|c| c.username == user).map(|c| c.id))
                        .unwrap_or_else(|| name.bytes().map(u64::from).sum::<u64>());
                    Some(untracked(|| avatar(name, seed, 40.0)))
                }))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            text(move || info.with(|i| i.0.clone()))
                                .number_of_lines(1)
                                .text_size(px(font::body()))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            text(move || info.with(|i| format!("@{}", i.1)))
                                .number_of_lines(1)
                                .text_size(px(font::CAPTION))
                                .text_color(move || c::bubble_meta()),
                        ),
                ),
        )
        // Air between the person and the actions.
        .child(view().h(px(PAD_X)))
        .child(view().h(px(1.0)).bg(rule))
        .child(
            view()
                .flex_row()
                .items_stretch()
                .child(view().flex_1().py_1().child(btn(
                    "Message",
                    Variant::Ghost,
                    BtnSize::Sm,
                    move || match known() {
                        Some(contact) => super::open(s.direct_chat(contact)),
                        None => toast("They're not on Songbird yet"),
                    },
                )))
                .child(view().w(px(1.0)).bg(rule))
                .child(view().flex_1().py_1().child(btn(
                    move || {
                        if info.with(|i| s.book().with(|_| s.saved_by_username(&i.1).is_some())) {
                            "Saved"
                        } else {
                            "Save to contacts"
                        }
                    },
                    Variant::Ghost,
                    BtnSize::Sm,
                    move || {
                        // An encrypted contact record (`k/<id>`), synced to
                        // the user's other devices.
                        let (name, user) = info.get_untracked();
                        if s.saved_by_username(&user).is_some() {
                            crate::features::contacts::open();
                            return;
                        }
                        s.save_from(&name, &user);
                        toast(format!("{name} saved to your contacts"));
                    },
                ))),
        )
        .into_element()
}

/// A mock street map (land, a park, water, streets) at `w` × `h`
/// points, different per `seed`: stands in for the map tile the sender's
/// device renders (picture content, so its colours are its own).
fn map_markup(seed: u64, w: f32, h: f32) -> String {
    let r = |lane: u64, n: u64| {
        let mut x = seed.wrapping_add(lane).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 29;
        (x % n) as f32
    };
    // Water across one corner, a park block, a grid of streets with one
    // diagonal avenue; the place is at the centre.
    let water = format!(
        r##"<path d="M{x0:.0} {h:.0} C{c1:.0} {c1y:.0} {c2:.0} {c2y:.0} {w:.0} {y1:.0} L{w:.0} {h:.0} Z" fill="#AFD3E8"/>"##,
        x0 = w * (0.45 + r(1, 20) / 100.0),
        c1 = w * 0.62,
        c1y = h * 0.78,
        c2 = w * 0.8,
        c2y = h * (0.66 + r(2, 10) / 100.0),
        y1 = h * 0.52,
    );
    let (px0, py0) = (w * (0.08 + r(3, 10) / 100.0), h * 0.12);
    let park = format!(
        r##"<rect x="{px0:.0}" y="{py0:.0}" width="{pw:.0}" height="{ph:.0}" rx="6" fill="#C9E2B8"/><circle cx="{t1x:.0}" cy="{t1y:.0}" r="5" fill="#A9D08E"/><circle cx="{t2x:.0}" cy="{t2y:.0}" r="4" fill="#A9D08E"/><circle cx="{t3x:.0}" cy="{t3y:.0}" r="6" fill="#A9D08E"/>"##,
        pw = w * 0.24,
        ph = h * 0.3,
        t1x = px0 + w * 0.05,
        t1y = py0 + h * 0.1,
        t2x = px0 + w * 0.13,
        t2y = py0 + h * 0.2,
        t3x = px0 + w * 0.19,
        t3y = py0 + h * 0.09,
    );
    let mut streets = String::new();
    for (k, f) in [0.24_f32, 0.5, 0.78].iter().enumerate() {
        let y = h * (f + (r(10 + k as u64, 7) - 3.0) / 100.0);
        let wide = if k == 1 { 9.0 } else { 5.0 };
        streets += &format!(
            r##"<path d="M0 {y:.0}H{w:.0}" stroke="#D9D3C7" stroke-width="{:.0}"/><path d="M0 {y:.0}H{w:.0}" stroke="#FFFFFF" stroke-width="{wide:.0}"/>"##,
            wide + 2.0
        );
    }
    for (k, f) in [0.12_f32, 0.38, 0.64, 0.9].iter().enumerate() {
        let x = w * (f + (r(20 + k as u64, 5) - 2.0) / 100.0);
        let wide = if k == 2 { 8.0 } else { 4.0 };
        streets += &format!(
            r##"<path d="M{x:.0} 0V{h:.0}" stroke="#D9D3C7" stroke-width="{:.0}"/><path d="M{x:.0} 0V{h:.0}" stroke="#FFFFFF" stroke-width="{wide:.0}"/>"##,
            wide + 2.0
        );
    }
    let avenue = format!(
        r##"<path d="M{a:.0} 0L{b:.0} {h:.0}" stroke="#F3D99B" stroke-width="9"/><path d="M{a:.0} 0L{b:.0} {h:.0}" stroke="#FBE7B5" stroke-width="6"/>"##,
        a = w * (0.15 + r(30, 20) / 100.0),
        b = w * (0.7 + r(31, 20) / 100.0),
    );
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}"><rect width="{w:.0}" height="{h:.0}" fill="#EEEAE1"/>{park}{water}{streets}{avenue}</svg>"##
    )
}

/// The map as an image source, made once per message and size.
fn map_image(seed: u64, w: f32, h: f32) -> ImageSrc {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<(u64, u32, u32), ImageSrc>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with_borrow_mut(|c| {
        c.entry((seed, w as u32, h as u32))
            .or_insert_with(|| ImageSrc::svg_markup(&map_markup(seed, w, h)))
            .clone()
    })
}

/// A location: a map tile with the place's pin at its centre, then the
/// place's name.
fn location_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let _ = cv;
    let info = sel(item, |i| match &i.m.body {
        Body::Location { label, live, .. } => (label.clone(), *live),
        _ => Default::default(),
    });
    let at = sel(item, |i| match &i.m.body {
        Body::Location { at, .. } => *at,
        _ => None,
    });
    let id = sel(item, |i| i.m.id);
    let (w, h) = (if is_desktop() { 280.0 } else { 240.0 }, 140.0);
    const PIN: f32 = 34.0;
    view()
        .child(
            view()
                .w(px(w))
                .h(px(h))
                .rounded(px(INNER))
                .border_curve(BorderCurve::Continuous)
                .overflow_hidden()
                .bg(AdaptiveColor::rgb(0xEEEAE1, 0xEEEAE1))
                .role(AccessibilityRole::Image)
                .label(move || info.with(|i| format!("Map of {}", i.0)))
                // The real map of the place (self-hosted OSM tiles, D-0142).
                .child(dynamic(move || match at.get() {
                    Some(p) => crate::features::maps::map(signal(p), signal(15.0), false)
                        .absolute()
                        .inset_0()
                        .into_element(),
                    None => image(move || map_image(id.get(), w, h))
                        .absolute()
                        .inset_0()
                        .object_fit(ResizeMode::Cover)
                        .into_element(),
                }))
                // The pin: its tip on the centre, the whole of it inside.
                .child(
                    view()
                        .absolute()
                        .left(px(((w - 14.0) / 2.0).round()))
                        .top(px((h / 2.0 - 4.0).round()))
                        .w(px(14.0))
                        .h(px(8.0))
                        .rounded_full()
                        .bg(AdaptiveColor::rgb(0x000000, 0x000000).opacity(0.18)),
                )
                .child(
                    view()
                        .absolute()
                        .left(px(((w - PIN) / 2.0).round()))
                        .top(px((h / 2.0 - PIN - 6.0).round()))
                        .items_center()
                        .child(
                            view()
                                .size(px(PIN))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .border_2()
                                .border_color(AdaptiveColor::rgb(0xFFFFFF, 0xFFFFFF))
                                .bg(move || c::primary())
                                .text_color(move || c::on_primary())
                                .shadow_md()
                                .child(icon(Icon::MapPin, 18.0)),
                        )
                        .child(
                            view()
                                .w(px(3.0))
                                .h(px(7.0))
                                .rounded_full()
                                .bg(move || c::primary()),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px(px(PAD_X - INSET))
                .pt(px(6.0))
                .pb(px(2.0))
                .child(when(move || info.with(|i| i.1), || tag("Live")))
                .child(
                    text(move || info.with(|i| i.0.clone()))
                        .flex_1()
                        .text_size(px(font::SMALL + 1.0))
                        .font_weight(FontWeight::MEDIUM),
                ),
        )
        .into_element()
}

/// A sticker; one from a Sound stickers pack also has a speaker button
/// that plays its sound, and plays it by itself when it first shows if
/// the user lets sound stickers play automatically (Prompt 9).
fn sticker_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let s = cv.s;
    let info = sel(item, |i| match &i.m.body {
        Body::Sticker { emoji, pack } => (emoji.clone(), pack.clone()),
        _ => Default::default(),
    });
    let id = sel(item, |i| i.m.id);
    let sound = computed(move || info.with(|i| s.is_sound_sticker(&i.1)));
    // The cell may be given another message: read it where it's used.
    effect(move || {
        let (sticker, id, sound) = (info.with(|i| i.0.clone()), id.get(), sound.get());
        if sound && !sticker.is_empty() {
            untracked(|| super::scene::autoplay(cv.chat, id, &sticker));
        }
    });
    // A sticker from an installed pack carries its artwork (real chats); the built-in pack's
    // are emoji drawn here.
    let art = computed(move || {
        let m = id.get();
        crate::backend::chat::is_media(m).then(|| {
            crate::backend::chat::load_photo(m);
            crate::backend::chat::media_src(m)
        })
    });
    view()
        .flex_row()
        .items_end()
        .gap_1()
        .child(dynamic(move || {
            let a = art.get();
            untracked(move || {
                a.flatten().map(|src| {
                    image(src)
                        .size(px(128.0))
                        .object_fit(ResizeMode::Contain)
                        .label(move || info.with(|i| format!("Sticker from {}", i.1)))
                        .role(AccessibilityRole::Image)
                        .into_element()
                })
            })
        }))
        .child(
            text(move || {
                if art.with(|a| matches!(a, Some(Some(_)))) {
                    String::new()
                } else {
                    info.with(|i| i.0.clone())
                }
            })
            .text_size(px(96.0))
            .line_height(px(112.0))
            .px_1()
            .label(move || {
                info.with(|i| {
                    if sound.get() {
                        format!("Sound sticker from {}", i.1)
                    } else {
                        format!("Sticker from {}", i.1)
                    }
                })
            })
            .role(AccessibilityRole::Image),
        )
        .child(when(
            move || sound.get(),
            move || {
                icon_btn(Icon::Volume, "Play sound", move || {
                    super::scene::play(&info.with_untracked(|i| i.0.clone()))
                })
                .mb_2()
            },
        ))
        .into_element()
}

/// A view-once photo or video: what it is, and what to do ("Tap to
/// view"), then "Opened" once it has been seen.
fn view_once_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let ink = ink_of(cv, item);
    let id = sel(item, |i| i.m.id);
    let mine = sel(item, |i| i.m.mine());
    let info = sel(item, |i| match &i.m.body {
        Body::ViewOnce { photo, opened } => (*photo, *opened),
        _ => (true, false),
    });
    let title = move || {
        if info.get().0 {
            "View once photo"
        } else {
            "View once video"
        }
    };
    let hint = move || match (info.get().1, mine.get()) {
        (true, _) => "Opened",
        (false, true) => "Sent",
        (false, false) => "Tap to view",
    };
    let spent = move || info.get().1;
    pressable()
        .on_press(move || {
            let (photo, opened) = info.get_untracked();
            if mine.get_untracked() || opened {
                return;
            }
            cv.s.open_view_once(cv.chat, id.get_untracked());
            // mock: the viewer shows it once, then app-core deletes it.
            toast(if photo {
                "View once photo opened"
            } else {
                "View once video opened"
            });
        })
        .disabled(move || mine.get() || info.get().1)
        .accessibility_label(super::say(move || format!("{}, {}", title(), hint())))
        .flex_row()
        .items_center()
        .gap_3()
        .px(px(PAD_X))
        .pt(px(10.0))
        .pb(px(2.0))
        .w(px(230.0))
        .child(
            view()
                .size(px(38.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || {
                    if spent() {
                        c::foreground().opacity(0.07)
                    } else {
                        c::primary()
                    }
                })
                .text_color(move || {
                    if spent() {
                        c::bubble_meta()
                    } else {
                        c::on_primary()
                    }
                })
                .child(dynamic(move || {
                    let g = if info.get().0 {
                        Icon::Image
                    } else {
                        Icon::Video
                    };
                    Some(untracked(|| icon(g, 18.0)))
                }))
                // The "once" mark: a 1 in a ring at the corner.
                .child(
                    view()
                        .absolute()
                        .right(px(-3.0))
                        .bottom(px(-3.0))
                        .size(px(17.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .border_2()
                        .border_color(move || ink().fill)
                        .bg(move || {
                            if spent() {
                                c::bubble_meta()
                            } else {
                                c::primary()
                            }
                        })
                        .child(
                            text("1")
                                .text_size(px(9.0))
                                .line_height(px(10.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(move || {
                                    if spent() {
                                        c::bubble_in()
                                    } else {
                                        c::on_primary()
                                    }
                                }),
                        ),
                ),
        )
        .child(
            view()
                .flex_1()
                .gap(px(1.0))
                .child(
                    text(title)
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || {
                            if spent() {
                                c::bubble_meta()
                            } else {
                                c::foreground()
                            }
                        }),
                )
                .child(
                    text(hint)
                        .text_size(px(font::CAPTION))
                        .text_color(move || c::bubble_meta()),
                ),
        )
        .into_element()
}

/// A call: voice or video, missed in red with its own icon, the length,
/// and Call back.
fn call_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let info = sel(item, |i| match &i.m.body {
        Body::Call {
            video,
            missed,
            secs,
        } => (*video, *missed, *secs, i.m.mine()),
        _ => Default::default(),
    });
    let glyph = move |(video, missed, _, mine): (bool, bool, u32, bool)| match (missed, video, mine)
    {
        (true, _, _) => Icon::PhoneMissed,
        (false, true, _) => Icon::Video,
        (false, false, true) => Icon::PhoneOutgoing,
        (false, false, false) => Icon::PhoneIncoming,
    };
    let missed = move || info.get().1;
    view()
        .w(px(250.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px(px(PAD_X))
                .pt(px(10.0))
                .child(
                    view()
                        .size(px(38.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || {
                            if missed() {
                                c::destructive().opacity(0.12)
                            } else {
                                c::foreground().opacity(0.07)
                            }
                        })
                        .text_color(move || {
                            if missed() {
                                c::destructive()
                            } else {
                                c::primary()
                            }
                        })
                        .child(dynamic(move || {
                            let g = glyph(info.get());
                            Some(untracked(|| icon(g, 18.0)))
                        })),
                )
                .child(
                    view()
                        .flex_1()
                        .child(
                            text(move || {
                                let (video, missed, _, mine) = info.get();
                                logic::call_title(video, missed, mine)
                            })
                            .text_size(px(font::body()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || {
                                if missed() {
                                    c::destructive()
                                } else {
                                    c::foreground()
                                }
                            }),
                        )
                        .child(
                            text(move || {
                                let (_, missed, secs, _) = info.get();
                                if missed {
                                    "Tap to call back".to_string()
                                } else {
                                    logic::call_length(secs)
                                }
                            })
                            .text_size(px(font::CAPTION))
                            .text_color(move || c::bubble_meta()),
                        ),
                ),
        )
        .child(view().px_1().pt_1().child(btn_with_icon(
            "Call back",
            Some(Icon::Phone),
            Variant::Ghost,
            BtnSize::Sm,
            move || calls::start_call(cv.chat, info.get_untracked().0),
        )))
        .into_element()
}

fn deleted_body(cv: Conv, item: ItemMemo) -> AnyElement {
    let mine = sel(item, |i| i.m.mine());
    let ink = ink_of(cv, item);
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .px(px(PAD_X))
        .py(px(PAD_Y))
        .text_color(move || ink().quiet)
        .child(icon(Icon::Ban, 14.0))
        .child(
            text(move || {
                if mine.get() {
                    "You deleted this message"
                } else {
                    "This message was deleted"
                }
            })
            .italic()
            .text_size(px(font::body())),
        )
        .into_element()
}
