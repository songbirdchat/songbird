//! The conversation's header: back (phones), avatar, title, the subtitle
//! (presence, typing, members, followers, a custom status), verified and
//! WhatsApp marks, calls, and the More menu; or the search bar.

use nsh_core::{element::input, prelude::*};

use super::{Conv, list, logic};
use crate::{
    features::calls,
    nav,
    store::{ChatKind, presence_label},
    ui::{floating, *},
};

/// Followers as people read them: 18,420.
pub(crate) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A group's subtitle: "5 people · 2 online" ("5 people" when nobody
/// else is online).
pub(crate) fn people_line(people: usize, online: usize) -> String {
    let people = if people == 1 {
        "1 person".to_string()
    } else {
        format!("{people} people")
    };
    if online == 0 {
        people
    } else {
        format!("{people} · {online} online")
    }
}

/// What shows under the title.
fn subtitle(cv: Conv) -> impl Fn() -> (String, bool) + Copy + 'static {
    let s = cv.s;
    let chat = cv.chat;
    move || {
        let Some(c) = s.chat(chat) else {
            return (String::new(), false);
        };
        if let Some(t) = c.typing.as_ref() {
            return match c.kind {
                ChatKind::Direct(_) | ChatKind::WhatsApp(_) => (t.clone(), true),
                _ => (format!("Someone is {t}"), true),
            };
        }
        let line = match c.kind {
            ChatKind::Direct(id) | ChatKind::WhatsApp(id) => match s.contact(id) {
                Some(p) if p.blocked => "Blocked".into(),
                Some(p) => {
                    let presence = presence_label(&s.presence_of(&p));
                    match (presence.is_empty(), p.status_line) {
                        (false, Some(st)) => format!("{presence} · {st}"),
                        (false, None) => presence,
                        (true, Some(st)) => st,
                        (true, None) => "Tap here for contact info".into(),
                    }
                }
                None => String::new(),
            },
            // How many, never who: names belong in the group's info.
            ChatKind::Group | ChatKind::Community => {
                let (people, online) = s.people_online(chat);
                people_line(people, online)
            }
            ChatKind::Channel { followers, .. } => {
                format!("Channel · {} followers", thousands(followers))
            }
        };
        (line, false)
    }
}

/// The header: capsules floating over the canvas (REDESIGN.md "Content
/// floats"), or the search capsule. The strip itself takes no touches:
/// the messages under it stay reachable between the capsules.
pub(crate) fn header(cv: Conv) -> AnyElement {
    // `CAPSULE_INSET` from the pane's top and sides: the same band as the
    // list pane's header row (`floating::header_band`), so both pane tops
    // centre on one line.
    view()
        .w_full()
        .px(px(floating::CAPSULE_INSET))
        .pb(px(floating::CAPSULE_INSET))
        .when(!cv.wide, |v| {
            v.pt_safe_area_plus(px(floating::CAPSULE_INSET))
        })
        .when(cv.wide, |v| v.pt(px(floating::CAPSULE_INSET)))
        .pointer_events(PointerEvents::BoxNone)
        .child(when(cv.searching, move || search_bar(cv)).otherwise(move || title_bar(cv)))
        .into_element()
}

/// A round capsule holding one icon button, exactly the capsule's height
/// (back, a call, the jump button's kin).
pub(crate) fn round(glyph: Icon, label: &'static str, f: impl Fn() + 'static) -> View {
    let h = floating::capsule_height();
    floating::capsule(icon_btn(glyph, label, f).size(px(h)).rounded_full())
}

