//! "Transfer chat history to a new phone": both phones on one network,
//! paired by a code one shows and the other scans, then a direct,
//! encrypted copy (Prompt 10; PROTOCOL §11.9).
//!
//! The new phone listens and shows a QR code; the old phone scans it (or
//! pastes it), and both show the same six digits, which the user confirms
//! on both phones: the old one sends nothing before, the new one answers
//! and applies nothing before (D-0125). The new phone verifies the whole
//! backup before it says yes, then becomes the account's main phone with
//! the old phone's hand-over.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use nsh_core::prelude::*;

use crate::{
    backend::backup::{self as bk, Sending},
    nav,
    store::{
        ext_onboarding::{TransferStage, thousands},
        store,
    },
    ui::*,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Intro,
    /// This is the old phone: it scans the new one's code.
    Old,
    /// This is the new phone: it shows its code.
    New,
    /// Old phone: connected; the user compares the six digits.
    Compare,
    /// New phone: connected; waiting for the old phone's go-ahead.
    Waiting,
    Pairing,
    Moving,
    Done,
    Failed,
}

/// "482 913".
fn six(code: u32) -> String {
    format!("{:03} {:03}", code / 1000, code % 1000)
}

pub(super) fn transfer_body() -> AnyElement {
    let s = store();
    let stage = s.transfer_stage();
    let role = signal(None::<bool>);
    // What this phone shows or compares, and what went wrong.
    let qr = signal(String::new());
    let digits = signal(None::<u32>);
    let failure = signal(None::<String>);
    let pasted = signal(String::new());
    let sending: Stored<Option<Sending>> = stored(None);
    // The new phone: the user's answer to "do both phones show this code?".
    let confirm: Stored<Option<tokio::sync::oneshot::Sender<bool>>> = stored(None);
    let confirmed = signal(false);
    // Work started from a page's button must outlive that page's view (the
    // screen changes as soon as it starts), but not the transfer page: it
    // runs under the page's own owner.
    let page = Owner::current();
    let sent = Arc::new(AtomicU32::new(0));
    let total = signal(0u32);
    let screen = computed(move || match stage.get() {
        TransferStage::Idle => match (role.get(), digits.get(), failure.with(Option::is_some)) {
            (_, _, true) => Screen::Failed,
            (None, _, _) => Screen::Intro,
            (Some(true), None, _) => Screen::Old,
            (Some(true), Some(_), _) => Screen::Compare,
            (Some(false), None, _) => Screen::New,
            (Some(false), Some(_), _) => Screen::Waiting,
        },
        TransferStage::Pairing => Screen::Pairing,
        TransferStage::Moving(_) => Screen::Moving,
        TransferStage::Done => Screen::Done,
    });
    let moved = move || match stage.get() {
        TransferStage::Moving(p) => p,
        TransferStage::Done => 1.0,
        _ => 0.0,
    };
    let fail = move |why: String| {
        stage.set(TransferStage::Idle);
        failure.set(Some(why));
    };
    let restart = move || {
        stage.set(TransferStage::Idle);
        failure.set(None);
        digits.set(None);
        qr.set(String::new());
        pasted.set(String::new());
        sending.replace(None);
        if let Some(c) = confirm.replace(None) {
            let _ = c.send(false);
        }
        confirmed.set(false);
        role.set(None);
    };
    // The old phone: connects to the code it read.
    let connect = move |text: String| {
        if digits.get_untracked().is_some() || stage.get_untracked() != TransferStage::Idle {
            return;
        }
        stage.set(TransferStage::Pairing);
        drop(page.run(|| {
            spawn(async move {
                match bk::transfer_connect(text).await {
                    Ok(c) => {
                        digits.set(Some(c.code));
                        sending.replace(Some(c));
                        stage.set(TransferStage::Idle);
                    }
                    Err(p) => fail(p.0),
                }
            })
        }));
    };
    // The old phone, after the user saw the same digits on both: sends.
    let send = {
        let sent = sent.clone();
        move || {
            let Some(c) = sending.replace(None) else {
                return;
            };
            let snap = bk::snapshot(s);
            total.set(snap.messages.max(1));
            let sent2 = sent.clone();
            sent.store(0, Ordering::Relaxed);
            stage.set(TransferStage::Moving(0.0));
            let poll = page.run(|| {
                set_interval(Duration::from_millis(150), move || {
                    let n = sent2.load(Ordering::Relaxed);
                    if let TransferStage::Moving(_) = stage.get_untracked() {
                        stage.set(TransferStage::Moving(
                            (n as f32 / (total.get_untracked() + 200) as f32).min(0.95),
                        ));
                    }
                })
            });
            let sent3 = sent.clone();
            drop(page.run(|| {
                spawn(async move {
                    let handing = bk::handover().await;
                    let r = bk::transfer_send(c, snap, sent3, handing).await;
                    poll.cancel();
                    match r {
                        Ok(()) => stage.set(TransferStage::Done),
                        Err(p) => fail(p.0),
                    }
                })
            }));
        }
    };
    // The new phone: listens, shows its code, receives, verifies, recovers.
    let listen = move || {
        drop(page.run(|| {
            spawn(async move {
                let (l, code) = match bk::transfer_listen().await {
                    Ok(x) => x,
                    Err(p) => return fail(p.0),
                };
                qr.set(code.to_qr().to_string());
                let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(1);
                let (yes, answer) = tokio::sync::oneshot::channel::<bool>();
                confirm.replace(Some(yes));
                spawn(async move {
                    if let Some(c) = rx.recv().await {
                        let _ = digits.try_set(Some(c));
                    }
                });
                let Some(into) = bk::transfer_folder() else {
                    return fail("This phone has no folder for the transfer.".into());
                };
                match bk::transfer_receive(l, code, tx, answer, into).await {
                    Ok((restored, hello)) => {
                        stage.set(TransferStage::Moving(0.95));
                        if let Err(p) = bk::recover(&restored, bk::handover_of(&hello), None).await
                        {
                            return fail(p.0);
                        }
                        bk::apply(s, &restored);
                        if let Err(p) = bk::import_core(s, &restored).await {
                            return fail(p.0);
                        }
                        s.backup_extra().update(|b| {
                            b.messages = restored.messages;
                            b.chats = restored.chats;
                        });
                        stage.set(TransferStage::Done);
                    }
                    Err(p) => fail(p.0),
                }
            })
        }));
    };
    view()
        .flex_1()
        .child(dynamic(move || {
            let sc = screen.get();
            let send = send.clone();
            untracked(move || {
                let enter = motion().entering(
                    MotionSpec::new(KeyframeTiming::new(motion::BASE).easing(Easing::EaseOut))
                        .values(MotionChannel::Opacity, [0.0, 1.0])
                        .values(MotionChannel::TranslateX, [24.0, 0.0]),
                );
                let body = match sc {
                    Screen::Intro => flow_layout(
                        view()
                            .gap_4()
                            .child(hero_icon(Icon::Smartphone))
                            .child(flow_title("Move your chats to a new phone"))
                            .child(flow_lead(
                                "Your chats go straight from one phone to the other over your Wi\u{2011}Fi, encrypted. Nothing goes through Songbird's servers.",
                            ))
                            .child(
                                view()
                                    .gap_3()
                                    .child(numbered_step(1, "Keep both phones on the same Wi\u{2011}Fi, close to each other."))
                                    .child(numbered_step(2, "Open Songbird on both, and come to this screen on each."))
                                    .child(numbered_step(3, "Scan the code your new phone shows with your old phone.")),
                            ),
                        view()
                            .gap_2()
                            .child(btn("This is my old phone", Variant::Primary, BtnSize::Lg, move || role.set(Some(true))).w_full())
                            .child(btn("This is my new phone", Variant::Secondary, BtnSize::Lg, move || {
                                role.set(Some(false));
                                listen();
                            }).w_full()),
                    ),
                    Screen::Old => {
                        let session = crate::camera::use_session(
                            songbird_camera::Purpose::ScanCode,
                            move || screen.get() == Screen::Old,
                            move |text| {
                                if text.starts_with("songbird-transfer:") {
                                    connect(text);
                                }
                            },
                        );
                        flow_layout(
                            view()
                                .gap_4()
                                .child(flow_title("Scan your new phone's code"))
                                .child(crate::camera::camera_view(&session, 240.0, "Point this phone at the code on your new phone."))
                                .child(labelled_field(
                                    "Or paste the code",
                                    field("songbird-transfer:…", pasted)
                                        .font_family(MONO)
                                        .auto_capitalize(AutoCapitalize::None)
                                        .auto_correct(false)
                                        .on_submit(move |t: &str| connect(t.to_owned())),
                                    "Copy it on your new phone if the camera can't read it.",
                                    || false,
                                )),
                            view()
                                .gap_2()
                                .child(
                                    btn("Connect", Variant::Primary, BtnSize::Lg, move || connect(pasted.get_untracked()))
                                        .w_full()
                                        .disabled(move || !pasted.with(|p| p.starts_with("songbird-transfer:"))),
                                )
                                .child(btn("Back", Variant::Ghost, BtnSize::Md, move || role.set(None)).w_full()),
                        )
                    }
                    Screen::New => flow_layout(
                        view()
                            .gap_4()
                            .child(flow_title("Scan this code with your old phone"))
                            .child(flow_lead("On your old phone, open Songbird → You → Chats → Backups → Transfer to a new phone."))
                            .child(when(
                                move || qr.with(|q| !q.is_empty()),
                                move || {
                                    view()
                                        .items_center()
                                        .gap_3()
                                        .child(
                                            view()
                                                .role(AccessibilityRole::Image)
                                                .label("Transfer code for your old phone to scan")
                                                .child(qr_code_live(move || qr.get(), 4.0)),
                                        )
                                        .child(btn_with_icon("Copy code", Some(Icon::Copy), Variant::Ghost, BtnSize::Sm, move || {
                                            crate::ui::copy(qr.get_untracked(), "Code copied. It works once, for the next few minutes.");
                                        }))
                                },
                            ).otherwise(|| busy_bar("Getting ready"))),
                        view().child(btn("Back", Variant::Ghost, BtnSize::Md, restart).w_full()),
                    ),
                    Screen::Compare => flow_layout(
                        view()
                            .gap_4()
                            .child(hero_icon(Icon::ShieldCheck))
                            .child(flow_title("Do both phones show this code?"))
                            .child(numeric_code(move || digits.get().map(six).unwrap_or_default()))
                            .child(flow_lead("If your new phone shows the same six digits, no one else is connected. If not, stop.")),
                        view()
                            .gap_2()
                            .child(btn("They match", Variant::Primary, BtnSize::Lg, move || send()).w_full())
                            .child(btn("They don't match", Variant::Ghost, BtnSize::Md, restart).w_full()),
                    ),
                    Screen::Waiting => flow_layout(
                        view()
                            .gap_4()
                            .child(hero_icon(Icon::ShieldCheck))
                            .child(flow_title("Does your old phone show this code?"))
                            .child(numeric_code(move || digits.get().map(six).unwrap_or_default()))
                            .child(flow_lead("Confirm on both phones. If the digits differ, someone else may be connected: stop, and nothing changes on this phone."))
                            .child(when(move || confirmed.get(), || busy_bar("Waiting for your old phone"))),
                        view()
                            .gap_2()
                            .child(when(
                                move || !confirmed.get(),
                                move || {
                                    btn("They match", Variant::Primary, BtnSize::Lg, move || {
                                        if let Some(c) = confirm.replace(None) {
                                            let _ = c.send(true);
                                            confirmed.set(true);
                                        }
                                    })
                                    .w_full()
                                },
                            ))
                            .child(btn("They don't match", Variant::Ghost, BtnSize::Md, restart).w_full()),
                    ),
                    Screen::Pairing => flow_layout(
                        view()
                            .gap_4()
                            .child(flow_title("Connecting your phones…"))
                            .child(flow_lead("Setting up an encrypted connection that only these two phones can use."))
                            .child(busy_bar("Connecting")),
                        view(),
                    ),
                    Screen::Moving => flow_layout(
                        view()
                            .gap_4()
                            .child(flow_title("Transferring your chats"))
                            .child(flow_lead("Keep both phones open and close together."))
                            .child(progress_bar("Transferring", moved))
                            .child(muted(move || {
                                let t = total.get();
                                if t > 0 {
                                    let n = (moved() * t as f32).round() as u32;
                                    format!("{} of {} messages", thousands(n.min(t)), thousands(t))
                                } else {
                                    "Checking everything arrived…".into()
                                }
                            })),
                        view(),
                    ),
                    Screen::Done => flow_layout(
                        view()
                            .gap_4()
                            .child(hero_icon(Icon::ShieldCheck))
                            .child(flow_title("Transfer complete"))
                            .child(flow_lead(move || {
                                let (m, c) = s.backup_extra().with(|b| (b.messages, b.chats));
                                if role.get() == Some(true) {
                                    format!("{} messages are on your new phone. It is now your main phone, and this one will be signed out.", thousands(total.get()))
                                } else {
                                    format!("{} messages in {} chats are on this phone, and it's now your main phone.", thousands(m), thousands(c))
                                }
                            })),
                        view().child(
                            btn("Done", Variant::Primary, BtnSize::Lg, move || {
                                restart();
                                nav::pop();
                            })
                            .w_full(),
                        ),
                    ),
                    Screen::Failed => flow_layout(
                        view()
                            .gap_4()
                            .child(hero_icon(Icon::Alert))
                            .child(flow_title("The transfer stopped"))
                            .child(flow_lead(move || failure.get().unwrap_or_default())),
                        view().child(btn("Start again", Variant::Primary, BtnSize::Lg, restart).w_full()),
                    ),
                };
                view().flex_1().bind_motion(&enter).child(body).into_element()
            })
        }))
        .into_element()
}
