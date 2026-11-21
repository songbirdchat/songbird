//! Admin pages: roles and permissions, the invite link, join requests and
//! the member change log. Members see the same pages with the controls
//! disabled and a line saying who can change them.

use nsh_core::prelude::*;

use super::{ask_line, card_rows, create, glide, page_scroll, qr::qr_code, switch_row};
use crate::{
    nav,
    store::{
        ChatId, ContactId, GroupPermissions,
        ext_groups::{InviteExpiry, InviteUses, can_manage, invite_summary},
        store, when_label,
    },
    ui::*,
};

/// One permission: its title, what it means, how to read and write it.
struct Permission {
    title: &'static str,
    explain: &'static str,
    get: fn(&GroupPermissions) -> bool,
    set: fn(&mut GroupPermissions, bool),
}

const MEMBERS_CAN: [Permission; 5] = [
    Permission {
        title: "Edit group info",
        explain: "Change the group's name, photo, description and disappearing messages.",
        get: |p| p.members_edit_info,
        set: |p, v| p.members_edit_info = v,
    },
    Permission {
        title: "Add members",
        explain: "Add people and share the invite link. When off, only admins can.",
        get: |p| p.members_add,
        set: |p, v| p.members_add = v,
    },
    Permission {
        title: "Send messages",
        explain: "When off, only admins can send. Members can still read and react.",
        get: |p| p.members_send,
        set: |p, v| p.members_send = v,
    },
    Permission {
        title: "Pin messages",
        explain: "Pin a message to the top of the chat for everyone.",
        get: |p| p.members_pin,
        set: |p, v| p.members_pin = v,
    },
    Permission {
        title: "Start calls",
        explain: "Start a voice or video call with the whole group.",
        get: |p| p.members_call,
        set: |p, v| p.members_call = v,
    },
];

const ADMINS: [Permission; 2] = [
    Permission {
        title: "Approve new members",
        explain: "An admin approves anyone who joins with the invite link.",
        get: |p| p.approve_joins,
        set: |p, v| p.approve_joins = v,
    },
    Permission {
        title: "Admins can delete others' messages",
        explain: "Admins can delete any message for everyone. Members see who deleted it.",
        get: |p| p.admins_delete_any,
        set: |p, v| p.admins_delete_any = v,
    },
];

/// Roles and permissions, pushed.
pub fn roles_page(chat: ChatId) {
    nav::push(format!("roles:{chat}"), move || {
        let s = store();
        let ch = computed(move || s.chat(chat));
        let manage = move || ch.with(|c| c.as_ref().is_some_and(can_manage));
        let blocked_line = move || {
            ch.with(|c| {
                c.as_ref()
                    .map(|c| ask_line(s, c, "change these"))
                    .unwrap_or_default()
            })
        };
        // A real group's pinning and deleting others' messages are fixed by the protocol
        // (moderators and admins), not by its policy: shown, not switchable (D-0184).
        let real = crate::backend::chat::owns(chat);
        let row = move |p: &'static Permission| {
            let fixed = real && matches!(p.title, "Pin messages" | "Admins can delete others' messages");
            switch_row(
                p.title,
                if fixed { "Set by Songbird for every group: admins pin and delete others' messages." } else { p.explain },
                move || ch.with(|c| c.as_ref().is_some_and(|c| (p.get)(&c.permissions))),
                move || manage() && !fixed,
                move |v| {
                    s.set_group_permissions(chat, |perms, announcement| {
                        (p.set)(perms, v);
                        // "Send messages" and the announcement group are one setting.
                        if p.title == "Send messages" {
                            *announcement = !v;
                        }
                    })
                },
            )
        };
        page(
            "Group permissions",
            page_scroll(
                view()
                    .child(
                        view()
                            .px_4()
                            .pt_4()
                            .style_when(manage, |st| st.hidden())
                            .child(banner(Tone::Info, Icon::Lock, blocked_line)),
                    )
                    .child(section_title("Members can"))
                    .child(card_rows(MEMBERS_CAN.iter().map(row), 16.0))
                    .child(section_title("Admins"))
                    .child(card_rows(ADMINS.iter().map(row), 16.0))
                    .child(section_title("Announcement group"))
                    .child(group([switch_row(
                        "Only admins can send",
                        "Turns the group into announcements. Members read and react; admins post.",
                        move || ch.with(|c| c.as_ref().is_some_and(|c| c.announcement_only)),
                        manage,
                        move |v| {
                            s.set_group_permissions(chat, |perms, announcement| {
                                *announcement = v;
                                perms.members_send = !v;
                            })
                        },
                    )]))
                    .child(section_title("Roles"))
                    .child(group([
                        role_line(Icon::Key, "Owner", "Everything, including choosing admins. Nobody can remove the owner."),
                        role_line(Icon::Shield, "Admins", "Add and remove people, approve requests, manage the link and these settings."),
                        role_line(Icon::Users, "Members", "What's switched on above."),
                    ]))
                    .child(footnote(
                        "Roles and these settings are part of the group's encrypted state. Every member's app checks them; Songbird's servers can't change them.",
                    )),
            ),
        )
    });
}

