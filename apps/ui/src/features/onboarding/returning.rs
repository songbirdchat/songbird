//! A returning person: restore a backup, link to their phone, or what to
//! do without the recovery key.

use std::{rc::Rc, time::Duration};

use nsh_core::prelude::*;

use super::{Flow, Step, finish};
use crate::{features::backups, store::Store, ui::*};

pub(super) fn returning(f: Flow) -> View {
    flow_layout(
        view()
            .gap_4()
            .child(flow_title("Welcome back"))
            .child(flow_lead("How would you like to bring your chats to this device?"))
            .child(
                group([
                    Row::new("Restore from a backup")
                        .icon(Icon::Download)
                        .subtitle("Use your recovery key to bring back your messages.")
                        .chevron()
                        .on_press(move || f.go(Step::Restore))
                        .build(),
                    Row::new("Link to another device")
                        .icon(Icon::QrCode)
                        .subtitle("Use Songbird here alongside your phone. Your phone stays the main device.")
                        .chevron()
                        .on_press(move || f.go(Step::LinkDevice))
                        .build(),
                ])
                .mx_0(),
            ),
        view().child(
            btn("I lost my recovery key", Variant::Ghost, BtnSize::Md, move || f.go(Step::LostKey)).w_full(),
        ),
    )
}

pub(super) fn restore(s: Store, f: Flow) -> View {
    let welcome_back = move || {
        let name = s.me.with_untracked(|m| m.name.clone());
        finish(s, 1, format!("Welcome back, {name}"));
    };
    view().flex_1().child(backups::restore_view(
        true,
        Rc::new(welcome_back),
        Some(Rc::new(move || f.go(Step::Invite))),
        Some(f.host),
    ))
}

pub(super) fn link_device(s: Store, f: Flow) -> View {
    let linked = signal(false);
    // The code this device shows; a problem replaces the wait.
    let link = signal(String::new());
    let failed = signal(None::<String>);
    // The account's two-step PIN, when it has one (the server asks before linking).
    let pending = Rc::new(std::cell::RefCell::new(
        None::<crate::backend::link::Pending>,
    ));
    let need_pin = signal(false);
    let wrong_pin = signal(false);
    let pin = signal(String::new());
    let on_step = {
        let pending = pending.clone();
        move |step: Result<crate::backend::link::Step, crate::backend::Problem>| {
            use crate::backend::link::Step;
            match step {
                Ok(Step::Linked) => {
                    let _ = need_pin.try_set(false);
                    let _ = linked.try_set(true);
                }
                Ok(Step::NeedsPin(p)) => {
                    *pending.borrow_mut() = Some(p);
                    let _ = need_pin.try_set(true);
                }
                Ok(Step::WrongPin(p)) => {
                    *pending.borrow_mut() = Some(p);
                    let _ = pin.try_set(String::new());
                    let _ = wrong_pin.try_set(true);
                }
                Err(p) => {
                    let _ = need_pin.try_set(false);
                    let _ = failed.try_set(Some(p.to_string()));
                }
            }
        }
    };
    let on_step = Rc::new(on_step);
    let send_pin = {
        let (pending, on_step) = (pending.clone(), on_step.clone());
        move || {
            let code = pin.get_untracked();
            if !(6..=12).contains(&code.len()) || !code.bytes().all(|b| b.is_ascii_digit()) {
                return;
            }
            let Some(p) = pending.borrow_mut().take() else {
                return;
            };
            wrong_pin.set(false);
            let on_step = on_step.clone();
            spawn(async move { on_step(crate::backend::link::with_pin(p, code).await) });
        }
    };
    // Or one of the account's passkeys, kept by this device's system (D-0177).
    let send_passkey = {
        let (pending, on_step) = (pending.clone(), on_step.clone());
        move || {
            let Some(p) = pending.borrow_mut().take() else {
                return;
            };
            wrong_pin.set(false);
            let on_step = on_step.clone();
            spawn(async move { on_step(crate::backend::link::with_passkey(p).await) });
        }
    };
    // The real backend: a provisioning channel and a fresh X25519 key; the phone
    // scans the code and sends the account sealed to that key.
    let attempt = {
        let on_step = on_step.clone();
        move || {
            failed.set(None);
            link.set(String::new());
            let on_step = on_step.clone();
            spawn(async move {
                let result = match crate::backend::link::open_channel().await {
                    Ok(w) => {
                        let _ = link.try_set(w.code());
                        crate::backend::link::complete(w).await
                    }
                    Err(p) => Err(p),
                };
                on_step(result);
            });
        }
    };
    if !crate::store::demo() {
        // A real device links for real. One that already has an account here (Help → Replay
        // welcome) says so instead of pretending to link again (D-0184).
        if crate::backend::registered() {
            failed.set(Some(
                "This device already has an account. Sign out of it first to link this device to another."
                    .to_string(),
            ));
        } else {
            (attempt.clone())();
        }
    } else {
        // The demo: the "phone" scans this code and approves after a moment.
        set_timeout(Duration::from_millis(4500), move || {
            if f.history
                .try_get()
                .is_some_and(|h| h.last() == Some(&Step::LinkDevice))
            {
                let _ = linked.try_set(true);
            }
        });
        link.set(crate::features::devices::codes::link_code(
            &[0x5B; 16],
            &[0xC3; 32],
        ));
    }
    view().flex_1().child(
        when(
            move || linked.get(),
            move || {
                flow_layout(
                    view()
                        .gap_4()
                        .child(hero_icon(Icon::ShieldCheck))
                        .child(flow_title("Linked"))
                        .child(flow_lead(
                            "This device is linked to your phone. Your chats appear here as they sync, still end\u{2011}to\u{2011}end encrypted.",
                        )),
                    view().child(
                        btn("Continue", Variant::Primary, BtnSize::Lg, move || {
                            let name = s.me.with_untracked(|m| m.name.clone());
                            finish(s, 1, format!("Welcome back, {name}"));
                        })
                        .w_full(),
                    ),
                )
            },
        )
        .otherwise(move || {
            flow_layout(
                view()
                    .gap_4()
                    .child(flow_title("Scan this code with your phone"))
                    .child(
                        view()
                            .role(AccessibilityRole::Image)
                            .label("Code for your phone to scan")
                            .min_h(px(184.0))
                            .items_center()
                            .justify_center()
                            .child(dynamic(move || {
                                let code = link.get();
                                untracked(move || {
                                    if code.is_empty() {
                                        busy_bar("Making a code").into_element()
                                    } else {
                                        qr_code(&code, 184.0).into_element()
                                    }
                                })
                            })),
                    )
                    .child(
                        view()
                            .gap_3()
                            .child(numbered_step(1, "Open Songbird on your phone."))
                            .child(numbered_step(2, "Go to You → Linked devices → Link a device."))
                            .child(numbered_step(3, "Point your phone at this code.")),
                    )
                    .child(muted("If your account has a two-step PIN, this device asks for it.").text_center()),
                view().gap_2().child(when(
                    move || need_pin.get(),
                    {
                        let send_pin = send_pin.clone();
                        let send_passkey = send_passkey.clone();
                        move || {
                            let send_pin = send_pin.clone();
                            let send_passkey = send_passkey.clone();
                            view()
                                .gap_3()
                                .child(passcode_field_pin(pin, send_pin.clone()))
                                .child(when(move || wrong_pin.get(), || {
                                    status_line(
                                        || StatusTone::Problem,
                                        || Icon::Alert,
                                        || "That PIN isn't right. Try again.".into(),
                                    )
                                }))
                                .child(btn("Link this device", Variant::Primary, BtnSize::Lg, send_pin).w_full())
                                .child(crate::backend::passkeys::available().then(|| {
                                    btn("Use a passkey instead", Variant::Ghost, BtnSize::Md, send_passkey).w_full()
                                }))
                        }
                    },
                ).otherwise({
                    let attempt = attempt.clone();
                    move || {
                    let attempt = attempt.clone();
                    when(
                        move || failed.get().is_none(),
                        || {
                            view()
                                .gap_2()
                                .child(busy_bar("Waiting for your phone"))
                                .child(muted("Waiting for your phone…").text_center())
                        },
                    )
                    .otherwise(move || {
                        view()
                            .gap_2()
                            .child(status_line(
                                || StatusTone::Problem,
                                || Icon::Alert,
                                move || failed.get().unwrap_or_default().into(),
                            ))
                            .child(btn("Show a new code", Variant::Primary, BtnSize::Lg, attempt.clone()).w_full())
                    })
                }})),
            )
        }),
    )
}

