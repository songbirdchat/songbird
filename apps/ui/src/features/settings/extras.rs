//! The rest of the You list: chat lists, broadcast lists, starred
//! messages, presence and nudges, parental controls, the proxy.

use nsh_core::prelude::*;

use super::{PRESENCES, presence_name, presence_of, section, section_plain};
use crate::{
    backend::supervision as sup,
    features,
    store::{Body, ChildContact, ContactId, Message, proxy_host, store, when_label},
    ui::*,
};

// ---------------------------------------------------------------- lists

pub fn lists() {
    super::push("Lists", || {
        let s = store();
        let ext = s.ext();
        let draft = signal(String::new());
        let renaming = signal(None::<String>);
        let rename_open = signal(false);
        let rename_to = signal(String::new());
        effect(move || {
            let r = renaming.get();
            untracked(|| {
                if let Some(name) = r {
                    rename_to.set(name);
                    rename_open.set(true);
                }
            });
        });
        let add = move || {
            if s.create_list(draft.get_untracked()) {
                draft.set(String::new());
            } else {
                toast("That list already exists");
            }
        };
        view()
            .child(super::foot("Your lists sit beside All at the top of Chats. Add a chat to a list from its menu.").pt_4())
            .child(view().pt_3().child(dynamic(move || {
                let names = ext.with(|x| x.lists.clone());
                let len = names.len();
                untracked(move || {
                    group(names.into_iter().enumerate().map(|(i, name)| {
                        let n = s.list_count(&name);
                        let (up, down, edit, del) = (name.clone(), name.clone(), name.clone(), name.clone());
                        Row::new(name.clone())
                            .icon(Icon::Filter)
                            .subtitle(match n {
                                1 => "1 chat".to_string(),
                                n => format!("{n} chats"),
                            })
                            .trailing(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .child(
                                        icon_btn(Icon::ChevronDown, format!("Move {name} up"), move || s.move_list(&up, -1))
                                            .rotate(deg(180.0))
                                            .text_color(move || c::muted_fg())
                                            .disabled(i == 0),
                                    )
                                    .child(
                                        icon_btn(Icon::ChevronDown, format!("Move {name} down"), move || s.move_list(&down, 1))
                                            .text_color(move || c::muted_fg())
                                            .disabled(i + 1 == len),
                                    )
                                    .child(
                                        icon_btn(Icon::Edit, format!("Rename {name}"), move || renaming.set(Some(edit.clone())))
                                            .text_color(move || c::muted_fg()),
                                    )
                                    .child(icon_btn(Icon::Trash, format!("Delete {name}"), move || {
                                        let del = del.clone();
                                        confirm(
                                            format!("Delete the list \"{del}\"?"),
                                            "The chats stay in Chats. Only the list goes.",
                                            "Delete",
                                            move || s.delete_list(&del),
                                        )
                                    })
                                    .text_color(move || c::destructive())),
                            )
                            .build()
                    }))
                })
            })))
            .child(when(
                move || ext.with(|x| x.lists.is_empty()),
                || empty_state(Icon::Filter, "No lists yet", "Make one for Family, Work or anything you like.", None),
            ))
            .child(
                view()
                    .px_4()
                    .pt_4()
                    .flex_row()
                    .gap_2()
                    .child(field("New list name", draft).flex_1().max_length(24).on_submit(move |_: &str| add()))
                    .child(btn("Create", Variant::Primary, BtnSize::Md, add).disabled(move || draft.with(|d| d.trim().is_empty()))),
            )
            .child(app_sheet(
                rename_open,
                "Rename list",
                view()
                    .px_4()
                    .gap_3()
                    .child(field("List name", rename_to).max_length(24))
                    .child(btn("Save", Variant::Primary, BtnSize::Lg, move || {
                        if let Some(from) = renaming.get_untracked() {
                            s.rename_list(&from, rename_to.get_untracked());
                        }
                        rename_open.set(false);
                        renaming.set(None);
                    })),
            ))
            .into_element()
    });
}

// ---------------------------------------------------------------- broadcasts

