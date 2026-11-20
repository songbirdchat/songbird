//! The bottom of the conversation: the composer (emoji, a growing
//! input with the chat's draft, attach, send that turns into the mic),
//! the reply and edit bars, the voice recorder; or the bar that replaces
//! the composer (selection, a message request, a blocked contact, an
//! announcement group, a channel).

use std::time::Duration;

use nsh_core::{LayoutCurve, LayoutTransition, TouchEvent, TouchPhase, element::input, prelude::*};

use super::{Conv, Rec, logic};
use crate::{
    store::{Body, ChatKind, Role},
    ui::{floating, *},
};

/// What the bottom of the conversation shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Compose,
    Select,
    Request,
    Blocked,
    AdminsOnly,
    Follower,
}

fn mode(cv: Conv) -> Mode {
    let (s, chat) = (cv.s, cv.chat);
    if cv.selecting.get() {
        return Mode::Select;
    }
    let Some(c) = s.chat(chat) else {
        return Mode::Compose;
    };
    if c.request {
        return Mode::Request;
    }
    if s.peer(chat).is_some_and(|p| p.blocked) {
        return Mode::Blocked;
    }
    match c.kind {
        ChatKind::Channel { admin: false, .. } => Mode::Follower,
        _ => {
            let me_admin = c
                .members
                .iter()
                .any(|m| m.contact.is_none() && m.role != Role::Member);
            if c.announcement_only && !me_admin {
                Mode::AdminsOnly
            } else {
                Mode::Compose
            }
        }
    }
}

/// The bottom bar for the current mode, floating over the canvas 12pt in
/// from the pane's sides and bottom (REDESIGN.md "Phone layout"), in the
/// messages' column.
pub(crate) fn bottom(cv: Conv) -> AnyElement {
    let current = computed(move || mode(cv));
    view()
        .w_full()
        .items_center()
        .px(px(INSET))
        .when(cv.wide, |v| v.pb(px(INSET)))
        // Phones: over the home indicator at rest; on the keyboard's edge
        // while it shows (the indicator is under the keyboard then).
        .when(!cv.wide, |v| {
            v.pb(move || {
                px(if keyboard_height().0 > 0.0 {
                    INSET
                } else {
                    safe_area().bottom.0 + INSET
                })
            })
        })
        .pointer_events(PointerEvents::BoxNone)
        .child(
            view()
                .w_full()
                .max_w(px(760.0))
                .pointer_events(PointerEvents::BoxNone)
                .child(dynamic(move || {
                    let m = current.get();
                    Some(untracked(|| match m {
                        Mode::Compose => composer(cv),
                        Mode::Select => select_bar(cv),
                        Mode::Request => request_bar(cv),
                        Mode::Blocked => blocked_bar(cv),
                        Mode::AdminsOnly => {
                            notice_bar(Icon::Megaphone, "Only admins can send messages")
                        }
                        Mode::Follower => follower_bar(cv),
                    }))
                })),
        )
        .into_element()
}

/// How far the composer floats in from the pane's edges.
const INSET: f32 = 12.0;

/// A floating surface in the capsules' language with `corner` radius:
/// the composer (a capsule whose ends keep their one-line radius as it
/// grows) and the bars that replace it.
fn surface(corner: f32) -> View {
    let v = view().style_when(
        || true,
        move |st| floating::capsule_style(st).rounded(px(corner)),
    );
    if skin() == Skin::Apple {
        v.glass_effect(nsh_core::GlassEffect::Regular)
    } else {
        v
    }
}

/// A calm floating bar holding one line or a few actions.
fn bar() -> View {
    surface(spec().radius.card).px_4().py_3().items_center()
}

fn notice_bar(glyph: Icon, line: &'static str) -> AnyElement {
    bar()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(icon(glyph, 16.0).text_color(move || c::muted_fg()))
                .child(muted(line)),
        )
        .into_element()
}

