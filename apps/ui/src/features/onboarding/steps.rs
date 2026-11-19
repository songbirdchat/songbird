//! The new-account steps.

use std::{rc::Rc, time::Duration};

use nsh_core::{notifications::NotificationPermission, prelude::*};

use super::{Flow, Step, finish};
use crate::{
    features::backups,
    store::{
        Store,
        ext_onboarding::{
            INVITE_GROUP, INVITE_LEN, InviteCheck, UsernameCheck, check_invite, invite_complete,
            key_groups, key_tail_matches, normalize_invite, normalize_username, username_format,
            username_key, username_suggestions,
        },
    },
    ui::*,
};

/// A centred, single-column intro: everything in the middle, the actions
/// at the bottom (welcome, the "creating" wait).
fn centred(content: impl IntoElement, actions: impl IntoElement) -> View {
    view()
        .flex_1()
        .child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .px_6()
                .gap_4()
                .child(content),
        )
        .child(
            view()
                .w_full()
                .max_w(px(520.0))
                .self_center()
                .px_6()
                .pt_3()
                .pb_4()
                .gap_2()
                .child(actions),
        )
}

fn primary(label: &'static str, on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    btn(label, Variant::Primary, BtnSize::Lg, on_press).w_full()
}

fn quiet(label: &'static str, on_press: impl Fn() + 'static) -> nsh_base::ButtonElement {
    btn(label, Variant::Ghost, BtnSize::Md, on_press).w_full()
}

// ---------------------------------------------------------------- 1 welcome

const PROMISES: [(Icon, &str, &str); 3] = [
    (
        Icon::Chats,
        "Talk freely.",
        "Messages, voice and video calls with the people who matter to you.",
    ),
    (
        Icon::Lock,
        "Only you and the people you talk to can read your messages.",
        "Not Songbird, not anyone else.",
    ),
    (
        Icon::Heart,
        "Free. No ads. No tracking.",
        "Made in Europe, and paid for by people who choose Plus, never by your data.",
    ),
];

pub(super) fn welcome(f: Flow) -> View {
    let page = f.welcome_page;
    centred(
        view()
            .items_center()
            .gap_8()
            .w_full()
            .max_w(px(400.0))
            .child(
                view()
                    .size(px(72.0))
                    .rounded(move || radius::sheet())
                    .bg(move || c::primary())
                    .items_center()
                    .justify_center()
                    .text_color(move || c::on_primary())
                    .shadow_md()
                    .accessibility_elements_hidden(true)
                    .child(icon(Icon::Feather, 36.0)),
            )
            .child(
                // A fixed band, its text from the top, so neither the
                // headline nor the dots below move between pages.
                view()
                    .h(px(196.0))
                    .w_full()
                    .justify_start()
                    .child(dynamic(move || {
                        let i = page.get();
                        untracked(move || {
                            let (_, title, line) = PROMISES[i];
                            let rise = motion().entering(
                                MotionSpec::new(
                                    KeyframeTiming::new(motion::BASE).easing(Easing::EaseOut),
                                )
                                .values(MotionChannel::Opacity, [0.0, 1.0])
                                .values(
                                    MotionChannel::TranslateY,
                                    [if motion::reduced() { 0.0 } else { 8.0 }, 0.0],
                                ),
                            );
                            view()
                                .items_center()
                                .gap_3()
                                .bind_motion(&rise)
                                .child(flow_title(title).text_center().max_w(px(360.0)))
                                .child(flow_lead(line).text_center().max_w(px(340.0)))
                                .into_element()
                        })
                    })),
            )
            .child(page_dots(3, move || page.get(), move |i| page.set(i))),
        view()
            .gap_2()
            .child(primary("Get started", move || f.go(Step::Invite)))
            .child(quiet("I already have an account", move || {
                f.go(Step::Returning)
            })),
    )
}

// ---------------------------------------------------------------- 2 invite code

