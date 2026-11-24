//! Creating: the new group flow (members → name and settings → share the
//! invite → the group) and a new space.

use nsh_core::prelude::*;

use super::{bottom_bar, card_rows, community_home, first_name, page_scroll, qr::qr_code};
use crate::{
    nav,
    store::{ChatId, Contact, ContactId, Disappear, Store, store},
    ui::*,
};

/// Contacts the user can add: not blocked, not strangers from requests.
pub(crate) fn addable(s: Store) -> Vec<Contact> {
    let strangers: Vec<ContactId> = s.chats.with(|cs| {
        cs.iter()
            .filter(|c| c.request)
            .filter_map(|c| match c.kind {
                crate::store::ChatKind::Direct(x) => Some(x),
                _ => None,
            })
            .collect()
    });
    let mut out: Vec<Contact> = s.contacts.with(|cs| {
        cs.iter()
            .filter(|c| !c.blocked && !strangers.contains(&c.id))
            .cloned()
            .collect()
    });
    out.sort_by_key(|c| c.name.to_lowercase());
    out
}

/// Whether a contact matches a search (name or username).
pub(crate) fn matches(c: &Contact, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || c.name.to_lowercase().contains(&q) || c.username.to_lowercase().contains(&q)
}

/// Starts the flow. `community`: the space the group joins once made.
pub(crate) fn new_group_flow(community: Option<ChatId>) {
    let base = nav::depth();
    nav::push("new-group", move || {
        let s = store();
        let picked = signal(Vec::<ContactId>::new());
        let query = signal(String::new());
        page(
            "New group",
            pick_members(s, picked, query, move || {
                if picked.with_untracked(|p| !p.is_empty()) {
                    nav::push("new-group-details", move || {
                        details_page(base, community, picked)
                    });
                }
            }),
        )
    });
}

