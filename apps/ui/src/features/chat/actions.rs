//! The message menu, as a focus view: the conversation dims, a copy of
//! the message is lifted in place over the dim, the user's quick
//! reactions sit directly above it and the actions (Reply, Copy, Forward,
//! Star, Pin, Edit, Message info, Select, Delete) directly below it, or
//! above when there is no room. Everything stays 8pt (and the safe area)
//! inside the window and never covers the message. It grows in from
//! 0.97; Escape, a press outside or a choice closes it.

use nsh_core::prelude::*;

use super::{
    Conv, MenuAt,
    list::{Item, ItemMemo},
    logic,
};
use crate::{store::Body, ui::*};

/// The overlay while a message's menu is open.
pub(crate) fn overlay_menu(cv: Conv) -> AnyElement {
    dynamic(move || {
        let at = cv.menu.get()?;
        Some(untracked(|| menu(cv, at)))
    })
}

fn grow(from_y: f32) -> MotionSpec {
    MotionSpec::new(super::timing(motion::SLOW, Easing::EaseOut))
        .values(MotionChannel::Opacity, [0.0, 1.0])
        .values(MotionChannel::Scale, [0.97, 1.0])
        .values(MotionChannel::TranslateY, [from_y, 0.0])
}

/// The window's margin every part keeps.
const MARGIN: f32 = 8.0;
/// Between the message and the reaction bar or the menu.
const GAP: f32 = 8.0;

/// Sizes of the parts, by layout.
struct Metrics {
    button: f32,
    row: f32,
    menu_w: f32,
}

fn metrics() -> Metrics {
    if is_desktop() {
        Metrics {
            button: 36.0,
            row: 34.0,
            menu_w: 240.0,
        }
    } else {
        Metrics {
            button: 44.0,
            row: 48.0,
            menu_w: 250.0,
        }
    }
}

/// Where each part goes (window points).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// The lifted message: its top, and the height shown (a message taller
    /// than the room left is shown from its top, clipped).
    msg_y: f32,
    msg_h: f32,
    bar: (f32, f32),
    menu: (f32, f32),
    /// The menu is above the message (no room below).
    menu_above: bool,
}

/// Lays the parts out around a message at `msg` (x, y, w, h) in a `win`
/// (w, h) window with `safe` (top, bottom) insets: the bar (`bar` w, h)
/// directly above, the menu (`menu` w, h) directly below, or both above
/// when there is no room below; the message moves only when neither fits.
/// Mine (`trailing`) align their trailing edges with the message's.
fn layout(
    msg: (f32, f32, f32, f32),
    win: (f32, f32),
    safe: (f32, f32),
    bar: (f32, f32),
    menu: (f32, f32),
    trailing: bool,
) -> Layout {
    let (mx, my, mw, mh) = msg;
    let top = safe.0 + MARGIN;
    let bottom = win.1 - safe.1 - MARGIN;
    let has_bar = bar.1 > 0.0;
    let bar_room = if has_bar { bar.1 + GAP } else { 0.0 };
    let align = |w: f32| {
        let x = if trailing { mx + mw - w } else { mx };
        x.min(win.0 - MARGIN - w).max(MARGIN).round()
    };
    // As it is: bar above, menu below.
    let fits_below = my - bar_room >= top && my + mh + GAP + menu.1 <= bottom;
    // As it is: menu, then bar, above.
    let fits_above = my - bar_room - menu.1 - GAP >= top && my + mh <= bottom;
    let (msg_y, msg_h, menu_above) = if fits_below {
        (my, mh, false)
    } else if fits_above {
        (my, mh, true)
    } else {
        // Moved (and clipped when it must be) so all fits, menu below.
        let room = (bottom - top - bar_room - GAP - menu.1).max(48.0);
        let h = mh.min(room);
        let y = my.max(top + bar_room).min(bottom - menu.1 - GAP - h);
        (y, h, false)
    };
    let bar_y = msg_y - bar_room;
    let menu_y = if menu_above {
        bar_y - GAP - menu.1
    } else {
        msg_y + msg_h + GAP
    };
    Layout {
        msg_y: msg_y.round(),
        msg_h: msg_h.round(),
        bar: (align(bar.0), bar_y.round()),
        menu: (align(menu.0), menu_y.round()),
        menu_above,
    }
}