fn title_bar(cv: Conv) -> View {
    let (s, chat) = (cv.s, cv.chat);
    let info = computed(move || s.chat(chat));
    let peer = computed(move || s.peer(chat));
    let whatsapp = info.with_untracked(|c| {
        c.as_ref()
            .is_some_and(|c| matches!(c.kind, ChatKind::WhatsApp(_)))
    });
    let channel = info.with_untracked(|c| {
        c.as_ref()
            .is_some_and(|c| matches!(c.kind, ChatKind::Channel { .. }))
    });
    let direct = peer.with_untracked(Option::is_some);
    let calls = (!channel).then(|| {
        if cv.wide {
            // Desktops: both calls in one capsule, each button a circle of
            // the capsule's height.
            let h = floating::capsule_height();
            floating::capsule(
                view()
                    .flex_row()
                    .child(
                        icon_btn(Icon::Video, "Video call", move || {
                            calls::start_call(chat, true)
                        })
                        .size(px(h))
                        .rounded_full(),
                    )
                    .child(
                        icon_btn(Icon::Phone, "Voice call", move || {
                            calls::start_call(chat, false)
                        })
                        .size(px(h))
                        .rounded_full(),
                    ),
            )
        } else {
            // Phones keep the row for the name: the voice call here, the
            // video call in More.
            round(Icon::Phone, "Voice call", move || {
                calls::start_call(chat, false)
            })
        }
    });
    let more = {
        let h = floating::capsule_height();
        floating::capsule(
            view()
                .size(px(h))
                .items_center()
                .justify_center()
                .child(more_menu(cv, direct && !whatsapp)),
        )
    };
    let right = view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(calls)
        .child(more);
    if cv.wide {
        // The name capsule sits at the pane's centre: equal columns either
        // side, the actions in the right one.
        return view()
            .w_full()
            .flex_row()
            .items_center()
            .gap_2()
            .pointer_events(PointerEvents::BoxNone)
            .child(view().flex_1())
            .child(
                name_capsule(cv, info, peer)
                    .flex_shrink(1.0)
                    .max_w(px(420.0)),
            )
            .child(view().flex_1().flex_row().justify_end().child(right));
    }
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap_2()
        .pointer_events(PointerEvents::BoxNone)
        .child(round(Icon::Back, "Back", nav::pop))
        .child(name_capsule(cv, info, peer).flex_1())
        .child(right)
}

/// The chat's capsule: its avatar concentric with the capsule's round
/// end, then the name and the subtitle, centred. It opens chat info.
fn name_capsule(
    cv: Conv,
    info: Memo<Option<crate::store::Chat>>,
    peer: Memo<Option<crate::store::Contact>>,
) -> View {
    let chat = cv.chat;
    let h = floating::capsule_height();
    // 4pt around the avatar: its circle and the capsule's end share a
    // centre, so the band between them is even all the way round.
    let side = h - 8.0;
    let title = move || info.with(|c| c.as_ref().map(|c| c.title.clone()).unwrap_or_default());
    let seed = info.with_untracked(|c| c.as_ref().map_or(0, |c| c.seed));
    let verified = move || peer.with(|p| p.as_ref().is_some_and(|p| p.verified));
    let muted = move || info.with(|c| c.as_ref().is_some_and(|c| c.muted));
    let whatsapp = info.with_untracked(|c| {
        c.as_ref()
            .is_some_and(|c| matches!(c.kind, ChatKind::WhatsApp(_)))
    });
    let direct = peer.with_untracked(Option::is_some);
    let online = move || {
        peer.with(|p| {
            p.as_ref()
                .is_some_and(|p| p.presence == crate::store::Presence::Online)
        })
    };
    let timer = move || info.with(|c| c.as_ref().and_then(|c| c.disappearing));
    let sub = subtitle(cv);
    let reduced = motion::reduced();
    floating::capsule(
        pressable()
            .on_press(move || super::chat_info(chat))
            .accessibility_label(super::say(move || {
                format!("{}, {}, chat info", title(), sub().0)
            }))
            .h(px(h))
            .min_w(px(0.0))
            .flex_row()
            .items_center()
            .gap_2()
            .pl(px(4.0))
            // The text ends where the capsule's curve begins, so the
            // label's right margin matches the avatar's band on the left.
            .pr(px((h / 2.0 - 4.0).round()))
            .rounded_full()
            .cursor_pointer()
            .transition()
            .duration(motion::INSTANT)
            .hover(|st| st.bg(move || c::foreground().opacity(0.05)))
            .active(move |st| {
                if reduced {
                    st.opacity(0.85)
                } else {
                    st.bg(move || c::foreground().opacity(0.08))
                }
            })
            .focus_visible(|st| {
                st.outline(px(2.0))
                    .outline_offset(px(2.0))
                    .outline_color(move || c::ring())
            })
            .child(if direct {
                avatar_presence(title(), seed, side, online)
            } else {
                group_avatar(title(), seed, side)
            })
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .max_w(relative(1.0))
                            .child(
                                text(title)
                                    .flex_shrink(1.0)
                                    .number_of_lines(1)
                                    .text_size(px(font::body()))
                                    .line_height(px(18.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || c::foreground())
                                    .role(AccessibilityRole::Header),
                            )
                            .child(when(verified, || {
                                icon(Icon::ShieldCheck, 13.0)
                                    .text_color(move || c::primary())
                                    .label("Verified")
                            }))
                            .child(when(muted, || {
                                icon(Icon::BellOff, 12.0)
                                    .text_color(move || c::muted_fg())
                                    .label("Muted")
                            }))
                            .child(whatsapp.then(|| tag("WhatsApp"))),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .max_w(relative(1.0))
                            .child(when(
                                move || timer().is_some(),
                                move || {
                                    icon(Icon::Timer, 11.0)
                                        .text_color(move || c::muted_fg())
                                        .label(move || {
                                            format!(
                                                "Disappearing messages: {}",
                                                timer().map(|t| t.label()).unwrap_or("")
                                            )
                                        })
                                },
                            ))
                            .child(
                                text(move || sub().0)
                                    .flex_shrink(1.0)
                                    .number_of_lines(1)
                                    .text_size(px(font::CAPTION))
                                    .line_height(px(15.0))
                                    .text_color(
                                        move || if sub().1 { c::primary() } else { c::muted_fg() },
                                    ),
                            ),
                    ),
            ),
    )
}