pub(super) fn invite(f: Flow) -> View {
    let help = signal(false);
    let code = f.invite;
    // The code registration refused: editing it clears the message.
    let refused = move || {
        f.invite_refused
            .with(|r| r.as_ref().is_some_and(|r| code.with(|c| c == r)))
    };
    let complete = move || code.with(|c| invite_complete(c));
    // Only the format is checked here: there is no endpoint to ask about a
    // code before registering, which is where a bad one is refused.
    let submit = move || {
        if complete() && !refused() {
            dismiss_keyboard();
            f.go(Step::Username);
        }
    };
    let clean = |t: &str| normalize_invite(t);
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Enter your invite code"))
            .child(flow_lead("Songbird is invite-only while we grow. A friend on Songbird can send you a code."))
            .child(code_cells("Invite code", INVITE_LEN, INVITE_GROUP, code, clean, refused, submit).mt_2())
            .child(status_line(
                move || if refused() { StatusTone::Problem } else { StatusTone::Muted },
                move || if refused() { Icon::Alert } else { Icon::Gift },
                move || {
                    if refused() {
                        "This code doesn't work. It may have been used already or expired. Ask your friend for a new one.".into()
                    } else {
                        format!(
                            "{INVITE_LEN} letters and numbers, in groups of {INVITE_GROUP}. You can paste the whole code."
                        )
                    }
                },
            ))
            .child(
                view()
                    .gap_2()
                    .items_start()
                    .child(
                        // A text-style action on the content's leading edge.
                        btn_with_icon(
                            "Don't have a code?",
                            Some(Icon::HelpCircle),
                            Variant::Ghost,
                            BtnSize::Sm,
                            move || help.update(|h| *h = !*h),
                        )
                        .ml(px(-12.0)),
                    )
                    .child(when(
                        move || help.get(),
                        || {
                            card().w_full().child(muted(
                                "Anyone on Songbird can invite you from You → Invite a friend. Each person has a few codes to share, so ask someone you know.",
                            ))
                        },
                    )),
            ),
        view().child(primary("Continue", submit).disabled(move || !complete() || refused())),
    )
}

// ---------------------------------------------------------------- 3 username

/// What the username line says: the format rules, checked as you type.
/// Whether the name is free is only learned when it is claimed, right
/// after the account is made (no lookup before registering).
fn username_line(name: impl Fn() -> String + Copy + 'static) -> View {
    let check = move || username_format(&name());
    status_line(
        move || match check() {
            None => StatusTone::Good,
            Some(UsernameCheck::Empty) => StatusTone::Muted,
            Some(_) => StatusTone::Problem,
        },
        move || match check() {
            None => Icon::Check,
            Some(UsernameCheck::Empty) => Icon::Info,
            Some(_) => Icon::Alert,
        },
        move || check().unwrap_or(UsernameCheck::Available).message(&name()),
    )
}

fn username_field(
    value: Signal<String>,
    invalid: impl Fn() -> bool + Copy + 'static,
    next: impl Fn() + 'static,
) -> View {
    view()
        .gap(px(6.0))
        .child(
            text("Username")
                .text_size(px(font::SMALL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || c::muted_fg()),
        )
        .child(
            field("e.g. ana.popescu", value)
                // An invalid name shows the destructive border, focused or
                // not, as the invite code's blocks do.
                .border_color(move || {
                    if invalid() {
                        c::destructive()
                    } else {
                        c::input()
                    }
                })
                .focus(move |st| {
                    st.border_color(move || {
                        if invalid() {
                            c::destructive()
                        } else {
                            c::ring()
                        }
                    })
                })
                .on_submit(move |_: &str| next())
                .auto_capitalize(AutoCapitalize::None)
                .auto_correct(false)
                .auto_focus(true)
                .max_length(40),
        )
}

pub(super) fn username(_s: Store, f: Flow) -> View {
    let value = f.username;
    let name = move || normalize_username(&value.get());
    let ok = move || username_format(&name()).is_none();
    let next = move || {
        if ok() {
            dismiss_keyboard();
            f.go(Step::Creating);
        }
    };
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Choose a username"))
            .child(flow_lead("Friends find you by your username. You don't need a phone number, and you can change it later."))
            .child(username_field(value, move || matches!(username_format(&name()), Some(c) if c != UsernameCheck::Empty), next))
            .child(username_line(name))
            .child(
                view()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .mt_2()
                    .child(icon(Icon::Shield, 16.0).mt(px(2.0)).text_color(move || c::muted_fg()))
                    .child(muted("Only someone who knows your exact username can find you. There's no search.").flex_1()),
            ),
        view().child(primary("Continue", next).disabled(move || !ok())),
    )
}

