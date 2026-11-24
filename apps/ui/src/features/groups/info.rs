//! Group and channel info: the header, the four actions, media, starred,
//! notifications, disappearing messages, lock, encryption, admin tools,
//! the members with their roles, and leaving.

use nsh_core::prelude::*;

use super::{
    ask_line, chat_avatar, community, create, danger_icon, first_name, glide, icon_toggle_row,
    invite_link_page, join_requests_page, member_changes_page, page_scroll, role_tag, roles_page,
};
use crate::{
    features::{calls, chat as chat_feature, devices, media},
    nav,
    store::{
        Body, Chat, ChatId, ChatKind, Contact, ContactId, Disappear, Member, MsgId, Role, Store,
        ext_groups::{can_manage, followers_label, my_role, sorted_members},
        store, when_label,
    },
    ui::*,
};

/// Whether `me` may change the role of, or remove, someone with `target`.
/// Nobody acts on the owner; members act on nobody.
pub(crate) fn can_act_on(me: Option<Role>, target: Role) -> bool {
    matches!(me, Some(Role::Owner | Role::Admin)) && target != Role::Owner
}

/// Whether the user may add people: admins, or anyone when members may.
pub(crate) fn can_add(chat: &Chat) -> bool {
    can_manage(chat) || (my_role(chat).is_some() && chat.permissions.members_add)
}

/// Whether the user may edit the name, description and timer.
pub(crate) fn can_edit_info(chat: &Chat) -> bool {
    can_manage(chat) || (my_role(chat).is_some() && chat.permissions.members_edit_info)
}

/// The page for any group-like chat.
pub(super) fn info_page(chat: ChatId) -> Screen {
    let s = store();
    match s.chat(chat).map(|c| c.kind) {
        Some(ChatKind::Community) => community::home_page(chat),
        Some(ChatKind::Channel { .. }) => channel_page(s, chat),
        Some(_) => group_page(s, chat),
        None => page(
            "Group info",
            empty_state(
                Icon::Users,
                "Not found",
                "This chat is no longer here.",
                None,
            ),
        ),
    }
}

// ------------------------------------------------------------------ group