pub fn broadcasts() {
    super::push("Broadcast lists", || {
        let s = store();
        let ext = s.ext();
        let creating = signal(false);
        let name = signal(String::new());
        let chosen = signal(Vec::<ContactId>::new());
        let sending = signal(None::<u64>);
        let send_open = signal(false);
        let draft = signal(String::new());
        let people = s.contacts.with_untracked(|cs| {
            cs.iter()
                .filter(|c| {
                    !c.blocked && (crate::store::demo() || crate::backend::chat::owns(c.id))
                })
                .map(|c| (c.id, c.name.clone()))
                .collect::<Vec<_>>()
        });
        view()
            .child(view().px_4().pt_4().child(banner(
                Tone::Info,
                Icon::Megaphone,
                "One message, sent to each person separately, as if you wrote to them. Replies come to your chat with them. Only people who saved you as a contact get it.",
            )))
            .child(view().pt_4().child(dynamic(move || {
                let list = ext.with(|x| x.broadcasts.clone());
                untracked(move || {
                    group(list.into_iter().map(|b| {
                        let names: Vec<String> = b.members.iter().map(|m| s.contact_name(Some(*m))).collect();
                        let id = b.id;
                        Row::new(b.name.clone())
                            .icon(Icon::Megaphone)
                            .subtitle(names.join(", "))
                            .on_press(move || {
                                sending.set(Some(id));
                                send_open.set(true);
                            })
                            .trailing(
                                icon_btn(Icon::Trash, format!("Delete {}", b.name), move || s.delete_broadcast(id))
                                    .text_color(move || c::destructive()),
                            )
                            .build()
                    }))
                    .style_when(move || ext.with(|x| x.broadcasts.is_empty()), |st| st.hidden())
                })
            })))
            .child(view().px_4().pt_4().child(btn_with_icon(
                "New broadcast list",
                Some(Icon::Plus),
                Variant::Secondary,
                BtnSize::Md,
                move || creating.set(true),
            )))
            .child(app_sheet(
                send_open,
                "Send to the list",
                view()
                    .gap_3()
                    .px_4()
                    .pb_4()
                    .child(field("Message", draft).max_length(4_000))
                    .child(
                        btn("Send to each person", Variant::Primary, BtnSize::Lg, move || {
                            let Some(id) = sending.get_untracked() else { return };
                            let text = draft.get_untracked().trim().to_string();
                            let (sent, skipped) = s.send_broadcast(id, &text);
                            draft.set(String::new());
                            sending.set(None);
                            send_open.set(false);
                            toast(match (sent, skipped) {
                                (n, 0) => format!("Sent to {n} people"),
                                (n, k) => format!("Sent to {n}; {k} haven't saved you as a contact"),
                            });
                        })
                        .disabled(move || draft.with(|d| d.trim().is_empty())),
                    ),
            ))
            .child(app_sheet(
                creating,
                "New broadcast list",
                view()
                    .gap_3()
                    .child(view().px_4().child(field("List name", name).max_length(30)))
                    .child(
                        scroll().max_h(px(300.0)).child(group_plain(people.into_iter().map(move |(id, n)| {
                            toggle_row(n, None, move || chosen.with(|c| c.contains(&id)), move |on| {
                                chosen.update(|c| {
                                    c.retain(|x| *x != id);
                                    if on {
                                        c.push(id);
                                    }
                                })
                            })
                        }))),
                    )
                    .child(
                        view().px_4().child(
                            btn(
                                move || format!("Create with {} people", chosen.with(Vec::len)),
                                Variant::Primary,
                                BtnSize::Lg,
                                move || {
                                    s.create_broadcast(name.get_untracked().trim().to_string(), chosen.get_untracked());
                                    name.set(String::new());
                                    chosen.set(Vec::new());
                                    creating.set(false);
                                },
                            )
                            .disabled(move || name.with(|n| n.trim().is_empty()) || chosen.with(|c| c.len() < 2)),
                        ),
                    ),
            ))
            .into_element()
    });
}

// ---------------------------------------------------------------- starred

