//! The media viewer: full screen on the dark call surface. Swipe or the
//! arrow keys move between the chat's photos and videos, pinch or +/−
//! zoom, Escape closes. Save, Forward, Star, Reply and Delete.

use std::time::Duration;

use nsh_core::{element::ImageSrc, prelude::*};

use crate::{
    features::chat as chat_feature,
    nav,
    store::{Body, Chat, ChatId, Message, MsgId, Store, clock, day_label, store},
    ui::*,
};

// ------------------------------------------------------------------ logic

/// The chat's photos and videos, oldest first.
pub fn media_ids(messages: &[Message]) -> Vec<MsgId> {
    messages
        .iter()
        .filter(|m| matches!(m.body, Body::Image { .. } | Body::Video { .. }))
        .map(|m| m.id)
        .collect()
}

/// The item `delta` steps from `current`, if there is one.
pub fn step(ids: &[MsgId], current: MsgId, delta: i32) -> Option<MsgId> {
    let at = ids.iter().position(|&i| i == current)? as i64 + delta as i64;
    (0..ids.len() as i64)
        .contains(&at)
        .then(|| ids[at as usize])
}

/// The largest `w`:`h` box inside `avail_w` × `avail_h`.
pub fn fit(w: u32, h: u32, avail_w: f32, avail_h: f32) -> (f32, f32) {
    let ratio = w.max(1) as f32 / h.max(1) as f32;
    let (aw, ah) = (avail_w.max(80.0), avail_h.max(80.0));
    if aw / ah > ratio {
        (ah * ratio, ah)
    } else {
        (aw, aw / ratio)
    }
}

/// Why media from this chat can't be saved to the device, if it can't.
pub fn save_blocked(chat: &Chat) -> Option<&'static str> {
    if chat.locked {
        Some("Saving is off in locked chats, so their media stays inside Songbird.")
    } else if chat.disappearing.is_some() {
        Some("Saving is off while disappearing messages are on in this chat.")
    } else {
        None
    }
}

fn duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ------------------------------------------------------------------ page

