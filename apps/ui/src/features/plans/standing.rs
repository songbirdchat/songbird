//! A limited or suspended account (Prompt 11, D-0131): a calm banner at the top of Chats and a
//! page that says what it means, what still works, and how to appeal. Never a dead end:
//! receiving, export, deletion and appeals always work.

use nsh_core::prelude::*;

use crate::{
    nav,
    store::{Standing, extras, store},
    ui::*,
};

fn headline(s: Standing) -> &'static str {
    match s {
        Standing::Good => "",
        Standing::Limited => {
            "New conversations are slowed for a while. Your chats with contacts aren't affected."
        }
        Standing::Suspended => {
            "Your account can't send messages or start calls. You can still read, export and appeal."
        }
    }
}

/// The banner (nothing when the account is in good standing).
pub fn standing_banner() -> AnyElement {
    when(
        move || extras().with(|e| e.standing != Standing::Good),
        move || {
            let st = move || extras().with(|e| e.standing);
            view()
                .px_4()
                .pt_2()
                .gap_2()
                // The tone and icon follow the standing if it changes while shown.
                .child(dynamic(move || {
                    let now = st();
                    banner(
                        if now == Standing::Suspended {
                            Tone::Danger
                        } else {
                            Tone::Warning
                        },
                        if now == Standing::Suspended {
                            Icon::Ban
                        } else {
                            Icon::Clock
                        },
                        headline(now),
                    )
                }))
                .child(view().flex_row().child(btn(
                    "What this means",
                    Variant::Secondary,
                    BtnSize::Sm,
                    standing,
                )))
        },
    )
    .into_element()
}

/// The account status page, pushed.
pub fn standing() {
    nav::push("standing", standing_page);
}

fn standing_page() -> Screen {
    let s = store();
    let message = signal(String::new());
    let sent = signal(false);
    let st = move || extras().with(|e| e.standing);
    let ready = move || message.with(|m| m.trim().chars().count() >= 10) && !sent.get();
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(view().px_4().pt_4().child(banner(
            Tone::Info,
            Icon::ShieldCheck,
            "Songbird can't read your messages. This decision came from how the account was used (reports and blocks from people you messaged), never from what you wrote.",
        )))
        .child(section_title("What it means"))
        .child(view().px_4().child(muted(move || {
            SharedString::from(match st() {
                Standing::Limited => "For about a week, you can start only a couple of new conversations an hour, and you can't make new group invite links. It lifts by itself if nothing new comes up.",
                Standing::Suspended => "A moderator suspended this account. You can't send messages, start calls or make links until it's lifted. You still receive messages, and you can export or delete your account.",
                Standing::Good => "Your account is in good standing.",
            })
        })))
        .child(section_title("Appeal"))
        .child(view().px_4().child(labelled_field(
            "Tell us why this is a mistake",
            field("What happened", message).multiline(true).h(px(120.0)).py_2(),
            || "Your appeal names your account so a moderator can review it.".to_string(),
            || false,
        )))
        .child(
            view().px_4().pt_5().flex_row().child(
                btn("Send appeal", Variant::Primary, BtnSize::Md, move || {
                    if !ready() {
                        return;
                    }
                    // Said only once the server has it (D-0184).
                    s.appeal(&message.get_untracked(), move |ok| {
                        if ok {
                            let _ = sent.try_set(true);
                            toast("Appeal sent. A moderator will review it.");
                        } else {
                            toast("Your appeal couldn't be sent. Check your connection and try again.");
                        }
                    });
                })
                .disabled(move || !ready()),
            ),
        );
    page("Account status", page_scroll(body))
}
