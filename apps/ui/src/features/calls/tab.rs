//! The Calls tab: call links, recent calls (all or missed), a call's
//! info page, the new call page and the call link sheet.

use nsh_core::prelude::*;

use super::{join_link, simulate_in, start_call_in, state};
use crate::{
    nav,
    store::{
        CallDirection, CallLink, CallRecord, ChatKind, Contact, Store, clock, day_label, day_of,
        store, when_label,
    },
    ui::*,
};

/// All calls, or only missed ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Missed,
}

/// The glyph, the word and the danger of a call's direction. Missed calls
/// differ in glyph, word and colour, never colour alone.
fn direction(r: &CallRecord) -> (Icon, &'static str, bool) {
    match r.direction {
        CallDirection::Missed => (Icon::PhoneMissed, "Missed", true),
        CallDirection::Incoming if r.secs == 0 => (Icon::PhoneIncoming, "Declined", false),
        CallDirection::Incoming => (Icon::PhoneIncoming, "Incoming", false),
        CallDirection::Outgoing if r.secs == 0 => (Icon::PhoneOutgoing, "No answer", false),
        CallDirection::Outgoing => (Icon::PhoneOutgoing, "Outgoing", false),
    }
}

/// Who a call was with, as a picture: a 1:1 call shows the person's
/// portrait (seeded by their contact id, as in every list and header), a
/// group call the group's own picture, never a person's face.
fn who(s: &Store, r: &CallRecord, size: f32) -> View {
    if r.group {
        let seed = s.chat(r.chat).map_or(r.seed, |c| c.seed);
        group_avatar(r.title.clone(), seed, size)
    } else {
        let seed = s.peer(r.chat).map_or(r.seed, |p| p.id);
        avatar(r.title.clone(), seed, size)
    }
}

/// "Outgoing video call", "Missed voice call".
fn kind_line(r: &CallRecord) -> String {
    let (_, word, _) = direction(r);
    format!("{word} {} call", if r.video { "video" } else { "voice" })
}

