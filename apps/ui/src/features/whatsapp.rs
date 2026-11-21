//! A15: WhatsApp chats (third-party chats), honest and simple. Off until
//! the user links it; one screen says plainly what Meta can still see and
//! what doesn't work; unlinking takes effect at once.

use nsh_core::prelude::*;

use crate::{
    features::settings,
    nav,
    store::{WhatsAppLink, extras, store},
    ui::*,
};

/// What works in WhatsApp chats, against Songbird chats.
/// (feature, works in WhatsApp chats)
pub const FEATURES: &[(&str, bool)] = &[
    ("Text, photos, videos and files", true),
    ("Voice messages", true),
    ("Replies and reactions", true),
    ("Disappearing messages sync", false),
    ("Polls", false),
    ("Events", false),
    ("Nudges", false),
    ("View once", false),
    ("Calls", false),
    ("Safety number verification", false),
];

/// The features WhatsApp chats lack, for lists and notes.
pub fn unsupported() -> impl Iterator<Item = &'static str> {
    FEATURES.iter().filter(|(_, ok)| !ok).map(|(f, _)| *f)
}

/// The demo's pairing code (real accounts can't link yet, D-0184): the bridge would return the code WhatsApp
/// issues for "Link with phone number instead".
const PAIRING_CODE: &str = "K7Q2-9XMB";

/// A small marker for WhatsApp chats: a glyph in the WhatsApp token and
/// the word "WhatsApp", so it never relies on colour. For the chat list
/// row and the conversation header.
pub fn whatsapp_marker() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_1()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(move || radius::sm())
        .bg(move || c::secondary())
        .role(AccessibilityRole::Image)
        .label("WhatsApp chat")
        .child(icon(Icon::Chats, 12.0).text_color(move || c::whatsapp()))
        .child(
            text("WhatsApp")
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || c::on_secondary()),
        )
}

/// One calm line where a feature is missing in a WhatsApp chat (an
/// attachment menu, chat info): "Polls aren't available in WhatsApp chats."
pub fn unsupported_note(feature: &str) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_2()
        .px_3()
        .py_2()
        .child(
            icon(Icon::Info, 14.0)
                .mt(px(2.0))
                .text_color(move || c::muted_fg()),
        )
        .child(
            text(format!("{feature} aren't available in WhatsApp chats."))
                .flex_1()
                .text_size(px(font::SMALL))
                .line_height(px(18.0))
                .text_color(move || c::muted_fg()),
        )
}

/// The opt-in screen, pushed: plain words, off by default, "Not now".
pub fn opt_in() {
    nav::push("whatsapp-opt-in", || {
        page("WhatsApp chats", opt_in_screen(true))
    });
}

/// The opt-in as a whole screen: the explanation scrolls, and Not now /
/// Link WhatsApp stay in a footer at the bottom, never below the fold.
/// `pushed`: "Not now" goes back.
fn opt_in_screen(pushed: bool) -> View {
    let s = store();
    let linking = signal(false);
    let footer = view()
        .w_full()
        .border_t(hairline())
        .border_color(move || c::border())
        .bg(move || c::background())
        .style_when(
            move || linking.get(),
            |st| st.hidden().border_color(AdaptiveColor::TRANSPARENT),
        )
        .child(
            view()
                .w_full()
                .max_w(px(760.0))
                .self_center()
                .px_4()
                .pt_3()
                .pb_safe_area_plus(px(12.0))
                .flex_row()
                .gap_3()
                .child(view().flex_1().child(btn(
                    "Not now",
                    Variant::Outline,
                    BtnSize::Lg,
                    move || {
                        if pushed {
                            nav::pop();
                        }
                    },
                )))
                .child(view().flex_1().child(btn(
                    "Link WhatsApp…",
                    Variant::Primary,
                    BtnSize::Lg,
                    move || {
                        linking.set(true);
                        s.begin_whatsapp_link(move || {
                            toast("WhatsApp linked. Your chats will appear in a moment.");
                        });
                    },
                ))),
        );
    view()
        .flex_1()
        .child(page_scroll(opt_in_body(s, linking)))
        .child(footer)
}

