//! The pages the chat list leads to: Archived, Message requests, Locked
//! chats and Chat lists.

use nsh_core::{element::Position, prelude::*};

use super::{animate_change, filter, list_names, model, row};
use crate::{
    features, nav,
    store::{Store, store},
    ui::*,
};

// ---------------------------------------------------------------- archived

pub fn open_archived() {
    nav::push("chats:archived", || page("Archived", archived()));
}

fn archived() -> impl IntoElement {
    let s = store();
    let settings_open = signal(false);
    let rows = computed(move || {
        s.chats.with(|cs| {
            s.messages.with(|m| {
                s.contacts.with(|ct| {
                    model::rows(cs, m, ct, model::ListScope::Archived, &model::Filter::All)
                })
            })
        })
    });
    let keep = move || s.settings.with(|x| x.keep_archived);
    let note = button_note(
        move || {
            if keep() {
                "These chats stay archived when new messages arrive.".to_string()
            } else {
                "These chats move back to Chats when a new message arrives.".to_string()
            }
        },
        "Change",
        move || settings_open.set(true),
    );
    view()
        .flex_1()
        .child(
            row::chat_list(s, rows, model::ListScope::Archived)
                .header(note)
                .empty(empty_state(
                    Icon::Archive,
                    "No archived chats",
                    "Archive a chat from its menu to move it out of your chat list. You can bring it back any time.",
                    None,
                ))
                .footer(view().h(px(24.0))),
        )
        .child(app_sheet(
            settings_open,
            "Archive settings",
            view()
                .pb_2()
                .child(toggle_row(
                    "Keep chats archived",
                    Some("Archived chats stay archived when you get a new message."),
                    keep,
                    move |on| s.settings_update(|x| x.keep_archived = on),
                )),
        ))
}

/// Muted text with a link-coloured action at its end, all one control.
fn button_note(
    message: impl Fn() -> String + 'static,
    action: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let message = std::rc::Rc::new(message);
    let spoken = message.clone();
    view().px_4().py_3().child(
        nsh_base::button()
            .on_press(on_press)
            .accessibility_label(move || Some(SharedString::from(format!("{} {action}", spoken()))))
            .role(AccessibilityRole::Link)
            .cursor_pointer()
            .rounded(move || radius::sm())
            .focus_visible(|st| st.outline(px(2.0)).outline_color(move || c::ring()))
            .child(
                text(move || format!("{} ", message()))
                    .text_size(px(font::SMALL))
                    .line_height(px(18.0))
                    .text_center()
                    .text_color(move || c::muted_fg())
                    .child(
                        text(action)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || c::link()),
                    ),
            ),
    )
}

// ---------------------------------------------------------------- requests

pub fn open_requests() {
    nav::push("chats:requests", || page("Message requests", requests()));
}

fn requests() -> impl IntoElement {
    let s = store();
    let rows = computed(move || {
        s.chats.with(|cs| {
            s.messages
                .with(|m| s.contacts.with(|ct| model::requests(cs, m, ct)))
        })
    });
    virtual_list(move || rows.with(Vec::len), move |at: Position| {
        let req = computed(move || rows.with(|r| r.get(at.index()).cloned()));
        request_card(s, req)
    })
    .keys(move || rows.with(|r| r.iter().map(|x| x.chat).collect::<Vec<_>>()))
    .estimated_item_size(px(220.0))
    .header(view().px_4().pt_3().pb_1().child(banner(
        Tone::Info,
        Icon::Shield,
        "They're not in your contacts. They won't know you've seen this until you accept.",
    )))
    .empty(empty_state(
        Icon::Inbox,
        "No message requests",
        "When someone who isn't in your contacts writes to you, their first message waits here until you decide.",
        None,
    ))
    .footer(view().h(px(24.0)))
    .flex_1()
}