fn group_page(s: Store, chat: ChatId) -> Screen {
    let ch = computed(move || s.chat(chat));
    let get = move || ch.get().expect("chat exists while its info is open");
    let edit_open = signal(false);
    let add_open = signal(false);
    let timer_open = signal(false);
    let theme_open = signal(false);
    let crypto_open = signal(false);
    let member_open = signal(false);
    let member = signal(None::<ContactId>);
    let member_query = signal(String::new());

    let edit = move || {
        let c = get();
        if can_edit_info(&c) {
            edit_open.set(true);
        } else {
            toast(ask_line(s, &c, "edit the group info"));
        }
    };
    let add = move || {
        let c = get();
        if can_add(&c) {
            add_open.set(true);
        } else {
            toast(ask_line(s, &c, "add members"));
        }
    };
    let call = move |video: bool| {
        let c = get();
        if c.permissions.members_call || can_manage(&c) {
            calls::start_call(chat, video);
        } else {
            toast(ask_line(s, &c, "start calls"));
        }
    };

    let header_right = icon_btn(Icon::Edit, "Edit group info", edit).into_element();
    page_with(
        "Group info",
        Some(header_right),
        view()
            .flex_1()
            .child(page_scroll(
                view()
                    .gap_4()
                    .child(header(s, chat, move || {
                        let c = get();
                        let n = c.members.len();
                        if n == 1 {
                            "Group · 1 member".to_string()
                        } else {
                            format!("Group · {n} members")
                        }
                    }))
                    .child(
                        view()
                            .flex_row()
                            .gap_2()
                            .px_4()
                            .child(action_tile(Icon::Phone, "Audio", move || call(false)))
                            .child(action_tile(Icon::Video, "Video", move || call(true)))
                            .child(action_tile(Icon::UserPlus, "Add", add))
                            .child(action_tile(Icon::Search, "Search", move || {
                                search_page(chat)
                            })),
                    )
                    .child(dynamic(move || {
                        let media_n = s.messages.with(|m| {
                            m.get(&chat)
                                .map_or(0, |v| v.iter().filter(|x| is_shared(&x.body)).count())
                        });
                        let starred_n = s.messages.with(|m| {
                            m.get(&chat)
                                .map_or(0, |v| v.iter().filter(|x| x.starred).count())
                        });
                        untracked(move || {
                            group([
                                Row::new("Media, links and docs")
                                    .icon(Icon::Image)
                                    .value(media_n.to_string())
                                    .on_press(move || media_page(chat))
                                    .build(),
                                Row::new("Starred messages")
                                    .icon(Icon::Star)
                                    .value(if starred_n == 0 {
                                        "None".to_string()
                                    } else {
                                        starred_n.to_string()
                                    })
                                    .on_press(move || starred_page(chat))
                                    .build(),
                            ])
                            .into_element()
                        })
                    }))
                    .child(group([
                        icon_toggle_row(
                            Icon::BellOff,
                            "Mute notifications",
                            None,
                            move || ch.with(|c| c.as_ref().is_some_and(|c| c.muted)),
                            move |_| s.toggle_mute(chat),
                        ),
                        // The chat's own canvas and bubble colour, as in a
                        // one-to-one chat's info (REDESIGN.md "Per-chat themes").
                        crate::features::chat::theme_row(chat, theme_open),
                        view()
                            .child(dynamic(move || {
                                let t = ch.with(|c| c.as_ref().and_then(|c| c.disappearing));
                                untracked(move || {
                                    Row::new("Disappearing messages")
                                        .icon(Icon::Timer)
                                        .value(t.map_or("Off", Disappear::label))
                                        .on_press(move || timer_open.set(true))
                                        .build()
                                })
                            }))
                            .into_element(),
                        icon_toggle_row(
                            Icon::Lock,
                            "Lock chat",
                            Some("Only you can open it, with your device's lock."),
                            move || ch.with(|c| c.as_ref().is_some_and(|c| c.locked)),
                            move |_| s.toggle_lock(chat),
                        ),
                        Row::new("Encryption")
                            .icon(Icon::ShieldCheck)
                            .subtitle(
                                "Messages and calls are end-to-end encrypted. Tap to learn more.",
                            )
                            .on_press(move || crypto_open.set(true))
                            .build(),
                    ]))
                    .child(admin_rows(s, chat, ch))
                    .child(members_section(s, ch, member_query, add, move |id| {
                        member.set(Some(id));
                        member_open.set(true);
                    }))
                    .child(group([
                        Row::new("Exit group")
                            .leading(danger_icon(Icon::LogOut))
                            .danger()
                            .on_press(move || exit(s, chat))
                            .build(),
                        Row::new("Report group")
                            .leading(danger_icon(Icon::Flag))
                            .danger()
                            .on_press(move || report(s, chat))
                            .build(),
                    ])),
            ))
            .child(app_sheet(
                edit_open,
                "Edit group info",
                edit_sheet(s, chat, edit_open),
            ))
            .child(app_sheet(
                add_open,
                "Add members",
                add_sheet(s, chat, add_open),
            ))
            .child(app_sheet(
                timer_open,
                "Disappearing messages",
                timer_sheet(s, chat, ch, timer_open),
            ))
            .child(crate::features::chat::theme_sheet(chat, theme_open))
            .child(app_sheet(crypto_open, "Encryption", crypto_sheet(s, chat)))
            .child(app_sheet(
                member_open,
                "Group member",
                member_sheet(s, chat, ch, member, member_open),
            )),
    )
}

/// Avatar, name, the kind line and the description.
fn header(s: Store, chat: ChatId, kind_line: impl Fn() -> String + 'static) -> View {
    let c0 = s.chat(chat);
    view()
        .items_center()
        .gap_1()
        .px_4()
        .pt_6()
        .child(c0.as_ref().map(|c| chat_avatar(c, 96.0)))
        .child(
            text(move || s.chat(chat).map(|c| c.title).unwrap_or_default())
                .mt_3()
                .text_size(px(font::HEADING))
                .font_weight(FontWeight::SEMIBOLD)
                .text_center()
                .role(AccessibilityRole::Header),
        )
        .child(muted(kind_line).text_center())
        .child(
            text(move || s.chat(chat).map(|c| c.about).unwrap_or_default())
                .mt_2()
                .max_w(px(480.0))
                .text_size(px(font::body()))
                .line_height(px(font::body() * 1.4))
                .text_center()
                .style_when(
                    move || s.chat(chat).is_none_or(|c| c.about.is_empty()),
                    |st| st.hidden(),
                ),
        )
}

/// One of the four actions under the header.
pub(crate) fn action_tile(
    glyph: Icon,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    press_surface(Press::Tile, on_press)
        .accessibility_label(label)
        .flex_1()
        .items_center()
        .gap(px(6.0))
        .py_3()
        .rounded(move || radius::lg())
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::border())
        .child(icon(glyph, 22.0).text_color(move || c::primary()))
        .child(
            text(label)
                .text_size(px(font::SMALL))
                .font_weight(FontWeight::MEDIUM),
        )
}