fn follower_bar(cv: Conv) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let muted_now = move || s.chat(chat).is_some_and(|c| c.muted);
    bar()
        .child(btn_with_icon(
            move || if muted_now() { "Unmute" } else { "Mute" },
            Some(Icon::BellOff),
            Variant::Ghost,
            BtnSize::Md,
            move || s.toggle_mute(chat),
        ))
        .into_element()
}

fn blocked_bar(cv: Conv) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    bar()
        .child(
            pressable()
                .on_press(move || {
                    if let Some(p) = s.peer(chat) {
                        s.block(p.id, false);
                        toast(format!("{} unblocked", p.name));
                    }
                })
                .accessibility_label("You blocked this contact. Tap to unblock.")
                .py_1()
                .child(muted("You blocked this contact. Tap to unblock.").text_center()),
        )
        .into_element()
}

/// A message request: Block, Report, Accept (they can't see you read it
/// until you accept).
fn request_bar(cv: Conv) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let peer = s.peer(chat);
    let name = peer
        .as_ref()
        .map(|p| p.username.clone())
        .unwrap_or_default();
    bar()
        .gap_3()
        .child(
            text(format!(
                "@{name} is not in your contacts. They won't know you've seen their message until you accept."
            ))
            .text_size(px(font::SMALL))
            .line_height(px(18.0))
            .text_center()
            .max_w(px(520.0))
            .text_color(move || c::muted_fg()),
        )
        .child(
            view()
                .flex_row()
                .gap_2()
                .w_full()
                .max_w(px(520.0))
                .child(view().flex_1().child(btn("Block", Variant::Danger, BtnSize::Md, move || {
                    if let Some(p) = s.peer(chat) {
                        s.block(p.id, true);
                        s.delete_chat(chat);
                        crate::nav::pop();
                        toast("Blocked and deleted");
                    }
                })))
                .child(view().flex_1().child(btn("Report", Variant::Outline, BtnSize::Md, move || {
                    s.report(chat);
                    toast("Reported. Thank you.");
                })))
                .child(view().flex_1().child(btn("Accept", Variant::Primary, BtnSize::Md, move || {
                    s.accept_request(chat);
                    toast("Request accepted");
                }))),
        )
        .into_element()
}

/// Multi-select: how many, then Star, Copy, Forward, Delete.
fn select_bar(cv: Conv) -> AnyElement {
    let s = cv.s;
    let chat = cv.chat;
    let count = move || cv.selected.with(Vec::len);
    let ids = move || cv.selected.get_untracked();
    let any = move || count() > 0;
    // The selection's actions on one capsule, where the composer was.
    surface(floating::capsule_height() / 2.0)
        .flex_row()
        .items_center()
        .gap_1()
        .min_h(px(floating::capsule_height()))
        .px(px(2.0))
        .child(icon_btn(Icon::Close, "Cancel selection", move || {
            cv.end_select()
        }))
        .child(
            text(move || match count() {
                0 => "Select messages".to_string(),
                1 => "1 selected".to_string(),
                n => format!("{n} selected"),
            })
            .flex_1()
            .text_size(px(font::body()))
            .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            icon_btn(Icon::Star, "Star", move || {
                let ids = ids();
                let all = s.messages.with_untracked(|m| {
                    m.get(&chat).is_some_and(|v| {
                        v.iter().filter(|x| ids.contains(&x.id)).all(|x| x.starred)
                    })
                });
                s.star_messages(chat, &ids, !all);
                toast(if all { "Unstarred" } else { "Starred" });
                cv.end_select();
            })
            .disabled(move || !any()),
        )
        .child(
            icon_btn(Icon::Copy, "Copy", move || {
                cv.copy(&ids());
                cv.end_select();
            })
            .disabled(move || !any()),
        )
        .child(
            icon_btn(Icon::Forward, "Forward", move || {
                cv.ask_forward(ids());
            })
            .disabled(move || !any()),
        )
        .child(
            icon_btn(Icon::Trash, "Delete", move || cv.ask_delete(ids())).disabled(move || !any()),
        )
        .into_element()
}