/// The Calls tab.
pub fn calls_tab() -> AnyElement {
    let s = store();
    let filter = signal(Filter::All);
    let link_open = signal(false);
    let list = computed(move || {
        let f = filter.get();
        s.calls.with(|cs| {
            cs.iter()
                .filter(|r| f == Filter::All || r.direction == CallDirection::Missed)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let header = view()
        .pb_1()
        .child(view().px_4().pt_3().child(segmented(
            vec![(Filter::All, "All"), (Filter::Missed, "Missed")],
            move || filter.get(),
            move |f| filter.set(f),
        )))
        // Call links run on the demo's sample links only until links are real on the device
        // (D-0152): a link must never be offered that nobody can join.
        .child(crate::store::demo().then(|| {
            view().pt_3().child(group([Row::new("Create call link")
                .leading(
                    view()
                        .size(px(40.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || c::accent())
                        .text_color(move || c::primary())
                        .child(icon(Icon::Link, 20.0)),
                )
                .subtitle("Anyone with the link can join")
                .on_press(move || link_open.set(true))
                .build()]))
        }))
        .child(when(
            move || crate::store::demo() && s.call_links.with(|l| !l.is_empty()),
            move || {
                view()
                    .child(section_title("Call links"))
                    .child(group_links(s))
            },
        ))
        .child(section_title(move || {
            if filter.get() == Filter::Missed {
                "Missed"
            } else {
                "Recent"
            }
        }));
    let footer = view()
        .pb_safe_area_plus(px(24.0))
        .child(when(
            move || list.with(Vec::is_empty),
            move || {
                let missed = filter.get_untracked() == Filter::Missed;
                empty_state(
                    if missed {
                        Icon::PhoneMissed
                    } else {
                        Icon::Calls
                    },
                    if missed {
                        "No missed calls"
                    } else {
                        "No calls yet"
                    },
                    if missed {
                        "Calls you didn't answer show here."
                    } else {
                        "Start a call from a chat, or with New call."
                    },
                    None,
                )
                .flex_none()
                .py_8()
            },
        ))
        .child(footnote(
            "Your call history is end-to-end encrypted and synced across your devices.",
        ));
    // Developer and preview builds only (never a release): rows that make
    // the demo ring, to test calls and their notifications on a device.
    let footer = if crate::store::demo() {
        footer.child(demo(s))
    } else {
        footer
    };
    view()
        .flex_1()
        .child(app_header(
            "Calls",
            false,
            Some(icon_btn(Icon::Plus, "New call", move || new_call(s)).into_element()),
        ))
        .child(
            virtual_list(
                move || list.with(Vec::len),
                move |at| {
                    let rec = computed(move || list.with(|l| l.get(at.index()).cloned()));
                    dynamic(move || {
                        let r = rec.get();
                        untracked(move || r.map(|r| call_row(s, r)))
                    })
                },
            )
            .keys(move || list.with(|l| l.iter().map(|r| r.id).collect::<Vec<_>>()))
            .estimated_item_size(px(if is_desktop() { 60.0 } else { 72.0 }))
            .header(header)
            .footer(footer)
            .label("Call history")
            .flex_1(),
        )
        .child(link_sheet(s, link_open))
        .into_element()
}

/// The user's call links: tap to join (as its admin), copy its link.
fn group_links(s: Store) -> View {
    view()
        .mx_4()
        .rounded(move || radius::lg())
        .overflow_hidden()
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::border())
        .child(each(
            move || s.call_links.get(),
            |l: &CallLink| l.id,
            |l, at| {
                let l = l.get_untracked();
                let url = l.url.clone();
                let name = l.name.clone();
                let lobby = l.lobby;
                let seed = l.id;
                // A rule between links, on the text column (16 + 40 + 12).
                view()
                    .child(when(move || at.get() > 0, || separator_inset(68.0)))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .pr_2()
                            .child(
                                view().flex_1().child(
                                    Row::new(l.name.clone())
                                        .leading(
                                            view()
                                                .size(px(40.0))
                                                .rounded_full()
                                                .items_center()
                                                .justify_center()
                                                .bg(move || c::secondary())
                                                .text_color(move || c::on_secondary())
                                                .child(icon(Icon::Link, 18.0)),
                                        )
                                        // One line: who gets in. The link itself is one press away
                                        // (the copy button), so it never wraps here.
                                        .subtitle(if lobby {
                                            "Admins admit people"
                                        } else {
                                            "Anyone with the link joins"
                                        })
                                        .on_press(move || {
                                            join_link(name.clone(), seed, lobby, false)
                                        })
                                        .build(),
                                ),
                            )
                            .child(icon_btn(
                                Icon::Copy,
                                format!("Copy link to {}", l.name),
                                move || {
                                    set_clipboard(url.clone());
                                    toast("Link copied");
                                },
                            )),
                    )
            },
        ))
}

/// One call in the list: who, which way, when, and a quick call back.
fn call_row(s: Store, r: CallRecord) -> View {
    let (glyph, word, missed) = direction(&r);
    let chat = r.chat;
    let video = r.video;
    let title = r.title.clone();
    let when = when_label(r.at);
    let label = format!(
        "{}, {}, {}{}",
        r.title,
        kind_line(&r),
        when,
        if r.group { ", group call" } else { "" }
    );
    let rec = r.clone();
    view()
        .flex_row()
        .items_center()
        .pr_2()
        .child(
            nsh_base::button()
                .flex_1()
                .on_press(move || call_info(s, rec.clone()))
                .accessibility_label(label)
                .cursor_pointer()
                .transition_colors()
                .duration(motion::INSTANT)
                .hover(|st| st.bg(move || c::accent()))
                .active(|st| st.bg(move || c::accent()))
                .focus_visible(|st| st.bg(move || c::accent()))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .min_h(px(if is_desktop() { 60.0 } else { 72.0 }))
                        .py_2()
                        .child(who(&s, &r, if is_desktop() { 40.0 } else { 48.0 }))
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
                                            text(r.title.clone())
                                                .flex_shrink(1.0)
                                                .number_of_lines(1)
                                                .text_size(px(font::body()))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(move || c::foreground()),
                                        )
                                        .child(r.group.then(|| tag("Group"))),
                                )
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        // Only the glyph and the word turn red for a
                                        // missed call; the name stays in body ink.
                                        .child(
                                            view()
                                                .flex_row()
                                                .items_center()
                                                .gap_1()
                                                .text_color(move || {
                                                    if missed {
                                                        c::destructive()
                                                    } else {
                                                        c::muted_fg()
                                                    }
                                                })
                                                // One line on one baseline: the direction glyph
                                                // (centred on the cap height), the word, then when
                                                // and how long in tabular figures. Voice or video
                                                // is the call-back button's glyph at the row's end.
                                                .child(icon(glyph, 14.0))
                                                .child(
                                                    text(word)
                                                        .text_size(px(font::SMALL + 1.0))
                                                        .line_height(px(18.0))
                                                        .font_weight(if missed {
                                                            FontWeight::MEDIUM
                                                        } else {
                                                            FontWeight::NORMAL
                                                        }),
                                                ),
                                        )
                                        .child(
                                            text(if r.secs > 0 {
                                                format!("· {} · {}", when, state::words(r.secs))
                                            } else {
                                                format!("· {when}")
                                            })
                                            .flex_shrink(1.0)
                                            .number_of_lines(1)
                                            .text_size(px(font::SMALL + 1.0))
                                            .line_height(px(18.0))
                                            .tabular_nums()
                                            .text_color(move || c::muted_fg()),
                                        ),
                                ),
                        ),
                ),
        )
        .child(icon_btn(
            if r.video { Icon::Video } else { Icon::Phone },
            format!(
                "{} {title}",
                if video { "Video call" } else { "Voice call" }
            ),
            move || start_call_in(&s, chat, video),
        ))
}