/// Starred messages grouped by chat, newest chat first.
pub fn starred_by_chat(all: &[(u64, String, Vec<Message>)]) -> Vec<(u64, String, Vec<Message>)> {
    let mut out: Vec<_> = all
        .iter()
        .map(|(id, title, msgs)| {
            (
                *id,
                title.clone(),
                msgs.iter()
                    .filter(|m| m.starred)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, _, m)| !m.is_empty())
        .collect();
    out.sort_by_key(|(_, _, m)| {
        std::cmp::Reverse(m.iter().map(|x| x.at).max().unwrap_or(i64::MIN))
    });
    out
}

pub fn starred() {
    super::push("Starred messages", || {
        let s = store();
        let groups = computed(move || {
            let chats = s.chats.with(|cs| {
                cs.iter()
                    .map(|c| (c.id, c.title.clone()))
                    .collect::<Vec<_>>()
            });
            let all: Vec<_> = chats
                .into_iter()
                .map(|(id, t)| (id, t, s.messages_of(id)))
                .collect();
            starred_by_chat(&all)
        });
        view()
            .child(when(
                move || groups.with(|g| g.is_empty()),
                || {
                    empty_state(
                        Icon::Star,
                        "No starred messages",
                        "Open a message's menu and choose Star to keep it here.",
                        None,
                    )
                },
            ))
            .child(dynamic(move || {
                let list = groups.get();
                untracked(move || {
                    view().children(list.into_iter().map(|(chat, title, msgs)| {
                        view().child(section_title(title)).child(group_with_inset(
                            msgs.into_iter().map(move |m| {
                                let who = s.contact_name(m.from);
                                let preview = match &m.body {
                                    Body::Text(t) => t.clone(),
                                    b => b.preview(),
                                };
                                Row::new(preview)
                                    .subtitle(format!("{who} · {}", when_label(m.at)))
                                    .leading(
                                        icon(Icon::Star, 16.0).text_color(move || c::primary()),
                                    )
                                    .chevron()
                                    .on_press(move || features::chat::open(chat))
                                    .build()
                                // Under the titles: the gutter, the 16pt star and the gap.
                            }),
                            INSET_PLAIN + 16.0 + 12.0,
                        ))
                    }))
                })
            }))
            .into_element()
    });
}

// ---------------------------------------------------------------- presence

pub fn presence() {
    super::push("Presence and nudges", || {
        let s = store();
        let me = s.me;
        let settings = s.settings;
        let picking = signal(false);
        let off = computed(move || {
            s.contacts.with(|cs| {
                cs.iter()
                    .filter(|c| c.nudges_off)
                    .map(|c| (c.id, c.name.clone()))
                    .collect::<Vec<_>>()
            })
        });
        view()
            .child(super::section_plain(
                "My presence",
                PRESENCES
                    .iter()
                    .map(|(label, line)| {
                        let label: &'static str = label;
                        let line: &'static str = line;
                        radio_row(label, Some(line), move || me.with(|p| presence_name(&p.presence) == label), move || {
                            s.set_presence(presence_of(label))
                        })
                    })
                    .collect(),
            ))
            .child(super::foot("Presence is sent encrypted, only to the contacts your Last seen setting allows. Songbird's servers never show anyone that you're online."))
            .child(super::section_plain(
                "Nudges",
                vec![toggle_row(
                    "Allow nudges",
                    Some("A nudge shakes the chat window to get someone's attention. Off means nobody can nudge you."),
                    move || settings.with(|x| x.nudges),
                    move |v| s.settings_update(|x| x.nudges = v),
                )],
            ))
            .child(when(
                move || settings.with(|x| x.nudges),
                move || {
                    view()
                        .child(section_title("Nudges off for"))
                        .child(dynamic(move || {
                            let list = off.get();
                            untracked(move || {
                                if list.is_empty() {
                                    super::foot("No one. Everyone in your contacts can nudge you.").pt_0().into_element()
                                } else {
                                    group_with_inset(list.into_iter().map(|(id, name)| {
                                        Row::new(name.clone())
                                            .leading(avatar(name, id, 36.0))
                                            .trailing(btn("Allow", Variant::Outline, BtnSize::Sm, move || s.set_nudges_off(id, false)))
                                            .build()
                                    }), INSET_AVATAR)
                                    .into_element()
                                }
                            })
                        }))
                        .child(view().px_4().pt_3().child(btn_with_icon(
                            "Turn off for someone",
                            Some(Icon::BellOff),
                            Variant::Secondary,
                            BtnSize::Md,
                            move || picking.set(true),
                        )))
                },
            ))
            .child(nudge_sheet(picking))
            .into_element()
    });
}

fn nudge_sheet(open: Signal<bool>) -> nsh_base::SheetElement {
    let s = store();
    let people = computed(move || {
        s.contacts.with(|cs| {
            cs.iter()
                .filter(|c| !c.nudges_off && !c.blocked)
                .map(|c| (c.id, c.name.clone()))
                .collect::<Vec<_>>()
        })
    });
    app_sheet(
        open,
        "Turn off nudges from",
        scroll()
            .max_h(px(420.0))
            .child(view().pb_2().child(dynamic(move || {
                let list = people.get();
                untracked(move || {
                    group_with_inset(
                        list.into_iter().map(|(id, name)| {
                            Row::new(name.clone())
                                .leading(avatar(name, id, 36.0))
                                .on_press(move || {
                                    s.set_nudges_off(id, true);
                                    open.set(false);
                                })
                                .build()
                        }),
                        INSET_AVATAR,
                    )
                })
            }))),
    )
}

// ---------------------------------------------------------------- parental controls

pub fn parental() {
    super::push("Parental controls", || {
        let s = store();
        let settings = s.settings;
        let ext = s.ext();
        let linked = move || settings.with(|x| x.parental_controls);
        view()
            .child(view().items_center().px_6().pt_6().gap_3().child(hero_icon(Icon::Users)).child(
                body("Help a child use Songbird safely. You choose who can reach them; their messages stay theirs.")
                    .text_center()
                    .max_w(px(420.0)),
            ))
            .child(view().px_4().pt_4().child(banner(
                Tone::Secure,
                Icon::Lock,
                "You'll never see their messages, calls or contacts. Rules only decide who can reach them.",
            )))
            .child(when(move || !linked(), || {
                section(
                    "Get started",
                    vec![
                        Row::new("Supervise a child's account")
                            .icon(Icon::QrCode)
                            .subtitle("Link in person: scan the QR code on their phone.")
                            .chevron()
                            .on_press(link_child)
                            .build(),
                        Row::new("I'm a child")
                            .icon(Icon::User)
                            .subtitle("Link with your parent's phone, side by side.")
                            .chevron()
                            .on_press(child_link)
                            .build(),
                    ],
                )
            }))
            .child(when(linked, move || {
                section(
                    "Supervising",
                    vec![
                        Row::new(ext.with_untracked(|x| x.parental.child_name.clone()))
                            .leading(avatar(ext.with_untracked(|x| x.parental.child_name.clone()), 7, 36.0))
                            .subtitle("Linked in person · rules on")
                            .chevron()
                            .on_press(rules)
                            .build(),
                    ],
                )
            }))
            .child(section(
                "See it from their side",
                vec![Row::new("Preview what your child sees").icon(Icon::Eye).chevron().on_press(child_view).build()],
            ))
            .child(super::foot("A child can't remove these rules alone, but can always delete their own account. The rules end when you unlink, or by themselves when your child reaches the age of digital consent in your country (13 to 16 in the EU)."))
            .into_element()
    });
}

/// Linking in person (PROTOCOL §10.6): this device makes a policy key and
/// a link secret just for this child and shows them as a code to scan;
/// both screens then show the same six digits, which parent and child
/// compare. The child's age (entered on the child's phone) and the
/// country's age of digital consent set the day supervision ends.
fn link_child() {
    if crate::backend::is_live() && crate::backend::registered() {
        return link_child_live();
    }
    // The preview: both sides on this device (no server, no second phone).
    super::push("Link a child's account", || {
        let s = store();
        // The country for the age of digital consent: the device's region.
        let country = locale().region().unwrap_or("EU").to_ascii_uppercase();
        let age_text = signal("12".to_string());
        let age =
            move || age_text.with(|t| t.trim().parse::<u8>().ok().filter(|a| (6..=17).contains(a)));
        let link = signal(s.start_parental_link(12, &country));
        let c2 = country.clone();
        effect(move || {
            if let Some(a) = age() {
                let l = untracked(|| s.start_parental_link(a, &c2));
                link.set(l);
            }
        });
        let consent = songbird_content::supervision::consent_age(&country);
        let code = move || {
            let c = link.with(sup::code);
            format!("{} {}", &c[..3.min(c.len())], &c[3.min(c.len())..])
        };
        let ends = move || {
            link.with(|l| {
                if l.unlink_at_s == 0 {
                    String::new()
                } else {
                    sup::date_label(l.unlink_at_s)
                }
            })
        };
        view()
            .items_center()
            .px_6()
            .pt_6()
            .gap_4()
            .child(dynamic(move || {
                let payload = link.with(sup::qr_payload);
                untracked(move || {
                    view()
                        .role(AccessibilityRole::Image)
                        .label("Linking code for your child's phone to scan")
                        .child(qr_code(&payload, 168.0))
                })
            }))
            .child(
                view()
                    .gap_3()
                    .max_w(px(420.0))
                    .child(step(1, "On your child's phone, open Songbird → You → Parental controls → I'm a child."))
                    .child(step(2, "Scan this code with their phone, side by side."))
                    .child(step(3, "Check that both screens show the same six digits.")),
            )
            .child(
                view()
                    .items_center()
                    .gap_1()
                    .child(muted("The code on both screens"))
                    .child(numeric_code(code).label(move || format!("Code {}", link.with(sup::code)))),
            )
            .child(
                view().w_full().max_w(px(420.0)).child(labelled_field(
                    "Their age (they enter it on their phone)",
                    field("Age", age_text),
                    move || match age() {
                        Some(_) if !ends().is_empty() => format!(
                            "Supervision ends by itself on {}, the month after they turn {consent}. Their birth date stays on their phone.",
                            ends()
                        ),
                        Some(_) => format!("Supervision ends when they turn {consent}."),
                        None => "Enter an age from 6 to 17.".into(),
                    },
                    move || age().is_none(),
                )),
            )
            .child(
                btn("We both see the same code", Variant::Primary, BtnSize::Lg, move || {
                    if age().is_none() {
                        toast("Enter their age first");
                        return;
                    }
                    s.confirm_parental_link();
                    crate::nav::pop();
                    rules();
                })
                .disabled(move || age().is_none()),
            )
            .child(muted("Linking only works in person, so nobody can take over a child's account remotely. Songbird's servers never learn who the parent is.").text_center().max_w(px(420.0)))
            .child(view().h(px(24.0)))
            .into_element()
    });
}

/// The parent's side on the real backend (PROTOCOL §10.6, D-0107, D-0108):
/// show a commitment, scan the child's code, then show the signed first
/// policy with the secret and the six digits to compare. Nothing passes
/// through the server between the two accounts.
fn link_child_live() {
    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        ShowCode,
        Scan,
        Compare([u8; 16]),
    }
    super::push("Link a child's account", || {
        let s = store();
        // A link in force is never replaced (its key is the only way to reach that child).
        if sup::parent_link().is_some_and(|l| {
            l.version >= 2 || (l.version == 1 && s.settings.with_untracked(|x| x.parental_controls))
        }) {
            return view()
                .px_4()
                .pt_4()
                .child(banner(
                    Tone::Info,
                    Icon::Users,
                    "You already supervise a child from this phone. Unlink them first to link another.",
                ))
                .into_element();
        }
        let link = sup::start_link([0; 16], 0);
        let payload = sup::qr_payload(&link);
        let phase = signal(Phase::ShowCode);
        let policy = signal(String::new());
        let failed = signal(None::<String>);
        let show_code = move || {
            view()
                .items_center()
                .gap_4()
                .child(
                    view()
                        .role(AccessibilityRole::Image)
                        .label("Linking code for your child's phone to scan")
                        .child(qr_code(&payload, 168.0)),
                )
                .child(
                    view()
                        .gap_3()
                        .max_w(px(420.0))
                        .child(step(1, "On your child's phone, open Songbird → You → Parental controls → I'm a child."))
                        .child(step(2, "Let their phone scan this code, then enter their age there."))
                        .child(step(3, "Their phone shows a code: scan it with this one.")),
                )
                .child(btn("Scan your child's code", Variant::Primary, BtnSize::Lg, move || {
                    phase.set(Phase::Scan);
                }))
                .into_element()
        };
        let scan = move || {
            let session = crate::camera::use_session(
                songbird_camera::Purpose::ScanCode,
                move || phase.get() == Phase::Scan,
                move |text| {
                    if let Some(features::devices::codes::Scanned::Child {
                        account,
                        unlink_at_s,
                    }) = features::devices::codes::parse(&text)
                    {
                        let rules = s.ext().with_untracked(|x| x.parental.clone());
                        match sup::first_policy(&rules, account, unlink_at_s) {
                            Ok(code) => {
                                failed.set(None);
                                policy.set(code);
                                phase.set(Phase::Compare(account));
                            }
                            Err(p) => failed.set(Some(p.to_string())),
                        }
                    }
                },
            );
            view()
                .items_center()
                .gap_4()
                .child(when(
                    move || failed.get().is_some(),
                    move || {
                        status_line(
                            || StatusTone::Problem,
                            || Icon::Alert,
                            move || failed.get().unwrap_or_default().into(),
                        )
                    },
                ))
                .child(crate::camera::camera_view(
                    &session,
                    280.0,
                    "Point at the code on your child's phone",
                ))
                .child(btn("Back", Variant::Ghost, BtnSize::Md, move || {
                    phase.set(Phase::ShowCode)
                }))
                .into_element()
        };
        let compare = move |account: [u8; 16]| {
            let digits = sup::parent_link()
                .map(|l| sup::digits_for(&l, &account))
                .unwrap_or_default();
            let spaced = format!(
                "{} {}",
                &digits[..3.min(digits.len())],
                &digits[3.min(digits.len())..]
            );
            view()
                .items_center()
                .gap_4()
                .child(
                    view()
                        .role(AccessibilityRole::Image)
                        .label("Your rules, for your child's phone to scan")
                        .child(dynamic(move || {
                            let code = policy.get();
                            untracked(move || qr_code(&code, 220.0))
                        })),
                )
                .child(
                    muted("Let your child's phone scan this. Then both phones show six digits:")
                        .text_center()
                        .max_w(px(420.0)),
                )
                .child(numeric_code(spaced).label(format!("Code {digits}")))
                .child(btn(
                    "We both see the same code",
                    Variant::Primary,
                    BtnSize::Lg,
                    move || {
                        s.settings_update(|x| x.parental_controls = true);
                        crate::nav::pop();
                        rules();
                        toast("Linked. Your child's phone applies the rules.");
                    },
                ))
                .child(btn(
                    "They don't match",
                    Variant::Ghost,
                    BtnSize::Md,
                    move || {
                        sup::abandon_first();
                        crate::nav::pop();
                        toast("Not linked. Start again side by side.");
                    },
                ))
                .into_element()
        };
        view()
            .items_center()
            .px_6()
            .pt_6()
            .gap_4()
            .child(dynamic(move || {
                let now = phase.get();
                let show_code = show_code.clone();
                untracked(move || match now {
                    Phase::ShowCode => show_code(),
                    Phase::Scan => scan(),
                    Phase::Compare(account) => compare(account),
                })
            }))
            .child(muted("Linking only works in person, so nobody can take over a child's account remotely. Songbird's servers never learn who the parent is.").text_center().max_w(px(420.0)))
            .child(view().h(px(24.0)))
            .into_element()
    });
}

