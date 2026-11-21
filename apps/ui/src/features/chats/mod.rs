//! A5: the chat list — the Lists capsule and the Unread toggle, pinned
//! people, "Needs a reply" (desktop), pin, mute, archive, row menus,
//! message requests, locked chats, local search, new chat. The Floating
//! design (`REDESIGN.md`).
//!
//! - `model`: what the lists show, as plain data (unit-tested).
//! - `capsule`: the Lists capsule and the Unread toggle.
//! - `pinned`: the pinned people above the list.
//! - `row`: one chat row and its menu (right-click or long-press).
//! - `search`: the results that replace the list while searching.
//! - `pages`: Archived, Message requests, Locked chats, Chat lists.
//! - `new_chat`: the New chat page and what it leads to.

mod capsule;
mod model;
pub(crate) mod new_chat;
mod pages;
mod pinned;
mod row;
mod search;

use nsh_base::menu;
use nsh_core::{LayoutAnimation, LayoutCurve, LayoutTransition, prelude::*};

use crate::{
    features,
    store::{Store, store},
    ui::*,
};

pub use model::Filter;
use model::ListScope;

// ---------------------------------------------------------------- state

/// What the list shows: the view chosen in the Lists capsule (or the list
/// menu), and the Unread toggle over it. Kept while the window switches
/// between the phone and desktop layouts.
#[derive(Default, Clone, PartialEq)]
pub struct ChatFilter {
    pub list: Filter,
    pub unread: bool,
}

fn filter() -> Signal<ChatFilter> {
    global::<ChatFilter>()
}

/// Every custom list name, in the user's order.
fn list_names(s: Store) -> Vec<String> {
    // The order is the user's (Settings → Lists owns it, `Store::ext`).
    let order = s.ext().with(|x| x.lists.clone());
    s.chats.with(|cs| model::list_names(&order, cs))
}

/// The surface the list sits on: the shell's floating list pane on wide
/// layouts, the page on phones. Cut-outs (badge rings, the group mark,
/// the capsule's edge fade) take its colour.
fn surface() -> fn() -> AdaptiveColor {
    if wide() {
        floating::pane_bg
    } else {
        c::background
    }
}

/// Animates the list's next layout change when it is the user's doing
/// (pin, archive, delete, accept, the Unread toggle, a list chosen): rows
/// glide to their new place, rows that leave fade, and a chat pinned or
/// unpinned leaves the list and joins the people above (or back).
/// Nothing with reduced motion.
fn animate_change() {
    if !motion::reduced() {
        animate_layout(LayoutAnimation::all(LayoutTransition::new(
            LayoutCurve::EaseInEaseOut,
            motion::BASE,
        )));
    }
}

// ---------------------------------------------------------------- tab

/// The Chats tab (phone) or the list pane's content (desktop).
pub fn chats_tab() -> AnyElement {
    let s = store();
    let query = signal(String::new());
    let searching = move || query.with(|q| !q.trim().is_empty());
    let desktop = wide();
    view()
        .flex_1()
        // Wide layouts: the shell's floating list pane is the card, so the
        // tab paints nothing behind itself. Phones: the page.
        .when(!desktop, |v| v.bg(move || c::background()))
        .child(app_header(
            "Chats",
            false,
            Some(header_actions(desktop).into_element()),
        ))
        .child(when(
            move || s.session.with(|x| x.offline),
            || {
                view().px_4().pt_2().child(banner(
                    Tone::Warning,
                    Icon::WifiOff,
                    "You're offline. Messages you send will go out when you're back online.",
                ))
            },
        ))
        .child(crate::features::plans::standing_banner())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px(px(row::GUTTER))
                .pt_2()
                .pb_2()
                .child(search_field("Search chats and messages", query).flex_1())
                .child(capsule::unread_toggle()),
        )
        // ⌘K / ⌘F (desktop shortcuts) bring the Chats tab forward; the
        // search field is the first thing there.
        .child({
            let focus = global::<features::desktop::SearchFocus>();
            let seen = stored(focus.get_untracked().0);
            effect(move || {
                let n = focus.get().0;
                if seen.replace(n) != n {
                    crate::nav::tab().set(crate::nav::Tab::Chats);
                }
            });
            view()
        })
        // The list stays mounted while searching, so it keeps its place.
        .child(
            view()
                .flex_1()
                .style_when(searching, |st| st.hidden())
                .child(capsule::lists_capsule(s))
                .child(inbox(s)),
        )
        .child(when(searching, move || search::results(s, query)))
        // The floating button where the platform has one (Material); the
        // others put New chat in the header (skin: `ui::spec().fab`).
        .child((!desktop && spec().fab).then(|| {
            view()
                .absolute()
                .right(px(16.0))
                .bottom(px(16.0))
                .child(fab(Icon::Plus, "New chat", new_chat))
        }))
        .into_element()
}