/// The account exists, but someone claimed its username first: pick
/// another. No way back: the account stays.
pub(super) fn claim_username(s: Store, f: Flow) -> View {
    let value = f.username;
    let saving = signal(false);
    let lost = f.taken.get_untracked().unwrap_or_default();
    let name = move || normalize_username(&value.get());
    let still_taken = move || {
        f.taken.with(|t| {
            t.as_deref()
                .is_some_and(|t| username_key(t) == username_key(&name()))
        })
    };
    let ok = move || username_format(&name()).is_none() && !still_taken();
    let next = move || {
        if !ok() || saving.get_untracked() {
            return;
        }
        dismiss_keyboard();
        saving.set(true);
        let wanted = name();
        // Claimed on the server (only its hash and a proof leave the device).
        {
            saving.set(false);
            if crate::backend::is_live() && crate::backend::registered() {
                spawn(async move {
                    match crate::backend::claim_username(&wanted).await {
                        Ok(()) => {
                            s.create_account(wanted);
                            f.replace(Step::Profile);
                        }
                        Err(_) => f.taken.set(Some(wanted)),
                    }
                });
            } else if s.username_available(&wanted) {
                s.create_account(wanted);
                f.replace(Step::Profile);
            } else {
                f.taken.set(Some(wanted));
            }
        }
    };
    let options = username_suggestions(&lost, |n| s.username_available(n));
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Pick another username"))
            .child(flow_lead(
                "Your account is ready. Choose a username to finish.",
            ))
            .child(username_field(
                value,
                move || {
                    still_taken()
                        || matches!(username_format(&name()), Some(c) if c != UsernameCheck::Empty)
                },
                next,
            ))
            .child(
                when(still_taken, move || {
                    status_line(
                        || StatusTone::Problem,
                        || Icon::Alert,
                        move || UsernameCheck::Taken.message(&name()),
                    )
                })
                .otherwise(move || username_line(name)),
            )
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .children(options.into_iter().map(move |o| {
                        let pick = o.clone();
                        chip(format!("@{o}"), || false, move || value.set(pick.clone()))
                    })),
            ),
        view().child(
            btn(
                move || {
                    if saving.get() {
                        "Saving…"
                    } else {
                        "Continue"
                    }
                },
                Variant::Primary,
                BtnSize::Lg,
                next,
            )
            .w_full()
            .disabled(move || !ok() || saving.get()),
        ),
    )
}

// ---------------------------------------------------------------- creating