fn role_line(glyph: Icon, title: &'static str, what: &'static str) -> AnyElement {
    Row::new(title).icon(glyph).subtitle(what).build()
}

/// The invite link: the link, copy, share, a code, reset, expiry and how
/// many may join.
pub fn invite_link_page(chat: ChatId) {
    nav::push(format!("invite:{chat}"), move || {
        let s = store();
        // A real group's link is made (or read) by app-core (D-0148).
        if crate::backend::chat::owns(chat) {
            crate::backend::chat::ensure_link(s, chat, false);
        }
        let ch = computed(move || s.chat(chat));
        let manage = move || ch.with(|c| c.as_ref().is_some_and(can_manage));
        let link = move || {
            ch.with(|c| {
                c.as_ref()
                    .map(|c| c.invite_link.clone())
                    .unwrap_or_default()
            })
        };
        let settings = move || s.invite_settings(chat);
        let reset = move || {
            confirm(
                "Reset the invite link?",
                "The current link stops working at once. Anyone who has it will need the new one.",
                "Reset link",
                move || {
                    if crate::backend::chat::owns(chat) {
                        crate::backend::chat::ensure_link(s, chat, true);
                    } else {
                        s.reset_invite(chat);
                    }
                    toast("New link made");
                },
            );
        };
        let blocked_line = move || {
            ch.with(|c| {
                c.as_ref()
                    .map(|c| ask_line(s, c, "change the link's settings"))
                    .unwrap_or_default()
            })
        };
        page(
            "Invite via link",
            page_scroll(
                view()
                    .gap_4()
                    .pt_4()
                    .child(
                        view()
                            .px_4()
                            .items_center()
                            .gap_3()
                            .child(qr_code(link, 6.0)),
                    )
                    .child(
                        view()
                            .px_4()
                            .gap_2()
                            .child(create::link_box(link))
                            .child(muted(move || invite_summary(&settings()))),
                    )
                    .child(group([
                        Row::new("Copy link")
                            .icon(Icon::Copy)
                            .on_press(move || {
                                set_clipboard(link());
                                toast("Link copied");
                            })
                            .build(),
                        Row::new("Share link…")
                            .icon(Icon::Share)
                            .on_press(move || share_text(link()))
                            .build(),
                        view()
                            .style_when(move || !manage(), |st| st.hidden())
                            .child(
                                Row::new("Reset link")
                                    .leading(super::danger_icon(Icon::Refresh))
                                    .danger()
                                    .on_press(reset)
                                    .build(),
                            )
                            .into_element(),
                    ]))
                    .child(
                        view()
                            .child(section_title("Link expires").pt_1())
                            .child(view().px_4().child(disabled_unless(
                                manage,
                                segmented(
                                    vec![
                                        (InviteExpiry::Never, "Never"),
                                        (InviteExpiry::Day, "1 day"),
                                        (InviteExpiry::Week, "7 days"),
                                    ],
                                    move || settings().expiry,
                                    move |v| {
                                        if manage() {
                                            s.change_invite_limits(chat, |x| x.expiry = v)
                                        }
                                    },
                                ),
                            )))
                            .child(section_title("People who can join"))
                            .child(view().px_4().child(disabled_unless(
                                manage,
                                segmented(
                                    vec![
                                        (InviteUses::Unlimited, "No limit"),
                                        (InviteUses::One, "1"),
                                        (InviteUses::Ten, "10"),
                                        (InviteUses::Hundred, "100"),
                                    ],
                                    move || settings().uses,
                                    move |v| {
                                        if manage() {
                                            s.change_invite_limits(chat, |x| x.uses = v)
                                        }
                                    },
                                ),
                            )))
                            .child(view().style_when(manage, |st| st.hidden()).child(footnote(blocked_line))),
                    )
                    .child(group([switch_row(
                        "Approve new members",
                        "An admin approves each person who uses the link.",
                        move || ch.with(|c| c.as_ref().is_some_and(|c| c.permissions.approve_joins)),
                        manage,
                        move |v| s.set_group_permissions(chat, |p, _| p.approve_joins = v),
                    )]))
                    .child(footnote(
                        "Anyone with the link sees the group's name and photo and can ask to join. The link itself never shows the group's messages.",
                    )
                    .pb_8()),
            ),
        )
    });
}

/// Dims a control and blocks it when `enabled` is false.
fn disabled_unless(enabled: impl Fn() -> bool + Copy + 'static, control: impl IntoElement) -> View {
    view()
        .opacity(move || if enabled() { 1.0_f32 } else { 0.5 })
        .pointer_events(move || {
            if enabled() {
                PointerEvents::Auto
            } else {
                PointerEvents::None
            }
        })
        .child(control)
}