/// The header's actions: New chat (desktop, and phones whose platform has
/// no floating button) and the list menu, which also holds the views the
/// capsule leaves out (Favourites, Groups, Channels).
fn header_actions(desktop: bool) -> View {
    let s = store();
    let f = filter();
    let item = |glyph: Icon, label: &'static str, f: fn()| {
        row::menu_row(glyph, move || label.to_string(), false).on_select(move || f())
    };
    let view_item = move |glyph: Icon, which: Filter| {
        let label = which.label();
        let held = stored(which);
        let on = move || f.with(|x| held.with(|w| x.list == *w));
        row::menu_row(glyph, move || label.clone(), false)
            .checked(move || Some(on()))
            .on_select(move || {
                animate_change();
                let which = held.get();
                // Choosing the view that is on goes back to All.
                f.update(|x| x.list = if x.list == which { Filter::All } else { which });
            })
    };
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            (desktop || !spec().fab).then(|| icon_btn(spec().compose_icon(), "New chat", new_chat)),
        )
        .child(
            menu()
                .offset(px(4.0))
                .popup_style(row::popup_style)
                .backdrop_style(|st| st.bg(c::overlay()))
                .trigger(row::icon_trigger(Icon::More, "More options"))
                .child(item(Icon::Users, "New group", features::groups::new_group))
                .child(item(Icon::Layers, "Chat lists", pages::open_lists))
                .child(
                    row::menu_row(Icon::Check, || "Mark all as read".to_string(), false).on_select(
                        move || {
                            s.mark_all_read();
                            toast("All chats marked as read");
                        },
                    ),
                )
                .child(row::separator_item())
                .child(view_item(Icon::Heart, Filter::Favourites))
                .child(view_item(Icon::Users, Filter::Groups))
                .child(view_item(Icon::Megaphone, Filter::Channels)),
        )
}

// ---------------------------------------------------------------- inbox

/// The inbox: the message-request notice, pinned people, "Needs a reply"
/// and "Recent" (desktop) or one list (phones), then the quiet footer
/// rows (Locked chats, Archived).
///
/// "Recent" (the long part) is the virtual list's rows, recycled; the
/// parts above it are short by design (eight people, five chats until
/// "Show all") and are the list's header, so they scroll with it.
fn inbox(s: Store) -> VirtualList {
    let f = filter();
    let triage = wide();
    let show_all = signal(false);
    let split = computed(move || {
        let which = f.with(|x| x.list.clone());
        let unread = f.with(|x| x.unread);
        let rows = s
            .chats
            .with(|cs| {
                s.messages.with(|m| {
                    s.contacts
                        .with(|ct| model::rows(cs, m, ct, ListScope::Inbox, &which))
                })
            })
            .into_iter()
            // WhatsApp chats (A15): gone when unlinked; in All only when the
            // user keeps them together, else under their own segment.
            .filter(|r| {
                !r.whatsapp
                    || (s.whatsapp_linked() && (which != Filter::All || s.whatsapp_in_all()))
            })
            .collect::<Vec<_>>();
        let order = s
            .chats
            .with(|cs| cs.iter().map(|c| c.id).collect::<Vec<_>>());
        model::inbox(rows, &order, unread, triage, show_all.get())
    });
    let pinned = computed(move || split.with(|x| x.pinned.clone()));
    let needs = computed(move || split.with(|x| x.needs.clone()));
    let recent = computed(move || split.with(|x| x.recent.clone()));
    // A list chosen as a filter that was then deleted falls back to All.
    effect(move || {
        let gone = f.with(|x| matches!(&x.list, Filter::List(n) if !list_names(s).contains(n)));
        if gone {
            f.update(|x| x.list = Filter::All);
        }
    });
    let sections = move || triage && needs.with(|n| !n.is_empty());
    row::chat_list(s, recent, ListScope::Inbox)
        .header(
            view()
                .child(requests_notice(s))
                .child(pinned::grid(s, pinned))
                .child(when(sections, move || {
                    needs_section(s, split, needs, show_all)
                }))
                .child(when(
                    move || sections() && recent.with(|r| !r.is_empty()),
                    || section_label("Recent"),
                ))
                .child(when(move || split.with(model::Inbox::is_empty), empty)),
        )
        .footer(footer(s))
}