pub(super) fn creating(s: Store, f: Flow) -> View {
    let name = normalize_username(&f.username.get_untracked());
    let code = f.invite.get_untracked();
    let here = move || {
        f.history
            .try_get()
            .is_some_and(|h| h.last() == Some(&Step::Creating))
    };
    // The real server (You → Server connection): challenge, Argon2id proof
    // of work on this device, both keys, the invite; then the username.
    if crate::backend::is_live() {
        let (code, name) = (code.clone(), name.clone());
        spawn(async move {
            let registered = crate::backend::register(Some(code.clone()), None).await;
            if !here() {
                return;
            }
            match registered {
                Err(p) if p.0.contains("code") => {
                    f.invite_refused.set(Some(code));
                    f.rewind_to(Step::Invite);
                }
                Err(p) => {
                    toast(p.0);
                    f.rewind_to(Step::Username);
                }
                Ok(()) => match crate::backend::claim_username(&name).await {
                    Ok(()) => {
                        s.create_account(name);
                        f.replace(Step::Profile);
                    }
                    Err(_) => {
                        f.taken.set(Some(name));
                        f.replace(Step::ClaimUsername);
                    }
                },
            }
        });
    } else {
        // mock: registration with an invisible Argon2id proof of work (D-0048),
        // about 1.5 s, then claiming the username.
        set_timeout(Duration::from_millis(1500), move || {
            if !f
                .history
                .try_get()
                .is_some_and(|h| h.last() == Some(&Step::Creating))
            {
                return;
            }
            if check_invite(&code) == InviteCheck::Refused {
                // One calm answer for every bad code; nothing was made.
                f.invite_refused.set(Some(code.clone()));
                f.rewind_to(Step::Invite);
            } else if s.username_available(&name) {
                s.create_account(name.clone());
                f.replace(Step::Profile);
            } else {
                f.taken.set(Some(name.clone()));
                f.replace(Step::ClaimUsername);
            }
        });
    }
    let live_busy = move || crate::backend::live().with(|l| l.busy.clone());
    centred(
        view()
            .items_center()
            .gap_4()
            .w_full()
            .max_w(px(320.0))
            .child(hero_icon(Icon::Sparkles))
            .child(flow_title("Creating your account…").text_center())
            .child(busy_bar("Creating your account"))
            .child(
                muted(move || {
                    SharedString::from(live_busy().unwrap_or_else(|| "This takes a moment.".into()))
                })
                .text_center(),
            ),
        view(),
    )
}

// ---------------------------------------------------------------- 4 profile

pub(super) fn profile(s: Store, f: Flow) -> View {
    let name = f.name;
    let ready = move || name.with(|n| !n.trim().is_empty());
    let next = move || {
        if ready() {
            let n = name.get_untracked().trim().to_string();
            s.profile_update(|p| p.name = n);
            dismiss_keyboard();
            f.go(Step::Recovery);
        }
    };
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Set up your profile"))
            .child(flow_lead("Add your name so friends know it's you."))
            .child(
                view().items_center().py_2().child(
                    view()
                        .size(px(96.0))
                        .child(
                            view()
                                .size(px(96.0))
                                .rounded_full()
                                .items_center()
                                .justify_center()
                                .bg(move || c::accent())
                                .accessibility_elements_hidden(true)
                                .child(
                                    text(move || {
                                        let n = name.get();
                                        if n.trim().is_empty() {
                                            String::new()
                                        } else {
                                            initials(&n)
                                        }
                                    })
                                    .text_size(px(font::DISPLAY + 8.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || c::primary()),
                                )
                                .child(
                                    icon(Icon::User, 40.0)
                                        .text_color(move || c::primary())
                                        .style_when(move || ready(), |s| s.hidden()),
                                ),
                        )
                        .child(
                            view()
                                .absolute()
                                .right(px(-4.0))
                                .bottom(px(-4.0))
                                .rounded_full()
                                .bg(move || c::card())
                                .border_1()
                                .border_color(move || c::border())
                                .text_color(move || c::primary())
                                // The system picker; encrypted and uploaded on this device.
                                .child(icon_btn(Icon::Camera, "Add a photo", move || {
                                    crate::backend::avatar::choose(crate::store::store())
                                })),
                        ),
                ),
            )
            .child(labelled_field(
                "Your name",
                field("First and last name, or a nickname", name)
                    .on_submit(move |_: &str| next())
                    .auto_capitalize(AutoCapitalize::Words)
                    .auto_focus(true)
                    .max_length(60),
                "",
                || false,
            ))
            .child(banner(
                Tone::Secure,
                Icon::Lock,
                "Only people you talk to can see your name and photo.",
            )),
        view().child(primary("Continue", next).disabled(move || !ready())),
    )
}