/// The explanation, and the link panel while linking.
fn opt_in_body(s: crate::store::Store, linking: Signal<bool>) -> View {
    view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .px_4()
        .pt_6()
        .gap_4()
        .child(
            // The standard lavender squircle, never a WhatsApp-green tile.
            view()
                .size(px(56.0))
                .rounded(move || radius::lg())
                .bg(move || c::accent())
                .border(hairline())
                .border_color(move || c::border())
                .items_center()
                .justify_center()
                .text_color(move || c::primary())
                .accessibility_elements_hidden(true)
                .child(icon(Icon::Chats, 28.0)),
        )
        .child(
            // The intro title and lead every guided screen uses (type scale).
            flow_title("See your WhatsApp chats in Songbird"),
        )
        .child(flow_lead(
            "While your friends switch, you can link WhatsApp and keep talking to them from here. It's off unless you turn it on.",
        ))
        .child(
            card()
                .gap_3()
                .child(
                    text("Good to know")
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(AccessibilityRole::Header),
                )
                .child(bullet(Icon::Lock, c::primary, "WhatsApp chats stay end\u{2011}to\u{2011}end encrypted."))
                .child(bullet(
                    Icon::Eye,
                    c::primary,
                    "Meta can still see some information about them: who you talk to, when, and from which phone number.",
                ))
                .child(bullet(Icon::Shield, c::primary, "Your Songbird chats are never shared with WhatsApp."))
                .child(bullet(Icon::LogOut, c::muted_fg, "You can unlink at any time. It takes effect immediately.")),
        )
        .child(
            card()
                .gap_3()
                .child(
                    text("Not available in WhatsApp chats")
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(AccessibilityRole::Header),
                )
                .children(unsupported().map(|f| bullet(Icon::Ban, c::muted_fg, f))),
        )
        .child(when(
            move || linking.get(),
            move || {
                link_panel(move || {
                    s.cancel_whatsapp_link();
                    linking.set(false);
                })
            },
        ))
}

