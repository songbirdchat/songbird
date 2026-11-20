//! A call ringing here (full screen on phones, a card in the corner on
//! desktops), its ended states, and the microphone explainer desktops
//! show before the first call.

use nsh_core::prelude::*;

use super::{
    accept, calls, field, hang_up,
    state::{self, MicAccess, Phase},
    surface::{arrive, ringing_avatar},
    with_call,
};
use crate::{store::Store, ui::*};

/// Rings here: answer, decline, and on desktops answer with video.
pub fn incoming(s: Store, roomy: bool) -> AnyElement {
    if roomy { card(s) } else { full(s) }
}

/// The caller, the kind of call, and whether it still rings.
fn who() -> (String, u64, bool) {
    calls().with_untracked(|x| {
        x.call
            .as_ref()
            .map(|c| (c.title.clone(), c.seed, c.group))
            .unwrap_or_default()
    })
}

/// Phones: the whole screen rings.
fn full(s: Store) -> AnyElement {
    let (name, seed, group) = who();
    let status = field(state::status, String::new());
    let ringing = field(|c| c.phase == Phase::Incoming, false);
    let video = field(|c| c.video, false);
    arrive(0.98, 0.0, motion::SLOW)
        .absolute()
        .inset_0()
        .bg(move || c::call_bg())
        .text_color(move || c::on_call())
        .pt_safe_area()
        .pb_safe_area_plus(px(32.0))
        .child(
            view()
                .flex_row()
                .justify_center()
                .gap_1()
                .items_center()
                .pt_4()
                .text_color(move || c::on_call().opacity(0.75))
                .child(icon(Icon::Lock, 13.0))
                .child(
                    text("End-to-end encrypted")
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::MEDIUM),
                ),
        )
        .child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .px_6()
                .child(ringing_avatar(
                    name.clone(),
                    seed,
                    group,
                    128.0,
                    move || ringing.get(),
                ))
                .child(
                    text(name.clone())
                        .text_size(px(font::DISPLAY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_center()
                        .number_of_lines(2)
                        .role(AccessibilityRole::Header),
                )
                .child(group.then(|| {
                    text("Group call")
                        .text_size(px(font::SMALL + 1.0))
                        .text_color(move || c::on_call().opacity(0.7))
                }))
                .child(
                    text(move || status.get())
                        .text_size(px(font::TITLE))
                        .text_center()
                        .text_color(move || c::on_call().opacity(0.85)),
                ),
        )
        .child(when(
            move || ringing.get(),
            move || {
                view()
                    .flex_row()
                    .justify_around()
                    .px_8()
                    .child(call_btn(
                        || Icon::PhoneOff,
                        || "Decline".into(),
                        CallTone::Decline,
                        || false,
                        72.0,
                        true,
                        move || hang_up(s),
                    ))
                    .child(call_btn(
                        move || {
                            if video.get() {
                                Icon::Video
                            } else {
                                Icon::Phone
                            }
                        },
                        || "Accept".into(),
                        CallTone::Accept,
                        || false,
                        72.0,
                        true,
                        move || accept(video.get_untracked()),
                    ))
            },
        ))
        .into_element()
}