// ---------------------------------------------------------------- 5 recovery key

pub(super) fn recovery(s: Store, f: Flow) -> View {
    let key = s.session.with_untracked(|x| x.recovery_key.clone());
    let typed = f.key_typed;
    let k = key.clone();
    let ok = computed(move || key_tail_matches(&k, &typed.get()));
    let full = move || typed.with(|t| t.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 4);
    let next = move || {
        if ok.get_untracked() {
            dismiss_keyboard();
            f.go(Step::BackupOffer);
        }
    };
    let copy_key = key.clone();
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Save your recovery key"))
            .child(flow_lead(
                "If you lose your phone, this key is the only way to get your messages back. Songbird can't recover it for you.",
            ))
            .child(key_blocks(key_groups(&key)))
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .justify_center()
                    .gap_2()
                    .child(btn_with_icon("Copy", Some(Icon::Copy), Variant::Secondary, BtnSize::Md, move || {
                        set_clipboard(copy_key.clone());
                        toast("Recovery key copied. Keep it somewhere safe.");
                    }))
                    .child(btn_with_icon(
                        "Save printable sheet",
                        Some(Icon::Download),
                        Variant::Secondary,
                        BtnSize::Md,
                        move || {
                            // A printable text sheet, saved where the user chooses.
                            let key = s.session.with_untracked(|x| x.recovery_key.clone());
                            let sheet = format!(
                                "SONGBIRD RECOVERY KEY\n\n{key}\n\nThis key restores your Songbird backups and your account.\nKeep it somewhere safe and offline. Anyone with it can read your backups.\nSongbird can't recover it for you.\n"
                            );
                            save_file("songbird-recovery-key.txt", sheet.into_bytes());
                        },
                    )),
            )
            .child(muted("Write it down or print it, and keep it somewhere safe and offline. Anyone with this key can read your backups.").text_center())
            .child(separator().my_1())
            .child(labelled_field(
                "To check you saved it, type the last 4 characters",
                field("Last 4 characters", typed)
                    .on_submit(move |_: &str| next())
                    .font_family(MONO)
                    .auto_capitalize(AutoCapitalize::Characters)
                    .auto_correct(false)
                    .max_length(4),
                "",
                || false,
            ))
            .child(status_line(
                move || match (full(), ok.get()) {
                    (false, _) => StatusTone::Muted,
                    (true, true) => StatusTone::Good,
                    (true, false) => StatusTone::Problem,
                },
                move || match (full(), ok.get()) {
                    (false, _) => Icon::Key,
                    (true, true) => Icon::Check,
                    (true, false) => Icon::Alert,
                },
                move || match (full(), ok.get()) {
                    (false, _) => "You won't see this key again after this step.".into(),
                    (true, true) => "That's right. Your key is saved.".into(),
                    (true, false) => "That doesn't match. Check the last group of your key.".into(),
                },
            )),
        view().child(primary("Continue", next).disabled(move || !ok.get())),
    )
}

// ---------------------------------------------------------------- backups

pub(super) fn backup_offer(f: Flow) -> View {
    flow_layout(
        view()
            .gap_4()
            .child(hero_icon(Icon::Cloud))
            .child(flow_title("Back up your chats?"))
            .child(flow_lead(if is_desktop() {
                "If you lose or replace a device, a backup brings your messages back. It's encrypted with your recovery key, so only you can open it."
            } else {
                "If you lose or replace your phone, a backup brings your messages back. It's encrypted with your recovery key, so only you can open it."
            }))
            .child(muted("You can change this any time in You → Chats → Backups.")),
        view()
            .gap_2()
            .child(primary("Set up backups", move || f.go(Step::BackupSetup)))
            .child(quiet("Later", move || f.go(Step::Notifications))),
    )
}