/// Permissions, invite link, join requests, member changes.
fn admin_rows(s: Store, chat: ChatId, ch: Memo<Option<Chat>>) -> View {
    view().child(dynamic(move || {
        let manage = ch.with(|c| c.as_ref().is_some_and(can_manage));
        let requests = ch.with(|c| c.as_ref().map_or(0, |c| c.join_requests.len()));
        let link_ok = ch.with(|c| c.as_ref().is_some_and(can_add));
        untracked(move || {
            let mut rows = vec![
                Row::new("Group permissions")
                    .icon(Icon::Shield)
                    .subtitle(if manage {
                        "Choose what members can do"
                    } else {
                        "See what members can do"
                    })
                    .chevron()
                    .on_press(move || roles_page(chat))
                    .build(),
                Row::new("Invite via link")
                    .icon(Icon::Link)
                    .chevron()
                    .on_press(move || {
                        if link_ok {
                            invite_link_page(chat);
                        } else if let Some(c) = s.chat(chat) {
                            toast(ask_line(s, &c, "share the invite link"));
                        }
                    })
                    .build(),
            ];
            if manage {
                let mut r = Row::new("Join requests").icon(Icon::UserPlus);
                r = if requests > 0 {
                    r.trailing(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(unread_badge(move || requests as u32, || false))
                            .child(
                                icon(Icon::ChevronRight, 16.0).text_color(move || c::muted_fg()),
                            ),
                    )
                } else {
                    r.value("None")
                };
                rows.push(r.on_press(move || join_requests_page(chat)).build());
            }
            rows.push(
                Row::new("View member changes")
                    .icon(Icon::Clock)
                    .chevron()
                    .on_press(move || member_changes_page(chat))
                    .build(),
            );
            group(rows).into_element()
        })
    }))
}

/// The members: a count, a search, Add, and one row per person.
fn members_section(
    s: Store,
    ch: Memo<Option<Chat>>,
    query: Signal<String>,
    add: impl Fn() + Copy + 'static,
    open_member: impl Fn(ContactId) + Copy + 'static,
) -> View {
    let list = computed(move || {
        let q = query.get().trim().to_lowercase();
        ch.with(|c| {
            c.as_ref()
                .map(|c| sorted_members(c, |id| s.contact_name(id)))
                .unwrap_or_default()
                .into_iter()
                .filter(|m| q.is_empty() || s.contact_name(m.contact).to_lowercase().contains(&q))
                .collect::<Vec<Member>>()
        })
    });
    let count = move || ch.with(|c| c.as_ref().map_or(0, |c| c.members.len()));
    view()
        // The page stacks sections 16pt apart; with the title's own
        // 4pt that is the 20pt every other section title sits below a card.
        .child(
            section_title(move || match count() {
                1 => "1 member".to_string(),
                n => format!("{n} members"),
            })
            .pt_1(),
        )
        .child(
            view()
                .px_4()
                .pb_3()
                .child(search_field("Search members", query)),
        )
        .child(
            view()
                .mx_4()
                .rounded(move || radius::lg())
                .overflow_hidden()
                .bg(move || c::card())
                .border_1()
                .border_color(move || c::border())
                .child(
                    view()
                        .style_when(
                            move || !ch.with(|c| c.as_ref().is_some_and(can_add)),
                            |st| st.hidden(),
                        )
                        .child(
                            Row::new("Add members")
                                .icon(Icon::UserPlus)
                                .on_press(add)
                                .build(),
                        )
                        .child(separator_inset(56.0)),
                )
                .child(
                    virtual_list(
                        move || list.with(Vec::len),
                        move |at| {
                            let m = computed(move || list.with(|l| l.get(at.index()).cloned()));
                            member_row(s, m, open_member)
                        },
                    )
                    .keys(move || {
                        list.with(|l| l.iter().map(|m| m.contact.unwrap_or(0)).collect::<Vec<_>>())
                    })
                    .viewport(ListViewport::Parent)
                    .estimated_item_size(px(if is_desktop() { 52.0 } else { 64.0 }))
                    .separator(|| separator_inset(68.0))
                    .empty(view().p_4().child(muted("No members match."))),
                ),
        )
}

