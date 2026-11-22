//! A6: groups, spaces and channels — the Spaces tab, the new group flow,
//! group, space and channel info, roles and permissions,
//! invite links, join requests and the member change log.
//!
//! Roles are clear everywhere: every action a member cannot take says who
//! can (`ask_line`), and the controls stay visible but disabled.

mod admin;
mod community;
mod create;
mod info;
pub mod qr;

use nsh_core::{
    element::{LayoutAnimation, LayoutCurve, LayoutTransition},
    prelude::*,
};

use crate::{
    store::{
        Chat, ChatId, ChatKind, Role, Store,
        ext_groups::{followers_label, who_to_ask},
        store,
    },
    ui::*,
};

pub use admin::{invite_link_page, join_requests_page, member_changes_page, roles_page};
pub use community::find_channels;
pub(crate) use create::check_circle;
pub use create::new_community;

// ------------------------------------------------------------------ entry points

/// The Spaces tab: the user's spaces with their groups, then the channels
/// they follow. (In the store a space is still a `ChatKind::Community`.)
pub fn spaces_tab() -> AnyElement {
    let s = store();
    let communities = move || {
        s.chats.with(|cs| {
            cs.iter()
                .filter(|c| matches!(c.kind, ChatKind::Community))
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    let none = move || {
        s.chats
            .with(|cs| !cs.iter().any(|c| matches!(c.kind, ChatKind::Community)))
    };
    view()
        .flex_1()
        .child(app_header(
            "Spaces",
            false,
            Some(icon_btn(Icon::Plus, "New space", new_community).into_element()),
        ))
        .child(page_scroll(
            view()
                .pt_3()
                .gap_4()
                .child(when(none, intro_card).otherwise(move || {
                    list_card([list_row(
                        square_tile(Icon::Plus),
                        "New space",
                        None,
                        None,
                        new_community,
                    )
                    .into_element()])
                }))
                .child(view().gap_4().child(each(
                    communities,
                    |c: &Chat| c.id,
                    move |c, _| community_block(s, c.get_untracked().id),
                )))
                .child(channels_section(s)),
        ))
        .into_element()
}

/// The new group flow: pick members, name it, choose its settings,
/// create it, share the invite link, then open the group.
pub fn new_group() {
    create::new_group_flow(None);
}

/// Group, community or channel info, pushed.
pub fn group_info(chat: ChatId) {
    crate::nav::push(format!("info:{chat}"), move || info::info_page(chat));
}

/// A space's home, pushed.
pub fn community_home(chat: ChatId) {
    crate::nav::push(format!("community:{chat}"), move || {
        community::home_page(chat)
    });
}

// ------------------------------------------------------------------ the tab's parts

fn intro_card() -> View {
    view().px_4().child(
        card()
            .items_center()
            .gap_3()
            .py_6()
            .child(square_tile(Icon::Spaces).size(px(56.0)))
            .child(
                text("Bring your groups together")
                    .text_size(px(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_center()
                    .role(AccessibilityRole::Header),
            )
            .child(
                muted("A space keeps related groups in one place, with an announcement group to reach everyone at once.")
                    .text_center()
                    .max_w(px(320.0)),
            )
            .child(btn("New space", Variant::Primary, BtnSize::Md, new_community)),
    )
}

/// One space: its header, its groups (Announcements first), View all.
fn community_block(s: Store, id: ChatId) -> View {
    view().child(dynamic(move || {
        let community = s.chat(id);
        let groups = s.community_groups(id);
        untracked(move || {
            let Some(c) = community else {
                return view().into_element();
            };
            let count = groups.len();
            let mut rows: Vec<AnyElement> = vec![
                list_row(
                    chat_avatar(&c, LANE),
                    c.title.clone(),
                    Some(if count == 1 {
                        "1 group".into()
                    } else {
                        format!("{count} groups").into()
                    }),
                    Some(chevron().into_element()),
                    move || community_home(id),
                )
                .into_element(),
            ];
            rows.extend(groups.iter().take(4).map(|g| group_row(s, g)));
            rows.push(view_all(move || community_home(id)).into_element());
            list_card(rows).into_element()
        })
    }))
}

/// A group inside a space's list: its last line and unread count.
pub(crate) fn group_row(s: Store, g: &Chat) -> AnyElement {
    let id = g.id;
    let unread = g.unread;
    let muted_chat = g.muted;
    let preview = s
        .last_message(id)
        .map(|m| {
            let who = if m.from.is_none() {
                "You".to_string()
            } else {
                first_name(&s.contact_name(m.from))
            };
            if matches!(m.body, crate::store::Body::System(_)) {
                m.body.preview()
            } else {
                format!("{who}: {}", m.body.preview())
            }
        })
        .unwrap_or_else(|| "No messages yet".into());
    list_row(
        chat_avatar(g, LANE),
        g.title.clone(),
        Some(preview.into()),
        (unread > 0).then(|| unread_badge(move || unread, move || muted_chat).into_element()),
        move || crate::features::chat::open(id),
    )
    .into_element()
}

fn channels_section(s: Store) -> View {
    let channels = move || {
        s.chats.with(|cs| {
            cs.iter()
                .filter(|c| matches!(c.kind, ChatKind::Channel { .. }))
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    view()
        .child(section_title("Channels"))
        .child(dynamic(move || {
            let list = channels();
            untracked(move || {
                let mut rows: Vec<AnyElement> = list
                    .iter()
                    .map(|ch| {
                        let id = ch.id;
                        let followers = match ch.kind {
                            ChatKind::Channel { followers, .. } => followers,
                            _ => 0,
                        };
                        list_row(
                            chat_avatar(ch, LANE),
                            ch.title.clone(),
                            Some(followers_label(followers).into()),
                            Some(chevron().into_element()),
                            move || crate::features::chat::open(id),
                        )
                        .into_element()
                    })
                    .collect();
                rows.push(
                    list_row(
                        square_tile(Icon::Search),
                        "Find channels",
                        None,
                        Some(chevron().into_element()),
                        find_channels,
                    )
                    .into_element(),
                );
                list_card(rows).into_element()
            })
        }))
        .child(footnote(
            "Channels are one-way: admins post, followers read. Followers can't see each other.",
        ))
}

// ------------------------------------------------------------------ shared parts

/// Animates the next layout change (a row added or removed), unless the
/// user asked for less motion.
pub(crate) fn glide() {
    if !motion::reduced() {
        animate_layout(LayoutAnimation::all(LayoutTransition::new(
            LayoutCurve::EaseInEaseOut,
            motion::BASE,
        )));
    }
}

/// The leading lane of the tab's rows: 40pt pictures and tiles.
pub(crate) const LANE: f32 = 40.0;

/// Where a row's text starts: the gutter, the lane and the gap. Separators
/// start here so they run under the text, never under the picture.
fn text_column() -> f32 {
    16.0 + LANE + 12.0
}

/// The tab's row height: the lane plus 12pt above and below (64pt), on
/// phones 72pt so the 16pt body text breathes.
fn row_height() -> f32 {
    if is_desktop() { 64.0 } else { 72.0 }
}

/// A trailing chevron in the muted ink.
fn chevron() -> View {
    view()
        .text_color(move || c::muted_fg())
        .child(icon(Icon::ChevronRight, 18.0))
}

/// One row of the Spaces tab and a space's home: a 40pt
/// picture, a one-line title, a one-line subtitle (previews never wrap,
/// so rows keep one height), and an optional trailing element. The
/// picture and the text block are centred as a unit.
pub(crate) fn list_row(
    leading: impl IntoElement,
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    trailing: Option<AnyElement>,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let title: SharedString = title.into();
    press_surface(Press::Row, on_press)
        .accessibility_label(title.clone())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .min_h(px(row_height()))
                .py_3()
                .child(leading)
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(2.0))
                        .child(text(title).text_size(px(font::body())).number_of_lines(1))
                        .child(subtitle.map(|s| {
                            text(s)
                                .text_size(px(font::SMALL + 1.0))
                                .line_height(px(18.0))
                                .text_color(move || c::muted_fg())
                                .number_of_lines(1)
                        })),
                )
                .child(trailing),
        )
}

/// "View all" under a space's groups: a short row whose label starts on
/// the text column, in the primary ink, so it reads as a link to the space
/// rather than as another group.
fn view_all(on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    press_surface(Press::Row, on_press)
        .accessibility_label("View all")
        .child(
            view()
                .flex_row()
                .items_center()
                .min_h(px(if is_desktop() { 44.0 } else { 52.0 }))
                .pl(px(text_column()))
                .pr_4()
                .child(
                    text("View all")
                        .flex_1()
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || c::primary()),
                )
                .child(
                    view()
                        .text_color(move || c::primary())
                        .child(icon(Icon::ChevronRight, 18.0)),
                ),
        )
}

/// A card of `list_row`s, the separators starting on the text column.
pub(crate) fn list_card(rows: impl IntoIterator<Item = AnyElement>) -> View {
    card_rows(rows, text_column())
}

/// A card of rows whose separators start `inset` points in: where the
/// rows' text starts (16 for rows without a leading element, 16 + lane +
/// 12 for rows with one), so a rule never runs under a picture or stops
/// short of the text it divides.
pub(crate) fn card_rows(rows: impl IntoIterator<Item = AnyElement>, inset: f32) -> View {
    let rows: Vec<AnyElement> = rows.into_iter().collect();
    let count = rows.len();
    view()
        .mx_4()
        .rounded(move || radius::lg())
        .overflow_hidden()
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::border())
        .children(rows.into_iter().enumerate().map(move |(i, row)| {
            view()
                .child(row)
                .child((i + 1 < count).then(|| separator_inset(inset)))
        }))
}

/// A space's mark: its picture (a mock scene) in a rounded square,
/// with the small kind badge the chat list puts on groups (a 2pt ring in
/// `ring`, the surface it sits on, around a quiet disc and glyph), here
/// with the Spaces glyph. Rounded squares are spaces; circles
/// are people, groups and channels.
pub(crate) fn community_mark(seed: u64, size: f32, ring: fn() -> AdaptiveColor) -> View {
    let big = size >= 64.0;
    // 16pt disc at list size (as the chat list's), growing with the mark;
    // even sizes so the glyph centres on whole points.
    let disc = ((size * 0.4 / 2.0).round() * 2.0).max(16.0);
    let corner = move || {
        if big {
            radius::sheet()
        } else {
            radius::control()
        }
    };
    view()
        .size(px(size))
        .accessibility_elements_hidden(true)
        .child(
            view()
                .size(px(size))
                .rounded(corner)
                .overflow_hidden()
                .bg(move || c::avatar(seed))
                .child(
                    image(crate::ui::portrait::scenery(seed))
                        .size(px(size))
                        .object_fit(ResizeMode::Cover),
                )
                // The hairline that keeps a light picture off a light surface.
                .child(
                    view()
                        .absolute()
                        .inset_0()
                        .rounded(corner)
                        .border_1()
                        .border_color(move || c::border())
                        .opacity(0.6),
                ),
        )
        .child(
            view()
                .absolute()
                .right(px(-2.0))
                .bottom(px(-2.0))
                .size(px(disc + 4.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(ring)
                .child(
                    view()
                        .size(px(disc))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || c::secondary())
                        .text_color(move || c::muted_fg())
                        .child(icon(Icon::Spaces, ((disc * 0.6) / 2.0).round() * 2.0)),
                ),
        )
}

/// A rounded square with an icon: spaces and the tab's actions.
pub(crate) fn square_tile(glyph: Icon) -> View {
    view()
        .size(px(40.0))
        .rounded(move || radius::control())
        .items_center()
        .justify_center()
        .bg(move || c::accent())
        .text_color(move || c::primary())
        .accessibility_elements_hidden(true)
        .child(icon(glyph, 20.0))
}

/// The picture of a group, space or channel.
pub(crate) fn chat_avatar(chat: &Chat, size: f32) -> View {
    let seed = chat.seed;
    match chat.kind {
        ChatKind::Community => community_mark(seed, size, c::card),
        _ if chat.announcement_only && chat.community.is_some() => view()
            .size(px(size))
            .rounded(move || radius::control())
            .items_center()
            .justify_center()
            .bg(move || c::accent())
            .text_color(move || c::primary())
            .accessibility_elements_hidden(true)
            .child(icon(Icon::Megaphone, size * 0.5)),
        // People get their (mock) portrait; groups and channels a scene.
        ChatKind::Direct(_) | ChatKind::WhatsApp(_) => avatar(chat.title.clone(), seed, size),
        _ => group_avatar(chat.title.clone(), seed, size),
    }
}

/// The picture of a group, space, channel or person, for other
/// features' rows (forward lists).
pub fn chat_avatar_for(chat: &Chat, size: f32) -> View {
    chat_avatar(chat, size)
}

/// A role's tag; members get none.
pub(crate) fn role_tag(role: Role) -> Option<View> {
    (role != Role::Member).then(|| tag(role.label()))
}

/// The first word of a name ("Ana" for "Ana Popescu").
pub(crate) fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

/// What a member who cannot do something is told: who can.
pub(crate) fn ask_line(s: Store, chat: &Chat, what: &str) -> String {
    match who_to_ask(chat) {
        Some((contact, role)) => {
            format!(
                "Only admins can {what}. Ask {} ({}).",
                s.contact_name(Some(contact)),
                role.label()
            )
        }
        None => format!("Only admins can {what}."),
    }
}

/// The bar under a page holding its main action.
pub(crate) fn bottom_bar(content: impl IntoElement) -> View {
    view()
        .px_4()
        .pt_3()
        // On the keyboard's edge while it shows (the home indicator is under it then), over the
        // home indicator otherwise: under the keyboard, "Create group" couldn't be reached
        // (found on an iPhone, D-0183).
        .pb(move || {
            let keyboard = keyboard_height().0;
            px(if keyboard > 0.0 {
                keyboard + 12.0
            } else {
                safe_area().bottom.0 + 12.0
            })
        })
        .border_t_1()
        .border_color(move || c::border())
        .bg(move || c::background())
        .child(readable(content))
}

/// A switch row with an icon in the row's icon lane, so it lines up with
/// the icon rows around it.
pub(crate) fn icon_toggle_row(
    glyph: Icon,
    title: &'static str,
    subtitle: Option<&'static str>,
    on: impl Fn() -> bool + Copy + 'static,
    set: impl Fn(bool) + 'static,
) -> AnyElement {
    let mut row = Row::new(title).icon(glyph);
    if let Some(sub) = subtitle {
        row = row.subtitle(sub);
    }
    row.trailing(toggle(title, on, set)).build()
}

/// A destructive row's icon, in the same 30pt lane as `icon_tile`.
pub(crate) fn danger_icon(glyph: Icon) -> View {
    view()
        .size(px(30.0))
        .items_center()
        .justify_center()
        .text_color(move || c::destructive())
        .child(icon(glyph, 20.0))
}

/// `ui::page_scroll` with the content at a readable width.
pub(crate) fn page_scroll(content: impl IntoElement) -> Scroll {
    crate::ui::page_scroll(readable(content))
}

/// Keeps a page's content at a readable width in the wide detail pane.
pub(crate) fn readable(content: impl IntoElement) -> View {
    view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(content)
}

/// A settings row with a switch that can be disabled (a member looking
/// at admin settings). The explanation sits under the title.
pub(crate) fn switch_row(
    title: &'static str,
    explain: &'static str,
    on: impl Fn() -> bool + Copy + 'static,
    enabled: impl Fn() -> bool + Copy + 'static,
    set: impl Fn(bool) + 'static,
) -> AnyElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .min_h(px(if is_desktop() { 44.0 } else { 56.0 }))
        .py_2()
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                // Read-only rows stay readable: only the switch dims.
                .child(text(title).text_size(px(font::body())))
                .child(
                    text(explain)
                        .text_size(px(font::SMALL))
                        .line_height(px(18.0))
                        .text_color(move || c::muted_fg()),
                ),
        )
        .child(
            toggle(title, on, set)
                .disabled(move || !enabled())
                .disabled_style(|st| st.opacity(0.45)),
        )
        .into_element()
}