/// Step 1: search and pick people. Picked people show as chips above the
/// list; the list marks them with a filled check.
fn pick_members(
    s: Store,
    picked: Signal<Vec<ContactId>>,
    query: Signal<String>,
    next: impl Fn() + 'static,
) -> View {
    let list = computed(move || {
        let q = query.get();
        addable(s)
            .into_iter()
            .filter(|c| matches(c, &q))
            .collect::<Vec<_>>()
    });
    let toggle_pick = move |id: ContactId| {
        picked.update(|p| match p.iter().position(|x| *x == id) {
            Some(i) => {
                p.remove(i);
            }
            None => p.push(id),
        });
    };
    view()
        .flex_1()
        .child(
            view()
                .px_4()
                .pt_3()
                .pb_2()
                .gap_3()
                .child(search_field("Search name or username", query))
                .child(
                    scroll()
                        .horizontal()
                        .style_when(move || picked.with(Vec::is_empty), |st| st.hidden())
                        .child(view().flex_row().gap_2().child(each(
                            picked,
                            |id: &ContactId| *id,
                            move |id, _| {
                                picked_chip(s, id.get_untracked(), move || {
                                    toggle_pick(id.get_untracked())
                                })
                            },
                        ))),
                ),
        )
        .child(
            text(move || match picked.with(Vec::len) {
                0 => "Contacts".to_string(),
                n => format!("Contacts · {n} selected"),
            })
            .text_size(px(font::SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || c::muted_fg())
            .role(AccessibilityRole::Header)
            .px_4()
            .pb_2(),
        )
        .child(
            virtual_list(
                move || list.with(Vec::len),
                move |at| {
                    let contact = computed(move || list.with(|l| l.get(at.index()).cloned()));
                    let id = move || contact.with(|c| c.as_ref().map_or(0, |c| c.id));
                    let chosen = move || picked.with(|p| p.contains(&id()));
                    contact_pick_row(contact, chosen, move || toggle_pick(id()))
                },
            )
            .keys(move || list.with(|l| l.iter().map(|c| c.id).collect::<Vec<_>>()))
            .estimated_item_size(px(if is_desktop() { 52.0 } else { 64.0 }))
            .empty(empty_state(
                Icon::Search,
                "No one found",
                "Try another name or username.",
                None,
            ))
            .flex_1(),
        )
        .child(bottom_bar(
            view()
                .gap_2()
                // What the disabled button waits for, said above it: the
                // button's own label stays an action.
                .child(
                    text("Pick at least one person to continue.")
                        .text_center()
                        .text_size(px(font::SMALL))
                        .text_color(move || c::muted_fg())
                        .style_when(move || !picked.with(Vec::is_empty), |st| st.hidden()),
                )
                .child(
                    btn(
                        move || match picked.with(Vec::len) {
                            0 => "Next".to_string(),
                            1 => "Next: 1 person".to_string(),
                            n => format!("Next: {n} people"),
                        },
                        Variant::Primary,
                        BtnSize::Lg,
                        next,
                    )
                    .disabled(move || picked.with(Vec::is_empty)),
                ),
        ))
}

/// A contact row in a picker: avatar, name, username, a check circle.
pub(crate) fn contact_pick_row(
    contact: Memo<Option<Contact>>,
    chosen: impl Fn() -> bool + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let name = move || contact.with(|c| c.as_ref().map(|c| c.name.clone()).unwrap_or_default());
    let seed = move || contact.with(|c| c.as_ref().map_or(0, |c| c.id));
    press_surface(Press::Row, on_press)
        .selected(chosen)
        .accessibility_label(move || Some(SharedString::from(name())))
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .min_h(px(if is_desktop() { 52.0 } else { 64.0 }))
        // Their photo (or initials), as everywhere else people appear.
        .child(dynamic(move || {
            let (n, id) = (name(), seed());
            untracked(move || avatar(n, id, 40.0))
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                // Start-aligned in this row's direction: a name in Arabic or
                // Hebrew still starts at the text column, not the far edge.
                .child(
                    text(name)
                        .text_size(px(font::body()))
                        .number_of_lines(1)
                        .text_left(),
                )
                .child(
                    text(move || {
                        contact.with(|c| {
                            // A contact whose username this account never learned shows none,
                            // not a bare "@".
                            c.as_ref().map(|c| handle(&c.username)).unwrap_or_default()
                        })
                    })
                    .text_size(px(font::SMALL))
                    .text_color(move || c::muted_fg())
                    .number_of_lines(1),
                ),
        )
        .child(check_circle(chosen))
}

/// A round check: an empty ring, or filled with a tick when chosen
/// (shape and colour both change).
pub(crate) fn check_circle(chosen: impl Fn() -> bool + Copy + 'static) -> View {
    view()
        .size(px(22.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(move || if chosen() { c::primary() } else { c::input() })
        .bg(move || {
            if chosen() {
                c::primary()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .text_color(move || c::on_primary())
        .transition_colors()
        .duration(motion::FAST)
        .accessibility_elements_hidden(true)
        .child(icon(Icon::Check, 14.0).opacity(move || if chosen() { 1.0_f32 } else { 0.0 }))
}

/// A picked person above the list; pressing it removes them. Comes in
/// with a short fade and grow.
fn picked_chip(s: Store, id: ContactId, remove: impl Fn() + 'static) -> impl IntoElement {
    let name = s.contact_name(Some(id));
    let fade = animated_value(if motion::reduced() { 1.0 } else { 0.0 });
    let grow = animated_value(if motion::reduced() { 1.0 } else { 0.9 });
    fade.timing(1.0, motion::FAST, Easing::EaseOut);
    grow.timing(
        1.0,
        motion::FAST,
        Easing::Bezier(
            motion::SETTLE.0,
            motion::SETTLE.1,
            motion::SETTLE.2,
            motion::SETTLE.3,
        ),
    );
    press_surface(Press::Chip, remove)
        .accessibility_label(format!("Remove {name}"))
        .bind_opacity(&fade)
        .bind_scale(&grow)
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(if is_desktop() { 30.0 } else { 36.0 }))
        .pl(px(3.0))
        .pr_3()
        .rounded_full()
        .bg(move || c::secondary())
        .child(avatar(
            name.clone(),
            id,
            if is_desktop() { 24.0 } else { 30.0 },
        ))
        .child(
            text(first_name(&name))
                .text_size(px(font::SMALL + 1.0))
                .font_weight(FontWeight::MEDIUM),
        )
        .child(icon(Icon::Close, 14.0).text_color(move || c::muted_fg()))
}

/// Step 2: name, photo, disappearing messages and who may add and join.
fn details_page(base: usize, community: Option<ChatId>, picked: Signal<Vec<ContactId>>) -> Screen {
    let s = store();
    let name = signal(String::new());
    let timer = signal(s.settings.with_untracked(|x| x.default_timer));
    let members_add = signal(false);
    let approve = signal(true);
    let created = signal(None::<ChatId>);
    let share_open = signal(false);

    let create = move || {
        if let Some(id) = created.get_untracked() {
            finish(base, id);
            return;
        }
        let title = name.get_untracked().trim().to_string();
        if title.is_empty() {
            return;
        }
        let id = s.create_group(title, picked.get_untracked());
        let (add, appr) = (members_add.get_untracked(), approve.get_untracked());
        s.set_group_permissions(id, |p, _| {
            p.members_add = add;
            p.approve_joins = appr;
        });
        if let Some(t) = timer.get_untracked() {
            s.set_disappearing(id, Some(t));
        }
        if let Some(cm) = community {
            s.link_group(cm, id);
        }
        created.set(Some(id));
        share_open.set(true);
    };

    let n = picked.with_untracked(Vec::len);
    let members: Vec<AnyElement> = picked
        .get_untracked()
        .into_iter()
        .map(|id| {
            let c = s.contact(id);
            Row::new(s.contact_name(Some(id)))
                .leading(avatar(s.contact_name(Some(id)), id, 36.0))
                .subtitle(c.map(|c| handle(&c.username)).unwrap_or_default())
                .build()
        })
        .collect();

    page(
        "New group",
        view()
            .flex_1()
            .child(page_scroll(
                view()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_4()
                            .px_4()
                            .pt_5()
                            .child(
                                press_surface(Press::Tile, || {
                                    toast("Photos come with the media picker")
                                })
                                .accessibility_label("Add group photo")
                                .size(px(64.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .bg(move || c::secondary())
                                .text_color(move || c::muted_fg())
                                .child(icon(Icon::Camera, 26.0)),
                            )
                            .child(
                                view().flex_1().child(labelled_field(
                                    "Group name",
                                    field("Group name", name)
                                        .max_length(100_u32)
                                        .auto_focus(true),
                                    move || format!("{} of 100", name.with(|n| n.chars().count())),
                                    || false,
                                )),
                            ),
                    )
                    .child(section_title("Settings"))
                    // Rows without icons: rules start on the 16pt gutter.
                    .child(card_rows(
                        [
                            view()
                                .px_4()
                                .py_3()
                                .gap_2()
                                .child(text("Disappearing messages").text_size(px(font::body())))
                                .child(segmented(
                                    vec![
                                        (None, "Off"),
                                        (Some(Disappear::Day), "24 hours"),
                                        (Some(Disappear::Week), "7 days"),
                                        (Some(Disappear::FourWeeks), "4 weeks"),
                                    ],
                                    move || timer.get(),
                                    move |v| timer.set(v),
                                ))
                                .into_element(),
                            toggle_row(
                                "Members can add others",
                                Some("When off, only admins can add people."),
                                move || members_add.get(),
                                move |v| members_add.set(v),
                            ),
                            toggle_row(
                                "Approve new members",
                                Some("An admin approves anyone who joins with the invite link."),
                                move || approve.get(),
                                move |v| approve.set(v),
                            ),
                        ],
                        16.0,
                    ))
                    .child(footnote(
                        "You'll be the owner. You can change these later in Group permissions.",
                    ))
                    .child(section_title(if n == 1 {
                        "1 member".to_string()
                    } else {
                        format!("{n} members")
                    }))
                    .child(card_rows(members, 16.0 + 36.0 + 12.0)),
            ))
            // Hidden while the share sheet (with its own Open group) is up,
            // so there is one Open group on screen at a time.
            .child(
                bottom_bar(
                    btn(
                        move || {
                            if created.get().is_some() {
                                "Open group"
                            } else {
                                "Create group"
                            }
                        },
                        Variant::Primary,
                        BtnSize::Lg,
                        create,
                    )
                    .disabled(move || {
                        created.get().is_none() && name.with(|n| n.trim().is_empty())
                    }),
                )
                .style_when(
                    move || share_open.get(),
                    |st| st.opacity(0.0).pointer_events(PointerEvents::None),
                ),
            )
            .child(app_sheet(
                share_open,
                "Share invite link",
                share_sheet(s, created, base),
            )),
    )
}

/// After creating: the link, Copy, Share, a code to scan, then the group.
fn share_sheet(s: Store, created: Signal<Option<ChatId>>, base: usize) -> View {
    let link = move || {
        created
            .get()
            .and_then(|id| s.chat(id))
            .map(|c| c.invite_link)
            .unwrap_or_default()
    };
    let title = move || {
        created
            .get()
            .and_then(|id| s.chat(id))
            .map(|c| c.title)
            .unwrap_or_default()
    };
    view()
        .px_4()
        .pb_2()
        .gap_4()
        .child(muted(move || {
            format!(
                "Anyone with this link can ask to join “{}”. You approve who gets in.",
                title()
            )
        }))
        .child(view().items_center().child(qr_code(link, 5.0)))
        .child(link_box(link))
        .child(
            view()
                .flex_row()
                .gap_2()
                .child(
                    btn_with_icon(
                        "Copy link",
                        Some(Icon::Copy),
                        Variant::Secondary,
                        BtnSize::Md,
                        move || {
                            set_clipboard(link());
                            toast("Link copied");
                        },
                    )
                    .flex_1(),
                )
                .child(
                    btn_with_icon(
                        "Share…",
                        Some(Icon::Share),
                        Variant::Secondary,
                        BtnSize::Md,
                        // The platform's share sheet (copied where there is none).
                        move || share_text(link()),
                    )
                    .flex_1(),
                ),
        )
        .child(btn(
            "Open group",
            Variant::Primary,
            BtnSize::Lg,
            move || {
                if let Some(id) = created.get_untracked() {
                    finish(base, id);
                }
            },
        ))
}

/// The link, selectable, in a quiet box.
pub(crate) fn link_box(link: impl Fn() -> String + 'static) -> View {
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
            text(link)
                .flex_1()
                .selectable(true)
                .number_of_lines(1)
                .text_size(px(font::SMALL + 1.0))
                .text_color(move || c::link()),
        )
}

/// Leaves the flow and opens the new group.
fn finish(base: usize, id: ChatId) {
    // Open first (it reads the store while this page still exists), then
    // drop the flow's pages under whatever it pushed.
    crate::features::chat::open(id);
    nav::routes().update(|r| {
        let top = (r.0.len() > base + 2).then(|| r.0.pop()).flatten();
        r.0.truncate(base);
        r.0.extend(top);
    });
}

/// A new space: a name and a description, then its home.
/// (`new_community` keeps the store's name for it.)
pub fn new_community() {
    let base = nav::depth();
    nav::push("new-community", move || {
        let s = store();
        let name = signal(String::new());
        let about = signal(String::new());
        let create = move || {
            let title = name.get_untracked().trim().to_string();
            if title.is_empty() {
                return;
            }
            let id = s.create_community(title, about.get_untracked().trim().to_string());
            nav::truncate(base);
            community_home(id);
        };
        page(
            "New space",
            view()
                .flex_1()
                .child(page_scroll(
                    view()
                        .px_4()
                        .pt_5()
                        .gap_4()
                        .child(muted("Bring related groups together, like a school's classes or a building's neighbours. Everyone in the space gets your announcements."))
                        .child(labelled_field(
                            "Space name",
                            field("Space name", name).max_length(100_u32).auto_focus(true),
                            "Members see this name.",
                            || false,
                        ))
                        .child(labelled_field(
                            "Description",
                            field("What's it for?", about).multiline(true).h(px(96.0)).py_2(),
                            "Optional. Shown on the space's home.",
                            || false,
                        ))
                        .child(banner(
                            Tone::Secure,
                            Icon::Lock,
                            "The space's name and description are end-to-end encrypted. Songbird's servers only see which groups belong together.",
                        )),
                ))
                .child(bottom_bar(
                    btn("Create space", Variant::Primary, BtnSize::Lg, create)
                        .disabled(move || name.with(|n| n.trim().is_empty())),
                )),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, username: &str) -> Contact {
        Contact {
            id: 1,
            name: name.into(),
            username: username.into(),
            about: String::new(),
            status_line: None,
            presence: crate::store::Presence::Online,
            verified: false,
            key_changed: false,
            safety_number: String::new(),
            blocked: false,
            nudges_off: false,
        }
    }

    #[test]
    fn search_matches_name_or_username() {
        let c = contact("Ioana Ștefănescu", "ioana.st");
        assert!(matches(&c, ""));
        assert!(matches(&c, "ștef"));
        assert!(matches(&c, "IOANA"));
        assert!(matches(&c, "ioana.st"));
        assert!(!matches(&c, "mihai"));
    }
}
