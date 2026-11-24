//! Contact info (groups, communities and channels go to their own info,
//! A6): the person, quick actions, media, starred, notifications, theme,
//! disappearing messages, lock, encryption, groups in common, and the
//! danger zone.

use nsh_core::prelude::*;

use super::sheets;
use crate::{
    features::{calls, devices, groups, media},
    nav,
    store::{Body, ChatId, ChatKind, Message, Store, presence_label, store, when_label},
    ui::*,
};

/// The chat info screen (contact or group), pushed.
pub fn chat_info(chat: ChatId) {
    let s = store();
    let Some(c) = s.chat(chat) else { return };
    match c.kind {
        ChatKind::Group | ChatKind::Community | ChatKind::Channel { .. } => {
            groups::group_info(chat)
        }
        ChatKind::Direct(_) | ChatKind::WhatsApp(_) => {
            nav::push(format!("chat-info:{chat}"), move || {
                page("Contact info", contact_info(chat))
            });
        }
    }
}

/// A round quick action under the name: Audio, Video, Search.
fn quick(glyph: Icon, label: &'static str, f: impl Fn() + 'static) -> AnyElement {
    pressable()
        .on_press(f)
        .accessibility_label(label)
        .w(px(96.0))
        .py_3()
        .gap_1()
        .items_center()
        .rounded(move || radius::lg())
        .border_1()
        .border_color(move || c::border())
        .bg(move || c::card())
        .hover(|st| st.bg(move || c::accent()))
        .text_color(move || c::primary())
        .child(icon(glyph, 22.0))
        .child(
            text(label)
                .text_size(px(font::SMALL))
                .text_color(move || c::foreground()),
        )
        .into_element()
}