/// A member: avatar, name ("You" for the user), username, role tag.
fn member_row(
    s: Store,
    m: Memo<Option<Member>>,
    open: impl Fn(ContactId) + Copy + 'static,
) -> impl IntoElement {
    let contact = move || m.with(|m| m.as_ref().and_then(|m| m.contact));
    let name = move || s.contact_name(contact());
    let role = move || m.with(|m| m.as_ref().map_or(Role::Member, |m| m.role));
    let sub = move || match contact() {
        None => s.me.with(|p| p.about.clone()),
        Some(id) => s
            .contact(id)
            .map(|c| {
                if c.about.is_empty() {
                    handle(&c.username)
                } else {
                    c.about
                }
            })
            .unwrap_or_default(),
    };
    let seed = move || contact().unwrap_or(s.me.with(|p| p.seed));
    press_surface(Press::Row, move || {
        if let Some(id) = contact() {
            open(id);
        }
    })
    .accessibility_label(move || {
        Some(SharedString::from(format!(
            "{}, {}",
            name(),
            role().label()
        )))
    })
    .flex_row()
    .items_center()
    .gap_3()
    .px_4()
    .min_h(px(if is_desktop() { 52.0 } else { 64.0 }))
    .py_2()
    // Their portrait, seeded by their contact id (the user's own seed for
    // the user), as in every other list.
    .child(dynamic(move || {
        let who = contact().map_or_else(|| s.me.with(|p| p.name.clone()), |_| name());
        let seed = seed();
        untracked(move || avatar(who, seed, 40.0))
    }))
    .child(
        view()
            .flex_1()
            .min_w(px(0.0))
            .gap(px(2.0))
            .child(text(name).text_size(px(font::body())).number_of_lines(1))
            .child(
                text(sub)
                    .text_size(px(font::SMALL))
                    .text_color(move || c::muted_fg())
                    .number_of_lines(1),
            ),
    )
    .child(
        view()
            .style_when(move || role() == Role::Member, |st| st.hidden())
            .child(tag(move || SharedString::from(role().label()))),
    )
}

/// What the user can do about one member.
fn member_sheet(
    s: Store,
    chat: ChatId,
    ch: Memo<Option<Chat>>,
    member: Signal<Option<ContactId>>,
    open: Signal<bool>,
) -> View {
    view().child(dynamic(move || {
        let Some(id) = member.get() else { return view().into_element() };
        let c = ch.get();
        untracked(move || {
            let Some(c) = c else { return view().into_element() };
            let Some(contact) = s.contact(id) else { return view().into_element() };
            let target = c.members.iter().find(|m| m.contact == Some(id)).map_or(Role::Member, |m| m.role);
            let mine = my_role(&c);
            let close = move || open.set(false);
            let mut rows = vec![
                Row::new(format!("Message {}", first_name(&contact.name)))
                    .icon(Icon::Chats)
                    .on_press(move || {
                        close();
                        chat_feature::open(s.direct_chat(id));
                    })
                    .build(),
            ];
            if can_act_on(mine, target) {
                rows.push(if target == Role::Admin {
                    Row::new("Dismiss as admin")
                        .icon(Icon::Shield)
                        .on_press(move || {
                            s.change_role(chat, id, Role::Member);
                            close();
                        })
                        .build()
                } else {
                    Row::new("Make group admin")
                        .icon(Icon::ShieldCheck)
                        .on_press(move || {
                            s.change_role(chat, id, Role::Admin);
                            close();
                        })
                        .build()
                });
            }
            rows.push(
                Row::new("View safety number")
                    .icon(Icon::Key)
                    .on_press(move || {
                        close();
                        devices::safety_number(id);
                    })
                    .build(),
            );
            if can_act_on(mine, target) {
                let name = contact.name.clone();
                let title = c.title.clone();
                rows.push(
                    Row::new("Remove from group")
                        .leading(danger_icon(Icon::Trash))
                        .danger()
                        .on_press(move || {
                            close();
                            confirm(
                                format!("Remove {name}?"),
                                format!("{name} will no longer get messages from “{title}”. They'll see that you removed them."),
                                "Remove",
                                move || {
                                    glide();
                                    s.remove_from_group(chat, id);
                                },
                            );
                        })
                        .build(),
                );
            }
            let note = if mine == Some(Role::Member) {
                Some(ask_line(s, &c, "change roles or remove people"))
            } else if target == Role::Owner {
                Some("The owner can't be removed or dismissed. They can hand the group over by leaving it.".to_string())
            } else {
                None
            };
            view()
                .gap_3()
                .child(person_header(&contact, target))
                .child(group(rows))
                .child(note.map(footnote))
                .into_element()
        })
    }))
}

fn person_header(contact: &Contact, role: Role) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .child(avatar(contact.name.clone(), contact.id, 56.0))
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(contact.name.clone())
                                .text_size(px(font::TITLE))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(contact.verified.then(|| {
                            view()
                                .label("Verified")
                                .role(AccessibilityRole::Image)
                                .child(
                                    icon(Icon::ShieldCheck, 16.0).text_color(move || c::primary()),
                                )
                        }))
                        .child(role_tag(role)),
                )
                .child(muted(handle(&contact.username))),
        )
}

