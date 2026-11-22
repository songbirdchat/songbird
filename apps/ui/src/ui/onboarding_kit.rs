//! Pieces shared by onboarding, devices and backups: a decorative code
//! (QR-like), the camera frame of a scanner, progress bars and steps, an
//! inline status line, the recovery key's blocks, a big icon and a
//! numeric code. Built only from tokens.

use nsh_core::prelude::*;

use super::{
    icons::{Icon, icon},
    tokens::{c, font, motion, radius},
};

/// The monospace family for keys, codes and numbers that are read aloud
/// or compared (the platform's own monospace).
pub const MONO: &str = "monospace";

/// The camera view of a scanner (mocked): a dark frame, four corner
/// brackets and a line sweeping down, which rests with reduced motion.
pub fn scanner(size: f32, caption: impl IntoReactive<SharedString>) -> View {
    let corners = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round"><path d="M4 22V10a6 6 0 0 1 6-6h12"/><path d="M78 4h12a6 6 0 0 1 6 6v12"/><path d="M96 78v12a6 6 0 0 1-6 6H78"/><path d="M22 96H10a6 6 0 0 1-6-6V78"/></svg>"#;
    let sweep = motion();
    if !motion::reduced() {
        let travel = size * 0.62;
        let _ = sweep.play(
            MotionSpec::new(
                KeyframeTiming::new(1800.0)
                    .easing(Easing::EaseInOut)
                    .direction(PlaybackDirection::Alternate)
                    .infinite(),
            )
            .values(MotionChannel::TranslateY, [0.0, travel]),
        );
    }
    view()
        .items_center()
        .gap_3()
        .child(
            view()
                .size(px(size))
                .rounded(move || radius::lg())
                .overflow_hidden()
                .bg(move || c::call_bg())
                .items_center()
                .justify_center()
                .accessibility_elements_hidden(true)
                .child(
                    svg(nsh_core::element::ImageSrc::svg_markup(corners))
                        .absolute()
                        .top(px(size * 0.14))
                        .left(px(size * 0.14))
                        .size(px(size * 0.72))
                        .text_color(move || c::on_call()),
                )
                .child(
                    view()
                        .absolute()
                        .top(px(size * 0.18))
                        .left(px(size * 0.2))
                        .w(px(size * 0.6))
                        .h(px(2.0))
                        .rounded_full()
                        .bg(move || c::primary())
                        .bind_motion(&sweep),
                ),
        )
        .child(
            text(caption)
                .text_size(px(font::SMALL + 1.0))
                .line_height(px(20.0))
                .text_center()
                .max_w(px(300.0))
                .text_color(move || c::muted_fg()),
        )
}

/// A determinate progress bar; `fraction` is 0.0 to 1.0. The fill grows
/// by scaling (a transform), over `BASE`.
pub fn progress_bar(label: &'static str, fraction: impl Fn() -> f32 + Copy + 'static) -> View {
    view()
        .h(px(6.0))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(move || c::muted())
        .role(AccessibilityRole::Progressbar)
        .label(move || format!("{label}, {}%", (fraction().clamp(0.0, 1.0) * 100.0).round()))
        .child(
            view()
                .h_full()
                .w_full()
                .rounded_full()
                .bg(move || c::primary())
                .origin_left()
                .scale_x(move || fraction().clamp(0.0, 1.0))
                .when(!motion::reduced(), |v| {
                    v.transition_transform().duration(motion::BASE).ease_out()
                }),
        )
}