fn request_card(s: Store, req: Memo<Option<model::RequestRow>>) -> impl IntoElement {
    let get = move |f: fn(&model::RequestRow) -> String| {
        move || req.with(|r| r.as_ref().map(f).unwrap_or_default())
    };
    let chat = move || req.with_untracked(|r| r.as_ref().map(|r| r.chat));
    let who = move || {
        req.with_untracked(|r| {
            r.as_ref()
                .map(|r| format!("@{}", r.username))
                .unwrap_or_default()
        })
    };
    let accept = move || {
        if let Some(id) = chat() {
            animate_change();
            s.accept_request(id);
            toast("Request accepted. They can now see that you read their messages.");
            nav::pop();
            features::chat::open(id);
        }
    };
    let block = move || {
        if let Some(id) = chat() {
            confirm(
                format!("Block {}?", who()),
                "They won't be able to message or call you, and they won't be told.",
                "Block",
                move || {
                    animate_change();
                    s.block_request(id);
                    toast("Blocked");
                },
            );
        }
    };
    let report = move || {
        if let Some(id) = chat() {
            confirm(
                format!("Report {}?", who()),
                "Their messages in this request are sent to Songbird for review, and they're blocked. They won't be told.",
                "Report and block",
                move || {
                    animate_change();
                    s.report_request(id);
                    toast("Reported and blocked");
                },
            );
        }
    };
    view().px_4().pt_3().child(
        card()
            .gap_3()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        view()
                            .size(px(44.0))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(move || c::secondary())
                            .text_color(move || c::muted_fg())
                            .accessibility_elements_hidden(true)
                            .child(icon(Icon::User, 22.0)),
                    )
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(2.0))
                            .child(
                                text(get(|r| format!("@{}", r.username)))
                                    .number_of_lines(1)
                                    .text_size(px(font::body()))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                text("Not in your contacts")
                                    .text_size(px(font::SMALL))
                                    .text_color(move || c::muted_fg()),
                            ),
                    )
                    .child(
                        text(get(|r| r.when.clone()))
                            .text_size(px(font::CAPTION))
                            .tabular_nums()
                            .text_color(move || c::muted_fg()),
                    ),
            )
            // The first message as plain text: links are not opened from
            // a request.
            .child(
                view()
                    .px_3()
                    .py(px(10.0))
                    .rounded(move || radius::bubble())
                    .bg(move || c::bubble_in())
                    .self_start()
                    .child(
                        text(get(|r| r.first.clone()))
                            .number_of_lines(6)
                            .text_size(px(font::body()))
                            .line_height(px(font::body() * 1.35))
                            .text_color(move || c::on_bubble_in()),
                    ),
            )
            .child(
                text(get(|r| match r.more {
                    0 => String::new(),
                    1 => "1 more message".into(),
                    n => format!("{n} more messages"),
                }))
                .text_size(px(font::SMALL))
                .text_color(move || c::muted_fg())
                .style_when(
                    move || req.with(|r| r.as_ref().is_none_or(|r| r.more == 0)),
                    |st| st.hidden(),
                ),
            )
            .child(
                view()
                    .flex_row()
                    .gap_2()
                    .child(
                        btn_with_icon(
                            "Block",
                            Some(Icon::Ban),
                            Variant::Outline,
                            BtnSize::Sm,
                            block,
                        )
                        .flex_1(),
                    )
                    .child(
                        btn_with_icon(
                            "Report",
                            Some(Icon::Flag),
                            Variant::Outline,
                            BtnSize::Sm,
                            report,
                        )
                        .flex_1(),
                    )
                    .child(btn("Accept", Variant::Primary, BtnSize::Sm, accept).flex_1()),
            ),
    )
}

// ---------------------------------------------------------------- locked

pub fn open_locked() {
    nav::push("chats:locked", || page("Locked chats", locked()));
}