/// Name and description.
fn edit_sheet(s: Store, chat: ChatId, open: Signal<bool>) -> View {
    let c = s.chat(chat);
    let name = signal(c.as_ref().map(|c| c.title.clone()).unwrap_or_default());
    let about = signal(c.as_ref().map(|c| c.about.clone()).unwrap_or_default());
    view()
        .px_4()
        .gap_4()
        .child(labelled_field(
            "Group name",
            field("Group name", name).max_length(100_u32),
            "Everyone in the group sees it.",
            || false,
        ))
        .child(labelled_field(
            "Description",
            field("Add a description", about)
                .multiline(true)
                .h(px(96.0))
                .py_2(),
            "Shown at the top of the group's info.",
            || false,
        ))
        .child(
            btn("Save", Variant::Primary, BtnSize::Lg, move || {
                let title = name.get_untracked().trim().to_string();
                if title.is_empty() {
                    return;
                }
                let desc = about.get_untracked().trim().to_string();
                s.set_group_info(chat, title, desc);
                s.push_message(
                    chat,
                    None,
                    Body::System("You changed the group info".into()),
                );
                open.set(false);
            })
            .disabled(move || name.with(|n| n.trim().is_empty())),
        )
}

/// People to add: those not in the group yet.
fn add_sheet(s: Store, chat: ChatId, open: Signal<bool>) -> View {
    let picked = signal(Vec::<ContactId>::new());
    let query = signal(String::new());
    let list = computed(move || {
        let q = query.get();
        let inside: Vec<ContactId> = s
            .chat(chat)
            .map(|c| c.members.iter().filter_map(|m| m.contact).collect())
            .unwrap_or_default();
        create::addable(s)
            .into_iter()
            .filter(|c| !inside.contains(&c.id) && create::matches(c, &q))
            .collect::<Vec<_>>()
    });
    view()
        .gap_3()
        .child(
            view()
                .px_4()
                .child(search_field("Search name or username", query)),
        )
        .child(scroll().max_h(px(360.0)).child(view().child(each(
            move || list.get(),
            |c: &Contact| c.id,
            move |c, _| {
                let id = c.get_untracked().id;
                let memo = computed(move || Some(c.get()));
                let chosen = move || picked.with(|p| p.contains(&id));
                create::contact_pick_row(memo, chosen, move || {
                    picked.update(|p| match p.iter().position(|x| *x == id) {
                        Some(i) => {
                            p.remove(i);
                        }
                        None => p.push(id),
                    })
                })
            },
        ))))
        .child(
            view().px_4().child(
                btn(
                    move || match picked.with(Vec::len) {
                        0 => "Add".to_string(),
                        1 => "Add 1 person".to_string(),
                        n => format!("Add {n} people"),
                    },
                    Variant::Primary,
                    BtnSize::Lg,
                    move || {
                        let ids = picked.get_untracked();
                        if ids.is_empty() {
                            return;
                        }
                        glide();
                        s.add_to_group(chat, ids);
                        picked.set(Vec::new());
                        open.set(false);
                    },
                )
                .disabled(move || picked.with(Vec::is_empty)),
            ),
        )
}

/// Off, 24 hours, 7 days, 4 weeks.
fn timer_sheet(s: Store, chat: ChatId, ch: Memo<Option<Chat>>, open: Signal<bool>) -> View {
    let allowed = move || ch.with(|c| c.as_ref().is_some_and(can_edit_info));
    let current = move || ch.with(|c| c.as_ref().and_then(|c| c.disappearing));
    let option = move |value: Option<Disappear>, label: &'static str| {
        let chosen = move || current() == value;
        press_surface(Press::Row, move || {
            if allowed() && current() != value {
                s.set_disappearing(chat, value);
                open.set(false);
            }
        })
        .disabled(move || !allowed())
        .selected(chosen)
        .accessibility_label(label)
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .min_h(px(if is_desktop() { 44.0 } else { 56.0 }))
        .child(text(label).flex_1().text_size(px(font::body())))
        .child(create::check_circle(chosen))
        .into_element()
    };
    view()
        .gap_3()
        .child(view().px_4().child(muted(
            "New messages will disappear for everyone in the group after the time you choose. Messages already sent stay.",
        )))
        .child(group([
            option(None, "Off"),
            option(Some(Disappear::Day), "24 hours"),
            option(Some(Disappear::Week), "7 days"),
            option(Some(Disappear::FourWeeks), "4 weeks"),
        ]))
        .child(
            view().style_when(allowed, |st| st.hidden()).child(footnote(move || {
                s.chat(chat).map(|c| ask_line(s, &c, "change this")).unwrap_or_default()
            })),
        )
}