/// Linking: a QR code or a pairing code, while WhatsApp waits.
fn link_panel(cancel: impl Fn() + Copy + 'static) -> View {
    let ext = extras();
    let with_code = signal(false);
    let waiting = move || ext.with(|e| e.whatsapp_link == WhatsAppLink::Pairing);
    card()
        .gap_3()
        .items_center()
        .child(
            view().w_full().child(segmented(
                vec![(false, "QR code"), (true, "Pairing code")],
                move || with_code.get(),
                move |v| with_code.set(v),
            )),
        )
        .child(when(
            move || !with_code.get(),
            || {
                view()
                    .items_center()
                    .gap_2()
                    .child(qr_code("whatsapp-link-session", 200.0).label("QR code to link WhatsApp").role(AccessibilityRole::Image))
                    .child(
                        muted("On your phone, open WhatsApp, then Settings, Linked devices, Link a device, and scan this code.")
                            .text_center(),
                    )
            },
        ))
        .child(when(
            move || with_code.get(),
            || {
                view()
                    .items_center()
                    .gap_2()
                    .child(
                        text(PAIRING_CODE)
                            .text_size(px(font::DISPLAY))
                            .font_weight(FontWeight::BOLD)
                            .letter_spacing(px(2.0))
                            .tabular_nums()
                            .select_text()
                            .label("Pairing code: K 7 Q 2, 9 X M B"),
                    )
                    .child(
                        muted("In WhatsApp, choose Link with phone number instead, and enter this code.")
                            .text_center(),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .style_when(move || !waiting(), |st| st.hidden())
                .child(icon(Icon::Clock, 14.0).text_color(move || c::muted_fg()))
                .child(muted("Waiting for WhatsApp…")),
        )
        .child(
            view()
                .style_when(move || !waiting(), |st| st.hidden())
                .child(btn("Cancel", Variant::Ghost, BtnSize::Md, cancel)),
        )
}

/// WhatsApp chats settings, pushed (You → WhatsApp chats).
pub fn whatsapp_settings() {
    nav::push("whatsapp", whatsapp_settings_page);
}

fn whatsapp_settings_page() -> Screen {
    // Linking needs Songbird's WhatsApp bridge, which waits for the interoperability rules and
    // a legal review (Prompt 14): a real account is told so, never shown a link that isn't
    // there (D-0184). The demo fixtures keep the designed flow.
    if !crate::store::demo() {
        return page("WhatsApp chats", page_scroll(unavailable_body()));
    }
    let s = store();
    let linked = move || s.whatsapp_linked();
    let me = s.me.get_untracked();
    let body = when(linked, move || {
            let me = me.clone();
            page_scroll(view()
                .w_full()
                .max_w(px(760.0))
                .self_center()
                .child(section_title("Status"))
                .child(group([Row::new("Linked")
                    .icon(Icon::Check)
                    // The demo's linked account (only the demo reaches this page linked).
                    // The number keeps its country code, first digit and
                    // last three; the rest are dots, in its own groups.
                    .subtitle(format!("{} · +40\u{00A0}7••\u{00A0}•••\u{00A0}123 · synced just now", me.name))
                    .trailing(whatsapp_marker())
                    .build()]))
                .child(section_title("Show WhatsApp chats"))
                .child(view().px_4().child(segmented(
                    vec![(false, "In all chats"), (true, "Separately")],
                    move || !s.whatsapp_in_all(),
                    move |sep| s.set_whatsapp_separate(sep),
                )))
                .child(crate::features::settings::foot(move || {
                    if s.whatsapp_in_all() {
                        "WhatsApp chats show in All, each with a WhatsApp marker. The WhatsApp filter shows them alone."
                    } else {
                        "WhatsApp chats show only under the WhatsApp filter, apart from your Songbird chats."
                    }
                }))
                .child(section_title("What works"))
                .child(feature_table())
                .child(section_title("Privacy"))
                .child(group([
                    Row::new("What Meta can see")
                        .icon(Icon::Eye)
                        .subtitle("Who you talk to on WhatsApp, and when. Never your Songbird chats.")
                        .build(),
                    Row::new("Privacy centre")
                        .icon(Icon::Shield)
                        .subtitle("What Songbird can see")
                        .chevron()
                        .on_press(settings::privacy_centre)
                        .build(),
                ]))
                .child(view().pt_5().child(group([Row::new("Unlink WhatsApp")
                    .icon(Icon::LogOut)
                    .danger()
                    .on_press(move || {
                        confirm(
                            "Unlink WhatsApp?",
                            "Songbird loses access to your WhatsApp chats immediately and removes them from this device. Your chats stay in WhatsApp.",
                            "Unlink",
                            move || {
                                s.unlink_whatsapp();
                                toast("WhatsApp unlinked");
                            },
                        );
                    })
                    .build()]))))
        })
        .otherwise(|| opt_in_screen(false));
    page("WhatsApp chats", view().flex_1().child(body))
}

/// What a real account sees: WhatsApp chats aren't available yet, and why.
fn unavailable_body() -> View {
    view()
        .w_full()
        .max_w(px(760.0))
        .self_center()
        .px_4()
        .pt_6()
        .gap_4()
        .child(
            view()
                .size(px(56.0))
                .rounded(move || radius::lg())
                .bg(move || c::accent())
                .border(hairline())
                .border_color(move || c::border())
                .items_center()
                .justify_center()
                .text_color(move || c::primary())
                .accessibility_elements_hidden(true)
                .child(icon(Icon::Chats, 28.0)),
        )
        .child(flow_title("WhatsApp chats aren't available yet"))
        .child(flow_lead(
            "Talking to WhatsApp from Songbird needs a bridge between the two services. We're building it to the EU's interoperability rules, and it opens only once a legal review is done.",
        ))
        .child(
            card()
                .gap_3()
                .child(bullet(Icon::Shield, c::primary, "Nothing is linked: Songbird has no access to your WhatsApp account."))
                .child(bullet(Icon::Info, c::muted_fg, "When it's ready, it will be off until you turn it on here.")),
        )
}

/// The availability table: each feature with a mark that differs in shape
/// (a tick or a dash) and in words for screen readers.
fn feature_table() -> View {
    // Both mark columns one width, wide enough for a two-line header.
    const COL: f32 = 84.0;
    let head = |label: &'static str, w: Option<f32>| {
        text(label)
            .map(|t| match w {
                Some(w) => t.w(px(w)).text_center(),
                None => t.flex_1(),
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || c::muted_fg())
    };
    view()
        .mx_4()
        .rounded(move || radius::lg())
        .overflow_hidden()
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::border())
        .role(AccessibilityRole::List)
        .child(
            view()
                .flex_row()
                .px_4()
                .py_2()
                .bg(move || c::muted())
                .accessibility_elements_hidden(true)
                .child(head("Feature", None))
                .child(head("In Songbird chats", Some(COL)))
                .child(head("In WhatsApp chats", Some(COL))),
        )
        .children(FEATURES.iter().enumerate().map(|(i, (name, ok))| {
            let ok = *ok;
            let mark = move |yes: bool| {
                view().w(px(COL)).items_center().child(
                    icon(if yes { Icon::Check } else { Icon::Close }, 16.0)
                        .text_color(move || if yes { c::primary() } else { c::muted_fg() }),
                )
            };
            view().child((i > 0).then(|| separator_inset(16.0))).child(
                view()
                    .flex_row()
                    .items_center()
                    .px_4()
                    .py_2()
                    .min_h(px(if is_desktop() { 40.0 } else { 48.0 }))
                    .label(format!(
                        "{name}: in Songbird, yes; in WhatsApp chats, {}",
                        if ok { "yes" } else { "not available" }
                    ))
                    .child(text(*name).flex_1().text_size(px(font::SMALL + 1.0)))
                    .child(mark(true))
                    .child(mark(ok)),
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_lists_are_unsupported() {
        let missing: Vec<_> = unsupported().collect();
        for f in [
            "Disappearing messages sync",
            "Polls",
            "Events",
            "Nudges",
            "View once",
        ] {
            assert!(missing.contains(&f), "{f}");
        }
    }
}
