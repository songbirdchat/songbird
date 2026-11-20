//! A10 (+A9 notification settings): the You tab — profile, account,
//! privacy and the privacy centre, chats, appearance, notifications,
//! presence and status, nudges, parental controls, proxy, lists.
//!
//! Desktop: the You tab is the list pane; each page pushes into the
//! detail pane (a top-level page replaces the one open there). Phones:
//! pages push over the tabs. Every change goes through
//! `Store::settings_update` / `profile_update` or the intents in
//! `store::ext_settings`.

pub(crate) mod account;
mod appearance;
mod centre;
pub(crate) mod chats;
mod extras;
pub use extras::ask_approval;
mod links;
mod notifications;
mod privacy;
pub use privacy::app_lock;
mod profile;

use std::rc::Rc;

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{Presence, matches, store, until_label},
    ui::*,
};

pub use centre::privacy_centre;

// ---------------------------------------------------------------- helpers

/// Pushes a settings page whose scrolling body `body` makes.
pub(crate) fn push(title: &'static str, body: impl Fn() -> AnyElement + 'static) {
    nav::push(title, move || {
        page(
            title,
            page_scroll(view().w_full().max_w(px(760.0)).self_center().child(body())),
        )
    });
}

/// Which You row's page is open in the desktop detail pane: the row's
/// title and the key of the page it pushed. The row stays selected while
/// that page is the bottom of the detail stack.
#[derive(Default, Clone, PartialEq)]
struct OpenRow(Option<(&'static str, SharedString)>);

fn open_row() -> Signal<OpenRow> {
    global::<OpenRow>()
}

/// Opens a top-level page from the You list: on desktop it replaces the
/// page in the detail pane, as a list selection does.
fn open_top(f: impl Fn()) {
    if wide() {
        nav::pop_to_root();
    }
    f();
}

/// [`open_top`] from the row titled `title`, remembered for its selected
/// state.
fn open_from(title: &'static str, f: impl Fn()) {
    open_top(f);
    let key = nav::routes().with_untracked(|r| r.0.first().map(|x| x.key.clone()));
    open_row().set(OpenRow(key.map(|k| (title, k))));
}

/// Whether the row titled `title` is the open one (desktop only: on a
/// phone the page covers the list).
fn row_selected(title: &'static str) -> bool {
    wide()
        && open_row().with(|o| match &o.0 {
            Some((t, key)) => {
                *t == title && nav::routes().with(|r| r.0.first().is_some_and(|x| &x.key == key))
            }
            None => false,
        })
}

/// A You row with the calm selected wash behind it: the row's own hover
/// and press washes sit on top, so selected reads as a steady fill and the
/// leading bar adds a shape, not colour alone.
fn selectable(title: &'static str, row: AnyElement) -> AnyElement {
    let selected = move || row_selected(title);
    view()
        .relative()
        .bg(move || {
            if selected() {
                c::sidebar_accent()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .transition_colors()
        .duration(motion::INSTANT)
        .child(row)
        .child(
            view()
                .absolute()
                .left_0()
                .top(px(10.0))
                .bottom(px(10.0))
                .w(px(3.0))
                .rounded_r(px(3.0))
                .bg(move || c::primary())
                .opacity(move || if selected() { 1.0_f32 } else { 0.0 })
                .transition_opacity()
                .duration(motion::INSTANT)
                .accessibility_elements_hidden(true),
        )
        .into_element()
}

/// A titled group of rows that lead with icon tiles (separators start
/// under the titles).
pub(crate) fn section(title: &'static str, rows: Vec<AnyElement>) -> View {
    view().child(section_title(title)).child(group(rows))
}

/// A titled group of rows without icons: separators start at the gutter.
pub(crate) fn section_plain(title: &'static str, rows: Vec<AnyElement>) -> View {
    view().child(section_title(title)).child(group_plain(rows))
}

/// Help text under a group that always wraps. The text sits in a padded
/// box rather than carrying the padding itself, so it is measured at the
/// width it is drawn at and never clips its last line.
pub(crate) fn foot(label: impl IntoReactive<SharedString>) -> View {
    view()
        .w_full()
        .px_4()
        .pt_2()
        .child(footnote(label).px_0().pt_0())
}

/// The leading tile of a destructive row (delete the account): the
/// standard squircle, in the danger tint.
pub(crate) fn danger_tile(glyph: Icon) -> View {
    icon_tile(glyph)
        .bg(move || c::danger_bg())
        .text_color(move || c::destructive())
}

/// A row whose trailing value follows the store (rebuilt when it changes).
pub(crate) fn value_row(
    title: &'static str,
    glyph: Option<Icon>,
    value: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> AnyElement {
    let value = computed(value);
    let on_press: Rc<dyn Fn()> = Rc::new(on_press);
    dynamic(move || {
        let v = value.get();
        let f = on_press.clone();
        untracked(move || {
            let row = Row::new(title).value(v).on_press(move || f());
            match glyph {
                Some(g) => row.icon(g).build(),
                None => row.build(),
            }
        })
    })
}

/// A plain navigation row with an icon and a chevron.
pub(crate) fn nav_row(
    title: &'static str,
    glyph: Icon,
    on_press: impl Fn() + 'static,
) -> AnyElement {
    Row::new(title)
        .icon(glyph)
        .chevron()
        .on_press(on_press)
        .build()
}

/// The presence choices, with a shape for each (never colour alone).
pub(crate) const PRESENCES: [(&str, &str); 4] = [
    ("Online", "Contacts you allow see you're here."),
    ("Away", "When you're idle, or when you choose it."),
    ("Busy", "Nudges and call sounds stay quiet."),
    (
        "Invisible",
        "You look offline. Your last seen stops updating.",
    ),
];

pub(crate) fn presence_of(label: &str) -> Presence {
    match label {
        "Away" => Presence::Away,
        "Busy" => Presence::Busy,
        "Invisible" => Presence::Invisible,
        _ => Presence::Online,
    }
}

pub(crate) fn presence_name(p: &Presence) -> &'static str {
    match p {
        Presence::Online => "Online",
        Presence::Away => "Away",
        Presence::Busy => "Busy",
        Presence::Invisible => "Invisible",
        Presence::LastSeen(_) => "Offline",
    }
}

/// The presence mark: a filled dot (online), a clock (away), a bar
/// (busy), a hollow ring (invisible).
pub(crate) fn presence_mark(label: &'static str, size: f32) -> AnyElement {
    match label {
        "Away" => icon(Icon::Clock, size)
            .text_color(move || c::warning())
            .into_element(),
        "Busy" => view()
            .size(px(size))
            .rounded_full()
            .items_center()
            .justify_center()
            .bg(move || c::destructive())
            .child(
                view()
                    .w(px(size * 0.55))
                    .h(px(2.0))
                    .rounded_full()
                    .bg(move || c::on_destructive()),
            )
            .into_element(),
        "Invisible" => view()
            .size(px(size))
            .rounded_full()
            .border_2()
            .border_color(move || c::muted_fg())
            .into_element(),
        _ => view()
            .size(px(size))
            .rounded_full()
            .bg(move || c::online())
            .into_element(),
    }
}

// ---------------------------------------------------------------- jump-to

/// A settings page as the desktop's ⌘K jump-to lists it.
pub struct JumpPage {
    /// What the row says: "Settings: Privacy", or the page's own name
    /// for places that read as destinations ("Linked devices").
    pub label: String,
    /// More words that find it ("dark", "blocked").
    pub keywords: &'static str,
    pub glyph: Icon,
    /// The You row's title, for [`open_page`].
    pub title: &'static str,
}

/// Pages people go to rather than settings they change: their name alone
/// says it, so they skip the "Settings:" prefix.
const DESTINATIONS: &[&str] = &[
    "Contacts",
    "Linked devices",
    "Broadcast lists",
    "Starred messages",
    "WhatsApp chats",
    "Subscription",
    "Help and feedback",
    "Invite a friend",
    "What Songbird can see",
    "Chat backup",
    "Server connection",
];

/// Every settings page, for jump-to. Keyboard shortcuts is left to the
/// desktop's own list, which shows its key.
pub fn jump_pages() -> Vec<JumpPage> {
    entries()
        .into_iter()
        .filter(|e| e.title != "Keyboard shortcuts")
        .map(|e| JumpPage {
            label: if DESTINATIONS.contains(&e.title) {
                e.title.to_string()
            } else {
                format!("Settings: {}", e.title)
            },
            keywords: e.keywords,
            glyph: e.glyph,
            title: e.title,
        })
        .collect()
}

/// Opens the settings page `title` (a [`JumpPage::title`]) from anywhere:
/// the You tab comes forward and, on desktop, its row shows selected.
pub fn open_page(title: &str) {
    let Some(e) = entries().into_iter().find(|e| e.title == title) else {
        return;
    };
    nav::pop_to_root();
    nav::tab().set(nav::Tab::You);
    open_from(e.title, e.open);
}

// ---------------------------------------------------------------- the You tab

/// One row of the You list, also what the settings search finds.
struct Entry {
    title: &'static str,
    keywords: &'static str,
    glyph: Icon,
    open: fn(),
}

fn entries() -> Vec<Entry> {
    let e = |title, keywords, glyph, open| Entry {
        title,
        keywords,
        glyph,
        open,
    };
    vec![
        e(
            "Account",
            "security notifications two-step pin passkeys delete request info",
            Icon::Key,
            account::account as fn(),
        ),
        e(
            "Privacy",
            "last seen online profile photo about status read receipts groups blocked timer disappearing app lock screenshots link previews ip",
            Icon::Lock,
            privacy::privacy,
        ),
        e(
            "Chats",
            "theme wallpaper animations reactions enter send archived save photos stickers backup transfer export clear",
            Icon::Chats,
            chats::chats,
        ),
        e(
            "Appearance",
            "theme dark light high contrast app icon text size motion",
            Icon::Palette,
            appearance::appearance,
        ),
        e(
            "Notifications",
            "sound ringtone tone reactions hide preview mute calls groups",
            Icon::Bell,
            notifications::notifications,
        ),
        e(
            "Storage and data",
            "media auto-download network usage proxy quality",
            Icon::Database,
            links::storage_and_data,
        ),
        e(
            "Contacts",
            "people address book save buddy groups notes friends work",
            Icon::Contact,
            crate::features::contacts::open,
        ),
        e(
            "Linked devices",
            "computer desktop tablet qr",
            Icon::Laptop,
            links::linked_devices,
        ),
        e(
            "Lists",
            "custom chat lists family work filters",
            Icon::Filter,
            extras::lists,
        ),
        e(
            "Broadcast lists",
            "send many contacts",
            Icon::Megaphone,
            extras::broadcasts,
        ),
        e(
            "Starred messages",
            "saved favourite star",
            Icon::Star,
            extras::starred,
        ),
        e(
            "Presence and nudges",
            "online away busy invisible nudge buzz status",
            Icon::Zap,
            extras::presence,
        ),
        e(
            "Parental controls",
            "child family supervised screen time quiet hours",
            Icon::Users,
            extras::parental,
        ),
        e(
            "Proxy",
            "blocked censorship connection network",
            Icon::Globe,
            extras::proxy,
        ),
        e(
            "WhatsApp chats",
            "bridge interop import",
            Icon::Chats,
            links::whatsapp,
        ),
        e(
            "Subscription",
            "plus plan family billing",
            Icon::Sparkles,
            links::subscription,
        ),
        e(
            "Help and feedback",
            "support contact faq crash reports",
            Icon::HelpCircle,
            links::help,
        ),
        e(
            "Invite a friend",
            "share invite code",
            Icon::Heart,
            links::invite_friend,
        ),
        e(
            "What Songbird can see",
            "privacy centre server data encryption threat model eu",
            Icon::ShieldCheck,
            privacy_centre,
        ),
        e(
            "Read receipts",
            "privacy ticks seen read status",
            Icon::Check,
            privacy::privacy,
        ),
        e(
            "Last seen and online",
            "privacy presence",
            Icon::Eye,
            privacy::last_seen,
        ),
        e(
            "Blocked contacts",
            "privacy block unblock",
            Icon::Ban,
            privacy::blocked,
        ),
        e(
            "App lock",
            "face id fingerprint pin privacy",
            Icon::Lock,
            privacy::app_lock,
        ),
        e(
            "Default message timer",
            "disappearing messages privacy",
            Icon::Timer,
            privacy::default_timer,
        ),
        e(
            "Theme",
            "appearance dark light high contrast",
            Icon::Moon,
            appearance::appearance,
        ),
        e(
            "Chat wallpaper",
            "chats theme background colour",
            Icon::Image,
            chats::chats,
        ),
        e(
            "Ringtone",
            "notifications calls sound",
            Icon::Volume,
            notifications::ringtone,
        ),
        e(
            "Two-step PIN",
            "account security",
            Icon::Key,
            links::account_security,
        ),
        e(
            "Crash reports",
            "privacy help diagnostics",
            Icon::Alert,
            privacy::privacy,
        ),
        e(
            "Chat backup",
            "backups recovery key restore",
            Icon::Cloud,
            links::backups,
        ),
        e(
            "Keyboard shortcuts",
            "desktop keys",
            Icon::Keyboard,
            links::shortcuts,
        ),
        e(
            "Server connection",
            "backend real server register account device connection developer",
            Icon::Server,
            links::server,
        ),
    ]
}

/// The You tab: profile card, presence, search, then the settings in
/// WhatsApp's order.
pub fn you_tab() -> AnyElement {
    let query = signal(String::new());
    let status_open = signal(false);
    let searching = move || query.with(|q| !q.trim().is_empty());
    view()
        .flex_1()
        .child(app_header(
            "You",
            false,
            Some(
                icon_btn(Icon::QrCode, "My QR code", move || {
                    open_from("Profile", profile::profile)
                })
                .into_element(),
            ),
        ))
        .child(page_scroll(
            view()
                .child(
                    view()
                        .px_4()
                        .pt_3()
                        .child(search_field("Search settings", query)),
                )
                .child(
                    when(searching, move || search_results(query))
                        .otherwise(move || you_home(status_open)),
                ),
        ))
        .child(profile::status_sheet(status_open))
        .into_element()
}

fn you_home(status_open: Signal<bool>) -> View {
    let all = entries();
    let pick = |names: &[&str]| -> Vec<AnyElement> {
        names
            .iter()
            .filter_map(|n| all.iter().find(|e| e.title == *n))
            .map(|e| {
                let open = e.open;
                let title = e.title;
                let row = if e.title == "Subscription" {
                    // The plan you're on, as the row's value (the page
                    // says the same).
                    value_row(
                        title,
                        Some(e.glyph),
                        || {
                            crate::features::plans::catalog::info(store().plan())
                                .name
                                .to_string()
                        },
                        move || open_from(title, open),
                    )
                } else {
                    Row::new(e.title)
                        .icon(e.glyph)
                        .on_press(move || open_from(title, open))
                        .chevron()
                        .build()
                };
                selectable(title, row)
            })
            .collect()
    };
    let mut last = pick(&[
        "Subscription",
        "Help and feedback",
        "Invite a friend",
        "Server connection",
    ]);
    if is_desktop() {
        last.push(selectable(
            "Desktop",
            nav_row("Desktop", Icon::Laptop, || {
                open_from("Desktop", links::desktop)
            }),
        ));
        last.push(selectable(
            "Keyboard shortcuts",
            nav_row("Keyboard shortcuts", Icon::Keyboard, || {
                open_from("Keyboard shortcuts", links::shortcuts)
            }),
        ));
    }
    // One rhythm: 12 inside the profile block, 24 to the call-out, 16
    // between groups.
    view()
        .child(profile_card())
        .child(presence_bar(status_open))
        .child(view().pt_6().child(callout(
            Icon::ShieldCheck,
            "What Songbird can see",
            "Exactly what our servers know, and what they never see.",
            || open_top(privacy_centre),
        )))
        .child(view().pt_4().child(group(pick(&[
            "Account",
            "Privacy",
            "Chats",
            "Appearance",
            "Notifications",
            "Storage and data",
        ]))))
        .child(view().pt_4().child(group(pick(&[
            "Contacts",
            "Linked devices",
            "Lists",
            "Broadcast lists",
            "Starred messages",
        ]))))
        .child(view().pt_4().child(group(pick(&[
            "Presence and nudges",
            "Parental controls",
            "Proxy",
            "WhatsApp chats",
        ]))))
        .child(view().pt_4().child(group(last)))
        .child(
            footnote(format!(
                "Songbird {} · Made in the EU",
                env!("CARGO_PKG_VERSION")
            ))
            .text_center()
            .pt_6(),
        )
}

/// The profile card: avatar with presence, name, about, status, username.
fn profile_card() -> AnyElement {
    let s = store();
    let me = s.me;
    let ext = s.ext();
    let reduced = motion::reduced();
    let status = move || {
        me.with(|p| p.status_line.clone()).map(|line| {
            let (emoji, until) = ext.with(|x| (x.status_emoji.clone(), x.status_until));
            format!(
                "{emoji} {line} · {}",
                until_label(until, crate::store::now()).to_lowercase()
            )
        })
    };
    nsh_base::button()
        .on_press(|| open_from("Profile", profile::profile))
        .selected(move || row_selected("Profile"))
        .accessibility_label(move || {
            Some(SharedString::from(format!(
                "Your profile, {}",
                me.with(|p| p.name.clone())
            )))
        })
        .mx_4()
        .mt_3()
        .p_4()
        .flex_row()
        .items_center()
        .gap_4()
        .rounded(move || radius::lg())
        .bg(move || {
            if row_selected("Profile") {
                c::sidebar_accent()
            } else {
                c::card()
            }
        })
        .border_1()
        .border_color(move || {
            if row_selected("Profile") {
                c::primary()
            } else {
                c::border()
            }
        })
        .cursor_pointer()
        .transition()
        .duration(motion::INSTANT)
        .hover(|st| st.bg(move || c::accent()))
        .active(move |st| {
            let st = st.bg(move || c::accent());
            if reduced { st } else { st.scale(0.99) }
        })
        .focus_visible(|st| {
            st.outline(px(2.0))
                .outline_offset(px(2.0))
                .outline_color(move || c::ring())
        })
        // Presence shows once on this tab: the choice right below.
        .child(avatar(
            me.with_untracked(|p| p.name.clone()),
            me.with_untracked(|p| p.seed),
            64.0,
        ))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(move || me.with(|p| p.name.clone()))
                        .text_size(px(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1),
                )
                .child(muted(move || me.with(|p| p.about.clone())).number_of_lines(1))
                .child(when(
                    move || status().is_some(),
                    move || {
                        text(move || status().unwrap_or_default())
                            .text_size(px(font::SMALL))
                            .text_color(move || c::primary())
                            .number_of_lines(1)
                    },
                ))
                .child(
                    text(move || format!("@{}", me.with(|p| p.username.clone())))
                        .text_size(px(font::SMALL))
                        .text_color(move || c::muted_fg())
                        .number_of_lines(1),
                ),
        )
        .child(icon(Icon::ChevronRight, 18.0).text_color(move || c::muted_fg()))
        .into_element()
}

/// Quick presence and "Set a status".
fn presence_bar(status_open: Signal<bool>) -> View {
    let s = store();
    let me = s.me;
    view()
        .px_4()
        .pt_3()
        .gap_2()
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .role(AccessibilityRole::Radiogroup)
                .label("Presence")
                .children(PRESENCES.iter().map(move |(label, _)| {
                    let label: &'static str = label;
                    let selected = move || me.with(|p| presence_name(&p.presence) == label);
                    option_pill(label, presence_mark(label, 10.0), selected, move || {
                        s.set_presence(presence_of(label));
                        if label == "Invisible" {
                            toast("You're invisible. You can still chat.");
                        }
                    })
                })),
        )
        .child(view().flex_row().child(btn_with_icon(
            move || {
                if me.with(|p| p.status_line.is_some()) {
                    "Change status"
                } else {
                    "Set a status"
                }
            },
            Some(Icon::Smile),
            Variant::Outline,
            BtnSize::Sm,
            move || status_open.set(true),
        )))
}

/// The search results: the matching rows, or a calm empty line.
fn search_results(query: Signal<String>) -> View {
    let found = computed(move || {
        query.with(|q| {
            entries()
                .into_iter()
                .filter(|e| matches(q, e.title, e.keywords))
                .map(|e| e.title)
                .collect::<Vec<_>>()
        })
    });
    view()
        .pt_3()
        .child(when(
            move || found.with(|f| f.is_empty()),
            move || {
                empty_state(
                    Icon::Search,
                    "No settings found",
                    "Try another word, like \"theme\" or \"blocked\".",
                    None,
                )
            },
        ))
        .child(when(
            move || found.with(|f| !f.is_empty()),
            move || {
                dynamic(move || {
                    let titles = found.get();
                    untracked(move || {
                        let all = entries();
                        group(
                            titles
                                .iter()
                                .filter_map(|t| all.iter().find(|e| e.title == *t))
                                .map(|e| {
                                    let open = e.open;
                                    let title = e.title;
                                    selectable(
                                        title,
                                        Row::new(title)
                                            .icon(e.glyph)
                                            .chevron()
                                            .on_press(move || open_from(title, open))
                                            .build(),
                                    )
                                }),
                        )
                    })
                })
            },
        ))
}
