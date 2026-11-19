//! Invites: the user's invite codes (whether each was used, never by
//! whom) and a personal invite link that opens a chat with the user once
//! the friend signs up.

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{personal_link, store},
    ui::*,
};

/// The invites page, pushed (Account → Invites; the page has the row's name).
pub fn invites() {
    nav::push("invites", invites_page);
}

fn invites_page() -> Screen {
    let s = store();
    s.refresh_invites();
    let shared = signal(String::new());
    let left = move || s.session.with(|x| x.invites_left);
    let used = move || s.session.with(|x| x.invites_used);
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(
            view().px_4().pt_4().child(
                card()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(icon_tile(Icon::Gift))
                    .child(
                        view()
                            .flex_1()
                            .gap(px(2.0))
                            .child(
                                text(move || match left() {
                                    0 => "You've used all your invites".to_string(),
                                    1 => "You can invite 1 more person".to_string(),
                                    n => format!("You can invite {n} more people"),
                                })
                                .text_size(px(font::TITLE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .role(AccessibilityRole::Header),
                            )
                            .child(muted(move || match used() {
                                0 => "Nobody has joined with your invites yet.".to_string(),
                                1 => "1 person joined with your invites.".to_string(),
                                n => format!("{n} people joined with your invites."),
                            })),
                    ),
            ),
        )
        .child(section_title("Your invite link"))
        .child(group([Row::new("Invite with your link")
            .icon(Icon::Link)
            .subtitle("When they join, a chat with you is waiting for them")
            .chevron()
            .on_press(invite_friend)
            .build()]))
        .child(section_title("Your invite codes"))
        .child(
            view()
                .mx_4()
                .rounded(move || radius::lg())
                .overflow_hidden()
                .bg(move || c::card())
                .border_1()
                .border_color(move || c::border())
                .child(each(
                    move || s.invite_codes(),
                    |code| code.code.clone(),
                    move |code, index| {
                        let code = code.get_untracked();
                        let value = code.code.clone();
                        let copy_value = value.clone();
                        let share_value = value.clone();
                        view()
                            .child(view().style_when(move || index.get() == 0, |st| st.hidden()).child(separator_inset(INSET_ICON)))
                            .child(
                                code_row(
                                    &code.code,
                                    code.used,
                                    view()
                                        .flex_row()
                                        .gap_1()
                                        .style_when(move || code.used, |st| st.hidden())
                                        .child(
                                            icon_btn(Icon::Copy, format!("Copy {value}"), move || {
                                                set_clipboard(copy_value.clone());
                                                toast("Invite code copied");
                                            })
                                            .text_color(move || c::muted_fg()),
                                        )
                                        .child(
                                            icon_btn(Icon::Share, format!("Share {}", share_value), move || {
                                                shared.set(format!(
                                                    "Join me on Songbird, a private messenger. Use my invite code {share_value} at https://songbird.chat/join"
                                                ));
                                                share_text(shared.get_untracked());
                                            })
                                            .text_color(move || c::muted_fg()),
                                        ),
                                ),
                            )
                    },
                )),
        )
        .child(crate::features::settings::foot(
            "Songbird is invite-only while in beta, so we can grow carefully and keep it fast. You can see whether a code was used, but never who used it.",
        ));
    page(
        "Invites",
        view()
            .flex_1()
            .child(page_scroll(body))
    )
}

/// The personal invite link page, pushed.
pub fn invite_friend() {
    nav::push("invite-link", invite_link_page);
}

fn invite_link_page() -> Screen {
    let s = store();
    let me = s.me.get_untracked();
    // The real backend: the inviter's card in the link's fragment, which no server sees.
    let link_url = crate::backend::plans::my_invite_link()
        .filter(|_| crate::backend::is_live())
        .unwrap_or_else(|| personal_link(&me.username));
    let shared = signal(format!(
        "Let's talk on Songbird, a private messenger: {link_url}"
    ));
    let copy_url = link_url.clone();
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .px_4()
        .pt_6()
        .gap_4()
        .items_center()
        .child(avatar(me.name.clone(), me.seed, 64.0))
        .child(
            text("Invite a friend")
                .text_size(px(font::HEADING))
                .font_weight(FontWeight::BOLD)
                .role(AccessibilityRole::Header),
        )
        .child(
            muted("When a friend joins with your link, a chat with you is waiting. It uses an invite only once they sign up.")
                .text_center()
                .max_w(px(420.0)),
        )
        .child(qr_code(&link_url, 200.0).label("QR code of your invite link").role(AccessibilityRole::Image))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded(move || radius::control())
                .bg(move || c::secondary())
                .child(icon(Icon::Link, 16.0).text_color(move || c::muted_fg()))
                .child(
                    text(link_url.clone())
                        .text_size(px(font::SMALL + 1.0))
                        .select_text()
                        .number_of_lines(1),
                ),
        )
        .child(
            view()
                .flex_row()
                .gap_3()
                .child(btn_with_icon("Copy link", Some(Icon::Copy), Variant::Secondary, BtnSize::Md, move || {
                    set_clipboard(copy_url.clone());
                    toast("Link copied");
                }))
                .child(btn_with_icon("Share…", Some(Icon::Share), Variant::Primary, BtnSize::Md, move || {
                    share_text(shared.get_untracked())
                })),
        )
        .child(
            muted(move || {
                let left = s.session.with(|x| x.invites_left);
                format!("{left} invites left")
            })
            .text_center(),
        );
    page(
        "Invite a friend",
        view()
            .flex_1()
            .child(page_scroll(body))
    )
}


/// A grouped code on two lines, split between its middle groups, the
/// hyphen kept at the end of the first ("K7P2-QX9M-AB3D-" over
/// "EH4J-NR5S-TV6W"), so it reads as one code.
fn two_lines(code: &str) -> String {
    let groups: Vec<&str> = code.split('-').collect();
    let half = groups.len().div_ceil(2);
    if half >= groups.len() {
        return code.to_string();
    }
    format!(
        "{}-\n{}",
        groups[..half].join("-"),
        groups[half..].join("-")
    )
}

/// One invite code: monospace (0 and O, 1 and I stay apart), on one line
/// when there is room, else on two at a visible hyphen; whether it was
/// used; its actions.
fn code_row(code: &str, used: bool, actions: impl IntoElement) -> View {
    let shown = if wide() {
        code.to_string()
    } else {
        two_lines(code)
    };
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .min_h(px(if is_desktop() { 44.0 } else { 56.0 }))
        .role(AccessibilityRole::Text)
        .label(format!(
            "{code}, {}",
            if used { "used" } else { "not used yet" }
        ))
        .child(icon_tile(if used { Icon::Check } else { Icon::Gift }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(shown)
                        .font_family(MONO)
                        .text_size(px(font::body()))
                        .line_height(px(font::body() * 1.35))
                        .letter_spacing(px(0.5))
                        .text_color(move || if used { c::muted_fg() } else { c::foreground() })
                        .select_text(),
                )
                .child(
                    text(if used { "Used" } else { "Not used yet" })
                        .text_size(px(font::SMALL))
                        .text_color(move || c::muted_fg()),
                ),
        )
        .child(actions)
}
