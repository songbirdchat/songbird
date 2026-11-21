//! Help and support: searchable articles, contact us (with logs only by
//! explicit consent, after a preview of exactly what is sent), the status
//! page, replaying the welcome, the design system, version and licences.

use nsh_core::prelude::*;

use super::catalog::{self, ARTICLES, SAMPLE_LOG, TOPICS, scrub, search};
use crate::{
    nav,
    store::{Plan, store},
    ui::*,
};

/// The help page, pushed (You → Help).
pub fn help() {
    nav::push("help", help_page);
}

fn help_page() -> Screen {
    let s = store();
    let query = signal(String::new());
    let results = computed(move || query.with(|q| search(q)));
    let searching = move || query.with(|q| !q.trim().is_empty());
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(
            view()
                .px_4()
                .pt_4()
                .child(search_field("Search help", query)),
        )
        .child(section_title(move || {
            if searching() {
                "Results"
            } else {
                "Popular articles"
            }
        }))
        .child(when(
            move || results.with(|r| r.is_empty()),
            move || {
                empty_state(
                    Icon::Search,
                    "No articles found",
                    "Try other words, or contact us and a person will help.",
                    Some(
                        btn("Contact us…", Variant::Secondary, BtnSize::Md, contact).into_element(),
                    ),
                )
            },
        ))
        .child(
            view()
                .mx_4()
                .rounded(move || radius::lg())
                .overflow_hidden()
                .bg(move || c::card())
                .border_1()
                .border_color(move || c::border())
                .style_when(
                    move || results.with(|r| r.is_empty()),
                    |st| st.hidden().border_color(AdaptiveColor::TRANSPARENT),
                )
                .child(each(
                    move || results.get(),
                    |i| *i,
                    |i, index| {
                        let a = &ARTICLES[i.get_untracked()];
                        let id = a.id;
                        view()
                            .child(
                                view()
                                    .style_when(move || index.get() == 0, |st| st.hidden())
                                    .child(separator_inset(16.0)),
                            )
                            .child(
                                Row::new(a.title)
                                    .subtitle(a.summary)
                                    .chevron()
                                    .on_press(move || open_article(id))
                                    .build(),
                            )
                    },
                )),
        )
        .child(section_title("Get in touch"))
        .child(group([
            Row::new("Contact us…")
                .icon(Icon::Send)
                .subtitle("A person reads every message.")
                .chevron()
                .on_press(contact)
                .build(),
            // An ordinary row (it opens the status page in the browser),
            // not an underlined link of its own.
            Row::new("Service status")
                .icon(Icon::Globe)
                .subtitle("status.songbird.chat")
                .chevron()
                .on_press(|| {
                    let _ = open_url("https://status.songbird.chat");
                })
                .build(),
        ]))
        .child(section_title("About"))
        .child(group(about_rows(move || {
            nav::pop_to_root();
            s.replay_onboarding();
        })));
    // The page's title is the You row's.
    page("Help and feedback", page_scroll(body))
}

fn about_rows(replay: impl Fn() + 'static) -> Vec<AnyElement> {
    #[allow(unused_mut)]
    let mut rows = vec![
        Row::new("Replay welcome")
            .icon(Icon::Sparkles)
            .subtitle("See the first-run tour again. Your chats stay as they are.")
            .on_press(replay)
            .build(),
        Row::new("App version and licences")
            .icon(Icon::Info)
            .chevron()
            .on_press(licences)
            .build(),
    ];
    // Debug builds only, marked as a developer tool.
    #[cfg(debug_assertions)]
    rows.push(
        Row::new("UI gallery")
            .icon(Icon::Grid)
            .subtitle("Developer preview: every component, colour and icon")
            .chevron()
            .on_press(crate::features::gallery::ui_gallery)
            .build(),
    );
    rows
}

/// A help article, pushed.
pub fn open_article(id: &'static str) {
    nav::push(format!("help:{id}"), move || article_page(id));
}