/// The child's side (PROTOCOL §10.6, D-0107, D-0108): scan the parent's
/// first code, enter an age, show this phone's code, scan the parent's
/// rules, compare six digits, then install the rules.
fn child_link() {
    #[derive(Clone, Copy, PartialEq)]
    enum Phase {
        Scan,
        Age([u8; 32]),
        Show,
        ScanRules,
        Compare,
        Installing,
        Done,
    }
    super::push("Link with a parent", || {
        let live = crate::backend::is_live() && crate::backend::registered();
        if !live {
            return view()
                .px_4()
                .pt_4()
                .child(banner(
                    Tone::Info,
                    Icon::Server,
                    "Linking with a parent needs Songbird's server. Turn on Server connection on both phones first.",
                ))
                .into_element();
        }
        let phase = signal(Phase::Scan);
        let age_text = signal(String::new());
        let linking = signal(None::<sup::ChildLinking>);
        let revealed = signal(None::<sup::Revealed>);
        let failed = signal(None::<String>);
        let rules = signal(Vec::<String>::new());
        let country = locale().region().unwrap_or("EU").to_ascii_uppercase();
        let age =
            move || age_text.with(|t| t.trim().parse::<u8>().ok().filter(|a| (6..=17).contains(a)));
        let start = move |commitment: [u8; 32], age: u8| match sup::start_child(
            commitment,
            age,
            country.clone(),
        ) {
            Ok(c) => {
                failed.set(None);
                linking.set(Some(c));
                phase.set(Phase::Show);
            }
            Err(p) => failed.set(Some(p.to_string())),
        };
        let start = std::rc::Rc::new(start);
        let install = move || {
            let (Some(c), Some(r)) = (linking.get_untracked(), revealed.get_untracked()) else {
                return;
            };
            phase.set(Phase::Installing);
            spawn(async move {
                match sup::finish_child(c, r).await {
                    Ok(p) => {
                        let secret = sup::child_link().map(|c| c.secret);
                        let _ = rules.try_set(sup::rules_in_force(&p, secret.as_ref()));
                        let _ = failed.try_set(None);
                        let _ = phase.try_set(Phase::Done);
                    }
                    Err(p) => {
                        let _ = failed.try_set(Some(p.to_string()));
                        let _ = phase.try_set(Phase::Compare);
                    }
                }
            });
        };
        let problem = move || {
            when(
                move || failed.get().is_some(),
                move || {
                    status_line(
                        || StatusTone::Problem,
                        || Icon::Alert,
                        move || failed.get().unwrap_or_default().into(),
                    )
                },
            )
        };
        view()
            .items_center()
            .px_6()
            .pt_6()
            .gap_4()
            .child(dynamic(move || {
                let now = phase.get();
                let start = start.clone();
                untracked(move || match now {
                    Phase::Scan => {
                        let session = crate::camera::use_session(
                            songbird_camera::Purpose::ScanCode,
                            move || phase.get() == Phase::Scan,
                            move |text| {
                                if let Some(features::devices::codes::Scanned::Supervise(commitment)) =
                                    features::devices::codes::parse(&text)
                                {
                                    phase.set(Phase::Age(commitment));
                                }
                            },
                        );
                        view()
                            .items_center()
                            .gap_4()
                            .child(flow_lead("Hold this phone next to your parent's and scan the code on it.").text_center())
                            .child(crate::camera::camera_view(&session, 280.0, "Point at the code on your parent's phone"))
                            .into_element()
                    }
                    Phase::Age(commitment) => view()
                        .w_full()
                        .max_w(px(420.0))
                        .gap_4()
                        .child(labelled_field(
                            "Your age",
                            field("Age", age_text),
                            "It sets the day supervision ends by itself. It stays on this phone.",
                            move || age_text.with(|t| !t.trim().is_empty()) && age().is_none(),
                        ))
                        .child(problem())
                        .child(
                            btn("Show my code", Variant::Primary, BtnSize::Lg, move || {
                                if let Some(a) = age() {
                                    start(commitment, a);
                                }
                            })
                            .disabled(move || age().is_none()),
                        )
                        .into_element(),
                    Phase::Show => {
                        let Some(c) = linking.get_untracked() else {
                            return view().into_element();
                        };
                        view()
                            .items_center()
                            .gap_3()
                            .child(
                                view()
                                    .role(AccessibilityRole::Image)
                                    .label("Your code for your parent's phone to scan")
                                    .child(qr_code(&c.code(), 168.0)),
                            )
                            .child(muted("Let your parent's phone scan this. It then shows your rules as a code.").text_center().max_w(px(420.0)))
                            .child(btn("Scan the rules", Variant::Primary, BtnSize::Lg, move || phase.set(Phase::ScanRules)))
                            .into_element()
                    }
                    Phase::ScanRules => {
                        let session = crate::camera::use_session(
                            songbird_camera::Purpose::ScanCode,
                            move || phase.get() == Phase::ScanRules,
                            move |text| {
                                let Some(features::devices::codes::Scanned::Policy { secret, policy }) =
                                    features::devices::codes::parse(&text)
                                else {
                                    return;
                                };
                                let Some(c) = linking.get_untracked() else {
                                    return;
                                };
                                match sup::reveal(&c, secret, &policy) {
                                    Ok(r) => {
                                        failed.set(None);
                                        revealed.set(Some(r));
                                        phase.set(Phase::Compare);
                                    }
                                    Err(p) => failed.set(Some(p.to_string())),
                                }
                            },
                        );
                        view()
                            .items_center()
                            .gap_4()
                            .child(problem())
                            .child(crate::camera::camera_view(&session, 280.0, "Point at the rules on your parent's phone"))
                            .child(btn("Back", Variant::Ghost, BtnSize::Md, move || phase.set(Phase::Show)))
                            .into_element()
                    }
                    Phase::Compare => {
                        let digits = revealed.with_untracked(|r| r.as_ref().map(|r| r.digits.clone())).unwrap_or_default();
                        let spaced = format!("{} {}", &digits[..3.min(digits.len())], &digits[3.min(digits.len())..]);
                        view()
                            .items_center()
                            .gap_4()
                            .child(muted("The code on both screens"))
                            .child(numeric_code(spaced).label(format!("Code {digits}")))
                            .child(muted("Check your parent's phone shows the same six digits.").text_center().max_w(px(420.0)))
                            .child(problem())
                            .child(btn("We both see the same code", Variant::Primary, BtnSize::Lg, install))
                            .child(btn("They don't match", Variant::Ghost, BtnSize::Md, move || {
                                revealed.set(None);
                                linking.set(None);
                                phase.set(Phase::Scan);
                                toast("Not linked. Start again side by side.");
                            }))
                            .into_element()
                    }
                    Phase::Installing => busy_bar("Applying the rules").into_element(),
                    Phase::Done => view()
                        .w_full()
                        .gap_3()
                        .child(banner(
                            Tone::Secure,
                            Icon::ShieldCheck,
                            "Linked. Your parent never sees your messages, calls, photos or contacts.",
                        ))
                        .child(section_title("Rules that apply to you"))
                        .child(dynamic(move || {
                            let list = rules.get();
                            untracked(move || group(list.into_iter().map(|r| Row::new(r).icon(Icon::Check).build())))
                        }))
                        .into_element(),
                })
            }))
            .child(view().h(px(24.0)))
            .into_element()
    });
}