/// The empty list, for what is chosen: no chats yet, an empty list or
/// view, or nothing unread.
fn empty() -> impl IntoElement {
    let f = filter();
    dynamic(move || {
        let (which, unread) = f.with(|x| (x.list.clone(), x.unread));
        let (title, message) = which.empty_copy(unread);
        let glyph = match (&which, unread) {
            (_, true) => Icon::Check,
            (Filter::Favourites, _) => Icon::Heart,
            (Filter::Groups, _) => Icon::Users,
            (Filter::Channels, _) => Icon::Megaphone,
            (Filter::WhatsApp, _) => Icon::Chats,
            (Filter::List(_), _) => Icon::Layers,
            (Filter::All, _) => Icon::Chats,
        };
        let action = match (&which, unread) {
            (_, true) => btn("Show all chats", Variant::Ghost, BtnSize::Md, move || {
                animate_change();
                f.update(|x| x.unread = false);
            }),
            (Filter::All, false) => btn("Start a chat", Variant::Primary, BtnSize::Md, new_chat),
            _ => btn("Show all chats", Variant::Ghost, BtnSize::Md, move || {
                animate_change();
                f.update(|x| x.list = Filter::All);
            }),
        };
        empty_state(glyph, title, message, Some(action.into_element()))
    })
}

