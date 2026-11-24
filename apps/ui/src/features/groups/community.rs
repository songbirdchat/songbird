//! A space's home, adding groups to it, and Find channels. In the store a
//! space is still a `ChatKind::Community`; every word the user reads says
//! "space".

use nsh_core::prelude::*;

use super::{
    LANE, chat_avatar, community_mark, create, glide, group_row, invite_link_page, list_card,
    list_row, page_scroll, square_tile,
};
use crate::{
    nav,
    store::{
        ChatId, ChatKind, Role, Store,
        ext_groups::{can_manage, directory, followers_label, my_role},
        store,
    },
    ui::*,
};

/// The space's page: header, Announcements pinned, its groups, Add
/// group for admins, members, the invite link, leaving.
pub(super) fn home_page(chat: ChatId) -> Screen {
    let s = store();
    let ch = computed(move || s.chat(chat));
    let manage = move || ch.with(|c| c.as_ref().is_some_and(can_manage));
    let add_open = signal(false);
    let c0 = s.chat(chat);
    let title = c0
        .as_ref()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| "Space".into());
    page(
        title,
        view()
            .flex_1()
            .child(page_scroll(
                view()
                    .gap_4()
                    .child(
                        view()
                            .items_center()
                            .gap_1()
                            .px_4()
                            .pt_6()
                            .child(c0.as_ref().map(|c| community_mark(c.seed, 88.0, c::background)))
                            .child(
                                text(move || ch.with(|c| c.as_ref().map(|c| c.title.clone()).unwrap_or_default()))
                                    .mt_3()
                                    .text_size(px(font::HEADING))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_center()
                                    .role(AccessibilityRole::Header),
                            )
                            .child(muted(move || {
                                let n = ch.with(|c| c.as_ref().map_or(0, |c| c.members.len()));
                                let g = s.community_groups(chat).len();
                                format!("Space · {} · {}", count(n, "member"), count(g, "group"))
                            }))
                            .child(
                                text(move || ch.with(|c| c.as_ref().map(|c| c.about.clone()).unwrap_or_default()))
                                    .mt_2()
                                    .max_w(px(480.0))
                                    .text_size(px(font::body()))
                                    .line_height(px(font::body() * 1.4))
                                    .text_center(),
                            ),
                    )
                    .child(dynamic(move || {
                        let groups = s.community_groups(chat);
                        let admin = manage();
                        untracked(move || {
                            let (ann, rest): (Vec<_>, Vec<_>) = groups.into_iter().partition(|g| g.announcement_only);
                            let mut rows: Vec<AnyElement> = rest.iter().map(|g| group_row(s, g)).collect();
                            if admin {
                                rows.push(
                                    list_row(
                                        square_tile(Icon::Plus),
                                        "Add group",
                                        Some("Create a new group or add one you admin".into()),
                                        None,
                                        move || add_open.set(true),
                                    )
                                    .into_element(),
                                );
                            }
                            view()
                                .child(section_title("Announcements").pt_1())
                                .child(list_card(ann.iter().map(|g| group_row(s, g))))
                                .child(footnote("Everyone in the space gets announcements. Only admins post."))
                                .child(section_title("Groups in this space"))
                                .child(if rows.is_empty() {
                                    footnote("No other groups yet.").pt_0().into_element()
                                } else {
                                    list_card(rows).into_element()
                                })
                                .into_element()
                        })
                    }))
                    // The same 40pt lane as the groups above, so every
                    // title on the page starts on one line.
                    .child(list_card([
                        list_row(
                            square_tile(Icon::Link),
                            "Invite members",
                            Some("Share a link to the space".into()),
                            Some(trailing_value(None)),
                            move || invite_link_page(chat),
                        )
                        .into_element(),
                        view()
                            .child(dynamic(move || {
                                let n = ch.with(|c| c.as_ref().map_or(0, |c| c.members.len()));
                                untracked(move || {
                                    list_row(
                                        square_tile(Icon::Users),
                                        "Members",
                                        None,
                                        Some(trailing_value(Some(n.to_string()))),
                                        move || members_page(chat),
                                    )
                                    .into_element()
                                })
                            }))
                            .into_element(),
                    ]))
                    .child(list_card([view()
                        .child(
                            press_surface(Press::Row, move || leave_space(s, chat))
                                .accessibility_label("Leave space")
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_3()
                                        .px_4()
                                        .min_h(px(if is_desktop() { 52.0 } else { 56.0 }))
                                        .child(
                                            view()
                                                .size(px(LANE))
                                                .items_center()
                                                .justify_center()
                                                .text_color(move || c::destructive())
                                                .child(icon(Icon::LogOut, 20.0)),
                                        )
                                        .child(
                                            text("Leave space")
                                                .text_size(px(font::body()))
                                                .text_color(move || c::destructive()),
                                        ),
                                ),
                        )
                        .into_element()]))
                    .child(footnote(
                        "The space's name and its list of groups are end-to-end encrypted. Members of one group can't see the messages of another.",
                    )),
            ))
            .child(app_sheet(add_open, "Add group", add_group_sheet(s, chat, add_open))),
    )
}

