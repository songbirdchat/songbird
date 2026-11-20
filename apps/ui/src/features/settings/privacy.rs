//! Privacy: who sees what, read receipts, groups, blocked contacts, the
//! default timer, app lock, screen protections, link previews, and the
//! advanced options. Each audience page is a list of radio rows, as
//! WhatsApp's are.

use nsh_core::prelude::*;

use super::{extras, foot, nav_row, section, section_plain, value_row};
use crate::{
    store::{Audience, Disappear, LockAfter, Settings, store},
    ui::*,
};

/// Who may see the profile and presence: only contacts hold the key that
/// opens them, so "Everyone" would mean the same as "My contacts" (D-0101).
const AUDIENCES: [Audience; 2] = [Audience::Contacts, Audience::Nobody];
/// Who may add me to groups.
const GROUP_AUDIENCES: [Audience; 3] = [Audience::Everyone, Audience::Contacts, Audience::Nobody];

pub fn privacy() {
    super::push("Privacy", content);
}

fn content() -> AnyElement {
    let s = store();
    let settings = s.settings;
    let ext = s.ext();
    let count_blocked = move || {
        s.contacts
            .with(|cs| cs.iter().filter(|c| c.blocked).count())
    };
    view()
        .child(view().pt_4().child(callout(
            Icon::ShieldCheck,
            "What Songbird can see",
            "In plain words: what our servers know, and what they never see.",
            super::privacy_centre,
        )))
        .child(section(
            "Who can see my personal info",
            vec![
                value_row("Last seen and online", Some(Icon::Eye), move || settings.with(|x| x.last_seen.label().into()), last_seen),
                value_row("Profile photo", Some(Icon::User), move || settings.with(|x| x.photo.label().into()), || {
                    audience_page("Profile photo", "Who can see my profile photo", |x| x.photo, |x, a| x.photo = a)
                }),
                value_row("About", Some(Icon::Info), move || settings.with(|x| x.about.label().into()), || {
                    audience_page("About", "Who can see my about", |x| x.about, |x, a| x.about = a)
                }),
                value_row("Status", Some(Icon::Smile), move || settings.with(|x| x.status.label().into()), || {
                    audience_page("Status", "Who can see my custom status", |x| x.status, |x, a| x.status = a)
                }),
            ],
        ))
        .child(foot(
            "Your profile is end\u{2011}to\u{2011}end encrypted. These choices decide who gets the key to read it.",
        ))
        .child(view().pt_4().child(group(vec![
            toggle_row(
                "Read receipts",
                Some("When it's off, you don't send or see read receipts, except in groups."),
                move || settings.with(|x| x.read_receipts),
                move |on| s.settings_update(|x| x.read_receipts = on),
            ),
            toggle_row(
                "Typing indicators",
                Some("When it's off, people don't see when you're typing, and you don't see when they are."),
                move || settings.with(|x| x.typing_indicators),
                move |on| s.settings_update(|x| x.typing_indicators = on),
            ),
        ])))
        .child(section(
            "Messages and groups",
            vec![
                value_row("Groups", Some(Icon::Users), move || settings.with(|x| x.groups_add.label().into()), groups),
                value_row(
                    "Message requests",
                    Some(Icon::Chats),
                    move || if settings.with(|x| x.message_requests) { "Everyone".into() } else { "Nobody".into() },
                    requests,
                ),
                toggle_row_icon(
                    Icon::AtSign,
                    "Find me by username",
                    Some("When it's off, people can still reach you with your username link or QR code."),
                    move || settings.with(|x| x.discoverable),
                    move |on| s.set_discoverable(on),
                ),
                value_row(
                    "Default message timer",
                    Some(Icon::Timer),
                    move || settings.with(|x| x.default_timer.map_or("Off", |t| t.label()).into()),
                    default_timer,
                ),
                value_row(
                    "Blocked contacts",
                    Some(Icon::Ban),
                    move || match count_blocked() {
                        0 => "None".into(),
                        n => n.to_string(),
                    },
                    blocked,
                ),
            ],
        ))
        .child(section(
            "On this device",
            vec![
                value_row(
                    "App lock",
                    Some(Icon::Lock),
                    move || if settings.with(|x| x.app_lock) { "On".into() } else { "Off".into() },
                    app_lock,
                ),
                toggle_row_icon(
                    Icon::EyeOff,
                    "Block screenshots",
                    Some("Stops screenshots and screen recording of Songbird on this device."),
                    move || settings.with(|x| x.block_screenshots),
                    move |on| s.settings_update(|x| x.block_screenshots = on),
                ),
                toggle_row_icon(
                    Icon::Layers,
                    "Hide content in app switcher",
                    Some("Shows a blank screen when you switch apps."),
                    move || settings.with(|x| x.hide_in_switcher),
                    move |on| s.settings_update(|x| x.hide_in_switcher = on),
                ),
                toggle_row_icon(
                    Icon::Link,
                    "Link previews",
                    Some("Previews are made on your device. Nobody else learns which links you share."),
                    move || settings.with(|x| x.link_previews),
                    move |on| s.settings_update(|x| x.link_previews = on),
                ),
            ],
        ))
        .child(section(
            "Advanced",
            vec![
                toggle_row_icon(
                    Icon::Shield,
                    "Protect IP address in calls",
                    Some("Calls go through Songbird's relay, so the other person can't see your IP address. Call quality may drop a little."),
                    move || ext.with(|x| x.protect_ip_in_calls),
                    move |on| s.ext_update(|x| x.protect_ip_in_calls = on),
                ),
                toggle_row_icon(
                    Icon::Alert,
                    "Send crash reports",
                    Some("Off by default. Reports are scrubbed on your device before sending: no messages, names or numbers."),
                    move || settings.with(|x| x.crash_reports),
                    move |on| s.settings_update(|x| x.crash_reports = on),
                ),
                nav_row("Proxy", Icon::Globe, extras::proxy),
            ],
        ))
        .into_element()
}