/// Join requests, with Approve and Reject for each person.
pub fn join_requests_page(chat: ChatId) {
    if crate::backend::chat::owns(chat) {
        crate::backend::chat::refresh_requests(store(), chat);
    }
    nav::push(format!("requests:{chat}"), move || {
        let s = store();
        let requests = computed(move || s.chat(chat).map(|c| c.join_requests).unwrap_or_default());
        let answer = move |id: ContactId, approve: bool| {
            glide();
            s.answer_request(chat, id, approve);
            let name = s.contact_name(Some(id));
            toast(if approve {
                format!("{name} joined")
            } else {
                format!("Request from {name} rejected")
            });
        };
        page(
            "Join requests",
            view().flex_1().child(
                virtual_list(move || requests.with(Vec::len), move |at| {
                    let id = computed(move || requests.with(|r| r.get(at.index()).copied()));
                    let name = move || id.get().map(|i| s.contact_name(Some(i))).unwrap_or_default();
                    let username = move || id.get().and_then(|i| s.contact(i)).map(|c| format!("@{}", c.username)).unwrap_or_default();
                    let seed = move || id.get().unwrap_or(0);
                    // Wide windows: the buttons at the row's end. Narrow ones:
                    // under the name, on the text column, so the line beside
                    // the avatar keeps its measure.
                    let roomy = wide();
                    let buttons = view()
                        .flex_row()
                        .gap_2()
                        .child(btn("Reject", Variant::Outline, BtnSize::Sm, move || {
                            if let Some(i) = id.get_untracked() {
                                answer(i, false)
                            }
                        }))
                        .child(btn("Approve", Variant::Primary, BtnSize::Sm, move || {
                            if let Some(i) = id.get_untracked() {
                                answer(i, true)
                            }
                        }));
                    let words = view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(2.0))
                        .child(text(name).text_size(px(font::body())).number_of_lines(1))
                        .child(
                            text(move || format!("{} · asked with the invite link", username()))
                                .text_size(px(font::SMALL))
                                .line_height(px(18.0))
                                .text_color(move || c::muted_fg())
                                .number_of_lines(2),
                        );
                    view()
                        .flex_row()
                        .when(roomy, |v| v.items_center())
                        .when(!roomy, |v| v.items_start())
                        .gap_3()
                        .px_4()
                        .py_3()
                        .child(dynamic(move || {
                            let (n, id) = (name(), seed());
                            untracked(move || avatar(n, id, 40.0))
                        }))
                        .child(if roomy {
                            view().flex_1().flex_row().items_center().gap_3().child(words).child(buttons)
                        } else {
                            view().flex_1().gap_2().child(words).child(buttons)
                        })
                })
                .keys(move || requests.get())
                .estimated_item_size(px(64.0))
                .separator(|| separator_inset(68.0))
                .header(footnote("People who use the invite link wait here until an admin approves them. They can't see any messages before that.").pb_3())
                .empty(empty_state(Icon::UserPlus, "No requests", "When someone asks to join with the link, they'll show here.", None))
                .flex_1(),
            ),
        )
    });
}

/// The glyph of a member change, by what happened.
fn change_icon(line: &str) -> Icon {
    let has = |w: &str| line.contains(w);
    if has(" created ") {
        Icon::Sparkles
    } else if has(" an admin") || has(" the owner") || has(" as admin") {
        Icon::Shield
    } else if has(" removed ") || has(" rejected ") {
        Icon::Ban
    } else if has(" left") {
        Icon::LogOut
    } else if has(" reset the invite link") || has(" invite link") {
        Icon::Link
    } else {
        Icon::UserPlus
    }
}

/// Who joined, left, was added, removed or made admin, newest first.
pub fn member_changes_page(chat: ChatId) {
    // A real group's log is fetched and opened on this device each time the page opens.
    crate::backend::chat::load_member_changes(chat);
    nav::push(format!("changes:{chat}"), move || {
        let s = store();
        let changes = computed(move || s.member_history(chat));
        page(
            "Member changes",
            view().flex_1().child(
                virtual_list(move || changes.with(Vec::len), move |at| {
                    let ch = computed(move || changes.with(|c| c.get(at.index()).cloned()));
                    view()
                        .flex_row()
                        .items_start()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .child(dynamic(move || {
                            let glyph = ch.with(|c| c.as_ref().map_or(Icon::Users, |c| change_icon(&c.text)));
                            untracked(move || icon_tile(glyph))
                        }))
                        .child(
                            view()
                                .flex_1()
                                .gap(px(2.0))
                                .child(text(move || ch.with(|c| c.as_ref().map(|c| c.text.clone()).unwrap_or_default())).text_size(px(font::body())))
                                .child(
                                    text(move || ch.with(|c| c.as_ref().map(|c| when_label(c.at)).unwrap_or_default()))
                                        .text_size(px(font::SMALL))
                                        .text_color(move || c::muted_fg()),
                                ),
                        )
                })
                .estimated_item_size(px(60.0))
                .header(footnote("Every member can see this list. It comes from the group's encrypted change log.").pb_3())
                .flex_1(),
            ),
        )
    });
}