/// Search in chat, Mute, Disappearing messages, Chat info, Select,
/// Nudge; then Clear chat, Block and Report.
fn more_menu(cv: Conv, nudge: bool) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let peer = s.peer(chat);
    let group = peer.is_none();
    let muted = move || s.chat(chat).is_some_and(|c| c.muted);
    let channel = s
        .chat(chat)
        .is_some_and(|c| matches!(c.kind, ChatKind::Channel { .. }));
    let mut items = Vec::new();
    // Phones show one call in the header; the other is here.
    if !cv.wide && !channel {
        items.push(menu_entry(Icon::Video, "Video call", false, move || {
            calls::start_call(chat, true)
        }));
    }
    items.extend([
        menu_entry(Icon::Search, "Search", false, move || {
            batch(|| {
                cv.query.set(String::new());
                cv.hit.set(0);
                cv.searching.set(true);
            });
        }),
        menu_entry(
            Icon::Info,
            if group { "Group info" } else { "Contact info" },
            false,
            move || super::chat_info(chat),
        ),
        menu_entry(Icon::Check, "Select messages", false, move || {
            cv.selecting.set(true)
        }),
        menu_entry(
            Icon::BellOff,
            if muted() {
                "Unmute notifications"
            } else {
                "Mute for 8 hours"
            },
            false,
            move || {
                if muted() {
                    s.unmute(chat);
                    toast("Notifications on");
                } else {
                    s.mute(chat, Some(8 * 3_600_000));
                    toast("Muted for 8 hours");
                }
            },
        ),
    ]);
    if !muted() {
        items.extend([
            menu_entry(Icon::BellOff, "Mute for 1 week", false, move || {
                s.mute(chat, Some(7 * 24 * 3_600_000));
                toast("Muted for 1 week");
            }),
            menu_entry(Icon::BellOff, "Mute always", false, move || {
                s.mute(chat, None);
                toast("Muted until you unmute");
            }),
        ]);
    }
    items.extend([menu_entry(
        Icon::Timer,
        "Disappearing messages",
        false,
        move || cv.timer_open.set(true),
    )]);
    if nudge {
        items.push(menu_entry(Icon::Zap, "Send a nudge", false, move || {
            super::send_nudge(s, chat)
        }));
    }
    items.push(menu_rule());
    items.push(menu_entry(Icon::Trash, "Clear chat", true, move || {
        confirm(
            "Clear this chat?",
            "Messages will be removed from this device. Starred messages are removed too.",
            "Clear chat",
            move || {
                s.clear_chat(chat);
                toast("Chat cleared");
            },
        )
    }));
    if let Some(p) = peer.clone() {
        let id = p.id;
        let name = p.name.clone();
        items.push(menu_entry(Icon::Ban, format!("Block {}", p.name.split_whitespace().next().unwrap_or_default()), true, move || {
            let name = name.clone();
            confirm(
                format!("Block {name}?"),
                "Blocked contacts can't call you or send you messages. They won't be told you blocked them.",
                "Block",
                move || {
                    s.block(id, true);
                    toast(format!("{name} blocked"));
                },
            )
        }));
    }
    items.push(menu_entry(Icon::Flag, "Report", true, move || {
        confirm(
            "Report this chat?",
            "The last five messages are sent to Songbird so we can look into it. Nobody in the chat is told.",
            "Report",
            move || {
                s.report(chat);
            },
        )
    }));
    app_menu(Icon::More, "More options", items)
}