/// Who can add me to groups (the server checks it, Prompt 9).
fn groups() {
    super::push("Groups", || {
        let s = store();
        let labels = |a: Audience| match a {
            Audience::Everyone => ("Everyone", None),
            Audience::Contacts => ("My contacts", None),
            Audience::Nobody => (
                "Nobody",
                Some("Contacts send you an invitation you can accept."),
            ),
        };
        view()
            .child(section_plain(
                "Who can add me to groups",
                GROUP_AUDIENCES
                    .into_iter()
                    .map(|a| {
                        let (label, hint) = labels(a);
                        radio_row(label, hint, move || s.settings.with(|x| x.groups_add == a), move || {
                            s.set_groups_add(a)
                        })
                    })
                    .collect(),
            ))
            .child(foot("Songbird's servers refuse adds from people who aren't your contacts, so this holds even if someone's app ignores it."))
            .into_element()
    });
}

/// Who can send me message requests (the server checks it).
fn requests() {
    super::push("Message requests", || {
        let s = store();
        view()
            .child(section_plain(
                "Who can send me message requests",
                vec![
                    radio_row("Everyone", None, move || s.settings.with(|x| x.message_requests), move || {
                        s.set_message_requests(true)
                    }),
                    radio_row("Nobody", Some("Only your contacts can reach you."), move || {
                        s.settings.with(|x| !x.message_requests)
                    }, move || s.set_message_requests(false)),
                ],
            ))
            .child(foot("A request shows you who it's from before you open it. Blocked people never reach you either way."))
            .into_element()
    });
}