/// A media tile: the message's picture (the same mock photo as its
/// bubble and the viewer, cropped square), a play mark on videos; screen
/// readers get the description.
fn media_tile(m: &Message, side: f32) -> AnyElement {
    let chat = m.chat;
    let id = m.id;
    let (shot, label, video) = match &m.body {
        Body::Image { alt, w, h, .. } => (
            portrait::message_photo(alt, id, *w, *h),
            format!("Photo: {alt}"),
            false,
        ),
        Body::Video { caption, secs } => (
            portrait::message_photo(&format!("video {caption}"), id, 16, 9),
            format!("Video, {}", super::logic::duration(*secs)),
            true,
        ),
        _ => (portrait::photo("", id, 1, 1), "Media".to_string(), false),
    };
    pressable()
        .on_press(move || media::open_viewer(chat, id))
        .accessibility_label(label)
        .size(px(side))
        .rounded(move || radius::sm())
        .overflow_hidden()
        .items_center()
        .justify_center()
        .bg(move || c::muted())
        .child(
            image(shot)
                .absolute()
                .inset_0()
                .object_fit(ResizeMode::Cover),
        )
        .child(video.then(|| {
            view()
                .size(px(28.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(AdaptiveColor::rgb(0x0F0D1A, 0x0F0D1A).opacity(0.6))
                .text_color(AdaptiveColor::rgb(0xFFFFFF, 0xFFFFFF))
                .child(icon(Icon::Play, 14.0))
        }))
        .into_element()
}

/// A document or link in the info strip: a tile with its glyph and a
/// short name, so the strip shows everything the count says.
fn doc_tile(m: &Message, side: f32, chat: ChatId) -> AnyElement {
    let (glyph, name, label) = match &m.body {
        Body::File { name, kind, .. } => (Icon::File, name.clone(), format!("{kind}: {name}")),
        Body::Link { site, title, .. } => {
            (Icon::Link, title.clone(), format!("Link: {title}, {site}"))
        }
        _ => (Icon::File, String::new(), String::new()),
    };
    pressable()
        .on_press(move || media_page(chat))
        .accessibility_label(label)
        .size(px(side))
        .rounded(move || radius::sm())
        .p_2()
        .gap_1()
        .items_center()
        .justify_center()
        .bg(move || c::accent())
        .text_color(move || c::primary())
        .child(icon(glyph, 20.0))
        .child(
            text(name)
                .number_of_lines(2)
                .text_center()
                .text_size(px(10.0))
                .line_height(px(12.0))
                .text_color(move || c::foreground()),
        )
        .into_element()
}

fn contact_info(chat: ChatId) -> AnyElement {
    let s = store();
    let peer_id = match s.chat(chat).map(|c| c.kind) {
        Some(ChatKind::Direct(id) | ChatKind::WhatsApp(id)) => id,
        _ => {
            return empty_state(Icon::User, "Not found", "This chat has no contact.", None)
                .into_element();
        }
    };
    let peer = computed(move || s.contact(peer_id));
    let info = computed(move || s.chat(chat));
    let name = move || peer.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let first = move || {
        name()
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string()
    };
    let seed = info.with_untracked(|c| c.as_ref().map_or(0, |c| c.seed));
    let timer_open = signal(false);
    let theme_open = signal(false);
    let scene_open = signal(false);
    // The strip shows what the count counts: photos and videos first,
    // then documents and links.
    let media_count = s.media_of(chat).len() + s.docs_of(chat).len() + s.links_of(chat).len();
    let strip: Vec<Message> = s
        .media_of(chat)
        .into_iter()
        .chain(s.docs_of(chat))
        .chain(s.links_of(chat))
        .take(8)
        .collect();
    let starred = s.starred_of(chat).len();
    let common = s.groups_in_common(peer_id);
    let whatsapp = info.with_untracked(|c| {
        c.as_ref()
            .is_some_and(|c| matches!(c.kind, ChatKind::WhatsApp(_)))
    });

    let identity = view()
        .items_center()
        .gap_1()
        .pt_6()
        .pb_4()
        .px_4()
        .child(avatar(name(), seed, 112.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .pt_3()
                .child(
                    text(name)
                        .text_size(px(font::HEADING + 2.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(AccessibilityRole::Header),
                )
                .child(when(
                    move || peer.with(|p| p.as_ref().is_some_and(|p| p.verified)),
                    || {
                        icon(Icon::ShieldCheck, 18.0)
                            .text_color(move || c::primary())
                            .label("Verified")
                    },
                )),
        )
        .child(muted(move || {
            peer.with(|p| p.as_ref().map(|p| handle(&p.username)).unwrap_or_default())
        }))
        .child(muted(move || {
            peer.with(|p| {
                p.as_ref()
                    .map(|p| presence_label(&s.presence_of(p)))
                    .unwrap_or_default()
            })
        }))
        .child(when(
            move || peer.with(|p| p.as_ref().is_some_and(|p| p.status_line.is_some())),
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .mt_1()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(move || c::secondary())
                    .child(
                        text(move || {
                            peer.with(|p| {
                                p.as_ref()
                                    .and_then(|p| p.status_line.clone())
                                    .unwrap_or_default()
                            })
                        })
                        .text_size(px(font::SMALL)),
                    )
            },
        ))
        .child(whatsapp.then(|| view().pt_1().child(tag("WhatsApp"))))
        .child(
            view()
                .flex_row()
                .gap_3()
                .pt_4()
                .child(quick(Icon::Phone, "Audio", move || {
                    calls::start_call(chat, false)
                }))
                .child(quick(Icon::Video, "Video", move || {
                    calls::start_call(chat, true)
                }))
                .child(quick(Icon::Search, "Search", move || {
                    super::search_in(chat)
                })),
        );

    let about = move || peer.with(|p| p.as_ref().map(|p| p.about.clone()).unwrap_or_default());
    // The label above its value (a form's reading order).
    let about_group = when(
        move || !about().is_empty(),
        move || {
            group_plain([view()
                .px_4()
                .py_3()
                .gap(px(2.0))
                .child(
                    text("About")
                        .text_size(px(font::SMALL))
                        .text_color(move || c::muted_fg()),
                )
                .child(
                    text(about)
                        .text_size(px(font::body()))
                        .text_color(move || c::foreground()),
                )
                .into_element()])
        },
    );

    // One card: the row names it, so no section title repeats the name.
    let media_group = view().child(
        card()
            .mx_4()
            .p_0()
            .child(
                Row::new("Media, links and docs")
                    .icon(Icon::Image)
                    .value(media_count.to_string())
                    .on_press(move || media_page(chat))
                    .build(),
            )
            .child((!strip.is_empty()).then(|| {
                scroll()
                    .horizontal()
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .px_4()
                            .pb_3()
                            .children(strip.iter().map(|m| match m.body {
                                Body::Image { .. } | Body::Video { .. } => media_tile(m, 72.0),
                                _ => doc_tile(m, 72.0, chat),
                            })),
                    )
            })),
    );

    let settings_group = group([
        Row::new("Starred messages")
            .icon(Icon::Star)
            .value(starred.to_string())
            .on_press(move || starred_page(chat))
            .build(),
        // Every row of the card has its icon tile, so titles share one
        // column with the inset separators.
        Row::new("Mute notifications")
            .icon(Icon::BellOff)
            .trailing(toggle(
                "Mute notifications",
                move || info.with(|c| c.as_ref().is_some_and(|c| c.muted)),
                move |_| s.toggle_mute(chat),
            ))
            .build(),
        super::theme::theme_row(chat, theme_open),
        super::scene::scene_row(chat, scene_open),
        Row::new("Disappearing messages")
            .icon(Icon::Timer)
            .trailing(
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(muted(move || {
                        info.with(|c| {
                            c.as_ref()
                                .and_then(|c| c.disappearing)
                                .map_or("Off", |t| t.label())
                        })
                    }))
                    // On the trailing spine with the other rows' chevrons.
                    .child(
                        icon(Icon::ChevronRight, 16.0)
                            .mr(px(-4.0))
                            .text_color(move || c::muted_fg()),
                    ),
            )
            .on_press(move || timer_open.set(true))
            .build(),
        // Lock is the encryption row's glyph; this one hides the chat.
        Row::new("Lock chat")
            .icon(Icon::EyeOff)
            .subtitle("Hide this chat behind your device lock")
            .trailing(toggle(
                "Lock chat",
                move || info.with(|c| c.as_ref().is_some_and(|c| c.locked)),
                move |_| s.toggle_lock(chat),
            ))
            .build(),
    ]);

    let encryption = group([Row::new("Encryption")
        .icon(Icon::Lock)
        .subtitle("Messages and calls are end-to-end encrypted. Tap to verify.")
        .chevron()
        .on_press(move || devices::safety_number(peer_id))
        .build()]);

    let common_group = view()
        .child(section_title(format!(
            "{} {} in common",
            common.len(),
            if common.len() == 1 { "group" } else { "groups" }
        )))
        .child(if common.is_empty() {
            footnote("No groups in common.").into_element()
        } else {
            group(common.into_iter().map(|g| {
                let id = g.id;
                Row::new(g.title.clone())
                    .leading(group_avatar(g.title.clone(), g.seed, 36.0))
                    .subtitle(format!("{} members", g.members.len()))
                    .on_press(move || super::open(id))
                    .build()
            }))
            .into_element()
        });

    let blocked = move || peer.with(|p| p.as_ref().is_some_and(|p| p.blocked));
    let danger = group([
        Row::new("Export chat").icon(Icon::Upload).on_press(move || {
            // A text file of the chat, saved where the user chooses. It
            // leaves Songbird's encryption: the user decides where it goes.
            let text = s.export_text(chat);
            save_file("songbird-chat.txt", text.into_bytes());
        }).build(),
        Row::new("Clear chat")
            .icon(Icon::Trash)
            .danger()
            .on_press(move || {
                confirm("Clear this chat?", "Messages will be removed from this device.", "Clear chat", move || {
                    s.clear_chat(chat);
                    toast("Chat cleared");
                })
            })
            .build(),
        Row::new(if blocked() { format!("Unblock {}", first()) } else { format!("Block {}", first()) })
            .icon(Icon::Ban)
            .danger()
            .on_press(move || {
                if blocked() {
                    s.block(peer_id, false);
                    toast(format!("{} unblocked", name()));
                    return;
                }
                let who = name();
                confirm(
                    format!("Block {who}?"),
                    "Blocked contacts can't call you or send you messages. They won't be told you blocked them.",
                    "Block",
                    move || {
                        s.block(peer_id, true);
                        toast(format!("{who} blocked"));
                    },
                )
            })
            .build(),
        Row::new(format!("Report {}", first()))
            .icon(Icon::Flag)
            .danger()
            .on_press(move || {
                confirm(
                    "Report this contact?",
                    "The last five messages are sent to Songbird so we can look into it. They won't be told.",
                    "Report",
                    move || {
                        s.report(chat);
                    },
                )
            })
            .build(),
    ]);

    // Save to contacts: an encrypted contact record, synced.
    let saved = move || {
        peer.with(|p| {
            p.as_ref().is_some_and(|p| {
                s.book()
                    .with(|_| s.saved_by_username(&p.username).is_some())
            })
        })
    };
    let save_group = dynamic(move || {
        let on = saved();
        untracked(move || {
            group([if on {
                Row::new("In your contacts")
                    .icon(Icon::Contact)
                    .chevron()
                    .on_press(crate::features::contacts::open)
                    .build()
            } else {
                Row::new("Save to contacts")
                    .icon(Icon::UserPlus)
                    .subtitle("Add notes and groups. Encrypted, and synced to your devices.")
                    .on_press(move || {
                        if let Some(p) = peer.get_untracked() {
                            s.save_from(&p.name, &p.username);
                            toast(format!("{} saved to your contacts", p.name));
                        }
                    })
                    .build()
            }])
        })
    });

    view()
        .flex_1()
        .child(page_scroll(
            view()
                .w_full()
                .max_w(px(640.0))
                .self_center()
                .gap_3()
                .child(identity)
                .child(about_group)
                .child(media_group)
                .child(save_group)
                .child(settings_group)
                .child(encryption)
                .child(common_group)
                .child(view().pt_3().child(danger)),
        ))
        .child(sheets::timer_sheet(s, chat, timer_open))
        .child(super::theme::theme_sheet(chat, theme_open))
        .child(super::scene::scene_sheet(chat, scene_open))
        .into_element()
}

/// Media, links and docs: a grid of media, then links and documents.
fn media_page(chat: ChatId) {
    nav::push(format!("chat-media:{chat}"), move || {
        let s = store();
        let tab = signal(0_u8);
        page("Media, links and docs", media_body(s, chat, tab))
    });
}

fn media_body(s: Store, chat: ChatId, tab: Signal<u8>) -> AnyElement {
    let media_items = s.media_of(chat);
    let docs = s.docs_of(chat);
    let links = s.links_of(chat);
    let side = if is_desktop() { 112.0 } else { 116.0 };
    let grid = if media_items.is_empty() {
        empty_state(
            Icon::Image,
            "No media",
            "Photos and videos you share in this chat show here.",
            None,
        )
        .into_element()
    } else {
        view()
            .flex_row()
            .flex_wrap()
            .gap(px(4.0))
            .px_4()
            .children(media_items.iter().map(|m| media_tile(m, side)))
            .into_element()
    };
    let doc_rows = if docs.is_empty() {
        empty_state(
            Icon::File,
            "No documents",
            "Documents you share in this chat show here.",
            None,
        )
        .into_element()
    } else {
        group(docs.into_iter().map(|m| {
            let (name, sub) = match &m.body {
                Body::File { name, size, kind } => (
                    name.clone(),
                    format!("{kind} · {size} · {}", when_label(m.at)),
                ),
                _ => Default::default(),
            };
            Row::new(name).icon(Icon::File).subtitle(sub).build()
        }))
        .into_element()
    };
    let link_rows = if links.is_empty() {
        empty_state(
            Icon::Link,
            "No links",
            "Links you share in this chat show here.",
            None,
        )
        .into_element()
    } else {
        group(links.into_iter().map(|m| {
            let (title, url) = match &m.body {
                Body::Link { title, url, .. } => (title.clone(), url.clone()),
                _ => Default::default(),
            };
            let open = url.clone();
            Row::new(title)
                .icon(Icon::Link)
                .subtitle(url)
                .on_press(move || {
                    let _ = open_url(open.clone());
                })
                .build()
        }))
        .into_element()
    };
    let grid = std::cell::RefCell::new(Some(grid));
    let doc_rows = std::cell::RefCell::new(Some(doc_rows));
    let link_rows = std::cell::RefCell::new(Some(link_rows));
    view()
        .flex_1()
        .child(view().p_4().child(segmented(
            vec![(0_u8, "Media"), (1, "Docs"), (2, "Links")],
            move || tab.get(),
            move |t| tab.set(t),
        )))
        .child(page_scroll(
            view()
                .child(
                    view()
                        .style_when(move || tab.get() != 0, |st| st.hidden())
                        .child(grid.borrow_mut().take()),
                )
                .child(
                    view()
                        .style_when(move || tab.get() != 1, |st| st.hidden())
                        .child(doc_rows.borrow_mut().take()),
                )
                .child(
                    view()
                        .style_when(move || tab.get() != 2, |st| st.hidden())
                        .child(link_rows.borrow_mut().take()),
                ),
        ))
        .into_element()
}

/// Starred messages of the chat; pressing one returns to the chat.
fn starred_page(chat: ChatId) {
    nav::push(format!("chat-starred:{chat}"), move || {
        let s = store();
        let items = s.starred_of(chat);
        let content = if items.is_empty() {
            empty_state(
                Icon::Star,
                "No starred messages",
                "Long-press or right-click a message and choose Star to find it here.",
                None,
            )
            .into_element()
        } else {
            page_scroll(view().pt_3().child(group(items.into_iter().map(|m| {
                Row::new(m.body.preview())
                    .icon(Icon::Star)
                    .subtitle(format!("{} · {}", s.contact_name(m.from), when_label(m.at)))
                    .build()
            }))))
            .into_element()
        };
        page("Starred messages", content)
    });
}