pub(super) fn backup_setup(f: Flow) -> View {
    view().flex_1().child(backups::wizard_body(
        true,
        Rc::new(move || f.go(Step::Notifications)),
        Some(f.host),
    ))
}

// ---------------------------------------------------------------- 6 notifications

pub(super) fn notifications(f: Flow) -> View {
    let asking = signal(false);
    let note = signal(None::<&'static str>);
    let turn_on = move || {
        if asking.get_untracked() {
            return;
        }
        asking.set(true);
        spawn(async move {
            let answer = request_notification_permission().await;
            let _ = asking.try_set(false);
            match answer {
                NotificationPermission::Granted => {
                    toast("Notifications are on");
                    f.go(Step::Connect);
                }
                NotificationPermission::Denied => {
                    let _ = note.try_set(Some(
                        "Notifications are off. You can turn them on later in your device's settings.",
                    ));
                }
                NotificationPermission::Unavailable => {
                    let _ =
                        note.try_set(Some("This device can't show notifications from Songbird."));
                }
                NotificationPermission::Default => {
                    let _ = note.try_set(Some("You can turn notifications on later in Settings."));
                }
            }
        });
    };
    flow_layout(
        view()
            .gap_4()
            .child(hero_icon(Icon::Bell))
            .child(flow_title("Turn on notifications"))
            .child(flow_lead(
                "So you know when someone messages or calls. Notifications never include your messages unless you allow it.",
            ))
            .child(when(
                move || note.get().is_some(),
                move || banner(Tone::Info, Icon::Info, move || note.get().unwrap_or_default().to_string()),
            )),
        view()
            .gap_2()
            .child(when(
                move || note.get().is_none(),
                move || {
                    btn(
                        move || if asking.get() { "Asking…" } else { "Turn on" },
                        Variant::Primary,
                        BtnSize::Lg,
                        turn_on,
                    )
                    .w_full()
                    .disabled(move || asking.get())
                },
            )
            .otherwise(move || primary("Continue", move || f.go(Step::Connect))))
            .child(when(move || note.get().is_none(), move || quiet("Not now", move || f.go(Step::Connect)))),
    )
}

// ---------------------------------------------------------------- 7 who to talk to

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Invite,
    Scan,
    Group,
}