/// A page of radio rows choosing who sees one thing.
fn audience_page(
    title: &'static str,
    heading: &'static str,
    get: fn(&Settings) -> Audience,
    set: fn(&mut Settings, Audience),
) {
    super::push(title, move || {
        let s = store();
        view()
            .child(section_plain(
                heading,
                AUDIENCES
                    .into_iter()
                    .map(|a| {
                        radio_row(
                            a.label(),
                            None,
                            move || s.settings.with(|x| get(x) == a),
                            move || s.settings_update(|x| set(x, a)),
                        )
                    })
                    .collect(),
            ))
            .into_element()
    });
}

/// Last seen and online, with the "Same as last seen" choice.
pub fn last_seen() {
    super::push("Last seen and online", || {
        let s = store();
        let settings = s.settings;
        let ext = s.ext();
        let set_last_seen = move |a: Audience| {
            let same = ext.with_untracked(|x| x.online_same_as_last_seen);
            s.settings_update(|x| {
                x.last_seen = a;
                if same {
                    x.online = a;
                }
            });
        };
        view()
            .child(section_plain(
                "Who can see my last seen",
                AUDIENCES
                    .into_iter()
                    .map(|a| radio_row(a.label(), None, move || settings.with(|x| x.last_seen == a), move || set_last_seen(a)))
                    .collect(),
            ))
            .child(section_plain(
                "Who can see when I'm online",
                vec![
                    radio_row("My contacts", None, move || !ext.with(|x| x.online_same_as_last_seen), move || {
                        s.ext_update(|x| x.online_same_as_last_seen = false);
                        s.settings_update(|x| x.online = Audience::Contacts);
                    }),
                    radio_row("Same as last seen", None, move || ext.with(|x| x.online_same_as_last_seen), move || {
                        s.ext_update(|x| x.online_same_as_last_seen = true);
                        s.settings_update(|x| x.online = x.last_seen);
                    }),
                ],
            ))
            .child(foot(
                "If you don't share your last seen and online, you won't see other people's.",
            ))
            .child(view().px_4().pt_4().child(banner(
                Tone::Info,
                Icon::EyeOff,
                "To look offline for a while, set your presence to Invisible on the You tab. Nothing is sent while you're invisible.",
            )))
            .into_element()
    });
}