/// An indeterminate bar for work with no known end ("Creating your
/// account…"): a short segment sliding across, still with reduced motion.
pub fn busy_bar(label: &'static str) -> View {
    let slide = motion();
    if !motion::reduced() {
        let _ = slide.play(
            MotionSpec::new(
                KeyframeTiming::new(1200.0)
                    .easing(Easing::EaseInOut)
                    .infinite(),
            )
            .values(MotionChannel::TranslateX, [-48.0, 320.0]),
        );
    }
    // The track is tinted (accent), so the bar reads as working even in
    // the instant the segment is off its edge.
    view()
        .h(px(6.0))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(move || c::accent())
        .role(AccessibilityRole::Progressbar)
        .label(label)
        .child(
            view()
                .h_full()
                .w(px(96.0))
                .rounded_full()
                .bg(move || c::primary())
                .when(motion::reduced(), |v| v.w_full().opacity(0.5))
                .bind_motion(&slide),
        )
}

/// "Step 2 of 6" as segments: done and current filled, the rest muted.
/// Colours fade over `BASE`; the count is in the label too.
pub fn step_segments(count: usize, current: impl Fn() -> usize + Copy + 'static) -> View {
    view()
        .flex_row()
        .gap_1()
        .flex_1()
        .role(AccessibilityRole::Progressbar)
        .label(move || format!("Step {} of {count}", current() + 1))
        .children((0..count).map(move |i| {
            view()
                .flex_1()
                .h(px(4.0))
                .rounded_full()
                // Inactive segments on the input tone: visible on a card in
                // dark as in light.
                .bg(move || {
                    if i <= current() {
                        c::primary()
                    } else {
                        c::input()
                    }
                })
                .transition_colors()
                .duration(motion::BASE)
        }))
}

/// Page dots: the current one a wider pill (shape and colour). Each dot is
/// a button for its page.
pub fn page_dots(
    count: usize,
    current: impl Fn() -> usize + Copy + 'static,
    on_select: impl Fn(usize) + Copy + 'static,
) -> View {
    view()
        .flex_row()
        .gap_1()
        .items_center()
        .justify_center()
        .role(AccessibilityRole::Tablist)
        .children((0..count).map(move |i| {
            let selected = move || current() == i;
            nsh_base::button()
                .on_press(move || on_select(i))
                .selected(selected)
                .accessibility_label(format!("Page {} of {count}", i + 1))
                .hit_slop(EdgeInsets::all(px(8.0)))
                .p(px(6.0))
                .rounded_full()
                .cursor_pointer()
                .focus_visible(|s| s.outline(px(2.0)).outline_color(move || c::ring()))
                .child(
                    view()
                        .h(px(8.0))
                        .w(move || px(if selected() { 22.0 } else { 8.0 }))
                        .rounded_full()
                        .bg(move || if selected() { c::primary() } else { c::input() })
                        .when(!motion::reduced(), |v| {
                            v.transition_all().duration(motion::BASE).ease_in_out()
                        }),
                )
        }))
}

/// How an inline status reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Muted,
    Good,
    Problem,
}

/// An icon and a sentence under a field (username availability, invite
/// check): never colour alone.
pub fn status_line(
    tone: impl Fn() -> StatusTone + Copy + 'static,
    glyph: impl Fn() -> Icon + Copy + 'static,
    message: impl Fn() -> String + 'static,
) -> View {
    let ink = move || match tone() {
        StatusTone::Muted => c::muted_fg(),
        StatusTone::Good => c::success(),
        StatusTone::Problem => c::destructive(),
    };
    view()
        .flex_row()
        .items_start()
        .gap_2()
        .min_h(px(20.0))
        .child(
            svg(move || glyph().src())
                .size(px(16.0))
                .mt(px(2.0))
                .flex_shrink_0()
                .accessibility_elements_hidden(true)
                .text_color(ink),
        )
        .child(
            text(message)
                .flex_1()
                .text_size(px(font::SMALL + 1.0))
                .line_height(px(20.0))
                .text_color(move || match tone() {
                    StatusTone::Muted => c::muted_fg(),
                    _ => c::foreground(),
                }),
        )
}

