//! A9 notification settings: what notifies, sounds, reactions, the
//! private preview option, ringtones, and how muting and per-chat
//! settings work. Desktop: the notification and its actions.

use nsh_core::prelude::*;

use super::{foot, section_plain, value_row};
use crate::{
    store::{MESSAGE_TONES, RINGTONES, store},
    ui::*,
};

pub fn notifications() {
    super::push("Notifications", content);
}

fn content() -> AnyElement {
    let s = store();
    let st = s.settings;
    let ext = s.ext();
    view()
        .child(section_plain(
            "Messages",
            vec![
                toggle_row("Show notifications", None, move || st.with(|x| x.notify_messages), move |v| {
                    s.settings_update(|x| x.notify_messages = v)
                }),
                toggle_row(
                    "Reaction notifications",
                    Some("Tell me when someone reacts to my messages."),
                    move || st.with(|x| x.reactions_messages),
                    move |v| s.settings_update(|x| x.reactions_messages = v),
                ),
                value_row("Message tone", None, move || ext.with(|x| x.message_tone.into()), message_tone),
            ],
        ))
        .child(section_plain(
            "Groups",
            vec![
                toggle_row("Show notifications", None, move || st.with(|x| x.notify_groups), move |v| {
                    s.settings_update(|x| x.notify_groups = v)
                }),
                toggle_row(
                    "Reaction notifications",
                    Some("Tell me when someone reacts to my messages in groups."),
                    move || st.with(|x| x.reactions_groups),
                    move |v| s.settings_update(|x| x.reactions_groups = v),
                ),
            ],
        ))
        .child(section_plain(
            "Calls",
            vec![
                toggle_row("Show notifications", None, move || st.with(|x| x.notify_calls), move |v| {
                    s.settings_update(|x| x.notify_calls = v)
                }),
                value_row("Ringtone", None, move || ext.with(|x| x.ringtone.into()), ringtone),
            ],
        ))
        .child(section_plain(
            "Sound and privacy",
            vec![
                toggle_row(
                    "Play sounds",
                    Some("Sounds for incoming messages and calls while Songbird is open."),
                    move || st.with(|x| x.notify_sound),
                    move |v| s.settings_update(|x| x.notify_sound = v),
                ),
                toggle_row(
                    "Hide sender and message",
                    Some("Notifications only say \"New message\". Open Songbird to see who wrote and what."),
                    move || st.with(|x| x.hide_notification_content),
                    move |v| s.settings_update(|x| x.hide_notification_content = v),
                ),
            ],
        ))
        .child(when(is_desktop, preview))
        .child(section_title("Good to know"))
        .child(
            view()
                .mx_4()
                .gap_3()
                .child(note(
                    Icon::BellOff,
                    "Muting a chat",
                    "Mute any chat for 8 hours, 1 week or always from its info. Muted chats still count messages, quietly, and mentions of you still notify.",
                ))
                .child(note(
                    Icon::Sliders,
                    "Per-chat settings",
                    "A chat's own sound or notification setting, set in its info, comes before these.",
                ))
                .child(note(
                    Icon::Lock,
                    "What Apple and Google see",
                    "Only that something arrived for your device. The message is opened on your device.",
                )),
        )
        .into_element()
}

fn note(glyph: Icon, title: &'static str, line: &'static str) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_3()
        .child(icon_tile(glyph))
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    text(title)
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::MEDIUM),
                )
                .child(muted(line)),
        )
}

/// Desktop: what a notification looks like, with its actions.
fn preview() -> View {
    let s = store();
    let hidden = move || s.settings.with(|x| x.hide_notification_content);
    view()
        .child(section_title("On this computer"))
        .child(
            // The notification itself is the only surface: no card around it.
            view().mx_4().gap_3().child(
                view()
                    .gap_3()
                    .child(
                        view()
                            .p_3()
                            .gap_2()
                            .rounded(move || radius::control())
                            .bg(move || c::popover())
                            .border_1()
                            .border_color(move || c::border())
                            .shadow_md()
                            .label(move || if hidden() { "Notification preview: New message".to_string() } else { "Notification preview: Ana Popescu, Are you coming tonight?".to_string() })
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        view()
                                            .size(px(28.0))
                                            .rounded(move || radius::sm())
                                            .items_center()
                                            .justify_center()
                                            .bg(move || c::primary())
                                            .text_color(move || c::on_primary())
                                            .child(icon(Icon::Feather, 16.0)),
                                    )
                                    .child(
                                        view()
                                            .flex_1()
                                            .child(
                                                text(move || if hidden() { "Songbird" } else { "Ana Popescu" })
                                                    .text_size(px(font::body()))
                                                    .font_weight(FontWeight::SEMIBOLD),
                                            )
                                            .child(muted(move || if hidden() { "New message" } else { "Are you coming tonight?" })),
                                    )
                                    .child(text("now").text_size(px(font::CAPTION)).text_color(move || c::muted_fg())),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .gap_2()
                                    .child(btn("Reply", Variant::Secondary, BtnSize::Sm, || toast("You can reply right from the notification")))
                                    .child(btn("Mark as read", Variant::Ghost, BtnSize::Sm, || toast("Marked as read"))),
                            ),
                    )
                    .child(muted(
                        "Reply or mark as read right from the notification, without leaving what you're doing.",
                    )),
            ),
        )
}

pub fn ringtone() {
    tone_page("Ringtone", "Songbird ringtones", &RINGTONES, true);
}

fn message_tone() {
    tone_page("Message tone", "Message tones", &MESSAGE_TONES, false);
}

fn tone_page(
    title: &'static str,
    heading: &'static str,
    tones: &'static [&'static str],
    ring: bool,
) {
    super::push(title, move || {
        let s = store();
        let ext = s.ext();
        view()
            .child(super::section_plain(
                heading,
                tones
                    .iter()
                    .map(|t| {
                        let t: &'static str = t;
                        radio_row(
                            t,
                            None,
                            move || {
                                ext.with(|x| {
                                    if ring {
                                        x.ringtone == t
                                    } else {
                                        x.message_tone == t
                                    }
                                })
                            },
                            move || {
                                s.ext_update(|x| {
                                    if ring {
                                        x.ringtone = t
                                    } else {
                                        x.message_tone = t
                                    }
                                });
                                if t == "None" {
                                    crate::sounds::stop();
                                } else {
                                    crate::sounds::play(t);
                                    announce_for_accessibility(format!("Playing {t}"));
                                }
                            },
                        )
                    })
                    .collect(),
            ))
            .child(foot(
                "Songbird's own sounds. A chat can have its own, set in its info.",
            ))
            .into_element()
    });
}