fn crypto_sheet(s: Store, chat: ChatId) -> View {
    view()
        .px_4()
        .gap_4()
        .child(banner(Tone::Secure, Icon::Lock, "Messages and calls in this group are end-to-end encrypted."))
        .child(body(move || {
            let n = s.chat(chat).map_or(0, |c| c.members.len());
            format!(
                "Only the {n} people in this group can read, listen to or share what's sent here. Not Songbird, not anyone else. When someone joins or leaves, the keys change, so new members can't read earlier messages."
            )
        }))
        .child(muted("To check you're talking to the right people, compare safety numbers with a member: open them in the member list."))
        .child(btn("What Songbird can see", Variant::Outline, BtnSize::Md, crate::features::settings::privacy_centre))
}

fn exit(s: Store, chat: ChatId) {
    let Some(c) = s.chat(chat) else { return };
    let heir = crate::store::ext_groups::successor(&c).map(|h| s.contact_name(Some(h)));
    let message = match (my_role(&c), heir) {
        (Some(Role::Owner), Some(h)) => {
            format!(
                "You're the owner. {h} will become the owner. You won't get new messages from this group."
            )
        }
        _ => "You won't get new messages from this group. Members will see that you left."
            .to_string(),
    };
    confirm(
        format!("Exit “{}”?", c.title),
        message,
        "Exit group",
        move || {
            s.exit_group(chat);
            nav::pop();
        },
    );
}

fn report(s: Store, chat: ChatId) {
    let title = s.chat(chat).map(|c| c.title).unwrap_or_default();
    confirm(
        format!("Report “{title}”?"),
        "The last five messages in this chat are sent to Songbird so we can review them. Nobody in the group is told.",
        "Report",
        move || {
            s.report(chat);
        },
    );
}

// ------------------------------------------------------------------ channel

fn channel_page(s: Store, chat: ChatId) -> Screen {
    let ch = computed(move || s.chat(chat));
    let followers = move || match ch.with(|c| c.as_ref().map(|c| c.kind.clone())) {
        Some(ChatKind::Channel { followers, .. }) => followers,
        _ => 0,
    };
    let admin = matches!(
        s.chat(chat).map(|c| c.kind),
        Some(ChatKind::Channel { admin: true, .. })
    );
    let unfollow = move || {
        let title = s.chat(chat).map(|c| c.title).unwrap_or_default();
        confirm(
            format!("Unfollow “{title}”?"),
            "You'll stop getting its updates. You can follow it again from Find channels.",
            "Unfollow",
            move || {
                nav::pop();
                if nav::open_chat().get_untracked().0 == Some(chat) {
                    nav::open_chat().set(nav::OpenChat(None));
                }
                s.unfollow(chat);
            },
        );
    };
    page(
        "Channel info",
        page_scroll(
            view()
                .gap_4()
                .child(header(s, chat, move || {
                    format!("Channel · {}", followers_label(followers()))
                }))
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .px_4()
                        .child(action_tile(Icon::BellOff, "Mute", move || {
                            s.toggle_mute(chat);
                            let muted = s.chat(chat).is_some_and(|c| c.muted);
                            toast(if muted { "Muted" } else { "Unmuted" });
                        }))
                        .child(action_tile(Icon::Share, "Share", move || {
                            if let Some(c) = s.chat(chat) {
                                set_clipboard(c.invite_link);
                                toast("Channel link copied");
                            }
                        }))
                        .child(action_tile(Icon::LogOut, "Unfollow", unfollow)),
                )
                .child(view().px_4().child(banner(
                    Tone::Secure,
                    Icon::EyeOff,
                    "Followers can't see each other. Only the channel's admins can post.",
                )))
                .child(group([
                    icon_toggle_row(
                        Icon::BellOff,
                        "Mute notifications",
                        None,
                        move || ch.with(|c| c.as_ref().is_some_and(|c| c.muted)),
                        move |_| s.toggle_mute(chat),
                    ),
                    Row::new("Media, links and docs")
                        .icon(Icon::Image)
                        .chevron()
                        .on_press(move || media_page(chat))
                        .build(),
                ]))
                .child(admin.then(|| {
                    view()
                        .child(section_title("Posting tools"))
                        .child(group([
                            Row::new("New post")
                                .icon(Icon::Edit)
                                .on_press(move || chat_feature::open(chat))
                                .build(),
                            Row::new("Poll")
                                .icon(Icon::Poll)
                                .on_press(move || chat_feature::open(chat))
                                .build(),
                            Row::new("Channel admins")
                                .icon(Icon::Shield)
                                .chevron()
                                .on_press(move || roles_page(chat))
                                .build(),
                        ]))
                        .child(footnote(
                            "Posts go to every follower. Followers can react but not reply.",
                        ))
                }))
                .child(group([
                    Row::new("Report channel")
                        .leading(danger_icon(Icon::Flag))
                        .danger()
                        .on_press(move || report(s, chat))
                        .build(),
                    Row::new("Unfollow channel")
                        .leading(danger_icon(Icon::LogOut))
                        .danger()
                        .on_press(unfollow)
                        .build(),
                ])),
        ),
    )
}