fn menu(cv: Conv, at: MenuAt) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let Some(m) = s.message(chat, at.id) else {
        return view().into_element();
    };
    let close = move || cv.menu.set(None);
    let k = metrics();
    let reacts = !matches!(m.body, Body::Deleted);
    let quick = s.settings.with_untracked(|x| x.default_reactions.len()) + 1;
    let bar = if reacts {
        (
            quick as f32 * k.button + (quick - 1) as f32 * 2.0 + 8.0 + 2.0,
            k.button + 8.0 + 2.0,
        )
    } else {
        (0.0, 0.0)
    };
    let rows = rows_count(&m);
    let menu_h = rows as f32 * k.row + 8.0 + 2.0;
    let win = window();
    let safe = safe_area();
    let at_layout = layout(
        (at.x, at.y, at.w, at.h),
        (win.width.0, win.height.0),
        (safe.top.0, safe.bottom.0),
        bar,
        (k.menu_w, menu_h),
        m.mine(),
    );
    let (bar_part, menu_part) = panel(cv, &m, k.menu_w);
    // The message, lifted: the same bubble, made again for the overlay at
    // the frame it has in the list.
    let item: ItemMemo =
        computed(move || s.message(chat, at.id).map(|m| Item::single(m, at.place)));
    let lifted = view()
        .absolute()
        .left(px(at.x.round()))
        .top(px(at_layout.msg_y))
        .w(px(at.w.ceil()))
        .h(px(at_layout.msg_h))
        .when(at_layout.msg_h < at.h, |v| v.overflow_hidden())
        .accessibility_elements_hidden(true)
        .pointer_events(PointerEvents::None)
        .child(super::bubbles::bubble(cv, item));
    // A small rise as it lifts (none with reduced motion).
    let lifted_in = if motion::reduced() {
        motion()
    } else {
        motion().entering(
            MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                .values(MotionChannel::Scale, [1.0, 1.03, 1.0]),
        )
    };
    let fade = motion().entering(
        MotionSpec::new(super::timing(motion::FAST, Easing::EaseOut))
            .values(MotionChannel::Opacity, [0.0, 1.0]),
    );
    let from_menu = if at_layout.menu_above { 6.0 } else { -6.0 };
    let bar_in = motion().entering(grow(6.0));
    let menu_in = motion().entering(grow(from_menu));
    view()
        .child(
            overlay().absolute().inset_0().child(
                pressable()
                    .absolute()
                    .inset_0()
                    .bind_motion(&fade)
                    .bg(move || c::overlay())
                    .accessibility_label("Close menu")
                    .on_press(close),
            ),
        )
        .child(
            overlay()
                .absolute()
                .inset_0()
                .pointer_events(PointerEvents::BoxNone)
                .child(view().bind_motion(&lifted_in).child(lifted)),
        )
        .child(bar_part.map(|b| {
            overlay()
                .absolute()
                .left(px(at_layout.bar.0))
                .top(px(at_layout.bar.1))
                .child(view().bind_motion(&bar_in).child(b))
        }))
        .child(
            overlay()
                .absolute()
                .left(px(at_layout.menu.0))
                .top(px(at_layout.menu.1))
                .child(view().bind_motion(&menu_in).child(menu_part)),
        )
        .into_element()
}

/// How many action rows the panel lists for `m` (as [`panel`] builds them).
fn rows_count(m: &crate::store::Message) -> usize {
    let deleted = matches!(m.body, Body::Deleted);
    let copyable = matches!(m.body, Body::Text(_) | Body::Link { .. });
    let mut n = 2; // Select, Delete
    if !deleted {
        n += 4 + usize::from(copyable) + usize::from(logic::editable(m));
    }
    n + usize::from(m.mine())
}