/// The viewer, as a pushed page with its own dark header.
pub(super) fn viewer_page(chat: ChatId, start: MsgId) -> Screen {
    let s = store();
    let items = computed(move || {
        s.messages
            .with(|m| m.get(&chat).map(|v| media_ids(v)).unwrap_or_default())
    });
    let current = signal(start);
    let msg = computed(move || s.message(chat, current.get()));
    let zoom = signal(1.0_f32);
    let scale = animated_value(1.0);
    let drag = animated_value(0.0);
    let forward_open = signal(false);
    let playing = signal(false);
    let position = signal(0.0_f32);
    let reduced = motion::reduced();

    let set_zoom = move |z: f32| {
        let z = z.clamp(1.0, 4.0);
        zoom.set(z);
        if reduced {
            scale.set(z);
        } else {
            scale.timing(
                z,
                motion::FAST,
                Easing::Bezier(
                    motion::SETTLE.0,
                    motion::SETTLE.1,
                    motion::SETTLE.2,
                    motion::SETTLE.3,
                ),
            );
        }
    };
    let go = move |delta: i32| {
        if let Some(next) = step(&items.get_untracked(), current.get_untracked(), delta) {
            batch(|| {
                current.set(next);
                zoom.set(1.0);
                playing.set(false);
                position.set(0.0);
            });
            scale.set(1.0);
        }
    };
    on_key("escape", |_| nav::pop());
    on_key("left", move |_| go(-1));
    on_key("right", move |_| go(1));
    on_key("plus", move |_| set_zoom(zoom.get_untracked() + 0.5));
    on_key("minus", move |_| set_zoom(zoom.get_untracked() - 0.5));
    // The demo's stand-in playback (a real video's player keeps its own position).
    set_interval(Duration::from_millis(250), move || {
        if playing.get_untracked() {
            let secs = match s.message(chat, current.get_untracked()).map(|m| m.body) {
                Some(Body::Video { secs, .. }) => secs.max(1),
                _ => 1,
            };
            let next = position.get_untracked() + 0.25 / secs as f32;
            if next >= 1.0 {
                position.set(1.0);
                playing.set(false);
            } else {
                position.set(next);
            }
        }
    });

    let counter = move || {
        let ids = items.get();
        let at = ids
            .iter()
            .position(|&i| i == current.get())
            .map_or(0, |i| i + 1);
        format!("{at} of {}", ids.len())
    };
    let has = move |delta: i32| step(&items.get(), current.get(), delta).is_some();
    let is_video =
        move || msg.with(|m| matches!(m.as_ref().map(|m| &m.body), Some(Body::Video { .. })));
    // A real video plays in the platform's player with its own controls and position; only
    // the demo's stand-in has the simulated scrubber (D-0184).
    let demo_video = move || {
        is_video() && !crate::backend::chat::is_media(current.get())
    };

    let stage = view()
        .flex_1()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .pan_values(Some(&drag), None)
        .on_pan_end(move |e: &PanEvent| {
            let swiped = zoom.get_untracked() <= 1.0
                && (e.translation.x.0.abs() > 72.0 || e.velocity_x.abs() > 600.0);
            if swiped {
                let delta = if e.translation.x.0 < 0.0 { 1 } else { -1 };
                drag.set(0.0);
                go(delta);
            } else if reduced {
                drag.set(0.0);
            } else {
                drag.spring(
                    0.0,
                    Spring {
                        velocity: Some(e.velocity_x),
                        ..Spring::default()
                    },
                );
            }
        })
        .pinch_value(&scale)
        .on_pinch_end(move |e: &GestureEvent| set_zoom(e.value))
        .child(dynamic(move || {
            let id = current.get();
            untracked(move || picture(s, chat, id, scale, drag, playing).into_element())
        }))
        .child(when(is_desktop, move || {
            view()
                .absolute()
                .inset_0()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .pointer_events(PointerEvents::BoxNone)
                .child(
                    dark_btn(Icon::Back, "Previous", move || go(-1))
                        .disabled(move || !has(-1))
                        .bg(move || c::on_call().opacity(0.16)),
                )
                .child(
                    dark_btn(Icon::ChevronRight, "Next", move || go(1))
                        .disabled(move || !has(1))
                        .bg(move || c::on_call().opacity(0.16)),
                )
        }));

    nsh_base::nav_page()
        .key(format!("viewer:{chat}"))
        .title("Photo")
        .header_visible(false)
        .child(
            view()
                .flex_1()
                .bg(move || c::call_bg())
                .text_color(move || c::on_call())
                .pt_safe_area()
                .child(top_bar(s, msg, counter, zoom, set_zoom))
                .child(stage)
                .child(caption(msg))
                .child(when(demo_video, move || scrubber(msg, playing, position)))
                .child(action_bar(s, chat, msg, current, items, forward_open))
                .child(app_sheet(
                    forward_open,
                    "Forward to…",
                    forward_sheet(s, chat, current, forward_open),
                )),
        )
}

/// Close, who sent it and when, the position, and zoom on desktops.
fn top_bar(
    s: Store,
    msg: Memo<Option<Message>>,
    counter: impl Fn() -> String + 'static,
    zoom: Signal<f32>,
    set_zoom: impl Fn(f32) + Copy + 'static,
) -> View {
    let who = move || {
        msg.with(|m| {
            m.as_ref()
                .map(|m| s.contact_name(m.from))
                .unwrap_or_default()
        })
    };
    let when = move || {
        msg.with(|m| {
            m.as_ref()
                .map(|m| format!("{}, {}", day_label(m.at), clock(m.at)))
                .unwrap_or_default()
        })
    };
    view()
        .h(px(56.0))
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .child(dark_btn(Icon::Close, "Close", nav::pop))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    text(who)
                        .text_size(px(font::body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .number_of_lines(1),
                )
                .child(
                    text(move || format!("{} · {}", when(), counter()))
                        .text_size(px(font::SMALL))
                        .opacity(0.75)
                        .number_of_lines(1),
                ),
        )
        .child(is_desktop().then(|| {
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    glyph_btn(ZOOM_OUT, "Zoom out", move || {
                        set_zoom(zoom.get_untracked() - 0.5)
                    })
                    .disabled(move || zoom.get() <= 1.0),
                )
                .child(
                    text(move || format!("{}%", (zoom.get() * 100.0).round() as u32))
                        .w(px(48.0))
                        .text_center()
                        .text_size(px(font::SMALL))
                        .tabular_nums(),
                )
                .child(
                    glyph_btn(ZOOM_IN, "Zoom in", move || {
                        set_zoom(zoom.get_untracked() + 0.5)
                    })
                    .disabled(move || zoom.get() >= 4.0),
                )
        }))
}