fn step(n: u32, line: &'static str) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_3()
        .child(
            view()
                .size(px(26.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::accent())
                .child(
                    text(n.to_string())
                        .text_size(px(font::SMALL))
                        .font_weight(FontWeight::BOLD)
                        .text_color(move || c::primary()),
                ),
        )
        .child(body(line).flex_1())
}

/// The rules a parent sets. Each change is a new policy, signed on this
/// device and sent when the account is live; the child's app shows it.
fn rules() {
    super::push("Rules", || {
        let s = store();
        let ext = s.ext();
        let p =
            move |f: fn(&crate::store::ParentalRules) -> bool| move || ext.with(|x| f(&x.parental));
        let name = ext.with_untracked(|x| x.parental.child_name.clone());
        view()
            .child(view().px_4().pt_4().child(banner(
                Tone::Secure,
                Icon::Lock,
                "You'll never see their messages, calls or contacts. These rules are signed on this phone and shown to them in full.",
            )))
            .child(section_plain(
                "Who can message them",
                [
                    ChildContact::ApprovedOnly,
                    ChildContact::ContactsAndRequests,
                ]
                .into_iter()
                .map(|w| {
                    radio_row(
                        w.label(),
                        None,
                        move || ext.with(|x| x.parental.who_can_message == w),
                        move || s.set_parental_rules(|r| r.who_can_message = w),
                    )
                })
                .collect(),
            ))
            .child(section_plain(
                "Groups and calls",
                vec![
                    toggle_row(
                        "Group invites need my approval",
                        None,
                        p(|r| r.groups_need_approval),
                        move |v| s.set_parental_rules(|r| r.groups_need_approval = v),
                    ),
                    toggle_row(
                        "Block calls from people not in contacts",
                        None,
                        p(|r| r.block_unknown_calls),
                        move |v| s.set_parental_rules(|r| r.block_unknown_calls = v),
                    ),
                    toggle_row(
                        "Others can find them by username",
                        Some("Off: only people they add, or you approve, can start a chat."),
                        p(|r| r.discoverable),
                        move |v| s.set_parental_rules(|r| r.discoverable = v),
                    ),
                    toggle_row("Allow nudges", None, p(|r| r.nudges), move |v| {
                        s.set_parental_rules(|r| r.nudges = v)
                    }),
                ],
            ))
            .child(section_title("Quiet hours"))
            .child(view().px_4().child(segmented(
                vec![
                    (None, "Off"),
                    (Some("21:00–07:00"), "21:00–07:00"),
                    (Some("22:00–07:00"), "22:00–07:00"),
                ],
                move || ext.with(|x| x.parental.quiet_hours),
                move |q| s.set_parental_rules(|r| r.quiet_hours = q),
            )))
            .child(super::foot(
                "No notifications during quiet hours, except calls from you.",
            ))
            .child(super::foot(
                "Songbird's servers keep the rules they enforce, the date supervision ends and a key that isn't linked to your account. The other rules are encrypted for your two phones.",
            ))
            .child(view().pt_4().child(group(vec![
                    Row::new("Unlink")
                        .icon(Icon::LogOut)
                        .danger()
                        .on_press(move || {
                            confirm(
                                format!("Unlink {name}'s account?"),
                                format!("The rules stop at once. {name} is told, and keeps the settings to change."),
                                "Unlink",
                                move || {
                                    s.unlink_child();
                                    crate::nav::pop();
                                },
                            )
                        })
                        .build(),
                ])))
            .into_element()
    });
}

/// The child's side: every rule in force, read from the signed policy and
/// checked against the key their phone scanned. When the account is live
/// it is the server's copy (`GET /v1/supervision`); otherwise the policy
/// this device signed last (the preview a parent sees).
fn child_view() {
    super::push("Rules from your parent", || {
        let s = store();
        let ext = s.ext();
        // (rules, a problem) from the server when live, else locally.
        let fetched = resource(
            move || Some(crate::backend::is_live() && crate::backend::registered()),
            |live, _info| async move {
                if live {
                    sup::fetch().await.map_err(|p| p.0)
                } else {
                    Ok(None)
                }
            },
        );
        let lines = move || -> Vec<String> {
            let child = sup::child_link();
            let key = child.as_ref().map(|c| c.policy_key);
            let secret = child.as_ref().map(|c| c.secret);
            let server = fetched.get().flatten();
            let from_server = server.is_some();
            let policy = server.or_else(|| {
                let link = sup::parent_link()?;
                let rules = ext.with(|x| x.parental.clone());
                sup::sign_policy(&link, &rules, false, link.version.max(1))
            });
            if let Some(e) = fetched.error() {
                return vec![format!("The rules couldn't be checked just now. {e}")];
            }
            match policy {
                // The server's copy is checked only against a key this phone scanned itself
                // (D-0108): another device of the child can't check it.
                Some(_) if from_server && key.is_none() => vec!["This device didn't link with your parent, so it can't check these rules. Open them on the phone that did.".into()],
                Some(p) if sup::verify(&p, key.as_ref()) => sup::rules_in_force(&p, secret.as_ref()),
                Some(_) => vec!["These rules aren't signed by the parent you linked with, so none of them apply.".into()],
                None => vec!["No parent supervises this account.".into()],
            }
        };
        view()
            .child(view().px_4().pt_4().child(banner(
                Tone::Secure,
                Icon::ShieldCheck,
                "Your parent never sees your messages, calls, photos or contacts. They only set who can reach you.",
            )))
            .child(section_title("Rules that apply to you"))
            .child(dynamic(move || {
                let list = lines();
                untracked(move || group(list.into_iter().map(|r| Row::new(r).icon(Icon::Check).build())))
            }))
            .child(super::foot("Every rule your parent set is listed here; there are no hidden ones. Each was signed by the phone you linked with in person. You can still delete your own account."))
            .into_element()
    });
}

// ---------------------------------------------------------------- proxy

pub fn proxy() {
    super::push("Proxy", || {
        let s = store();
        let settings = s.settings;
        let link = signal(String::new());
        let current = move || settings.with(|x| x.proxy.clone());
        let parsed = move || link.with(|l| proxy_host(l));
        let invalid = move || link.with(|l| !l.trim().is_empty()) && parsed().is_none();
        view()
            .child(
                view().px_4().pt_4().child(
                    card()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        // The standard lavender tile; on or off is said in words.
                        .child(icon_tile(Icon::Globe))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(2.0))
                                .child(
                                    text(move || if current().is_some() { "Proxy on" } else { "Proxy off" })
                                        .text_size(px(font::body()))
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(muted(move || match current() {
                                    Some(h) => format!("Going through {h}"),
                                    None => "Connecting to Songbird directly".into(),
                                })),
                        )
                        .child(toggle(
                            "Use proxy",
                            move || current().is_some(),
                            move |on| {
                                if on {
                                    match parsed() {
                                        Some(h) => s.set_proxy(Some(h)),
                                        None => toast("Paste a proxy link first"),
                                    }
                                } else {
                                    s.set_proxy(None);
                                }
                            },
                        )),
                ),
            )
            .child(
                view().px_4().pt_4().gap_3().child(labelled_field(
                    "Proxy link",
                    field("https://songbird.chat/proxy#…", link)
                        .auto_capitalize(AutoCapitalize::None)
                        .keyboard_type(KeyboardType::Url),
                    move || {
                        if invalid() {
                            "That isn't a proxy link. Ask whoever runs the proxy for its link.".to_string()
                        } else if let Some(h) = parsed() {
                            format!("Proxy at {h}")
                        } else {
                            "Paste a link someone you trust shared with you.".to_string()
                        }
                    },
                    invalid,
                ))
                .child(view().flex_row().child(
                    // The connection starts again through the proxy (Prompt 9).
                    btn("Connect", Variant::Primary, BtnSize::Md, move || {
                        if let Some(h) = parsed() {
                            s.set_proxy(Some(h.clone()));
                            link.set(String::new());
                            toast(format!("Using the proxy {h}"));
                        }
                    })
                    .disabled(move || parsed().is_none()),
                )),
            )
            .child(section_title("What a proxy does"))
            .child(
                view()
                    .px_4()
                    .gap_2()
                    .child(muted("A proxy helps where Songbird is blocked. Your messages stay end\u{2011}to\u{2011}end encrypted through it."))
                    .child(muted("The proxy's operator sees your IP address and when you connect, never your messages or who you talk to."))
                    .child(muted("Songbird's servers see the proxy's address instead of yours.")),
            )
            .into_element()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::Body, ui::Tick};

    fn msg(id: u64, chat: u64, at: i64, starred: bool) -> Message {
        Message {
            id,
            chat,
            from: None,
            body: Body::Text(format!("m{id}")),
            at,
            status: Tick::Read,
            reply_to: None,
            reactions: vec![],
            edited: false,
            starred,
            pinned: false,
            forwarded: false,
            receipts: vec![],
        }
    }

    #[test]
    fn starred_groups_by_chat_newest_first() {
        let all = vec![
            (
                1,
                "A".to_string(),
                vec![msg(1, 1, -100, true), msg(2, 1, -5, false)],
            ),
            (
                2,
                "B".to_string(),
                vec![msg(3, 2, -10, true), msg(4, 2, -50, true)],
            ),
            (3, "C".to_string(), vec![msg(5, 3, -1, false)]),
        ];
        let out = starred_by_chat(&all);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 2);
        assert_eq!(out[0].2.len(), 2);
        assert_eq!(out[1].0, 1);
    }
}
