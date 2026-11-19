//! New chat: search contacts, New group, New community, New broadcast
//! list, Invite a friend, Scan QR code, then contacts A–Z.

use std::collections::HashSet;

use nsh_core::{element::Position, prelude::*};

use super::{model, row, search};
use crate::{
    features,
    nav::{self, Tab},
    store::{ContactId, Store, store},
    ui::*,
};

pub fn open() {
    nav::push("chats:new", || page("New chat", body()));
}

/// The round tile the action rows lead with, the size of a contact's
/// avatar so the column lines up.
fn action_tile(glyph: Icon) -> View {
    let side = if is_desktop() { 40.0 } else { 44.0 };
    view()
        .size(px(side))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || c::primary())
        .text_color(move || c::on_primary())
        .accessibility_elements_hidden(true)
        .child(icon(glyph, if is_desktop() { 18.0 } else { 20.0 }))
}

fn action(glyph: Icon, label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    search::pressable(
        label.to_string(),
        on_press,
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .px(px(row::GUTTER - row::fill_inset()))
            .min_h(px(if is_desktop() { 52.0 } else { 60.0 }))
            .child(action_tile(glyph))
            .child(
                text(label)
                    .flex_1()
                    .text_size(px(font::body()))
                    .font_weight(FontWeight::MEDIUM),
            ),
    )
}

fn body() -> impl IntoElement {
    let s = store();
    let query = signal(String::new());
    let people = computed(move || {
        let q = query.get();
        s.chats
            .with(|cs| s.contacts.with(|ct| model::contacts_az(cs, ct, &q)))
    });
    let browsing = move || query.with(|q| q.trim().is_empty());
    let username = s.me.with_untracked(|m| m.username.clone());
    view()
        .flex_1()
        .child(
            view()
                .px_4()
                .py_2()
                .child(search_field("Search name or username", query)),
        )
        .child(
            virtual_list(
                move || people.with(Vec::len),
                move |at: Position| {
                    let p = computed(move || people.with(|v| v.get(at.index()).cloned()));
                    contact_cell(s, p)
                },
            )
            .keys(move || people.with(|v| v.iter().map(|p| p.id).collect::<Vec<_>>()))
            .estimated_item_size(px(64.0))
            .header(
                view()
                    .child(when(browsing, move || {
                        let username = username.clone();
                        view()
                            .child(action(
                                Icon::Users,
                                "New group",
                                features::groups::new_group,
                            ))
                            .child(action(Icon::Communities, "New community", || {
                                nav::pop_to_root();
                                nav::tab().set(Tab::Spaces);
                            }))
                            .child(action(
                                Icon::Megaphone,
                                "New broadcast list",
                                open_broadcast,
                            ))
                            .child(action(Icon::Share, "Invite a friend", move || {
                                // mock: app-core makes a one-time invite
                                // (A10 invites); the link format is final.
                                set_clipboard(format!("https://songbird.chat/invite/{username}"));
                                toast("Invite link copied. Share it with a friend.");
                            }))
                            .child(action(Icon::QrCode, "Scan QR code", open_scan))
                            .child(action(
                                Icon::Contact,
                                "Saved contacts",
                                features::contacts::open,
                            ))
                    }))
                    // A real account by its exact username (the real backend on;
                    // D-0139): only the name's hash leaves the device.
                    .child(when(
                        move || {
                            !browsing() && crate::backend::is_live() && crate::backend::registered()
                        },
                        move || {
                            action(
                                Icon::Search,
                                "Message this username on Songbird",
                                move || {
                                    let name = query.get_untracked();
                                    crate::backend::run(async move {
                                        let chat = crate::backend::chat::start_with_username(&name)
                                            .await?;
                                        nav::pop_to_root();
                                        features::chat::open(chat);
                                        Ok(())
                                    });
                                },
                            )
                        },
                    ))
                    .child(section_title("Contacts on Songbird")),
            )
            .empty(empty_state(
                Icon::Search,
                "No contacts found",
                "Check the spelling, or invite them to Songbird with a link.",
                None,
            ))
            .footer(view().h(px(24.0)))
            .flex_1(),
        )
}

fn contact_cell(s: Store, p: Memo<Option<crate::store::Contact>>) -> impl IntoElement {
    // Each cell is rebuilt for the contact it shows: contacts carry no
    // live state here, so a keyed region per contact is enough.
    dynamic(move || {
        p.get()
            .map(|p| search::contact_row(s, p, None).into_element())
            .unwrap_or_else(|| view().into_element())
    })
}

// ---------------------------------------------------------------- broadcast

fn open_broadcast() {
    nav::push("chats:broadcast", || {
        page("New broadcast list", broadcast())
    });
}