/// A call's info: who, the calls with them that day, call back.
fn call_info(s: Store, r: CallRecord) {
    let key = format!("call-info:{}", r.id);
    nav::push(key, move || {
        let r = r.clone();
        let day = day_of(r.at);
        let entries: Vec<CallRecord> = s.calls.with_untracked(|cs| {
            cs.iter()
                .filter(|x| x.chat == r.chat && day_of(x.at) == day)
                .cloned()
                .collect()
        });
        let chat = r.chat;
        let exists = s.chat(chat).is_some();
        let direct = matches!(
            s.chat(chat).map(|c| c.kind),
            Some(ChatKind::Direct(_) | ChatKind::WhatsApp(_))
        );
        let action = |glyph: Icon, label: &'static str, f: Box<dyn Fn()>| {
            view().flex_1().child(
                btn_with_icon(
                    label,
                    Some(glyph),
                    Variant::Secondary,
                    BtnSize::Md,
                    move || f(),
                )
                .disabled(!exists),
            )
        };
        page(
            "Call info",
            page_scroll(
                view()
                    .gap_4()
                    .pt_6()
                    .child(
                        view()
                            .items_center()
                            .gap_2()
                            .px_4()
                            .child(who(&s, &r, 80.0))
                            .child(text(r.title.clone()).text_size(px(font::HEADING)).font_weight(FontWeight::SEMIBOLD).text_center().role(AccessibilityRole::Header))
                            .child(r.group.then(|| tag("Group"))),
                    )
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .px_4()
                            .child(action(Icon::Phone, "Voice call", Box::new(move || start_call_in(&s, chat, false))))
                            .child(action(Icon::Video, "Video call", Box::new(move || start_call_in(&s, chat, true))))
                            .child(action(Icon::Chats, "Message", Box::new(move || crate::features::chat::open(chat)))),
                    )
                    .child(
                        view()
                            .child(section_title(day_label(r.at)))
                            .child(group(entries.iter().map(|e| {
                                let (glyph, _, missed) = direction(e);
                                Row::new(kind_line(e))
                                    .leading(
                                        view()
                                            .size(px(30.0))
                                            .rounded(move || radius::sm())
                                            .items_center()
                                            .justify_center()
                                            .bg(move || if missed { c::warning_bg() } else { c::accent() })
                                            .text_color(move || if missed { c::destructive() } else { c::primary() })
                                            .child(icon(glyph, 16.0)),
                                    )
                                    .subtitle(clock(e.at))
                                    .value(if e.secs > 0 { state::words(e.secs) } else { String::new() })
                                    .build()
                            }))),
                    )
                    .child(footnote(if direct {
                        "Calls are end-to-end encrypted. Only you and the people in the call can hear or see them."
                    } else {
                        "Group calls are end-to-end encrypted. Only the people in the call can hear or see them."
                    })),
            ),
        )
    });
}