pub(super) fn connect(s: Store, f: Flow) -> View {
    let way = signal(None::<Way>);
    let group_name = signal(String::new());
    let found = signal(false);
    let link = s.invite_link();
    let first = move || {
        let n = f.name.get_untracked();
        n.split_whitespace().next().unwrap_or("").to_string()
    };
    let welcome = move || {
        let n = first();
        if n.is_empty() {
            "Welcome to Songbird".to_string()
        } else {
            format!("Welcome to Songbird, {n}")
        }
    };
    let pick = move |w: Way| {
        way.set(if way.get_untracked() == Some(w) {
            None
        } else {
            Some(w)
        });
        found.set(false);
        if w == Way::Scan {
            // mock: the camera finds a friend's code (app-core resolves the username link).
            set_timeout(Duration::from_millis(2600), move || {
                if way.try_get() == Some(Some(Way::Scan)) {
                    let _ = found.try_set(true);
                }
            });
        }
    };
    let option = move |w: Way, glyph: Icon, title: &'static str, sub: &'static str| {
        let open = move || way.get() == Some(w);
        Row::new(title)
            .icon(glyph)
            .subtitle(sub)
            .trailing(
                // Two glyphs, not a rotated one: a rotation pivots on the
                // box's corner and leaves a bent "L" at rest.
                view()
                    .size(px(18.0))
                    .text_color(move || c::muted_fg())
                    .child(
                        when(open, || icon(Icon::ChevronDown, 18.0))
                            .otherwise(|| icon(Icon::ChevronRight, 18.0)),
                    ),
            )
            .on_press(move || pick(w))
            .build()
    };
    let create_group = move || {
        let title = group_name.get_untracked().trim().to_string();
        if title.is_empty() {
            return;
        }
        let id = s.create_group(title.clone(), Vec::new());
        if let Some(chat) = s.chat(id) {
            set_clipboard(chat.invite_link);
        }
        finish(
            s,
            id,
            format!(
                "\"{title}\" is ready. Its invite link is copied, so you can share it with your group."
            ),
        );
    };
    let link2 = link.clone();
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Who do you want to talk to?"))
            .child(flow_lead("Songbird is better with your people. Invite a friend, or bring your whole group."))
            .child(
                group([
                    option(Way::Invite, Icon::Share, "Share an invite link", "Send it any way you like"),
                    option(Way::Scan, Icon::Scan, "Scan a friend's QR code", "When you're together"),
                    option(Way::Group, Icon::Users, "Create a group and share its link", "Bring your group"),
                ])
                .mx_0(),
            )
            .child(dynamic(move || {
                let w = way.get();
                let link = link2.clone();
                untracked(move || {
                    let rise = motion().entering(
                        MotionSpec::new(KeyframeTiming::new(motion::BASE).easing(Easing::EaseOut))
                            .values(MotionChannel::Opacity, [0.0, 1.0])
                            .values(MotionChannel::TranslateY, [8.0, 0.0]),
                    );
                    let panel = match w {
                        None => return view().into_element(),
                        Some(Way::Invite) => card()
                            .gap_3()
                            .child(muted("Anyone with this link can join Songbird and message you. It works for 7 days."))
                            .child(
                                text(link.clone())
                                    .font_family(MONO)
                                    .text_size(px(font::SMALL))
                                    .select_text()
                                    .p_2()
                                    .rounded(move || radius::sm())
                                    .bg(move || c::secondary()),
                            )
                            .child(
                                view().flex_row().gap_2().child(btn_with_icon(
                                    "Copy link",
                                    Some(Icon::Copy),
                                    Variant::Secondary,
                                    BtnSize::Md,
                                    move || {
                                        set_clipboard(link.clone());
                                        toast("Invite link copied");
                                    },
                                )),
                            ),
                        Some(Way::Scan) => card().items_center().gap_3().child(
                            when(
                                move || found.get(),
                                move || {
                                    view()
                                        .w_full()
                                        .gap_3()
                                        .child(
                                            view()
                                                .flex_row()
                                                .items_center()
                                                .gap_3()
                                                .child(avatar("Ana Popescu", 1, 44.0))
                                                .child(
                                                    view()
                                                        .flex_1()
                                                        .child(
                                                            text("Ana Popescu")
                                                                .text_size(px(font::body()))
                                                                .font_weight(FontWeight::SEMIBOLD),
                                                        )
                                                        .child(muted("@ana")),
                                                ),
                                        )
                                        .child(
                                            btn("Message Ana", Variant::Secondary, BtnSize::Md, move || {
                                                finish(s, 1, welcome())
                                            })
                                            .w_full(),
                                        )
                                },
                            )
                            .otherwise(|| scanner(200.0, "Point your camera at your friend's code. They'll find it in You → Your code.")),
                        ),
                        Some(Way::Group) => card()
                            .gap_3()
                            .child(labelled_field(
                                "Group name",
                                field("e.g. Family, Book club", group_name)
                                    .on_submit(move |_: &str| create_group())
                                    .auto_capitalize(AutoCapitalize::Words)
                                    .auto_focus(true)
                                    .max_length(60),
                                "You'll get a link to share with your group.",
                                || false,
                            ))
                            .child(
                                view().flex_row().child(
                                    btn("Create group", Variant::Secondary, BtnSize::Md, create_group)
                                        .disabled(move || group_name.with(|n| n.trim().is_empty())),
                                ),
                            ),
                    };
                    panel.bind_motion(&rise).into_element()
                })
            })),
        view().child(primary("Start chatting", move || finish(s, 1, welcome()))),
    )
}