/// The photo or video stand-in at its aspect ratio, with its description.
/// Fades in when it changes; follows the finger while swiped.
fn picture(
    s: Store,
    chat: ChatId,
    id: MsgId,
    scale: AnimatedValue,
    drag: AnimatedValue,
    playing: Signal<bool>,
) -> View {
    let Some(m) = s.message(chat, id) else {
        return view();
    };
    let (w, h, alt, video) = match &m.body {
        Body::Image { w, h, alt, .. } => (*w, *h, alt.clone(), false),
        Body::Video { caption, .. } => (
            16,
            9,
            if caption.is_empty() {
                "A video".to_string()
            } else {
                caption.clone()
            },
            true,
        ),
        _ => (1, 1, String::new(), false),
    };
    // A real message shows its own photo (or plays its video); a demo one the same
    // illustration as its bubble.
    let real = crate::backend::chat::is_media(m.id);
    let (pw, ph) = (w.max(1), h.max(1));
    let src = if real {
        crate::backend::chat::load_photo(m.id);
        crate::backend::chat::media_src(m.id).unwrap_or_else(|| ImageSrc::from(""))
    } else {
        crate::ui::portrait::message_photo(&alt, m.id, pw, ph)
    };
    let playing_url = signal(None::<String>);
    if real && video {
        crate::backend::chat::media_file(m.id, move |url| playing_url.set(Some(url)));
    }
    let fade = animated_value(if motion::reduced() { 1.0 } else { 0.0 });
    fade.timing(1.0, motion::BASE, Easing::EaseOut);
    let size = move || {
        let win = window();
        let side = if wide() { 84.0 + 360.0 } else { 0.0 };
        // Whole points, so the picture's edges land on the pixel grid.
        // Desktops keep a lane each side for the previous and next
        // buttons (12 + 36 + 12), so they sit beside the picture, not on it.
        let lanes = if is_desktop() { 2.0 * 60.0 } else { 32.0 };
        let (fw, fh) = fit(w, h, win.width.0 - side - lanes, win.height.0 - 250.0);
        (fw.floor(), fh.floor())
    };
    view()
        .w(move || px(size().0))
        .h(move || px(size().1))
        .rounded(move || radius::sm())
        .overflow_hidden()
        .bind_opacity(&fade)
        .bind_scale(&scale)
        .bind_translate_x(&drag)
        .role(AccessibilityRole::Image)
        .label(if video {
            format!("Video: {alt}")
        } else {
            format!("Photo: {alt}")
        })
        .child((!(real && video)).then(|| {
            image(src)
                .absolute()
                .inset_0()
                .object_fit(ResizeMode::Cover)
                .accessibility_elements_hidden(true)
        }))
        // A real video plays in the platform's player, with its own controls.
        .child((real && video).then(|| {
            view().absolute().inset_0().child(dynamic(move || {
                let url = playing_url.get();
                untracked(move || match url {
                    Some(u) => nsh_core::element::video(u)
                        .controls(true)
                        .absolute()
                        .inset_0()
                        .into_element(),
                    None => view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .child(crate::ui::muted("Loading…"))
                        .into_element(),
                })
            }))
        }))
        .child((video && !real).then(|| {
            view().flex_1().items_center().justify_center().child(
                press_surface(Press::OnDark, move || playing.update(|p| *p = !*p))
                    .accessibility_label(move || {
                        Some(SharedString::from(if playing.get() {
                            "Pause"
                        } else {
                            "Play"
                        }))
                    })
                    .size(px(64.0))
                    .rounded_full()
                    .bg(move || c::call_bg().opacity(0.6))
                    .items_center()
                    .justify_center()
                    .child(dynamic(move || {
                        let p = playing.get();
                        untracked(move || {
                            icon(if p { Icon::Pause } else { Icon::Play }, 28.0).into_element()
                        })
                    })),
            )
        }))
}