/// "1 member", "9 members": a count with its noun, singular for one.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A row's trailing value (muted, tabular) and chevron.
fn trailing_value(value: Option<String>) -> AnyElement {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .text_color(move || c::muted_fg())
        .child(value.map(|v| text(v).text_size(px(font::SMALL + 1.0)).tabular_nums()))
        .child(icon(Icon::ChevronRight, 18.0))
        .into_element()
}

/// New group, or a group the user admins that is in no community.
fn add_group_sheet(s: Store, community: ChatId, open: Signal<bool>) -> View {
    let candidates = move || {
        s.chats.with(|cs| {
            cs.iter()
                .filter(|c| c.kind == ChatKind::Group && c.community.is_none() && can_manage(c))
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    view()
        .gap_3()
        .child(group([Row::new("Create new group")
            .leading(super::square_tile(Icon::Plus))
            .on_press(move || {
                open.set(false);
                create::new_group_flow(Some(community));
            })
            .build()]))
        .child(section_title("Groups you admin"))
        .child(dynamic(move || {
            let list = candidates();
            untracked(move || {
                if list.is_empty() {
                    return footnote("Groups you admin that aren't in a space show here.")
                        .into_element();
                }
                group(list.iter().map(|g| {
                    let id = g.id;
                    let n = g.members.len();
                    Row::new(g.title.clone())
                        .leading(chat_avatar(g, 36.0))
                        .subtitle(format!("{n} members"))
                        .on_press(move || {
                            glide();
                            s.link_group(community, id);
                            open.set(false);
                            toast("Group added");
                        })
                        .build()
                }))
                .into_element()
            })
        }))
        .child(footnote(
            "A group's own admins must agree before it joins. Its messages stay within the group.",
        ))
}

fn members_page(chat: ChatId) {
    nav::push(format!("community-members:{chat}"), move || {
        let s = store();
        let members = s
            .chat(chat)
            .map(|c| crate::store::ext_groups::sorted_members(&c, |id| s.contact_name(id)))
            .unwrap_or_default();
        page(
            "Members",
            page_scroll(view().pt_3().child(group(members.into_iter().map(|m| {
                let name = s.contact_name(m.contact);
                let mut row =
                    Row::new(name.clone()).leading(avatar(name, m.contact.unwrap_or(42), 36.0));
                if m.role != Role::Member {
                    row = row.trailing(tag(m.role.label()));
                }
                row.build()
            })))),
        )
    });
}

fn leave_space(s: Store, chat: ChatId) {
    let title = s.chat(chat).map(|c| c.title).unwrap_or_default();
    confirm(
        format!("Leave “{title}”?"),
        "You'll leave the space and every group in it you're in. Members will see that you left.",
        "Leave space",
        move || {
            for g in s.community_groups(chat) {
                if my_role(&g).is_some() {
                    s.exit_group(g.id);
                }
            }
            s.exit_group(chat);
            nav::pop();
        },
    );
}

/// Find channels: a directory with Follow buttons. There is no public directory yet (public
/// channels await the owner's legal review, D-0065 §4), so a real account follows a channel by
/// the link someone shares: the page says so and takes the link.
pub fn find_channels() {
    if directory().is_empty() {
        return follow_by_link();
    }
    nav::push("find-channels", move || {
        let s = store();
        let query = signal(String::new());
        let list = computed(move || {
            let q = query.get().trim().to_lowercase();
            directory()
                .into_iter()
                .filter(|d| {
                    q.is_empty()
                        || d.name.to_lowercase().contains(&q)
                        || d.about.to_lowercase().contains(&q)
                })
                .collect::<Vec<_>>()
        });
        page(
            "Find channels",
            view()
                .flex_1()
                .child(view().px_4().pt_3().pb_2().child(search_field("Search channels", query)))
                .child(
                    virtual_list(move || list.with(Vec::len), move |at| {
                        let d = computed(move || list.with(|l| l.get(at.index()).cloned()));
                        let name = move || d.with(|d| d.as_ref().map(|d| d.name).unwrap_or_default());
                        let following = move || s.followed(name()).is_some();
                        let seed = move || d.with(|d| d.as_ref().map_or(0, |d| d.id));
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_3()
                            // A channel's picture, as in the Spaces tab.
                            .child(dynamic(move || {
                                let (n, seed) = (name(), seed());
                                untracked(move || group_avatar(n, seed, LANE))
                            }))
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(2.0))
                                    .child(text(name).text_size(px(font::body())).number_of_lines(1))
                                    .child(
                                        text(move || {
                                            d.with(|d| d.as_ref().map(|d| format!("{} · {}", followers_label(d.followers), d.about)).unwrap_or_default())
                                        })
                                        .text_size(px(font::SMALL))
                                        .text_color(move || c::muted_fg())
                                        .number_of_lines(2),
                                    ),
                            )
                            .child(dynamic(move || {
                                // Following: a quiet filled button with a check;
                                // Follow: the outlined call to action.
                                let on = following();
                                untracked(move || {
                                    btn_with_icon(
                                        if on { "Following" } else { "Follow" },
                                        on.then_some(Icon::Check),
                                        if on { Variant::Secondary } else { Variant::Outline },
                                        BtnSize::Sm,
                                        move || {
                                            let Some(entry) = d.get_untracked() else { return };
                                            match s.followed(entry.name) {
                                                Some(id) => {
                                                    s.unfollow(id);
                                                    toast(format!("Unfollowed {}", entry.name));
                                                }
                                                None => {
                                                    s.follow_channel(&entry);
                                                    toast(format!("Following {}", entry.name));
                                                }
                                            }
                                        },
                                    )
                                    .selected(on)
                                })
                            }))
                    })
                    .keys(move || list.with(|l| l.iter().map(|d| d.id).collect::<Vec<_>>()))
                    .estimated_item_size(px(72.0))
                    .separator(|| separator_inset(16.0 + LANE + 12.0))
                    .empty(view().p_4().child(muted("No channels match.")))
                    .footer(footnote(
                        "Featured channels. You can also follow any channel from a link someone shares with you."
                    ).pb_6())
                    .flex_1(),
                ),
        )
    });
}

/// Following a channel by its link: the way in while there is no directory.
fn follow_by_link() {
    nav::push("find-channels", move || {
        let link = signal(String::new());
        let ours = move || link.with(|l| l.trim().starts_with("https://songbird.chat/g#"));
        let invalid = move || link.with(|l| !l.trim().is_empty()) && !ours();
        let open = move || {
            let url = link.get_untracked();
            if crate::features::open_songbird_link(&url) {
                link.set(String::new());
            } else {
                toast("Not connected yet. Try again in a moment.");
            }
        };
        page(
            "Find channels",
            view()
                .flex_1()
                .px_4()
                .pt_4()
                .gap_3()
                .child(muted(
                    "There's no channel directory yet. To follow a channel, open the link its admin shared, or paste it here.",
                ))
                .child(labelled_field(
                    "Channel link",
                    field("https://songbird.chat/g#…", link)
                        .auto_capitalize(AutoCapitalize::None)
                        .keyboard_type(KeyboardType::Url)
                        .on_submit(move |_: &str| {
                            if ours() {
                                open();
                            }
                        }),
                    move || {
                        if invalid() {
                            "That isn't a Songbird channel link.".to_string()
                        } else {
                            "Links to channels and groups start with songbird.chat/g#".to_string()
                        }
                    },
                    invalid,
                ))
                .child(view().flex_row().child(
                    btn("Follow", Variant::Primary, BtnSize::Md, open).disabled(move || !ours()),
                )),
        )
    });
}