/// Desktops: a small ringing card in the bottom-right corner, over the
/// app but not in its way.
fn card(s: Store) -> AnyElement {
    let (name, seed, group) = who();
    let status = field(state::status, String::new());
    let ringing = field(|c| c.phase == Phase::Incoming, false);
    let video = field(|c| c.video, false);
    arrive(0.96, 12.0, motion::SLOW)
        .absolute()
        .right(px(20.0))
        .bottom(px(20.0))
        .w(px(360.0))
        .p_4()
        .gap_4()
        .rounded(move || radius::lg())
        .bg(move || c::call_bg())
        .text_color(move || c::on_call())
        .border_1()
        .border_color(move || c::on_call().opacity(0.14))
        .shadow_lg()
        .role(AccessibilityRole::Alert)
        .label(format!("{name} is calling"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .child(ringing_avatar(name.clone(), seed, group, 48.0, move || {
                    ringing.get()
                }))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(2.0))
                        .child(
                            text(name.clone())
                                .number_of_lines(1)
                                .text_size(px(font::TITLE))
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            text(move || status.get())
                                .number_of_lines(2)
                                .text_size(px(font::SMALL + 1.0))
                                .text_color(move || c::on_call().opacity(0.8)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .text_color(move || c::on_call().opacity(0.65))
                                .child(icon(Icon::Lock, 12.0))
                                .child(
                                    text(if group {
                                        "Group call · End-to-end encrypted"
                                    } else {
                                        "End-to-end encrypted"
                                    })
                                    .text_size(px(font::CAPTION)),
                                ),
                        ),
                ),
        )
        .child(when(
            move || ringing.get(),
            move || {
                view()
                    // Three equal columns: the circles sit evenly whatever
                    // their captions' lengths.
                    .flex_row()
                    .items_start()
                    .child(view().flex_1().items_center().child(call_btn(
                        || Icon::PhoneOff,
                        || "Decline".into(),
                        CallTone::Decline,
                        || false,
                        48.0,
                        true,
                        move || hang_up(s),
                    )))
                    .child(view().flex_1().items_center().child(call_btn(
                        || Icon::Video,
                        || "Answer with video".into(),
                        CallTone::Neutral,
                        || false,
                        48.0,
                        true,
                        || accept(true),
                    )))
                    .child(view().flex_1().items_center().child(call_btn(
                        || Icon::Phone,
                        || "Answer".into(),
                        CallTone::Accept,
                        || false,
                        48.0,
                        true,
                        move || accept(video.get_untracked()),
                    )))
            },
        ))
        .into_element()
}

/// Desktops, before the first call: why Songbird needs the microphone.
/// Continue hands over to the system's own prompt (demo: allowed at
/// once); after a "Not now", the next call explains how to turn it on.
pub fn permission() -> AnyElement {
    let mic = global::<MicAccess>();
    let denied = mic.get_untracked() == MicAccess::Denied;
    let allow = move || {
        // The system's own prompt follows when the call opens the microphone (macOS asks the
        // first time an app records; Windows and Linux follow their privacy settings).
        mic.set(MicAccess::Allowed);
        let mut started = None;
        with_call(|c| {
            c.permission(true);
            started = Some(c.clone());
        });
        if let Some(call) = started {
            super::start_real(&call);
        }
    };
    let not_now = move || {
        mic.set(MicAccess::Denied);
        with_call(|c| {
            c.permission(false);
        });
    };
    view()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .bg(move || c::overlay())
        .child(
            arrive(0.97, 10.0, motion::SLOW)
                .w(px(420.0))
                .p_6()
                .gap_4()
                .rounded(move || radius::sheet())
                .bg(move || c::popover())
                .shadow_lg()
                .role(AccessibilityRole::Dialog)
                .label("Microphone access")
                .child(
                    view()
                        .size(px(56.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || c::accent())
                        .text_color(move || c::primary())
                        .child(icon(Icon::Mic, 26.0)),
                )
                .child(
                    text("Songbird needs your microphone for calls")
                        .text_size(px(font::HEADING))
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(AccessibilityRole::Header),
                )
                .child(body(
                    "Your computer will ask you to allow it. Songbird uses the microphone only while you're in a call, and calls are end-to-end encrypted.",
                ))
                .child(denied.then(|| {
                    banner(
                        Tone::Info,
                        Icon::HelpCircle,
                        "Microphone access is off. Open your computer's privacy settings, allow Songbird to use the microphone, then press Continue.",
                    )
                }))
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(btn("Not now", Variant::Ghost, BtnSize::Md, not_now))
                        .child(btn("Continue", Variant::Primary, BtnSize::Md, allow)),
                ),
        )
        .into_element()
}