/// The new call page: a call link, or anyone in your contacts.
fn new_call(s: Store) {
    nav::push("new-call", move || {
        let query = signal(String::new());
        let people = computed(move || {
            let q = query.with(|q| q.to_lowercase());
            s.contacts.with(|cs| {
                cs.iter()
                    .filter(|c| {
                        !c.blocked
                            && (q.is_empty()
                                || c.name.to_lowercase().contains(&q)
                                || c.username.contains(&q))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
        });
        let link_open = signal(false);
        let call = move |contact: u64, video: bool| {
            let chat = s.direct_chat(contact);
            nav::pop();
            start_call_in(&s, chat, video);
        };
        page(
            "New call",
            view()
                .flex_1()
                .child(
                    virtual_list(
                        move || people.with(Vec::len),
                        move |at| {
                            let p = computed(move || people.with(|l| l.get(at.index()).cloned()));
                            dynamic(move || {
                                let p = p.get();
                                untracked(move || {
                                    p.map(|p: Contact| {
                                        let id = p.id;
                                        Row::new(p.name.clone())
                                            .leading(avatar(p.name.clone(), p.id, 40.0))
                                            .subtitle(format!("@{}", p.username))
                                            .trailing(
                                                view()
                                                    .flex_row()
                                                    .gap_1()
                                                    .child(icon_btn(
                                                        Icon::Phone,
                                                        format!("Voice call {}", p.name),
                                                        move || call(id, false),
                                                    ))
                                                    .child(icon_btn(
                                                        Icon::Video,
                                                        format!("Video call {}", p.name),
                                                        move || call(id, true),
                                                    )),
                                            )
                                            .build()
                                    })
                                })
                            })
                        },
                    )
                    .keys(move || people.with(|l| l.iter().map(|p| p.id).collect::<Vec<_>>()))
                    .estimated_item_size(px(60.0))
                    .header(
                        view()
                            .pb_2()
                            .child(
                                view()
                                    .px_4()
                                    .pt_3()
                                    .child(search_field("Search name or username", query)),
                            )
                            .child(
                                view().pt_3().child(group([Row::new("New call link")
                                    .icon(Icon::Link)
                                    .on_press(move || link_open.set(true))
                                    .build()])),
                            )
                            .child(section_title("Contacts")),
                    )
                    .footer(view().child(when(
                        move || people.with(Vec::is_empty),
                        || {
                            empty_state(
                                Icon::Search,
                                "No one found",
                                "Check the spelling, or search by username.",
                                None,
                            )
                            .flex_none()
                        },
                    )))
                    .flex_1(),
                )
                .child(link_sheet(s, link_open)),
        )
    });
}

/// Create a call link: a name, the lobby, then copy or join.
fn link_sheet(s: Store, open: Signal<bool>) -> nsh_base::SheetElement {
    let name = signal(String::new());
    let lobby = signal(true);
    let created = signal(None::<CallLink>);
    // A fresh form each time it opens.
    effect(move || {
        if open.get() {
            untracked(|| {
                name.set(String::new());
                lobby.set(true);
                created.set(None);
            });
        }
    });
    let create = move || {
        let n = name.with_untracked(|n| n.trim().to_string());
        let n = if n.is_empty() {
            "Songbird call".to_string()
        } else {
            n
        };
        // demo: app-core will create the link's room and its key; the key
        // travels in the link's fragment, never to the server.
        // One link per name and lobby setting: creating it again (a
        // double press, the same form twice) shows the link you have.
        let lob = lobby.get_untracked();
        let existing = s
            .call_links
            .with_untracked(|ls| ls.iter().find(|l| l.name == n && l.lobby == lob).cloned());
        let link = existing.unwrap_or_else(|| s.create_call_link(n, lob));
        announce_for_accessibility("Call link created");
        created.set(Some(link));
    };
    app_sheet(
        open,
        "Create call link",
        view().px_4().pb_4().gap_4().child(dynamic(move || {
            let done = created.get();
            untracked(move || match done {
                None => view()
                    .gap_4()
                    .child(labelled_field("Name", field("Call name", name), "People see this name when they join.", || false))
                    .child(
                        view()
                            .rounded(move || radius::lg())
                            .bg(move || c::card())
                            .border_1()
                            .border_color(move || c::border())
                            .child(toggle_row(
                                "Admins admit people",
                                Some("New people wait in a lobby until an admin lets them in."),
                                move || lobby.get(),
                                move |v| lobby.set(v),
                            )),
                    )
                    .child(btn("Create link", Variant::Primary, BtnSize::Lg, create))
                    .into_element(),
                Some(link) => {
                    let url = link.url.clone();
                    let (lname, seed, lob) = (link.name.clone(), link.id, link.lobby);
                    view()
                        .gap_4()
                        .child(banner(Tone::Secure, Icon::Lock, "Anyone with this link can ask to join. Calls are end-to-end encrypted."))
                        .child(
                            view()
                                .gap_1()
                                .p_3()
                                .rounded(move || radius::control())
                                .bg(move || c::secondary())
                                .child(text(link.name.clone()).text_size(px(font::body())).font_weight(FontWeight::SEMIBOLD))
                                .child(text(link.url.clone()).text_size(px(font::SMALL + 1.0)).number_of_lines(1).text_color(move || c::link()).selectable(true)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .gap_2()
                                .child(view().flex_1().child(btn_with_icon("Copy link", Some(Icon::Copy), Variant::Secondary, BtnSize::Lg, move || {
                                    set_clipboard(url.clone());
                                    toast("Link copied");
                                })))
                                .child(view().flex_1().child(btn_with_icon("Join call", Some(Icon::Video), Variant::Primary, BtnSize::Lg, move || {
                                    open.set(false);
                                    join_link(lname.clone(), seed, lob, false);
                                }))),
                        )
                        .into_element()
                }
            })
        })),
    )
}

/// Developer and preview builds only: make the demo ring here (no calls
/// arrive yet). Set apart and marked, never in a release build.
fn demo(s: Store) -> View {
    let ana = 1;
    let family = 2;
    view()
        .pt_6()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px_4()
                .pb_2()
                .child(icon(Icon::Zap, 14.0).text_color(move || c::muted_fg()))
                .child(text("Developer preview").text_size(px(font::SMALL)).font_weight(FontWeight::SEMIBOLD).text_color(move || c::muted_fg()))
                .child(tag("Debug build")),
        )
        .child(group([
            Row::new("Test incoming call")
                .icon(Icon::PhoneIncoming)
                .subtitle("Ana Popescu calls you, voice")
                .on_press(move || simulate_in(&s, ana, false, false))
                .build(),
            Row::new("Test incoming video call")
                .icon(Icon::Video)
                .subtitle("The Familia group calls you, video")
                .on_press(move || simulate_in(&s, family, true, false))
                .build(),
            Row::new("Test a call answered on your phone")
                .icon(Icon::Smartphone)
                .subtitle("Rings here, then another of your devices answers")
                .on_press(move || simulate_in(&s, ana, false, true))
                .build(),
            Row::new("Test joining a link with a lobby")
                .icon(Icon::Link)
                .subtitle("You wait until an admin lets you in")
                .on_press(|| join_link("Weekly design crit".into(), 1, true, true))
                .build(),
        ]))
        .child(footnote("Shown in developer builds only. These rows ring this device with made-up calls; nothing is sent."))
}
