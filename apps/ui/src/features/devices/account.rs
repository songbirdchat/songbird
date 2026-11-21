//! Account security, pushed from settings: username and its link, the
//! two-step PIN, passkeys, accounts on this device, the account report
//! and deleting the account.

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{Store, store},
    ui::*,
};

/// Account security, pushed (linked from You → Account).
pub fn account_security() {
    nav::push("account-security", || page("Account", body()));
}

fn body() -> AnyElement {
    let s = store();
    let extra = s.account_extra();
    let switcher_open = signal(false);
    let pin_open = signal(false);
    let delete_open = signal(false);
    let adding_passkey = signal(false);
    // The account's passkeys, as the server lists them (D-0177).
    if crate::backend::is_live() && crate::backend::registered() {
        crate::backend::run(async move { s.refresh_passkeys().await });
    }
    let username = move || s.me.with(|m| m.username.clone());

    let username_rows = group([
        Row::new("Username")
            .icon(Icon::User)
            .trailing(
                text(move || format!("@{}", username()))
                    .text_size(px(font::SMALL + 1.0))
                    .text_color(move || c::muted_fg()),
            )
            .build(),
        view()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(icon_tile(Icon::Link))
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(2.0))
                            .child(text("Username link").text_size(px(font::body())))
                            .child(
                                text(move || extra.with(|a| a.username_link.clone()))
                                    .font_family(MONO)
                                    .text_size(px(font::SMALL))
                                    .number_of_lines(1)
                                    .text_color(move || c::muted_fg())
                                    .select_text(),
                            ),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .gap_2()
                    .pl(px(42.0))
                    .child(btn_with_icon("Copy", Some(Icon::Copy), Variant::Secondary, BtnSize::Sm, move || {
                        set_clipboard(extra.with_untracked(|a| a.username_link.clone()));
                        toast("Username link copied");
                    }))
                    .child(btn_with_icon("Reset link", Some(Icon::Refresh), Variant::Ghost, BtnSize::Sm, move || {
                        confirm(
                            "Reset your username link?",
                            "Your old link and QR code stop working. People who already have you as a contact aren't affected.",
                            "Reset",
                            move || {
                                s.reset_username_link();
                                toast("New username link made");
                            },
                        )
                    })),
            )
            .into_element(),
    ]);

    let pin_rows = group([
        toggle_row_icon(
            Icon::Shield,
            "Two-step PIN",
            Some("Asked when you link a device or register again on a new phone."),
            move || s.settings.with(|x| x.two_step_pin),
            move |v| {
                if v {
                    pin_open.set(true);
                } else {
                    confirm(
                        "Turn off your PIN?",
                        "Anyone who gets your recovery key or a linked device could then link a new device without it.",
                        "Turn off",
                        move || {
                            if crate::backend::is_live() && crate::backend::registered() {
                                spawn(async move {
                                    match crate::backend::account::clear_pin().await {
                                        Ok(()) => s.settings_update(|x| x.two_step_pin = false),
                                        Err(p) => toast(p.0),
                                    }
                                });
                            } else {
                                s.settings_update(|x| x.two_step_pin = false);
                            }
                        },
                    );
                }
            },
        ),
        Row::new("Change PIN")
            .icon(Icon::Key)
            .chevron()
            .on_press(move || pin_open.set(true))
            .build(),
    ]);

    let passkeys = dynamic(move || {
        let keys = extra.with(|a| a.passkeys.clone());
        let busy = adding_passkey.get();
        untracked(move || {
            let mut rows: Vec<AnyElement> = keys
                .into_iter()
                .enumerate()
                .map(|(i, k)| {
                    Row::new(k)
                        .icon(Icon::Key)
                        .subtitle("Added today")
                        .trailing(
                            icon_btn(Icon::Trash, "Remove passkey", move || {
                                confirm(
                                    "Remove this passkey?",
                                    "You can't sign in with it any more.",
                                    "Remove",
                                    move || s.remove_passkey(i),
                                )
                            })
                            .text_color(move || c::destructive()),
                        )
                        .build()
                })
                .collect();
            rows.push(
                Row::new(if busy {
                    "Waiting for your device…"
                } else {
                    "Add a passkey"
                })
                .icon(Icon::Plus)
                .on_press(move || {
                    if adding_passkey.get_untracked() {
                        return;
                    }
                    if !crate::backend::passkeys::available() {
                        toast(
                            "This device can't make passkeys. Add one from your phone or computer.",
                        );
                        return;
                    }
                    adding_passkey.set(true);
                    // The system's passkey sheet, then the server (D-0177).
                    crate::backend::run(async move {
                        let r = s.add_passkey().await;
                        let _ = adding_passkey.try_set(false);
                        r?;
                        toast("Passkey added");
                        Ok(())
                    });
                })
                .build(),
            );
            group(rows).into_element()
        })
    });

    let accounts = dynamic(move || {
        let me = s.me.get();
        untracked(move || {
            group([
                Row::new(me.name.clone())
                    .leading(avatar(me.name.clone(), me.seed, 32.0))
                    .subtitle(format!("@{} · Using now", me.username))
                    .trailing(icon(Icon::Check, 18.0).text_color(move || c::primary()))
                    .build(),
                // One real account per device for now (app-core keeps one store): said plainly
                // instead of a switcher with nothing real behind it (D-0184).
                if crate::store::demo() {
                    Row::new("Switch accounts")
                        .icon(Icon::Users)
                        .subtitle("Up to 3 accounts on this device")
                        .chevron()
                        .on_press(move || switcher_open.set(true))
                        .build()
                } else {
                    Row::new("One account on this device")
                        .icon(Icon::Users)
                        .subtitle("Using a second account on the same device isn't available yet.")
                        .build()
                },
            ])
            .into_element()
        })
    });

    let report = dynamic(move || {
        let state = extra.with(|a| a.report);
        untracked(move || {
            let row = match state {
                None => Row::new("Request account info")
                    .icon(Icon::Download)
                    .subtitle("A report of your account and settings. It never includes messages.")
                    .on_press(move || s.request_account_report()),
                Some(false) => Row::new("Preparing your report…")
                    .icon(Icon::Clock)
                    .subtitle("This takes a moment."),
                Some(true) => Row::new("Export report…")
                    .icon(Icon::Download)
                    .subtitle("Ready. Choose where to save it.")
                    .on_press(move || {
                        crate::backend::run(async {
                            let bytes = crate::backend::account::report_file().await?;
                            crate::ui::save_file("songbird-account-report.json", bytes);
                            Ok(())
                        });
                    }),
            };
            group([row.build()]).into_element()
        })
    });

    view()
        .flex_1()
        .child(page_scroll(
            view()
                .w_full()
                .max_w(px(760.0))
                .self_center()
                // Section titles name the group, never repeat its first row.
                .child(section_title("How people find you"))
                .child(username_rows)
                .child(crate::features::settings::foot("People can find you only with your exact username or link."))
                .child(section_title("Security"))
                .child(pin_rows)
                .child(section_title("Passkeys"))
                .child(passkeys)
                .child(crate::features::settings::foot("Sign in on a new device with your face, fingerprint or screen lock instead of your PIN."))
                .child(section_title("Accounts"))
                .child(accounts)
                .child(section_title("Phone number"))
                .child(group([Row::new("Not needed")
                    .icon(Icon::Phone)
                    .subtitle("Songbird doesn't use phone numbers, so there's nothing to change here.")
                    .build()]))
                .child(section_title("Your data"))
                .child(report)
                .child(view().pt_6().child(group([Row::new("Delete account")
                    .leading(crate::features::settings::danger_tile(Icon::Trash))
                    .danger()
                    .on_press(move || delete_open.set(true))
                    .build()])))
                .child(crate::features::settings::foot("Only your main phone can delete your account.")),
        ))
        .child(account_switcher(switcher_open))
        .child(pin_sheet(s, pin_open))
        .child(delete_sheet(s, delete_open))
        .into_element()
}