/// Picks two or more recipients. The real app makes a broadcast list
/// (A6); the mock confirms and goes back.
fn broadcast() -> impl IntoElement {
    let s = store();
    let picked = signal(HashSet::<ContactId>::new());
    let people = computed(move || {
        s.chats
            .with(|cs| s.contacts.with(|ct| model::contacts_az(cs, ct, "")))
    });
    let count = move || picked.with(HashSet::len);
    view()
        .flex_1()
        .child(
            virtual_list(move || people.with(Vec::len), move |at: Position| {
                let p = computed(move || people.with(|v| v.get(at.index()).cloned()));
                let id = move || p.with(|p| p.as_ref().map(|p| p.id).unwrap_or_default());
                let on = move || picked.with(|set| set.contains(&id()));
                nsh_base::button()
                    .on_press(move || {
                        let id = id();
                        picked.update(|set| {
                            if !set.remove(&id) {
                                set.insert(id);
                            }
                        });
                    })
                    .role(AccessibilityRole::Checkbox)
                    .selected(on)
                    .accessibility_label(move || Some(SharedString::from(p.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default()))))
                    .cursor_pointer()
                    .hover(|st| st.bg(move || c::muted()))
                    .focus_visible(|st| st.bg(move || c::accent()))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .min_h(px(if is_desktop() { 52.0 } else { 60.0 }))
                            .child(
                                view()
                                    .size(px(22.0))
                                    .rounded_full()
                                    .border_2()
                                    .items_center()
                                    .justify_center()
                                    .border_color(move || if on() { c::primary() } else { c::input() })
                                    .bg(move || if on() { c::primary() } else { AdaptiveColor::TRANSPARENT })
                                    .text_color(move || c::on_primary())
                                    .transition_colors()
                                    .duration(motion::FAST)
                                    .child(icon(Icon::Check, 14.0).style_when(move || !on(), |st| st.hidden().border_color(AdaptiveColor::TRANSPARENT))),
                            )
                            .child(
                                text(move || p.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default()))
                                    .flex_1()
                                    .number_of_lines(1)
                                    .text_size(px(font::body())),
                            ),
                    )
            })
            .keys(move || people.with(|v| v.iter().map(|p| p.id).collect::<Vec<_>>()))
            .estimated_item_size(px(60.0))
            .header(view().pb_2().child(footnote(
                "Your message goes to each person as a separate chat. They don't see who else got it.",
            )))
            .flex_1(),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .pt_3()
                .pb_safe_area_plus(px(12.0))
                .border_t_1()
                .border_color(move || c::border())
                .bg(move || c::background())
                .child(
                    text(move || match count() {
                        0 => "Choose at least 2 people".to_string(),
                        1 => "1 selected".to_string(),
                        n => format!("{n} selected"),
                    })
                    .flex_1()
                    .text_size(px(font::SMALL + 1.0))
                    .tabular_nums()
                    .text_color(move || c::muted_fg()),
                )
                .child(
                    btn("Create", Variant::Primary, BtnSize::Md, move || {
                        let n = count();
                        nav::pop();
                        toast(format!("Broadcast list created with {n} people"));
                    })
                    .disabled(move || count() < 2),
                ),
        )
}

// ---------------------------------------------------------------- scan

fn open_scan() {
    nav::push("chats:scan", || page("Scan QR code", scan()));
}

/// The scanner (mock: the camera preview is a frame here; the real app
/// shows the camera and decodes on the device).
fn scan() -> impl IntoElement {
    let s = store();
    let username = s.me.with_untracked(|m| m.username.clone());
    page_scroll(
        view()
            .items_center()
            .gap_4()
            .px_6()
            .pt_6()
            .child(
                view()
                    .size(px(240.0))
                    .rounded(move || radius::lg())
                    .border_2()
                    .border_color(move || c::primary())
                    .bg(move || c::muted())
                    .items_center()
                    .justify_center()
                    .text_color(move || c::muted_fg())
                    .label("Camera preview")
                    .child(icon(Icon::Scan, 64.0)),
            )
            .child(
                text("Point your camera at someone's Songbird QR code to start a chat.")
                    .text_center()
                    .max_w(px(320.0))
                    .text_size(px(font::body()))
                    .line_height(px(font::body() * 1.4)),
            )
            .child(muted("The code is read on this device.").text_center())
            .child(
                card()
                    .w_full()
                    .max_w(px(360.0))
                    .gap_2()
                    .child(section_label("Your code"))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(icon_tile(Icon::QrCode))
                            .child(
                                text(format!("songbird.chat/u/{username}"))
                                    .flex_1()
                                    .number_of_lines(1)
                                    .text_size(px(font::SMALL + 1.0)),
                            )
                            .child(icon_btn(Icon::Copy, "Copy your link", move || {
                                set_clipboard(format!("https://songbird.chat/u/{username}"));
                                toast("Your link is copied");
                            })),
                    ),
            ),
    )
}

fn section_label(label: &'static str) -> Text {
    text(label)
        .text_size(px(font::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(move || c::muted_fg())
}