fn caption(msg: Memo<Option<Message>>) -> View {
    let text_of = move || {
        msg.with(|m| match m.as_ref().map(|m| &m.body) {
            Some(Body::Image { caption, .. } | Body::Video { caption, .. }) => caption.clone(),
            _ => String::new(),
        })
    };
    view().px_4().py_2().min_h(px(36.0)).items_center().child(
        text(text_of)
            .max_w(px(640.0))
            .text_center()
            .text_size(px(font::body()))
            .line_height(px(font::body() * 1.4))
            .number_of_lines(3),
    )
}

/// The demo video's scrubber: play or pause, position and length.
fn scrubber(msg: Memo<Option<Message>>, playing: Signal<bool>, position: Signal<f32>) -> View {
    let secs = move || match msg.with(|m| m.as_ref().map(|m| m.body.clone())) {
        Some(Body::Video { secs, .. }) => secs,
        _ => 0,
    };
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .pb_2()
        .child(
            dark_btn(Icon::Play, "Play or pause", move || {
                playing.update(|p| *p = !*p)
            })
            .accessibility_label(move || {
                Some(SharedString::from(if playing.get() {
                    "Pause"
                } else {
                    "Play"
                }))
            }),
        )
        .child(
            text(move || duration((position.get() * secs() as f32) as u32))
                .text_size(px(font::CAPTION))
                .tabular_nums(),
        )
        .child(
            view()
                .flex_1()
                .h(px(4.0))
                .rounded_full()
                .overflow_hidden()
                .bg(move || c::overlay())
                .role(AccessibilityRole::Progressbar)
                .label(move || format!("{}% played", (position.get() * 100.0) as u32))
                .child(
                    view()
                        .h_full()
                        .w(move || relative(position.get()))
                        .bg(move || c::on_call()),
                ),
        )
        .child(
            text(move || duration(secs()))
                .text_size(px(font::CAPTION))
                .tabular_nums(),
        )
}

/// Reply, Forward, Star, Save, Delete. Save explains itself when off.
fn action_bar(
    s: Store,
    chat: ChatId,
    msg: Memo<Option<Message>>,
    current: Signal<MsgId>,
    items: Memo<Vec<MsgId>>,
    forward_open: Signal<bool>,
) -> View {
    let blocked = move || s.chat(chat).and_then(|c| save_blocked(&c));
    let starred = move || msg.with(|m| m.as_ref().is_some_and(|m| m.starred));
    let delete = move || {
        let id = current.get_untracked();
        let video =
            msg.with_untracked(|m| matches!(m.as_ref().map(|m| &m.body), Some(Body::Video { .. })));
        confirm(
            if video {
                "Delete this video?"
            } else {
                "Delete this photo?"
            },
            "It's removed from this device. Others in the chat still have it.",
            "Delete for me",
            move || {
                let ids = items.get_untracked();
                let next = step(&ids, id, 1).or_else(|| step(&ids, id, -1));
                s.delete_for_me(chat, id);
                match next {
                    Some(n) => current.set(n),
                    None => nav::pop(),
                }
            },
        );
    };
    view()
        .pb_safe_area_plus(px(8.0))
        .child(
            view()
                .flex_row()
                .justify_center()
                .gap_1()
                .px_2()
                .pt_2()
                .child(bar_action(
                    Icon::Reply,
                    || "Reply".into(),
                    move || {
                        chat_feature::reply_to(chat, current.get_untracked());
                    },
                ))
                .child(bar_action(
                    Icon::Forward,
                    || "Forward".into(),
                    move || forward_open.set(true),
                ))
                .child(
                    bar_action(
                        Icon::Star,
                        move || {
                            if starred() {
                                "Unstar".into()
                            } else {
                                "Star".into()
                            }
                        },
                        move || {
                            s.toggle_star(chat, current.get_untracked());
                        },
                    )
                    .selected(starred),
                )
                .child(
                    bar_action(
                        Icon::Download,
                        || "Save".into(),
                        move || {
                            // The decrypted file, to the library (D-0174).
                            if !crate::backend::chat::save_to_library(
                                current.get_untracked(),
                                false,
                            ) {
                                toast("This photo is a sample: there's no file to save.");
                            }
                        },
                    )
                    .disabled(move || blocked().is_some()),
                )
                .child(bar_action(Icon::Trash, || "Delete".into(), delete)),
        )
        .child(
            text(move || blocked().unwrap_or_default())
                .px_4()
                .pt_1()
                .text_center()
                .text_size(px(font::SMALL))
                .opacity(0.75)
                .style_when(move || blocked().is_none(), |st| st.hidden()),
        )
}