/// The quick account switcher, a sheet (also for the settings agent to
/// mount, e.g. on a long press of the profile avatar).
pub fn account_switcher(open: Signal<bool>) -> AnyElement {
    let s = store();
    let extra = s.account_extra();
    app_sheet(
        open,
        "Accounts",
        view().pb_2().gap_2().child(dynamic(move || {
            let me = s.me.get();
            let others = extra.with(|a| a.others.clone());
            untracked(move || {
                let mut rows = vec![
                    Row::new(me.name.clone())
                        .leading(avatar(me.name.clone(), me.seed, 36.0))
                        .subtitle(format!("@{}", me.username))
                        .trailing(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(icon(Icon::Check, 16.0).text_color(move || c::primary()))
                                .child(
                                    text("Current")
                                        .text_size(px(font::SMALL))
                                        .text_color(move || c::muted_fg()),
                                ),
                        )
                        .build(),
                ];
                for (i, a) in others.into_iter().enumerate() {
                    let name = a.name.clone();
                    rows.push(
                        Row::new(a.name.clone())
                            .leading(avatar(a.name.clone(), a.seed, 36.0))
                            .subtitle(format!("@{}", a.username))
                            .on_press(move || {
                                s.switch_account(i);
                                open.set(false);
                                toast(format!("Switched to {name}"));
                            })
                            .build(),
                    );
                }
                // Only the demo lists other accounts; adding one needs a store per account,
                // which app-core doesn't keep yet, so no row offers it (D-0184).
                group(rows).into_element()
            })
        })),
    )
    .into_element()
}