/// The recovery key in blocks of four, monospace, four to a row. One
/// label for screen readers, read group by group.
pub fn key_blocks(groups: Vec<String>) -> View {
    let spoken = groups.join(", ");
    view()
        .flex_row()
        .flex_wrap()
        .justify_center()
        .gap_2()
        .p_3()
        .rounded(move || radius::lg())
        .bg(move || c::secondary())
        .role(AccessibilityRole::Text)
        .label(format!("Recovery key: {spoken}"))
        .children(groups.into_iter().map(|g| {
            view()
                .w(px(70.0))
                .py_2()
                .items_center()
                .rounded(move || radius::sm())
                .bg(move || c::card())
                .border(super::parts::hairline())
                .border_color(move || c::border())
                .accessibility_elements_hidden(true)
                .child(
                    text(g)
                        .font_family(MONO)
                        .text_size(px(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .letter_spacing(px(1.0))
                        .select_text(),
                )
        }))
}

/// A large round icon for intro screens.
pub fn hero_icon(glyph: Icon) -> View {
    view()
        .size(px(72.0))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(move || c::accent())
        .text_color(move || c::primary())
        .accessibility_elements_hidden(true)
        .child(icon(glyph, 32.0))
}

/// A short code to type ("482 913"), large, monospace, tabular.
pub fn numeric_code(code: impl IntoReactive<SharedString>) -> Text {
    text(code)
        .font_family(MONO)
        .tabular_nums()
        .text_size(px(font::DISPLAY))
        .font_weight(FontWeight::SEMIBOLD)
        .letter_spacing(px(2.0))
        .text_center()
        .select_text()
}

/// A numbered instruction ("1  Open Songbird on your phone").
pub fn numbered_step(n: u32, label: impl Into<SharedString>) -> View {
    view()
        .flex_row()
        .items_start()
        .gap_3()
        .child(
            view()
                .size(px(24.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::accent())
                .accessibility_elements_hidden(true)
                .child(
                    text(n.to_string())
                        .text_size(px(font::CAPTION))
                        .font_weight(FontWeight::BOLD)
                        .text_color(move || c::primary()),
                ),
        )
        .child(
            text(label.into())
                .flex_1()
                .text_size(px(font::body()))
                .line_height(px(font::body() * 1.4)),
        )
}

/// A long code typed into blocks (an invite code, `XXXX-XXXX-…`): one
/// native field, see-through and over the blocks, owns the keyboard,
/// paste and autofill, as `nsh_base::otp_input` does for digits. The
/// blocks show `group` symbols each, three blocks to a row, the empty
/// places as quiet dots; the block the next symbol goes into gets the
/// focus ring. `clean` turns what was typed or pasted into the code.
pub fn code_cells(
    label: &'static str,
    length: usize,
    group: usize,
    value: Signal<String>,
    clean: fn(&str) -> String,
    invalid: impl Fn() -> bool + Copy + 'static,
    on_submit: impl Fn() + 'static,
) -> View {
    cells(
        label, length, group, value, clean, invalid, on_submit, false,
    )
}

/// A passcode typed into six cells that show dots, never digits (the app
/// lock, Prompt 9): the same field as [`code_cells`], masked, a number pad
/// on phones.
pub fn passcode_cells(
    label: &'static str,
    value: Signal<String>,
    invalid: impl Fn() -> bool + Copy + 'static,
    on_submit: impl Fn() + 'static,
) -> View {
    fn digits(t: &str) -> String {
        t.chars().filter(char::is_ascii_digit).take(6).collect()
    }
    cells(label, 6, 3, value, digits, invalid, on_submit, true)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the two public forms share one body"
)]
fn cells(
    label: &'static str,
    length: usize,
    group: usize,
    value: Signal<String>,
    clean: fn(&str) -> String,
    invalid: impl Fn() -> bool + Copy + 'static,
    on_submit: impl Fn() + 'static,
    masked: bool,
) -> View {
    const PER_ROW: usize = 3;
    let focused = signal(false);
    let blocks = length.div_ceil(group);
    let narrow = !is_desktop();
    let block = move |b: usize| {
        let filled = move || value.with(|v| v.chars().count());
        let next = move || focused.get() && (filled().min(length - 1)) / group == b;
        view()
            .flex_1()
            .h(px(if narrow { 52.0 } else { 48.0 }))
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .rounded(move || radius::control())
            .bg(move || c::card())
            .border_1()
            .border_color(move || {
                if invalid() {
                    c::destructive()
                } else if next() {
                    c::ring()
                } else {
                    c::input()
                }
            })
            .style_when(next, |s| {
                s.outline(px(1.0)).outline_color(move || c::ring())
            })
            .transition_colors()
            .duration(motion::INSTANT)
            .children((0..group).map(move |k| {
                let i = b * group + k;
                let ch = move || value.with(|v| v.chars().nth(i));
                text(move || match ch() {
                    Some(_) if masked => "●".into(),
                    Some(c) => String::from(c),
                    None => "·".into(),
                })
                .w(px(14.0))
                .text_center()
                .font_family(MONO)
                .text_size(px(font::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || {
                    if ch().is_some() {
                        c::foreground()
                    } else {
                        c::muted_fg()
                    }
                })
            }))
    };
    view()
        .relative()
        .w_full()
        .child(view().gap_2().accessibility_elements_hidden(true).children(
            (0..blocks.div_ceil(PER_ROW)).map(move |row| {
                view()
                    .flex_row()
                    .gap_2()
                    .children((row * PER_ROW..((row + 1) * PER_ROW).min(blocks)).map(block))
            }),
        ))
        .child(
            nsh_base::input()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .value(move || value.get())
                .on_change(move |t: &str| value.set(clean(t)))
                .on_submit(move |_: &str| on_submit())
                .on_focus(move || focused.set(true))
                .on_blur(move || focused.set(false))
                .label(label)
                .keyboard_type(if masked {
                    KeyboardType::Number
                } else {
                    KeyboardType::Default
                })
                .masked(masked)
                .auto_capitalize(AutoCapitalize::Characters)
                .auto_correct(false)
                .submit_behavior(SubmitBehavior::Submit)
                // Room for a pasted code with its hyphens; `clean` keeps the symbols.
                .max_length((length * 2) as u32)
                .text_color(move || AdaptiveColor::TRANSPARENT)
                .caret_color(|| color(AdaptiveColor::TRANSPARENT))
                .selection_color(|| color(AdaptiveColor::TRANSPARENT)),
        )
}

/// A step of a guided flow (onboarding, the backup wizard, a transfer):
/// the content scrolls, the actions stay at the bottom. At most 520pt
/// wide, centred, so it reads the same in a card, a pane and a phone.
pub fn flow_layout(content: impl IntoElement, actions: impl IntoElement) -> View {
    view()
        .flex_1()
        // The step's actions ride on the keyboard's edge while it shows: under it, a step had no
        // way on but dismissing the keyboard first (found on an iPhone, D-0183).
        .pb_keyboard()
        .child(
            scroll()
                .flex_1()
                .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
                .keyboard_taps(KeyboardTaps::Handled)
                .child(
                    view()
                        .w_full()
                        .max_w(px(520.0))
                        .self_center()
                        .px_6()
                        .pt_4()
                        .pb_6()
                        .gap_4()
                        .child(content),
                ),
        )
        .child(
            view()
                .w_full()
                .max_w(px(520.0))
                .self_center()
                .px_6()
                .pt_3()
                .pb_4()
                .gap_2()
                .child(actions),
        )
}

/// A flow step's title.
pub fn flow_title(label: impl IntoReactive<SharedString>) -> Text {
    // Display size with a set leading, so two-line titles stay tight.
    super::parts::heading(label).line_height(px(font::DISPLAY + 6.0))
}

/// The sentence under a flow step's title.
pub fn flow_lead(label: impl IntoReactive<SharedString>) -> Text {
    super::parts::body(label).text_color(move || c::muted_fg())
}
