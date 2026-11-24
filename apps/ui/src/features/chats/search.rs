//! Search results: Chats, Messages (with the match highlighted) and
//! Contacts, from the device's own index (mocked in `model::search`).

use nsh_base::button;
use nsh_core::prelude::*;

use super::{model, row};
use crate::{
    features,
    store::{Contact, Store},
    ui::*,
};

/// The results for `query`, in place of the list.
pub fn results(s: Store, query: Signal<String>) -> impl IntoElement {
    let found = computed(move || {
        let q = query.get();
        s.chats.with(|cs| {
            s.messages
                .with(|m| s.contacts.with(|ct| model::search(&q, cs, m, ct)))
        })
    });
    let chats = move || found.with(|r| r.chats.clone());
    let messages = move || found.with(|r| r.messages.clone());
    let people = move || found.with(|r| r.contacts.clone());
    scroll()
        .flex_1()
        .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
        .child(
            view()
                .pb_safe_area_plus(px(24.0))
                .child(when(
                    move || found.with(model::Results::is_empty),
                    move || {
                        empty_state(
                            Icon::Search,
                            "No results",
                            "Try another spelling, or search for a name, a username or words from a message.",
                            None,
                        )
                    },
                ))
                .child(when(
                    move || found.with(|r| !r.chats.is_empty()),
                    move || {
                        view().child(super::section_label("Chats")).child(each(
                            chats,
                            |r: &model::ChatRow| r.id,
                            move |item, _| {
                                let row = computed(move || item.get());
                                row::chat_row(s, row, model::ListScope::Inbox, Some(query))
                            },
                        ))
                    },
                ))
                .child(when(
                    move || found.with(|r| !r.messages.is_empty()),
                    move || {
                        view().child(super::section_label("Messages")).child(each(
                            messages,
                            // The cut moves with the query: a row per hit and cut.
                            |h: &model::MessageHit| (h.chat, h.msg, h.before.clone(), h.matched.clone()),
                            |hit, _| message_hit(hit.get_untracked()),
                        ))
                    },
                ))
                .child(when(
                    move || found.with(|r| !r.contacts.is_empty()),
                    move || {
                        view().child(super::section_label("Contacts")).child(each(
                            people,
                            |p: &Contact| p.id,
                            move |p, _| contact_row(s, p.get_untracked(), Some(query)),
                        ))
                    },
                )),
        )
}

/// A pressable row with the list's hover, press and focus states.
pub fn pressable(
    label: String,
    on_press: impl Fn() + 'static,
    content: impl IntoElement,
) -> nsh_base::ButtonElement {
    // The rows' fill: inset and rounded on wide layouts (content keeps the
    // gutter, see `row::fill_inset`).
    let inset = row::fill_inset();
    button()
        .on_press(on_press)
        .accessibility_label(label)
        .mx(px(inset))
        .when(inset > 0.0, |b| b.rounded(move || radius::control()))
        .cursor_pointer()
        .transition_colors()
        .duration(motion::INSTANT)
        .hover(|st| st.bg(move || c::muted()))
        .active(|st| st.bg(move || c::accent()))
        .focus_visible(|st| st.bg(move || c::accent()))
        .child(content)
}

/// A message that matched: the chat, when, and the text around the match
/// with the match highlighted.
fn message_hit(hit: model::MessageHit) -> impl IntoElement {
    let chat = hit.chat;
    let spoken = format!(
        "{}, {}, {}{}{}",
        hit.title, hit.when, hit.before, hit.matched, hit.after
    );
    pressable(
        spoken,
        // The real app opens the chat at the message (app-core gives the
        // position); the mock opens the chat.
        move || features::chat::open(chat),
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .px(px(row::GUTTER - row::fill_inset()))
            .py_2()
            .min_h(px(64.0))
            .child(avatar(
                model::letters(&hit.title),
                hit.seed,
                row::avatar_size(),
            ))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                text(hit.title)
                                    .flex_1()
                                    .number_of_lines(1)
                                    .text_size(px(font::body()))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                text(hit.when)
                                    .text_size(px(font::CAPTION))
                                    .tabular_nums()
                                    .text_color(move || c::muted_fg()),
                            ),
                    )
                    .child(
                        text(hit.before)
                            .number_of_lines(2)
                            .text_size(px(font::SMALL + 1.0))
                            .line_height(px(19.0))
                            .text_color(move || c::muted_fg())
                            .child(
                                text(hit.matched)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(move || c::highlight())
                                    .text_color(move || c::foreground()),
                            )
                            .child(text(hit.after)),
                    ),
            ),
    )
}

/// A contact: opens (or starts) the direct chat. Clears `query` after.
pub fn contact_row(s: Store, p: Contact, query: Option<Signal<String>>) -> impl IntoElement {
    let id = p.id;
    let line = p
        .status_line
        .clone()
        .or_else(|| (!p.about.is_empty()).then(|| p.about.clone()))
        .unwrap_or_else(|| handle(&p.username));
    let spoken = format!(
        "{}{}, {}",
        p.name,
        if p.verified { ", verified" } else { "" },
        line
    );
    pressable(
        spoken,
        move || {
            let chat = s.direct_chat(id);
            if let Some(q) = query {
                q.set(String::new());
            }
            features::chat::open(chat);
        },
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .px(px(row::GUTTER - row::fill_inset()))
            .py_2()
            .min_h(px(if is_desktop() { 56.0 } else { 64.0 }))
            .child(avatar(
                p.name.clone(),
                p.id,
                if is_desktop() { 40.0 } else { 44.0 },
            ))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(5.0))
                            .child({
                                // In results, the part of the name that matched.
                                let name = p.name.clone();
                                let cut = computed(move || {
                                    query
                                        .and_then(|q| q.with(|q| model::cut_match(&name, q)))
                                        .unwrap_or((name.clone(), String::new(), String::new()))
                                });
                                text(move || cut.with(|c| c.0.clone()))
                                    .child(
                                        text(move || cut.with(|c| c.1.clone()))
                                            .bg(move || c::highlight())
                                            .text_color(move || c::foreground()),
                                    )
                                    .child(text(move || cut.with(|c| c.2.clone())))
                                    .number_of_lines(1)
                                    .text_size(px(font::body()))
                                    .font_weight(FontWeight::MEDIUM)
                            })
                            .child(p.verified.then(|| {
                                icon(Icon::ShieldCheck, 15.0).text_color(move || c::primary())
                            })),
                    )
                    .child(
                        text(line)
                            .number_of_lines(1)
                            .text_size(px(font::SMALL))
                            .text_color(move || c::muted_fg()),
                    ),
            ),
    )
}