/// An icon over its label, light on dark.
fn bar_action(
    glyph: Icon,
    label: impl Fn() -> String + Copy + 'static,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    press_surface(Press::OnDark, on_press)
        .accessibility_label(move || Some(SharedString::from(label())))
        .w(px(68.0))
        .min_h(px(TOUCH))
        .py_1()
        .items_center()
        .gap_1()
        .rounded(move || radius::control())
        .child(icon(glyph, 22.0))
        .child(
            text(move || label())
                .text_size(px(font::CAPTION))
                .font_weight(FontWeight::MEDIUM),
        )
}

/// A round icon button, light on dark.
fn dark_btn(
    glyph: Icon,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let side = if is_desktop() { 36.0 } else { 44.0 };
    press_surface(Press::OnDark, on_press)
        .accessibility_label(label)
        .size(px(side))
        .rounded_full()
        .items_center()
        .justify_center()
        .child(icon(glyph, 20.0))
}

const ZOOM_IN: &str = r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/><path d="M11 8v6"/><path d="M8 11h6"/>"#;
const ZOOM_OUT: &str =
    r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/><path d="M8 11h6"/>"#;

/// A dark button with a glyph `icons.rs` lacks (zoom in and out, Lucide).
fn glyph_btn(
    shapes: &'static str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> nsh_base::ButtonElement {
    let src = ImageSrc::svg_markup(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{shapes}</svg>"#
    ));
    press_surface(Press::OnDark, on_press)
        .accessibility_label(label)
        .size(px(36.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .child(svg(src).size(px(20.0)).accessibility_elements_hidden(true))
}

/// Recent chats to forward to.
fn forward_sheet(s: Store, from: ChatId, current: Signal<MsgId>, open: Signal<bool>) -> View {
    view().child(dynamic(move || {
        let chats: Vec<Chat> = s
            .inbox()
            .into_iter()
            .filter(|c| c.id != from)
            .take(8)
            .collect();
        untracked(move || {
            view()
                .gap_2()
                .child(group(chats.into_iter().map(|c| {
                    let to = c.id;
                    let title = c.title.clone();
                    Row::new(c.title.clone())
                        .leading(crate::features::groups::chat_avatar_for(&c, 36.0))
                        .on_press(move || {
                            s.forward(from, current.get_untracked(), to);
                            open.set(false);
                            toast(format!("Forwarded to {title}"));
                        })
                        .build()
                })))
                .child(footnote(
                    "Forwarded media is marked as forwarded. It stays end-to-end encrypted.",
                ))
                .into_element()
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_stops_at_the_ends() {
        let ids = [3, 7, 9];
        assert_eq!(step(&ids, 7, 1), Some(9));
        assert_eq!(step(&ids, 7, -1), Some(3));
        assert_eq!(step(&ids, 9, 1), None);
        assert_eq!(step(&ids, 3, -1), None);
        assert_eq!(step(&ids, 4, 1), None);
    }

    #[test]
    fn pictures_fit_their_box() {
        assert_eq!(fit(4, 3, 400.0, 600.0), (400.0, 300.0));
        assert_eq!(fit(3, 4, 800.0, 400.0), (300.0, 400.0));
        let (w, h) = fit(16, 9, 1000.0, 1000.0);
        assert!((w / h - 16.0 / 9.0).abs() < 0.001 && w <= 1000.0);
    }

    #[test]
    fn locked_and_disappearing_chats_keep_media_inside() {
        let mut c = crate::store::ext_groups::tests_support::group();
        assert_eq!(save_blocked(&c), None);
        c.locked = true;
        assert!(save_blocked(&c).is_some());
        c.locked = false;
        c.disappearing = Some(crate::store::Disappear::Week);
        assert!(save_blocked(&c).is_some());
        assert_eq!(duration(95), "1:35");
    }
}