/// One row of the menu.
fn action(glyph: Icon, label: &'static str, danger: bool, f: impl Fn() + 'static) -> AnyElement {
    let ink = move || {
        if danger {
            c::destructive()
        } else {
            c::foreground()
        }
    };
    pressable()
        .on_press(f)
        .accessibility_label(label)
        .role(AccessibilityRole::Menuitem)
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .min_h(px(if is_desktop() { 34.0 } else { 48.0 }))
        .rounded(move || radius::control())
        .hover(|st| st.bg(move || c::accent()))
        .child(icon(glyph, 18.0).text_color(ink))
        .child(text(label).text_size(px(font::body())).text_color(ink))
        .into_element()
}

/// The reaction bar (none for a deleted message) and the action menu.
fn panel(cv: Conv, m: &crate::store::Message, menu_w: f32) -> (Option<View>, View) {
    let (s, chat) = (cv.s, cv.chat);
    let id = m.id;
    let mine = m.mine();
    let deleted = matches!(m.body, Body::Deleted);
    let copyable = matches!(m.body, Body::Text(_) | Body::Link { .. });
    let starred = m.starred;
    let pinned = m.pinned;
    let reacted: Option<String> = m
        .reactions
        .iter()
        .find(|r| r.by.contains(&None))
        .map(|r| r.emoji.clone());
    let quick = s.settings.with_untracked(|x| x.default_reactions.clone());
    let mut rows: Vec<AnyElement> = Vec::new();
    if !deleted {
        rows.push(action(Icon::Reply, "Reply", false, move || {
            cv.start_reply(id)
        }));
        if copyable {
            rows.push(action(Icon::Copy, "Copy", false, move || cv.copy(&[id])));
        }
        rows.push(action(Icon::Forward, "Forward", false, move || {
            cv.ask_forward(vec![id])
        }));
        rows.push(action(
            Icon::Star,
            if starred { "Unstar" } else { "Star" },
            false,
            move || {
                cv.menu.set(None);
                s.toggle_star(chat, id);
                toast(if starred { "Unstarred" } else { "Starred" });
            },
        ));
        rows.push(action(
            Icon::Pin,
            if pinned { "Unpin" } else { "Pin" },
            false,
            move || {
                cv.menu.set(None);
                s.toggle_pin_message(chat, id);
                toast(if pinned {
                    "Message unpinned"
                } else {
                    "Message pinned"
                });
            },
        ));
        if logic::editable(m) {
            rows.push(action(Icon::Edit, "Edit", false, move || cv.start_edit(id)));
        }
    }
    if mine {
        rows.push(action(Icon::Info, "Message info", false, move || {
            cv.menu.set(None);
            super::message_info(chat, id);
        }));
    }
    rows.push(action(Icon::Check, "Select", false, move || {
        cv.start_select(id)
    }));
    rows.push(action(Icon::Trash, "Delete", true, move || {
        cv.ask_delete(vec![id])
    }));

    let reaction = move |emoji: String| {
        let on = reacted.as_deref() == Some(emoji.as_str());
        let e = emoji.clone();
        pressable()
            .on_press(move || cv.react(id, &e))
            .selected(on)
            .accessibility_label(if on {
                format!("Remove {emoji}")
            } else {
                format!("React with {emoji}")
            })
            .size(px(if is_desktop() { 36.0 } else { 44.0 }))
            .rounded_full()
            .items_center()
            .justify_center()
            .bg(move || {
                if on {
                    c::accent()
                } else {
                    AdaptiveColor::TRANSPARENT
                }
            })
            .hover(|st| st.bg(move || c::accent()))
            .transition_transform()
            .duration(motion::FAST)
            .active(|st| {
                if motion::reduced() {
                    st.opacity(0.7)
                } else {
                    st.scale(1.18)
                }
            })
            .child(text(emoji).text_size(px(if is_desktop() { 22.0 } else { 26.0 })))
    };
    let reactions = view()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .p_1()
        .rounded_full()
        .bg(move || c::popover())
        .border_1()
        .border_color(move || c::border())
        .shadow_lg()
        .role(AccessibilityRole::Toolbar)
        .label("Reactions")
        .children(quick.into_iter().map(reaction))
        .child(
            pressable()
                .on_press(move || {
                    cv.menu.set(None);
                    cv.acting_on.set(vec![id]);
                    cv.reactions_open.set(true);
                })
                .accessibility_label("More reactions")
                .size(px(if is_desktop() { 36.0 } else { 44.0 }))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::secondary())
                .text_color(move || c::muted_fg())
                .child(icon(Icon::Plus, 18.0)),
        );
    let actions = view()
        .w(px(menu_w))
        .p_1()
        .rounded(move || radius::lg())
        .bg(move || c::popover())
        .border_1()
        .border_color(move || c::border())
        .shadow_lg()
        .role(AccessibilityRole::Menu)
        .label("Message actions")
        .children(rows);
    ((!deleted).then_some(reactions), actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: (f32, f32) = (390.0, 844.0);
    const BAR: (f32, f32) = (330.0, 54.0);
    const MENU: (f32, f32) = (250.0, 442.0);

    fn no_overlap(a: (f32, f32, f32), b: (f32, f32, f32)) -> bool {
        // (top, height, unused): vertical spans only.
        a.0 + a.1 <= b.0 || b.0 + b.1 <= a.0
    }

    #[test]
    fn bar_above_and_menu_below_when_there_is_room() {
        let l = layout(
            (100.0, 200.0, 220.0, 40.0),
            WIN,
            (0.0, 0.0),
            BAR,
            MENU,
            true,
        );
        assert_eq!(l.msg_y, 200.0);
        assert_eq!(l.bar.1, 200.0 - 8.0 - 54.0);
        assert_eq!(l.menu.1, 248.0);
        assert!(!l.menu_above);
        // Trailing edges line up with the message's (kept inside).
        assert_eq!(l.menu.0, 320.0 - 250.0);
        assert_eq!(l.bar.0, 8.0_f32.max(320.0 - 330.0));
    }

    #[test]
    fn the_menu_flips_above_near_the_bottom() {
        let l = layout(
            (40.0, 700.0, 200.0, 40.0),
            WIN,
            (0.0, 0.0),
            BAR,
            MENU,
            false,
        );
        assert!(l.menu_above);
        assert_eq!(l.msg_y, 700.0);
        assert!(l.menu.1 >= 8.0);
        assert!(no_overlap((l.menu.1, MENU.1, 0.0), (l.msg_y, l.msg_h, 0.0)));
        assert!(no_overlap((l.bar.1, BAR.1, 0.0), (l.msg_y, l.msg_h, 0.0)));
        assert!(no_overlap((l.bar.1, BAR.1, 0.0), (l.menu.1, MENU.1, 0.0)));
    }

    #[test]
    fn the_message_moves_when_neither_fits_and_all_stays_inside() {
        let l = layout(
            (40.0, 380.0, 200.0, 120.0),
            WIN,
            (47.0, 34.0),
            BAR,
            MENU,
            false,
        );
        assert!(l.bar.1 >= 47.0 + 8.0);
        assert!(l.menu.1 + MENU.1 <= 844.0 - 34.0 - 8.0);
        assert!(no_overlap((l.menu.1, MENU.1, 0.0), (l.msg_y, l.msg_h, 0.0)));
        // A very tall message is shown from its top, clipped.
        let tall = layout(
            (40.0, 60.0, 200.0, 900.0),
            WIN,
            (0.0, 0.0),
            BAR,
            MENU,
            false,
        );
        assert!(tall.msg_h < 900.0);
        assert!(tall.menu.1 + MENU.1 <= 836.0);
    }
}