/// Search in chat: the query, which match of how many, older and newer.
fn search_bar(cv: Conv) -> View {
    let (s, chat) = (cv.s, cv.chat);
    let query = cv.query;
    // Matches, newest first.
    let hits = computed(move || {
        let q = query.get();
        if q.trim().chars().count() < 2 {
            return Vec::new();
        }
        s.messages.with(|m| {
            m.get(&chat)
                .map(|v| {
                    v.iter()
                        .rev()
                        .filter(|x| logic::matches_search(&logic::copy_text(x), &q))
                        .map(|x| x.id)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    let show = move |n: usize| {
        if let Some(id) = hits.with_untracked(|h| h.get(n).copied()) {
            cv.hit.set(n);
            if let Some(i) = cv.row_of(id) {
                cv.list
                    .scroll_to_index(i as u32, percentage(0.5), px(0.0), true);
            }
            list::flash(cv, id);
        }
    };
    let close = move || {
        batch(|| {
            cv.searching.set(false);
            query.set(String::new());
        });
    };
    let h = floating::capsule_height();
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap_2()
        .pointer_events(PointerEvents::BoxNone)
        .child(round(Icon::Close, "Close search", close))
        .child(
            floating::capsule(
                view()
                    .h(px(h))
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .pl(px(14.0))
                    .child(icon(Icon::Search, 16.0).text_color(move || c::muted_fg()))
                    .child(
                        input()
                            .flex_1()
                            .value(move || query.get())
                            .on_change(move |t: &str| query.set(t.to_owned()))
                            .auto_focus(true)
                            .placeholder("Search this chat".to_string())
                            .label("Search this chat")
                            .placeholder_color(|| color(c::muted_fg()))
                            .caret_color(|| color(c::primary()))
                            .selection_color(|| color(c::accent()))
                            .px_1()
                            .text_size(px(font::body()))
                            .text_color(move || c::foreground()),
                    )
                    .child(
                        text(move || {
                            let n = hits.with(Vec::len);
                            if query.with(|q| q.trim().chars().count() < 2) {
                                String::new()
                            } else if n == 0 {
                                "No results".into()
                            } else {
                                format!("{} of {}", cv.hit.get().min(n - 1) + 1, n)
                            }
                        })
                        .px_1()
                        .text_size(px(font::SMALL))
                        .tabular_nums()
                        .text_color(move || c::muted_fg()),
                    )
                    .child(
                        view().style_with(|st| st.rotate(deg(180.0))).child(
                            icon_btn(Icon::ChevronDown, "Older match", move || {
                                let n = hits.with_untracked(Vec::len);
                                if n > 0 {
                                    show((cv.hit.get_untracked() + 1).min(n - 1));
                                }
                            })
                            .size(px(h))
                            .rounded_full(),
                        ),
                    )
                    .child(
                        icon_btn(Icon::ChevronDown, "Newer match", move || {
                            show(cv.hit.get_untracked().saturating_sub(1));
                        })
                        .size(px(h))
                        .rounded_full(),
                    ),
            )
            .flex_1(),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_count_people_not_names() {
        assert_eq!(super::people_line(5, 2), "5 people · 2 online");
        assert_eq!(super::people_line(3, 0), "3 people");
        assert_eq!(super::people_line(1, 0), "1 person");
    }

    #[test]
    fn followers_have_separators() {
        assert_eq!(super::thousands(18_420), "18,420");
        assert_eq!(super::thousands(999), "999");
        assert_eq!(super::thousands(1_000_000), "1,000,000");
    }
}