fn article_page(id: &'static str) -> Screen {
    let a = catalog::article(id).unwrap_or(&ARTICLES[0]);
    let answered = signal(None::<bool>);
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .px_4()
        .pt_6()
        .gap_4()
        .child(
            text(a.title)
                .text_size(px(font::HEADING + 4.0))
                .font_weight(FontWeight::BOLD)
                .role(AccessibilityRole::Header),
        )
        .child(muted(a.summary))
        .children(a.body.iter().map(|p| body(*p).select_text()))
        .child(separator())
        .child(
            view()
                .gap_2()
                .child(
                    text("Was this helpful?")
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(when(
                    move || answered.get().is_none(),
                    move || {
                        view()
                            .flex_row()
                            .gap_2()
                            .child(chip(
                                "Yes",
                                || false,
                                move || {
                                    answered.set(Some(true));
                                    toast("Thanks for telling us");
                                },
                            ))
                            .child(chip("No", || false, move || answered.set(Some(false))))
                    },
                ))
                .child(when(
                    move || answered.get() == Some(true),
                    || muted("Thanks. We use this to improve these pages."),
                ))
                .child(when(
                    move || answered.get() == Some(false),
                    || {
                        view()
                            .gap_2()
                            .child(muted("Sorry about that. A person can help."))
                            .child(view().flex_row().child(btn(
                                "Contact us…",
                                Variant::Secondary,
                                BtnSize::Md,
                                contact,
                            )))
                    },
                )),
        );
    page(a.title, page_scroll(body))
}

// ---------------------------------------------------------------- contact

/// The contact form, pushed.
pub fn contact() {
    nav::push("help-contact", contact_page);
}

/// The log a request attaches, scrubbed on this device: this run's own events (the demo shows
/// a sample).
fn attached_log() -> String {
    if crate::store::demo() {
        scrub(SAMPLE_LOG)
    } else {
        crate::backend::applog::scrubbed()
    }
}

fn contact_page() -> Screen {
    let s = store();
    let sending = signal(false);
    let topic = signal(None::<&'static str>);
    let message = signal(String::new());
    let logs = signal(false);
    let attach = signal(false);
    let ready = move || topic.get().is_some() && message.with(|m| m.trim().len() >= 10);
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .child(section_title("What is it about?"))
        .child(
            view()
                .px_4()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .role(AccessibilityRole::Radiogroup)
                .label("Topic")
                .children(TOPICS.iter().map(move |t| {
                    let t: &'static str = t;
                    chip(t, move || topic.get() == Some(t), move || topic.set(Some(t)))
                })),
        )
        .child(
            view().px_4().pt_5().child(labelled_field(
                "Message",
                field("Tell us what happened", message).multiline(true).h(px(140.0)).py_2(),
                move || {
                    let n = message.with(|m| m.trim().chars().count());
                    if n < 10 { "A few words about what happened, at least 10 characters.".to_string() } else { "Don't include passwords or your recovery key.".to_string() }
                },
                || false,
            )),
        )
        .child(section_title("Your account"))
        .child(group([toggle_row(
            "About my account",
            Some("Attaches your account so we can look into it. Leave it off to write anonymously."),
            move || attach.get(),
            move |v| attach.set(v),
        )]))
        .child(section_title("Logs"))
        .child(group([
            toggle_row(
                "Include redacted logs",
                Some("Technical events since Songbird opened, up to the last 24 hours. Never message content."),
                move || logs.get(),
                move |v| logs.set(v),
            ),
            Row::new("Preview what is sent")
                .icon(Icon::Eye)
                .chevron()
                .on_press(|| nav::push("help-logs", logs_page))
                .build(),
        ]))
        .child(crate::features::settings::foot(move || {
            if logs.get() {
                "You agree to attach the redacted log you can preview above. Names, numbers, addresses and ids are removed on your device before anything is sent. We delete it when your request is closed."
            } else {
                "Logs help us find problems faster. They are attached only if you turn this on."
            }
        }))
        .child(crate::features::settings::foot(move || {
            if s.plan() == Plan::Free {
                "We usually reply within 2 working days."
            } else {
                "Priority support: we usually reply within 1 working day."
            }
        }))
        .child(
            view().px_4().pt_5().flex_row().child(
                btn("Send", Variant::Primary, BtnSize::Md, move || {
                    let Some(t) = topic.get_untracked() else { return };
                    if !ready() {
                        return;
                    }
                    // Exactly the scrubbed text the preview shows, and only with consent.
                    let log = logs.get_untracked().then(attached_log);
                    if sending.get_untracked() {
                        return;
                    }
                    sending.set(true);
                    // Said only once the server has the request (D-0184).
                    s.send_support(t, &message.get_untracked(), log, attach.get_untracked(), move |sent| {
                        let _ = sending.try_set(false);
                        if sent {
                            nav::pop();
                            toast("Sent. We'll get back to you soon.");
                        } else {
                            toast("Your request couldn't be sent. Check your connection and try again.");
                        }
                    });
                })
                .disabled(move || !ready() || sending.get()),
            ),
        );
    page("Contact us", page_scroll(body))
}

/// Exactly what a support request attaches: the scrubbed log.
fn logs_page() -> Screen {
    let scrubbed = attached_log();
    let scrubbed = if scrubbed.is_empty() {
        "Nothing logged yet in this session.".to_string()
    } else {
        scrubbed
    };
    let body = view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .px_4()
        .pt_4()
        .gap_3()
        .child(banner(
            Tone::Secure,
            Icon::ShieldCheck,
            "This is exactly what is attached. Email addresses, phone numbers, usernames, IP addresses and ids are replaced on your device. Logs never contain message content.",
        ))
        .child(
            view()
                .rounded(move || radius::lg())
                .bg(move || c::muted())
                .border_1()
                .border_color(move || c::border())
                .p_3()
                .role(AccessibilityRole::Text)
                .label("Redacted log")
                .child(
                    scroll().horizontal().child(
                        text(scrubbed)
                            .text_size(px(font::CAPTION))
                            .line_height(px(18.0))
                            .font_family(move || nsh_base::theme().font_mono.clone().unwrap_or_else(|| "monospace".into()))
                            .select_text(),
                    ),
                ),
        )
        .child(muted("Replaced items are shown in brackets, such as [email] or [ip]."));
    page("What is sent", page_scroll(body))
}

// ---------------------------------------------------------------- licences

/// Open-source software in Songbird, with its licence.
const LICENCES: &[(&str, &str)] = &[
    ("NSH (Native Shell)", "Apache-2.0"),
    ("Lucide icons", "ISC"),
    ("Rust standard library", "MIT or Apache-2.0"),
    ("serde_json", "MIT or Apache-2.0"),
    ("log", "MIT or Apache-2.0"),
    ("env_logger", "MIT or Apache-2.0"),
    ("Taffy (layout)", "MIT"),
    ("tokio", "MIT"),
    // The camera (D-0110 to D-0117).
    ("qrcode (QR codes)", "MIT or Apache-2.0"),
    ("rqrr (QR scanning)", "MIT or Apache-2.0, and ISC"),
    // IJG's licence asks for this acknowledgement.
    (
        "jpeg-encoder (photos on Windows and Linux). This software is based in part on the work of the Independent JPEG Group.",
        "MIT or Apache-2.0, and IJG",
    ),
    ("AndroidX CameraX (Android)", "Apache-2.0"),
];

/// Version and licences, pushed.
pub fn licences() {
    nav::push("licences", || {
        let body = view()
            .w_full()
            .max_w(px(760.0))
            .self_center()
            .child(
                view()
                    .items_center()
                    .gap_2()
                    .pt_6()
                    .pb_2()
                    .child(
                        view()
                            .size(px(56.0))
                            .rounded(move || radius::lg())
                            .bg(move || c::primary())
                            .items_center()
                            .justify_center()
                            .text_color(move || c::on_primary())
                            .child(icon(Icon::Feather, 28.0)),
                    )
                    .child(
                        text("Songbird")
                            .text_size(px(font::HEADING))
                            .font_weight(FontWeight::BOLD)
                            .role(AccessibilityRole::Header),
                    )
                    .child(muted("Version 0.1.0 (1) · Beta").select_text()),
            )
            .child(section_title("Open-source licences"))
            .child(group(
                LICENCES
                    .iter()
                    .map(|(name, licence)| super::info_row(name, *licence)),
            ))
            .child(crate::features::settings::foot(
                "The full texts of these licences ship with the app.",
            ));
        page("Version and licences", page_scroll(body))
    });
}