// ---------------------------------------------------------------- compose

/// The bar above the input: what is being answered or edited.
fn context_bar(cv: Conv) -> AnyElement {
    let (s, chat) = (cv.s, cv.chat);
    let target = computed(move || {
        cv.editing
            .get()
            .or(cv.reply.get())
            .and_then(|id| s.message(chat, id))
    });
    let editing = move || cv.editing.get().is_some();
    when(
        move || target.with(Option::is_some),
        move || {
            let enter = motion().entering(
                MotionSpec::new(super::timing(motion::BASE, Easing::EaseOut))
                    .values(MotionChannel::Opacity, [0.0, 1.0])
                    .values(MotionChannel::TranslateY, [6.0, 0.0]),
            );
            let who = move || {
                if editing() {
                    "Edit message".to_string()
                } else {
                    target.with(|t| {
                        t.as_ref()
                            .map(|t| s.contact_name(t.from))
                            .unwrap_or_default()
                    })
                }
            };
            let hue = move || match target.with(|t| t.as_ref().and_then(|t| t.from)) {
                Some(id) if !editing() => c::avatar(id),
                _ => c::primary(),
            };
            view()
                .bind_motion(&enter)
                .flex_row()
                .items_center()
                .gap_2()
                .mb_2()
                .style_when(
                    || true,
                    |st| floating::capsule_style(st).rounded(radius::lg()),
                )
                .overflow_hidden()
                .child(view().w(px(3.0)).self_stretch().bg(hue))
                .child(dynamic(move || {
                    let edit = editing();
                    Some(untracked(|| {
                        icon(if edit { Icon::Edit } else { Icon::Reply }, 16.0).text_color(hue)
                    }))
                }))
                .child(
                    view()
                        .flex_1()
                        .py_2()
                        .gap(px(1.0))
                        .child(
                            text(who)
                                .text_size(px(font::SMALL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hue),
                        )
                        .child(
                            text(move || {
                                target.with(|t| {
                                    t.as_ref().map(|t| t.body.preview()).unwrap_or_default()
                                })
                            })
                            .number_of_lines(1)
                            .text_size(px(font::SMALL))
                            .text_color(move || c::muted_fg()),
                        ),
                )
                .child(icon_btn(
                    Icon::Close,
                    if cv.editing.get_untracked().is_some() {
                        "Cancel edit"
                    } else {
                        "Cancel reply"
                    },
                    move || {
                        if cv.editing.get_untracked().is_some() {
                            cv.cancel_edit();
                        } else {
                            cv.reply.set(None);
                        }
                    },
                ))
        },
    )
    .into_element()
}

fn composer(cv: Conv) -> AnyElement {
    view()
        .w_full()
        .child(context_bar(cv))
        .child(
            when(move || cv.rec.get().is_some(), move || recorder(cv))
                .otherwise(move || input_row(cv)),
        )
        .into_element()
}

/// The input's line height.
fn input_line() -> f32 {
    if is_desktop() { 20.0 } else { 22.0 }
}

/// The most lines the field grows to; past them it scrolls.
const MAX_LINES: f32 = 6.0;

/// The composer: ONE capsule holding attach, the field, emoji, and the
/// mic (empty) or send (text). Every button is a square of the capsule's
/// height (a full touch target), drawn smaller inside; as the field grows
/// they stay on its last line, and the capsule keeps its one-line ends.
fn input_row(cv: Conv) -> View {
    let (s, chat) = (cv.s, cv.chat);
    let draft = cv.draft;
    let lh = input_line();
    let h = floating::capsule_height();
    let height = signal(lh);
    let shell = element_ref();
    let reduced = motion::reduced();
    // The field's padding centres one line on the capsule's height.
    let input_pad = ((h - lh) / 2.0).floor();
    let enter_sends = s.settings.with_untracked(|x| x.enter_sends) && is_desktop();
    let placeholder = if s
        .chat(chat)
        .is_some_and(|c| matches!(c.kind, ChatKind::WhatsApp(_)))
    {
        "Message on WhatsApp"
    } else {
        "Message"
    };
    let attach = {
        // Desktops: the attach popover opens at the button (measured as it
        // is pressed).
        let clip = element_ref();
        ghost(Icon::Plus, "Attach", move || {
            if !cv.wide {
                cv.attach_open.set(true);
                return;
            }
            spawn(async move {
                if let Some(m) = clip.measure().await {
                    let size = m.frame.size;
                    cv.attach_at
                        .set(Some((m.page.x.0, m.page.y.0, size.width.0, size.height.0)));
                }
                cv.attach_open.set(true);
            });
        })
        .element_ref(&clip)
    };
    surface(h / 2.0)
        .element_ref(&shell)
        .w_full()
        .flex_row()
        .items_end()
        .min_h(px(h))
        .child(attach)
        .child(
            input()
                .element_ref(&cv.input)
                .test_id("composer")
                .flex_1()
                .multiline(true)
                .value(move || draft.get())
                .on_change(move |t: &str| {
                    draft.set(t.to_owned());
                    if cv.editing.get_untracked().is_none() {
                        s.set_draft(chat, t.to_owned());
                    }
                })
                .submit_behavior(if enter_sends {
                    SubmitBehavior::Submit
                } else {
                    SubmitBehavior::Newline
                })
                .on_submit(move |_: &str| cv.send_draft())
                .on_content_size_change(move |size: &Size<Points>| {
                    let next = size.height.0.clamp(lh, lh * MAX_LINES).round();
                    if (next - height.get_untracked()).abs() < 0.5 {
                        return;
                    }
                    // The capsule grows and shrinks smoothly a line at a
                    // time (at once with reduced motion).
                    if !reduced {
                        shell.animate_layout(LayoutTransition::new(
                            LayoutCurve::EaseOut,
                            motion::FAST,
                        ));
                    }
                    height.set(next);
                })
                .placeholder(placeholder.to_string())
                .label(placeholder)
                .placeholder_color(|| color(c::muted_fg()))
                .caret_color(|| color(c::primary()))
                .selection_color(|| color(c::accent()))
                .h(move || px(height.get() + 2.0 * input_pad))
                .px_1()
                .py(px(input_pad))
                .text_size(px(font::body()))
                .line_height(px(lh))
                .text_color(move || c::foreground()),
        )
        .child(ghost(Icon::Smile, "Emoji", move || cv.emoji_open.set(true)))
        .child(send_button(cv))
}

/// A ghost icon button of the capsule's height, round.
fn ghost(glyph: Icon, label: &'static str, f: impl Fn() + 'static) -> nsh_base::ButtonElement {
    let h = floating::capsule_height();
    icon_btn(glyph, label, f)
        .size(px(h))
        .rounded_full()
        .text_color(move || c::muted_fg())
}

/// Send when there is text, the mic when there is none. The mic is a
/// ghost glyph; send is a filled `primary` circle that grows in from the
/// mic's place (scale 0.6 → 1 and a fade, `BASE`) as the first character
/// lands, and shrinks away when the field empties. Phones: hold the mic to
/// record, release to send, slide left to cancel, up to lock; a tap locks.
fn send_button(cv: Conv) -> View {
    let empty = move || cv.draft.with(|d| d.trim().is_empty());
    let reduced = motion::reduced();
    let h = floating::capsule_height();
    // The filled circle sits 4pt in from the capsule's end: concentric.
    let disc = h - 8.0;
    let shown = move |want_empty: bool| move || empty() == want_empty;
    let mic = icon(Icon::Mic, 20.0)
        .absolute()
        .text_color(move || c::muted_fg())
        .opacity(move || if shown(true)() { 1.0_f32 } else { 0.0 })
        .scale(move || {
            if shown(true)() || reduced {
                1.0_f32
            } else {
                0.6
            }
        })
        .transition_all()
        .duration(motion::BASE)
        .ease_in_out();
    let send = view()
        .absolute()
        .size(px(disc))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || c::primary())
        .text_color(move || c::on_primary())
        .opacity(move || if shown(false)() { 1.0_f32 } else { 0.0 })
        .scale(move || {
            if shown(false)() || reduced {
                1.0_f32
            } else {
                0.6
            }
        })
        .transition_all()
        .duration(motion::BASE)
        .ease_in_out()
        .child(icon(Icon::Send, 18.0));
    let button = pressable()
        .on_press(move || {
            if !empty() {
                cv.send_draft();
            } else if cv.rec.get_untracked().is_none() && !cv.held_press.get_untracked() {
                start_recording(cv, Rec::Locked);
            }
            cv.held_press.set(false);
        })
        .accessibility_label(super::say(move || {
            if empty() {
                "Record voice message"
            } else {
                "Send"
            }
        }))
        .size(px(h))
        .rounded_full()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .transition()
        .duration(motion::FAST)
        .hover(move |st| {
            if empty() {
                st.bg(move || c::foreground().opacity(0.06))
            } else {
                st
            }
        })
        .active(move |st| {
            if reduced {
                st.opacity(0.85)
            } else {
                st.scale(motion::ICON_PRESS_SCALE)
            }
        })
        .focus_visible(|st| {
            st.outline(px(2.0))
                .outline_offset(px(-2.0))
                .outline_color(move || c::ring())
        })
        .child(mic)
        .child(send);
    if is_desktop() {
        return view().child(button);
    }
    // Held: record while the finger stays down; slide left cancels, up
    // locks, lifting sends.
    let from = stored(None::<Point<Points>>);
    view()
        .on_long_press(move || {
            if empty() && cv.rec.get_untracked().is_none() {
                cv.held_press.set(true);
                start_recording(cv, Rec::Held);
            }
        })
        .on_touch(move |e: &TouchEvent| {
            let at = e.points.first().map(|p| p.page);
            match e.phase {
                TouchPhase::Start => {
                    from.replace(at);
                }
                TouchPhase::Move => {
                    if cv.rec.get_untracked() != Some(Rec::Held) {
                        return;
                    }
                    let Some((a, b)) = from.with(|f| *f).zip(at) else {
                        return;
                    };
                    if b.x.0 - a.x.0 < -110.0 {
                        cv.rec.set(None);
                        toast("Voice message cancelled");
                    } else if b.y.0 - a.y.0 < -80.0 {
                        cv.rec.set(Some(Rec::Locked));
                    }
                }
                TouchPhase::End => {
                    if cv.rec.get_untracked() == Some(Rec::Held) {
                        send_recording(cv);
                    }
                }
                _ => {
                    if cv.rec.get_untracked() == Some(Rec::Held) {
                        cv.rec.set(None);
                    }
                }
            }
        })
        .child(button)
}