/// The account's two-step PIN: 6 to 12 digits, sent with Enter.
fn passcode_field_pin(pin: Signal<String>, send: impl Fn() + Clone + 'static) -> View {
    labelled_field(
        "Your two-step PIN",
        field("PIN", pin)
            .on_submit(move |_: &str| send())
            .keyboard_type(KeyboardType::Number)
            .masked(true)
            .auto_correct(false)
            .max_length(12),
        "Your account has a two-step PIN. Enter it to link this device.",
        || false,
    )
}

pub(super) fn lost_key(f: Flow) -> View {
    let section = |title: &'static str, line: &'static str| {
        view()
            .gap_1()
            .child(
                text(title)
                    .text_size(px(font::body()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .role(AccessibilityRole::Header),
            )
            .child(muted(line))
    };
    flow_layout(
        view()
            .gap_4()
            .child(hero_icon(Icon::Key))
            .child(flow_title("Lost your recovery key?"))
            .child(flow_lead(
                "Songbird never sees your recovery key, so we can't show it to you or reset it. That's what keeps your backups private.",
            ))
            .child(section(
                "If Songbird still works on one of your devices",
                "Link this device to it to keep chatting. Then make a new recovery key in You → Chats → Backups, and keep it somewhere safe.",
            ))
            .child(section(
                "If you have no other device",
                "You can start again with a new account. Your old messages can't be brought back, and the people you talk to will see that your safety number changed.",
            )),
        view()
            .gap_2()
            .child(btn("Link to another device", Variant::Primary, BtnSize::Lg, move || f.go(Step::LinkDevice)).w_full())
            .child(btn("Start a new account", Variant::Ghost, BtnSize::Md, move || f.go(Step::Invite)).w_full()),
    )
}