pub fn default_timer() {
    super::push("Default message timer", || {
        let s = store();
        let options: [(Option<Disappear>, &'static str); 4] = [
            (None, "Off"),
            (Some(Disappear::Day), "24 hours"),
            (Some(Disappear::Week), "7 days"),
            (Some(Disappear::FourWeeks), "4 weeks"),
        ];
        view()
            .child(view().items_center().pt_6().pb_2().child(hero_icon(Icon::Timer)))
            .child(
                view().px_4().child(
                    footnote("Start new chats with disappearing messages. Messages vanish from every device after the time you pick.")
                        .px_0()
                        .text_center(),
                ),
            )
            .child(section_plain(
                "Message timer",
                options
                    .into_iter()
                    .map(|(t, label)| {
                        radio_row(label, None, move || s.settings.with(|x| x.default_timer == t), move || {
                            s.settings_update(|x| x.default_timer = t)
                        })
                    })
                    .collect(),
            ))
            .child(foot("Your existing chats keep their timer. Anyone in a chat can still save what they see."))
            .into_element()
    });
}

pub fn blocked() {
    super::push("Blocked contacts", || {
        let s = store();
        let picking = signal(false);
        let blocked = computed(move || {
            s.contacts.with(|cs| {
                cs.iter()
                    .filter(|c| c.blocked)
                    .map(|c| (c.id, c.name.clone(), c.id))
                    .collect::<Vec<_>>()
            })
        });
        view()
            .child(when(
                move || blocked.with(|b| b.is_empty()),
                || {
                    empty_state(
                        Icon::Ban,
                        "No one is blocked",
                        "Blocked people can't call you or send you messages. They aren't told you blocked them.",
                        None,
                    )
                },
            ))
            .child(when(
                move || blocked.with(|b| !b.is_empty()),
                move || {
                    view().pt_4().child(dynamic(move || {
                        let list = blocked.get();
                        untracked(move || {
                            group_with_inset(list.into_iter().map(|(id, name, seed)| {
                                Row::new(name.clone())
                                    .leading(avatar(name.clone(), seed, 36.0))
                                    .trailing(btn("Unblock", Variant::Outline, BtnSize::Sm, move || {
                                        s.block(id, false);
                                        toast("Unblocked");
                                    }))
                                    .build()
                            }), INSET_AVATAR)
                        })
                    }))
                },
            ))
            .child(view().px_4().pt_4().child(btn_with_icon(
                "Block someone",
                Some(Icon::Plus),
                Variant::Secondary,
                BtnSize::Md,
                move || picking.set(true),
            )))
            .child(foot("Blocking is kept on your devices. Songbird's servers only keep a code that drops their message requests, not who you blocked."))
            .child(block_sheet(picking))
            .into_element()
    });
}

fn block_sheet(open: Signal<bool>) -> nsh_base::SheetElement {
    let s = store();
    let people = computed(move || {
        s.contacts.with(|cs| {
            cs.iter()
                .filter(|c| !c.blocked)
                .map(|c| (c.id, c.name.clone()))
                .collect::<Vec<_>>()
        })
    });
    app_sheet(
        open,
        "Block someone",
        scroll()
            .max_h(px(420.0))
            .child(view().pb_2().child(dynamic(move || {
                let list = people.get();
                untracked(move || {
                    group_with_inset(
                        list.into_iter().map(|(id, name)| {
                            let n = name.clone();
                            Row::new(name.clone())
                                .leading(avatar(name, id, 36.0))
                                .on_press(move || {
                                    s.block(id, true);
                                    open.set(false);
                                    toast(format!("{n} is blocked"));
                                })
                                .build()
                        }),
                        INSET_AVATAR,
                    )
                })
            }))),
    )
}

pub fn app_lock() {
    super::push("App lock", || {
        let s = store();
        let settings = s.settings;
        let ext = s.ext();
        let on = move || settings.with(|x| x.app_lock);
        view()
            .child(view().pt_4().child(group(vec![toggle_row(
                "Lock with a passcode",
                Some("Songbird asks for six digits when you open it. They stay on this device."),
                on,
                move |v| {
                    if v {
                        crate::features::lock::set_passcode_page();
                    } else {
                        crate::features::lock::turn_off_page();
                    }
                },
            )])))
            .child(when(
                move || on() && crate::backend::biometric::kind().is_some(),
                move || {
                    let kind = crate::backend::biometric::kind()
                        .map_or("biometrics", |k| k.label());
                    view()
                        .pt_4()
                        .child(group(vec![toggle_row(
                            format!("Unlock with {kind}"),
                            Some("Your passcode still opens Songbird, and is needed after too many tries."),
                            move || ext.with(|x| x.app_lock_biometrics),
                            move |v| {
                                if !v {
                                    s.ext_update(|x| x.app_lock_biometrics = false);
                                    return;
                                }
                                // Turned on only after a match: proof the owner is here.
                                spawn(async move {
                                    if crate::backend::biometric::verify("turn on biometric unlock")
                                        .await
                                        .is_ok()
                                    {
                                        s.ext_update(|x| x.app_lock_biometrics = true);
                                    }
                                });
                            },
                        )]))
                        .into_element()
                },
            ))
            .child(when(on, move || {
                section_plain(
                    "Lock automatically",
                    [LockAfter::Immediately, LockAfter::Minute, LockAfter::Hour]
                        .into_iter()
                        .map(|a| {
                            radio_row(a.label(), None, move || ext.with(|x| x.app_lock_after == a), move || {
                                s.ext_update(|x| x.app_lock_after = a)
                            })
                        })
                        .collect(),
                )
            }))
            .child(foot(
                "Calls can still be answered while Songbird is locked. To hide who's writing, turn off previews in Notifications.",
            ))
            .into_element()
    });
}