thread_local! {
    /// The microphone's recording, while one goes on in a real chat.
    static RECORDER: std::cell::RefCell<Option<songbird_recorder::Recorder>> =
        const { std::cell::RefCell::new(None) };
}

fn start_recording(cv: Conv, how: Rec) {
    // A real chat records from the microphone (the system asks the first time); a demo chat
    // only counts.
    if crate::backend::chat::owns(cv.chat) {
        match songbird_recorder::Recorder::start() {
            Ok(r) => RECORDER.with_borrow_mut(|x| *x = Some(r)),
            Err(songbird_recorder::RecordError::NoMicrophone) => {
                toast("No microphone to record with. Check that Songbird may use it.");
                return;
            }
            Err(_) => {
                toast("The microphone couldn't be opened.");
                return;
            }
        }
    }
    batch(|| {
        cv.rec_secs.set(0);
        cv.rec.set(Some(how));
    });
    announce_for_accessibility("Recording voice message");
}

fn send_recording(cv: Conv) {
    let secs = cv.rec_secs.get_untracked().max(1);
    if let Some(r) = RECORDER.with_borrow_mut(Option::take) {
        cv.rec.set(None);
        match r.finish() {
            Some(rec) => crate::backend::chat::send_voice(cv.s, cv.chat, rec),
            None => toast("Too short to send. Hold to record."),
        }
        return;
    }
    cv.rec.set(None);
    cv.send_body(Body::Voice {
        secs,
        played: false,
    });
}