/// Setting or changing the PIN: twice, 6 to 12 digits (the server's rule).
fn pin_sheet(s: Store, open: Signal<bool>) -> nsh_base::SheetElement {
    let pin = signal(String::new());
    let again = signal(String::new());
    let digits = |p: &str| p.chars().filter(char::is_ascii_digit).collect::<String>();
    let long_enough = move || pin.with(|p| (6..=12).contains(&p.len()));
    let same = move || pin.get() == again.get();
    let save = move || {
        if long_enough() && same() {
            let chosen = pin.get_untracked();
            let done = move || {
                s.settings_update(|x| x.two_step_pin = true);
                pin.set(String::new());
                again.set(String::new());
                open.set(false);
                toast("PIN saved");
            };
            // The server keeps only a salted Argon2id verifier (Prompt 3).
            if crate::backend::is_live() && crate::backend::registered() {
                spawn(async move {
                    match crate::backend::account::set_pin(chosen).await {
                        Ok(()) => done(),
                        Err(p) => toast(p.0),
                    }
                });
            } else {
                done();
            }
        }
    };
    app_sheet(
        open,
        "Two-step PIN",
        view()
            .px_4()
            .pb_4()
            .gap_4()
            .child(muted("Choose a PIN you'll remember. Songbird asks for it now and then, so you don't forget it."))
            .child(labelled_field(
                "New PIN",
                field("6 to 12 digits", pin)
                    .on_change(move |t: &str| pin.set(digits(t)))
                    .masked(true)
                    .keyboard_type(KeyboardType::Number)
                    .max_length(12),
                move || if pin.with(|p| !p.is_empty() && !(6..=12).contains(&p.len())) { "Use 6 to 12 digits." } else { "" },
                move || pin.with(|p| !p.is_empty() && !(6..=12).contains(&p.len())),
            ))
            .child(labelled_field(
                "Confirm PIN",
                field("Type it again", again)
                    .on_change(move |t: &str| again.set(digits(t)))
                    .on_submit(move |_: &str| save())
                    .masked(true)
                    .keyboard_type(KeyboardType::Number)
                    .max_length(12),
                move || if again.with(|a| !a.is_empty()) && !same() { "The PINs don't match." } else { "" },
                move || again.with(|a| !a.is_empty()) && !same(),
            ))
            .child(
                btn("Save PIN", Variant::Primary, BtnSize::Md, save).disabled(move || !(long_enough() && same())),
            ),
    )
}

/// Deleting the account: what goes, then the username typed to confirm.
fn delete_sheet(s: Store, open: Signal<bool>) -> nsh_base::SheetElement {
    let typed = signal(String::new());
    let username = move || s.me.with(|m| m.username.clone());
    let matches = move || {
        typed.with(|t| {
            t.trim()
                .trim_start_matches('@')
                .eq_ignore_ascii_case(&username())
        })
    };
    app_sheet(
        open,
        "Delete account",
        view()
            .px_4()
            .pb_4()
            .gap_4()
            .child(banner(
                Tone::Danger,
                Icon::Alert,
                "This deletes your account, your username, your backups and your messages on every linked device. It can't be undone.",
            ))
            .child(muted(
                "Your contacts keep the messages you sent them. You'll leave all your groups.",
            ))
            .child(labelled_field(
                "Type your username to confirm",
                field("Your username", typed).auto_capitalize(AutoCapitalize::None).auto_correct(false),
                "",
                || false,
            ))
            .child(
                btn("Delete account", Variant::Danger, BtnSize::Md, move || {
                    open.set(false);
                    typed.set(String::new());
                    let _ = s;
                    // The server deletes every store (D-0041); this device erases its own.
                    crate::features::settings::account::delete_account();
                })
                .disabled(move || !matches()),
            ),
    )
}