/// A section's label in the list: small, muted, sentence case, with room
/// above it (space separates the sections, not rules). It starts on the
/// avatar column.
pub(super) fn section_label(label: impl IntoReactive<SharedString>) -> Text {
    text(label)
        .role(AccessibilityRole::Header)
        .text_size(px(font::CAPTION))
        .line_height(px(16.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(move || c::muted_fg())
        // Margins, not padding: a Text measures padding into its wrap
        // width.
        .mx(px(row::GUTTER))
        .mt_5()
        .mb_1()
}

/// "Needs a reply": the chats waiting for the user, five until "Show
/// all". Desktop only.
fn needs_section(
    s: Store,
    split: Memo<model::Inbox>,
    needs: Memo<Vec<model::ChatRow>>,
    show_all: Signal<bool>,
) -> View {
    let more = move || split.with(|x| x.more_needs);
    let total = move || needs.with(Vec::len) + more();
    let expandable = move || total() > model::MAX_NEEDS;
    view()
        .child(section_label("Needs a reply"))
        .child(each(
            move || needs.get(),
            |r: &model::ChatRow| r.id,
            move |item, _| {
                let row = computed(move || item.get());
                row::chat_row(s, row, ListScope::Inbox, None)
            },
        ))
        .child(when(expandable, move || {
            quiet_row(
                move || {
                    if show_all.get() {
                        Icon::ChevronUp
                    } else {
                        Icon::ChevronDown
                    }
                },
                move || {
                    (if show_all.get() {
                        "Show less"
                    } else {
                        "Show all"
                    })
                    .to_string()
                },
                move || {
                    if show_all.get() {
                        String::new()
                    } else {
                        total().to_string()
                    }
                },
                || false,
                move || {
                    if show_all.get() {
                        "Show less".to_string()
                    } else {
                        format!("Show all {} chats that need a reply", total())
                    }
                },
                move || {
                    animate_change();
                    show_all.update(|v| *v = !*v);
                },
            )
        }))
}

/// "1 message request": an inline notice at the top, only while someone
/// not in the user's contacts is waiting (in All).
fn requests_notice(s: Store) -> impl IntoElement {
    let f = filter();
    let count = computed(move || s.chats.with(|cs| cs.iter().filter(|c| c.request).count()));
    let label = move || {
        let n = count.get();
        if n == 1 {
            "1 message request".to_string()
        } else {
            format!("{n} message requests")
        }
    };
    when(
        move || count.get() > 0 && f.with(|x| x.list == Filter::All && !x.unread),
        move || {
            view().px(px(row::GUTTER)).pt_1().pb_2().child(
                nsh_base::button()
                    .on_press(pages::open_requests)
                    .accessibility_label(move || Some(SharedString::from(label())))
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(if is_desktop() { 36.0 } else { 44.0 }))
                    .px_3()
                    .rounded(move || radius::control())
                    .bg(move || c::secondary())
                    .cursor_pointer()
                    .transition_colors()
                    .duration(motion::INSTANT)
                    .hover(|st| st.bg(move || c::muted()))
                    .active(|st| st.bg(move || c::accent()))
                    .focus_visible(|st| {
                        st.outline(px(2.0))
                            .outline_offset(px(2.0))
                            .outline_color(move || c::ring())
                    })
                    .child(icon(Icon::Inbox, 16.0).text_color(move || c::muted_fg()))
                    .child(
                        text(label)
                            .flex_1()
                            .number_of_lines(1)
                            .text_size(px(row::preview_size()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || c::foreground()),
                    )
                    .child(icon(Icon::ChevronRight, 16.0).text_color(move || c::muted_fg())),
            )
        },
    )
}

/// The quiet rows after the chats: Locked chats and "Archived · 2", in
/// All; then room to scroll the last row clear of the bottom chrome.
fn footer(s: Store) -> View {
    let f = filter();
    let all = move || f.with(|x| x.list == Filter::All && !x.unread);
    let archived = computed(move || s.chats.with(|cs| model::count(cs, ListScope::Archived)));
    let locked = computed(move || s.chats.with(|cs| model::count(cs, ListScope::Locked).0));
    view()
        .pt_2()
        .child(when(
            move || all() && locked.get() > 0,
            move || {
                quiet_row(
                    || Icon::Lock,
                    || "Locked chats".to_string(),
                    String::new,
                    || false,
                    || "Locked chats".to_string(),
                    pages::open_locked,
                )
            },
        ))
        .child(when(
            move || all() && archived.get().0 > 0,
            move || {
                quiet_row(
                    || Icon::Archive,
                    || "Archived".to_string(),
                    move || archived.get().0.to_string(),
                    // An unread archived chat: the unread dot, as on rows.
                    move || archived.get().1 > 0,
                    move || {
                        let (n, unread) = archived.get();
                        let chats = if n == 1 {
                            "1 chat".to_string()
                        } else {
                            format!("{n} chats")
                        };
                        if unread > 0 {
                            format!("Archived, {chats}, {unread} unread")
                        } else {
                            format!("Archived, {chats}")
                        }
                    },
                    pages::open_archived,
                )
            },
        ))
        .child(view().h(px(if wide() { 16.0 } else { 88.0 })))
}

/// A quiet row ("Archived · 2", "Show all · 8"): a small muted glyph in
/// the avatar column, the label on the names' column at the preview's
/// size, an optional count after a middle dot, and the unread dot when
/// `dot` holds. Same fills as the rows.
fn quiet_row(
    glyph: impl Fn() -> Icon + 'static,
    label: impl Fn() -> String + 'static,
    count: impl Fn() -> String + 'static,
    dot: impl Fn() -> bool + 'static,
    spoken: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let inset = row::fill_inset();
    nsh_base::button()
        .on_press(on_press)
        .accessibility_label(move || Some(SharedString::from(spoken())))
        .mx(px(inset))
        .when(inset > 0.0, |b| b.rounded(move || radius::control()))
        .cursor_pointer()
        .transition_colors()
        .duration(motion::INSTANT)
        .hover(|st| st.bg(move || c::muted()))
        .active(|st| st.bg(move || c::accent()))
        .focus_visible(|st| st.bg(move || c::accent()))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(row::GUTTER - inset))
                .h(px(if is_desktop() { 40.0 } else { 48.0 }))
                .child(
                    view()
                        .w(px(row::avatar_size()))
                        .items_center()
                        .justify_center()
                        .text_color(move || c::muted_fg())
                        .child(dynamic(move || icon(glyph(), 16.0))),
                )
                .child(
                    text(label)
                        .number_of_lines(1)
                        .text_size(px(row::preview_size()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || c::muted_fg())
                        .child(
                            text(move || {
                                let n = count();
                                if n.is_empty() { n } else { format!(" · {n}") }
                            })
                            .font_weight(FontWeight::NORMAL)
                            .tabular_nums(),
                        ),
                )
                .child(
                    view()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(move || c::primary())
                        .style_when(move || !dot(), |st| st.hidden()),
                ),
        )
}

// ---------------------------------------------------------------- entry points

/// Starts a new chat (contact picker, new group, new community, invite).
pub fn new_chat() {
    new_chat::open();
}