fn locked() -> impl IntoElement {
    let s = store();
    let unlocked = signal(false);
    let rows = computed(move || {
        s.chats.with(|cs| {
            s.messages.with(|m| {
                s.contacts.with(|ct| {
                    model::rows(cs, m, ct, model::ListScope::Locked, &model::Filter::All)
                })
            })
        })
    });
    view().flex_1().child(
        when(unlocked, move || {
            row::chat_list(s, rows, model::ListScope::Locked)
                // `footnote` brings its own 16pt gutter; centred as the
                // Archived page's note.
                .header(view().pt_1().pb_3().child(footnote(
                    "Locked chats don't show in your chat list or in search. To lock or unlock a chat, use its menu.",
                ).text_center()))
                .empty(empty_state(Icon::Unlock, "No locked chats", "Chats you lock show here.", None))
        })
        .otherwise(move || {
            // Locked chats open with the app passcode (kept only on this device, a slow hash,
            // with the lock screen's wait after wrong tries). Face ID and fingerprints join it
            // with the native host projects.
            if !crate::backend::lock::is_set() {
                return empty_state(
                    Icon::Lock,
                    "Your locked chats",
                    "Locked chats open with your app passcode. Set one in Privacy → App lock.",
                    Some(
                        btn("Set a passcode", Variant::Primary, BtnSize::Md, move || {
                            crate::features::settings::app_lock();
                        })
                        .into_element(),
                    ),
                )
                .into_element();
            }
            let code = signal(String::new());
            let wrong = signal(false);
            let checking = signal(false);
            let submit = move || {
                if checking.get_untracked() {
                    return;
                }
                let entered = code.get_untracked();
                if entered.len() != 6 {
                    return;
                }
                let (_, left) = crate::backend::lock::failures();
                if left > 0 {
                    toast(format!("Too many wrong tries. Wait {} seconds.", left.div_ceil(1000)));
                    code.set(String::new());
                    return;
                }
                checking.set(true);
                spawn(async move {
                    let ok = crate::backend::lock::check(entered).await;
                    checking.set(false);
                    code.set(String::new());
                    if ok {
                        crate::backend::lock::reset_failures();
                        unlocked.set(true);
                    } else {
                        let (_, left) = crate::backend::lock::record_failure();
                        wrong.set(true);
                        // The wait is served here too, then cleared (the next try is allowed).
                        set_timeout(std::time::Duration::from_millis(left), || {
                            crate::backend::lock::wait_left(0);
                        });
                    }
                });
            };
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_4()
                .px_6()
                .child(icon(Icon::Lock, 32.0))
                .child(text("Enter your passcode to see locked chats").text_center())
                .child(passcode_cells("Passcode", code, move || wrong.get(), submit))
                .into_element()
        }),
    )
}

// ---------------------------------------------------------------- lists

pub fn open_lists() {
    nav::push("chats:lists", || page("Chat lists", lists()));
}

fn lists() -> impl IntoElement {
    let s = store();
    let draft = signal(String::new());
    let f = filter();
    let create = move || {
        let name = draft.with(|d| d.trim().to_string());
        if name.is_empty() {
            return;
        }
        if list_names(s).iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            toast(format!("You already have a list called {name}"));
            return;
        }
        s.create_list(name.clone());
        draft.set(String::new());
        toast(format!(
            "List {name} created. Add chats to it from a chat's menu."
        ));
    };
    let counts = move |name: &str| {
        s.chats.with(|cs| {
            cs.iter()
                .filter(|c| c.lists.iter().any(|l| l == name))
                .count()
        })
    };
    page_scroll(
        view()
            .gap_1()
            .child(section_title("Your lists"))
            .child(dynamic(move || {
                let names = list_names(s);
                if names.is_empty() {
                    return footnote("You have no lists yet.").into_element();
                }
                group(names.into_iter().map(|name| {
                    let n = counts(&name);
                    let shown = name.clone();
                    let gone = name.clone();
                    Row::new(name.clone())
                        .icon(Icon::Layers)
                        .subtitle(if n == 1 {
                            "1 chat".to_string()
                        } else {
                            format!("{n} chats")
                        })
                        .on_press(move || {
                            f.update(|x| x.list = model::Filter::List(shown.clone()));
                            nav::pop();
                        })
                        .trailing(icon_btn(
                            Icon::Trash,
                            format!("Delete list {name}"),
                            move || {
                                let gone = gone.clone();
                                confirm(
                                    format!("Delete the list {gone}?"),
                                    "The chats in it stay in your chat list.",
                                    "Delete list",
                                    move || {
                                        s.delete_list(&gone);
                                    },
                                );
                            },
                        ))
                        .build()
                }))
                .into_element()
            }))
            .child(section_title("New list"))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .child(
                        field("List name, like Neighbours", draft)
                            .flex_1()
                            .on_submit(move |_: &str| create()),
                    )
                    .child(
                        btn("Create", Variant::Primary, BtnSize::Md, create)
                            .disabled(move || draft.with(|d| d.trim().is_empty())),
                    ),
            )
            .child(footnote(
                "Your lists show above your chats, beside All. Only you see them.",
            )),
    )
}