// ------------------------------------------------------------------ media, starred, search

/// Photos, videos, links and files: what "Media, links and docs" counts.
pub(crate) fn is_shared(b: &Body) -> bool {
    matches!(
        b,
        Body::Image { .. } | Body::Video { .. } | Body::Link { .. } | Body::File { .. }
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Shelf {
    Media,
    Links,
    Docs,
}

fn media_page(chat: ChatId) {
    nav::push(format!("media:{chat}"), move || {
        let s = store();
        let shelf = signal(Shelf::Media);
        page(
            "Media, links and docs",
            view()
                .flex_1()
                .child(view().px_4().pt_3().pb_3().child(segmented(
                    vec![
                        (Shelf::Media, "Media"),
                        (Shelf::Links, "Links"),
                        (Shelf::Docs, "Docs"),
                    ],
                    move || shelf.get(),
                    move |v| shelf.set(v),
                )))
                .child(dynamic(move || {
                    let which = shelf.get();
                    let msgs = s.messages_of(chat);
                    untracked(move || shelf_view(s, chat, which, msgs))
                })),
        )
    });
}

fn shelf_view(
    s: Store,
    chat: ChatId,
    which: Shelf,
    msgs: Vec<crate::store::Message>,
) -> AnyElement {
    match which {
        Shelf::Media => {
            let items: Vec<_> = msgs
                .iter()
                .rev()
                .filter(|m| matches!(m.body, Body::Image { .. } | Body::Video { .. }))
                .cloned()
                .collect();
            if items.is_empty() {
                return empty_state(
                    Icon::Image,
                    "No media",
                    "Photos and videos shared in this chat show here.",
                    None,
                )
                .into_element();
            }
            page_scroll(
                view()
                    .px_4()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .children(items.into_iter().map(move |m| media_tile(s, chat, m))),
            )
            .into_element()
        }
        Shelf::Links => {
            let rows: Vec<AnyElement> = msgs
                .iter()
                .rev()
                .filter_map(|m| match &m.body {
                    Body::Link {
                        url, title, site, ..
                    } => {
                        let url = url.clone();
                        Some(
                            Row::new(title.clone())
                                .icon(Icon::Link)
                                .subtitle(format!("{site} · {}", when_label(m.at)))
                                .on_press(move || {
                                    let _ = open_url(url.clone());
                                })
                                .build(),
                        )
                    }
                    _ => None,
                })
                .collect();
            if rows.is_empty() {
                return empty_state(
                    Icon::Link,
                    "No links",
                    "Links shared in this chat show here.",
                    None,
                )
                .into_element();
            }
            page_scroll(group(rows)).into_element()
        }
        Shelf::Docs => {
            let rows: Vec<AnyElement> = msgs
                .iter()
                .rev()
                .filter_map(|m| match &m.body {
                    Body::File { name, size, kind } => Some(
                        Row::new(name.clone())
                            .icon(Icon::File)
                            .subtitle(format!("{kind} · {size} · {}", when_label(m.at)))
                            .build(),
                    ),
                    _ => None,
                })
                .collect();
            if rows.is_empty() {
                return empty_state(
                    Icon::File,
                    "No documents",
                    "Files shared in this chat show here.",
                    None,
                )
                .into_element();
            }
            page_scroll(group(rows)).into_element()
        }
    }
}

/// A square of the media grid; opens the viewer.
fn media_tile(_s: Store, chat: ChatId, m: crate::store::Message) -> AnyElement {
    let id: MsgId = m.id;
    // The message's own frame and id: the same mock photo as its bubble
    // and the viewer, cropped to the square.
    let (alt, video, w, h) = match &m.body {
        Body::Image { alt, w, h, .. } => (alt.clone(), false, (*w).max(1), (*h).max(1)),
        Body::Video { caption, .. } => (
            if caption.is_empty() {
                "A video".to_string()
            } else {
                caption.clone()
            },
            true,
            16,
            9,
        ),
        _ => (String::new(), false, 1, 1),
    };
    let src = crate::ui::portrait::message_photo(&alt, id, w, h);
    let side = if wide() { 112.0 } else { 116.0 };
    press_surface(Press::Tile, move || media::open_viewer(chat, id))
        .accessibility_label(if video {
            format!("Video: {alt}")
        } else {
            format!("Photo: {alt}")
        })
        .size(px(side))
        .rounded(move || radius::sm())
        .overflow_hidden()
        .items_center()
        .justify_center()
        .bg(move || c::muted())
        .child(
            image(src)
                .absolute()
                .inset_0()
                .object_fit(ResizeMode::Cover)
                .accessibility_elements_hidden(true),
        )
        .child(video.then(|| {
            view()
                .size(px(36.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::call_bg().opacity(0.55))
                .text_color(move || c::on_call())
                .accessibility_elements_hidden(true)
                .child(icon(Icon::Play, 18.0))
        }))
        .into_element()
}

fn starred_page(chat: ChatId) {
    nav::push(format!("starred:{chat}"), move || {
        let s = store();
        page(
            "Starred messages",
            view().flex_1().child(dynamic(move || {
                let starred: Vec<_> = s
                    .messages_of(chat)
                    .into_iter()
                    .filter(|m| m.starred)
                    .collect();
                untracked(move || {
                    if starred.is_empty() {
                        return empty_state(
                            Icon::Star,
                            "No starred messages",
                            "Star a message to find it here. Touch and hold it, then tap Star.",
                            None,
                        )
                        .into_element();
                    }
                    page_scroll(view().pt_3().child(group(starred.into_iter().map(|m| {
                        Row::new(m.body.preview())
                            .leading(avatar(s.contact_name(m.from), m.from.unwrap_or(42), 32.0))
                            .subtitle(format!("{} · {}", s.contact_name(m.from), when_label(m.at)))
                            .on_press(move || chat_feature::open(chat))
                            .build()
                    }))))
                    .into_element()
                })
            })),
        )
    });
}

fn search_page(chat: ChatId) {
    nav::push(format!("search:{chat}"), move || {
        let s = store();
        let query = signal(String::new());
        let results = computed(move || {
            let q = query.get().trim().to_lowercase();
            if q.len() < 2 {
                return Vec::new();
            }
            s.messages_of(chat)
                .into_iter()
                .rev()
                .filter(|m| {
                    !matches!(m.body, Body::System(_))
                        && m.body.preview().to_lowercase().contains(&q)
                })
                .take(200)
                .collect::<Vec<_>>()
        });
        page(
            "Search",
            view()
                .flex_1()
                .child(
                    view()
                        .px_4()
                        .pt_3()
                        .pb_2()
                        .child(search_field("Search messages", query)),
                )
                .child(
                    virtual_list(
                        move || results.with(Vec::len),
                        move |at| {
                            let m = computed(move || results.with(|r| r.get(at.index()).cloned()));
                            let title = move || {
                                m.with(|m| m.as_ref().map(|m| m.body.preview()).unwrap_or_default())
                            };
                            let sub = move || {
                                m.with(|m| {
                                    m.as_ref()
                                        .map(|m| {
                                            format!(
                                                "{} · {}",
                                                s.contact_name(m.from),
                                                when_label(m.at)
                                            )
                                        })
                                        .unwrap_or_default()
                                })
                            };
                            press_surface(Press::Row, move || chat_feature::open(chat))
                                .accessibility_label(move || Some(SharedString::from(title())))
                                .px_4()
                                .py_2()
                                .gap(px(2.0))
                                .child(text(title).text_size(px(font::body())).number_of_lines(2))
                                .child(
                                    text(sub)
                                        .text_size(px(font::SMALL))
                                        .text_color(move || c::muted_fg()),
                                )
                        },
                    )
                    .estimated_item_size(px(56.0))
                    .empty(view().p_4().child(muted(move || {
                        if query.with(|q| q.trim().chars().count() < 2) {
                            "Type at least two letters to search this chat.".to_string()
                        } else {
                            "No messages match.".to_string()
                        }
                    })))
                    .flex_1(),
                ),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_acts_on_the_owner_and_members_act_on_nobody() {
        assert!(can_act_on(Some(Role::Owner), Role::Admin));
        assert!(can_act_on(Some(Role::Admin), Role::Member));
        assert!(can_act_on(Some(Role::Admin), Role::Admin));
        assert!(!can_act_on(Some(Role::Admin), Role::Owner));
        assert!(!can_act_on(Some(Role::Member), Role::Member));
        assert!(!can_act_on(None, Role::Member));
    }
}
