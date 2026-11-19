//! Account: security notifications, two-step PIN, passkeys, linked
//! devices, the account report, deleting the account.

use nsh_core::prelude::*;

use super::{danger_tile, foot, links, nav_row, section, value_row};
use crate::{store::store, ui::*};

pub fn account() {
    super::push("Account", content);
}

fn content() -> AnyElement {
    let s = store();
    let settings = s.settings;
    let ext = s.ext();
    view()
        .child(section(
            "Security",
            vec![
                toggle_row_icon(
                    Icon::ShieldCheck,
                    "Security notifications",
                    Some("Tell me in the chat when a contact's safety number changes, for example after they get a new phone."),
                    move || ext.with(|x| x.security_notifications),
                    move |on| s.ext_update(|x| x.security_notifications = on),
                ),
                value_row(
                    "Two-step PIN",
                    Some(Icon::Key),
                    move || if settings.with(|x| x.two_step_pin) { "On".into() } else { "Off".into() },
                    links::account_security,
                ),
                value_row(
                    "Passkeys",
                    Some(Icon::Shield),
                    move || if settings.with(|x| x.passkey) { "On".into() } else { "Add".into() },
                    links::account_security,
                ),
                nav_row("Linked devices", Icon::Laptop, links::linked_devices),
            ],
        ))
        .child(foot(
            "Linking a new device or recovering your account needs your PIN or a passkey, so a stolen code isn't enough.",
        ))
        .child(section(
            "Your data",
            vec![
                value_row(
                    "Invites",
                    Some(Icon::Gift),
                    move || format!("{} left", s.session.with(|x| x.invites_left)),
                    links::invites,
                ),
                nav_row("Request account info", Icon::File, request_info),
                value_row(
                    "Delete if I'm away",
                    Some(Icon::Timer),
                    move || format!("After {} months", settings.with(|x| x.inactivity_months)),
                    inactivity,
                ),
                Row::new("Delete my account")
                    .leading(danger_tile(Icon::Trash))
                    .danger()
                    .on_press(delete_account)
                    .build(),
            ],
        ))
        .into_element()
}

/// How long the account may stay unused before the server deletes it
/// (6, 12 or 24 months; the main phone's choice).
fn inactivity() {
    super::push("Delete if I'm away", || {
        let s = store();
        view()
            .child(section(
                "Delete my account if I'm away for",
                [6u8, 12, 24]
                    .into_iter()
                    .map(|m| {
                        radio_row(
                            match m {
                                6 => "6 months",
                                12 => "12 months",
                                _ => "24 months",
                            },
                            None,
                            move || s.settings.with(|x| x.inactivity_months == m),
                            move || s.set_inactivity(m),
                        )
                    })
                    .collect(),
            ))
            .child(foot(
                "Your devices are reminded a month and a week before. Using Songbird on any device restarts the clock.",
            ))
            .into_element()
    });
}

/// What the account report holds: only what the server has.
fn request_info() {
    super::push("Request account info", || {
        let s = store();
        let ext = s.ext();
        view()
            .px_4()
            .pt_4()
            .gap_4()
            .child(body(
                "A report of what Songbird's servers hold about your account. It's short, because they hold very little.",
            ))
            .child(
                card().gap_2().children(
                    [
                        "Your account ID and username",
                        "When your account was made",
                        "Your linked devices and their public keys",
                        "Your plan and invites left",
                        "Your server-side settings, such as who can send you message requests",
                    ]
                    .into_iter()
                    .map(|line| {
                        view()
                            .flex_row()
                            .gap_2()
                            .child(icon(Icon::Check, 16.0).mt(px(2.0)).text_color(move || c::primary()))
                            .child(text(line).flex_1().text_size(px(font::SMALL + 1.0)).line_height(px(20.0)))
                    }),
                ),
            )
            .child(muted(
                "It doesn't include your messages, contacts or profile: those never reach our servers unencrypted.",
            ))
            .child(when(
                move || ext.with(|x| x.account_report.is_none()),
                move || {
                    // The server's GDPR export (Prompt 3, M7), summed up here.
                    btn("Request report", Variant::Primary, BtnSize::Lg, move || {
                        if crate::backend::is_live() && crate::backend::registered() {
                            spawn(async move {
                                match crate::backend::account::export().await {
                                    Ok(v) => {
                                        let summary = crate::backend::account::summarize(&v);
                                        s.ext_update(|x| x.account_report = Some(summary));
                                        toast("Report ready");
                                    }
                                    Err(p) => toast(p.0),
                                }
                            });
                        } else {
                            s.ext_update(|x| x.account_report = Some("This device isn't connected to a Songbird server yet, so there's nothing on a server to report.".into()));
                            toast("Report ready");
                        }
                    })
                },
            ))
            .child(when(
                move || ext.with(|x| x.account_report.is_some()),
                move || {
                    view()
                        .gap_2()
                        .child(banner(Tone::Secure, Icon::Check, "Your report is ready. It stays on this device until you export it."))
                        .child(card().child(body(move || ext.with(|x| x.account_report.clone().unwrap_or_default()))))
                        .child(btn_with_icon("Export report", Some(Icon::Download), Variant::Outline, BtnSize::Md, || {
                            crate::backend::run(async {
                                let bytes = crate::backend::account::report_file().await?;
                                crate::ui::save_file("songbird-account-report.json", bytes);
                                Ok(())
                            });
                        }))
                },
            ))
            .into_element()
    });
}

pub(crate) fn delete_account() {
    confirm(
        "Delete your account?",
        "Your account, username and devices are removed from Songbird's servers. Messages on your devices are erased. This can't be undone.",
        "Delete account",
        // The server journals and deletes every store, then verifies it (Prompt 3, M7).
        || {
            if crate::backend::is_live() && crate::backend::registered() {
                crate::backend::run(async {
                    crate::backend::delete_account().await?;
                    let s = crate::store::store();
                    crate::nav::pop_to_root();
                    s.replay_onboarding();
                    toast("Your account was deleted");
                    Ok(())
                });
            } else {
                toast("This device isn't connected to Songbird, so there's no account to delete.");
            }
        },
    );
}