/// The recorder's clock, while recording.
pub(crate) fn install_recorder(cv: Conv) {
    effect(move || {
        // Recording ended without sending (deleted, slid away, the chat closed): discarded.
        if cv.rec.get().is_none() {
            RECORDER.with_borrow_mut(|x| *x = None);
        }
        if cv.rec.get().is_some() {
            let clock = set_interval(Duration::from_secs(1), move || {
                cv.rec_secs.update(|n| *n += 1);
                // The longest a note may be: sent as it is.
                if RECORDER
                    .with_borrow(|r| r.as_ref().is_some_and(songbird_recorder::Recorder::is_full))
                {
                    send_recording(cv);
                }
            });
            cleanup(move || clock.cancel());
        }
    });
}

/// Recording: a breathing red dot, the time, slide-to-cancel (held) or
/// Delete and Send (locked, and on desktops).
fn recorder(cv: Conv) -> View {
    let held = move || cv.rec.get() == Some(Rec::Held);
    let h = floating::capsule_height();
    let dot = motion();
    let _ = dot.play(
        MotionSpec::new(super::looping(1200.0, 0.0))
            .values(MotionChannel::Opacity, [1.0, 0.25, 1.0]),
    );
    // The same capsule as the composer: Delete where attach was, Send
    // where send is.
    surface(h / 2.0)
        .w_full()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(h))
        .pl(px(2.0))
        .child(when(
            move || !held(),
            move || ghost(Icon::Trash, "Delete recording", move || cv.rec.set(None)),
        ))
        .child(
            view()
                .size(px(10.0))
                .ml(px(10.0))
                .rounded_full()
                .bg(move || c::destructive())
                .bind_motion(&dot),
        )
        .child(
            text(move || logic::duration(cv.rec_secs.get()))
                .text_size(px(font::body()))
                .tabular_nums()
                .role(AccessibilityRole::Timer),
        )
        .child(view().flex_1())
        .child(when(held, || {
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .text_color(move || c::muted_fg())
                .child(
                    view()
                        .style_with(|st| st.rotate(deg(90.0)))
                        .child(icon(Icon::ChevronDown, 16.0)),
                )
                .child(text("Slide to cancel").text_size(px(font::SMALL)))
        }))
        .child(when(held, || {
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .pl_2()
                .pr_4()
                .text_color(move || c::muted_fg())
                .child(icon(Icon::Lock, 14.0))
                .child(text("Slide up to lock").text_size(px(font::CAPTION)))
        }))
        .child(when(
            move || !held(),
            move || {
                pressable()
                    .on_press(move || send_recording(cv))
                    .accessibility_label("Send voice message")
                    .size(px(h))
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .active(|st| {
                        if motion::reduced() {
                            st.opacity(0.85)
                        } else {
                            st.scale(motion::ICON_PRESS_SCALE)
                        }
                    })
                    .focus_visible(|st| {
                        st.outline(px(2.0))
                            .outline_offset(px(-2.0))
                            .outline_color(move || c::ring())
                    })
                    .child(
                        view()
                            .size(px(h - 8.0))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(move || c::primary())
                            .text_color(move || c::on_primary())
                            .child(icon(Icon::Send, 18.0)),
                    )
            },
        ))
}
