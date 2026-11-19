//! Inputs, switches, segmented controls, pages, sheets, dialogs and
//! toasts: the stateful controls, styled once.

use std::time::Duration;

use nsh_base::{SheetEvent, alert_dialog, button, input, nav_page, sheet, switch};
use nsh_core::prelude::*;

use super::{
    button::icon_btn,
    floating,
    icons::{Icon, icon},
    parts::{hairline, wide},
    skin::{Elevation, FULL, Skin, TitleType, spec},
    tokens::{c, font, motion},
};

// ---------------------------------------------------------------- inputs

/// The app's text field, controlled by `value`.
pub fn field(placeholder: impl Into<SharedString>, value: Signal<String>) -> Input {
    let placeholder: SharedString = placeholder.into();
    let sp = spec();
    let fluent = sp.skin == Skin::Fluent;
    let material = sp.skin == Skin::Material;
    input()
        .value(move || value.get())
        .on_change(move |t: &str| value.set(t.to_owned()))
        .placeholder(placeholder.to_string())
        .label(placeholder)
        .placeholder_color(|| color(c::muted_fg()))
        .caret_color(|| color(c::primary()))
        .selection_color(|| color(c::accent()))
        .h(px(if is_desktop() {
            sp.field_h.0
        } else {
            sp.field_h.1
        }))
        .px_3()
        .text_size(px(font::body()))
        .text_color(move || c::foreground())
        .rounded(px(sp.radius.control))
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::input())
        // Fluent: the bottom edge turns accent and thickens; Material's
        // outlined field thickens all round; the others take the ring.
        .focus(move |s| {
            if fluent {
                s.border_b(px(2.0))
                    .border_color(move || c::input().opacity(0.5))
                    .border_b_color(move || c::primary())
            } else if material {
                s.border_2().border_color(move || c::primary())
            } else {
                s.border_color(move || c::ring())
            }
        })
}

/// A labelled field with help or error text under it.
pub fn labelled_field(
    label: impl Into<SharedString>,
    control: impl IntoElement,
    help: impl IntoReactive<SharedString>,
    invalid: impl Fn() -> bool + Copy + 'static,
) -> View {
    view()
        .gap(px(6.0))
        .child(
            text(label.into())
                .text_size(px(font::SMALL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || c::muted_fg()),
        )
        .child(control)
        .child(
            text(help)
                .text_size(px(font::SMALL))
                .line_height(px(18.0))
                .text_color(move || {
                    if invalid() {
                        c::destructive()
                    } else {
                        c::muted_fg()
                    }
                }),
        )
}

/// A rounded search field with a magnifier and a clear button.
pub fn search_field(placeholder: impl Into<SharedString>, query: Signal<String>) -> View {
    let placeholder: SharedString = placeholder.into();
    let sp = spec();
    // Apple: a capsule on the fill; Material: the 56dp search bar on a
    // container; Fluent: a 4pt box with its stroke; Adwaita: a 6pt entry.
    let height = match (sp.skin, is_desktop()) {
        (Skin::Material, false) => 52.0,
        (Skin::Material, true) => 40.0,
        (_, true) => sp.field_h.0,
        (Skin::Apple, false) => 38.0,
        (_, false) => sp.field_h.1 - 2.0,
    };
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(height))
        .px(px(if sp.skin == Skin::Material {
            16.0
        } else {
            12.0
        }))
        .rounded(px(sp.radius.search))
        .bg(move || c::secondary())
        .when(sp.skin == Skin::Fluent, |v| {
            v.border(hairline())
                .border_color(move || c::border())
                .border_b_1()
                .border_b_color(move || c::input().opacity(0.6))
        })
        .child(icon(Icon::Search, 16.0).text_color(move || c::muted_fg()))
        .child(
            input()
                .flex_1()
                .value(move || query.get())
                .on_change(move |t: &str| query.set(t.to_owned()))
                .placeholder(placeholder.to_string())
                .label(placeholder)
                .placeholder_color(|| color(c::muted_fg()))
                .caret_color(|| color(c::primary()))
                .text_size(px(font::body()))
                .text_color(move || c::foreground()),
        )
        .child(when(
            move || query.with(|q| !q.is_empty()),
            move || {
                button()
                    .accessibility_label("Clear search")
                    .on_press(move || query.set(String::new()))
                    .text_color(move || c::muted_fg())
                    .child(icon(Icon::Close, 16.0))
            },
        ))
}

// ---------------------------------------------------------------- toggles

/// The app's switch: a track and a thumb that slides, drawn from tokens.
/// The knob is near-white in both themes. Off: a light track outlined in
/// `input` (3:1 against the page), the knob ringed in the same colour. On:
/// the violet `switch-on` track. Disabled keeps both shapes and colours,
/// at half strength, so disabled-on and disabled-off still differ.
pub fn toggle(
    label: impl Into<SharedString>,
    on: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(bool) + 'static,
) -> nsh_base::SwitchElement {
    let reduced = motion::reduced();
    let sp = spec();
    let w = sp.switch;
    let apple = sp.skin == Skin::Apple;
    let soft_shadow = matches!(sp.skin, Skin::Apple | Skin::Adwaita);
    // The thumb sits `inset` from the track's outer edge; its box starts
    // inside the border.
    let off_x = w.inset_off - w.border;
    let on_x = w.track_w - w.inset_on - w.thumb_on - w.border;
    switch()
        .checked(on)
        .accessibility_label(label.into())
        .on_change(move |v: &bool| on_change(*v))
        .hit_slop(EdgeInsets::all(px(8.0)))
        .disabled_style(|s| s.opacity(0.45))
        .track_style(move |s| {
            let s = s
                .w(px(w.track_w))
                .h(px(w.track_h))
                .rounded_full()
                .justify_center()
                .bg(c::switch_track())
                .border(px(w.border))
                .border_color(c::input())
                .checked(|s| s.bg(c::switch_on()).border_color(c::switch_on()));
            if reduced {
                s
            } else {
                s.transition_colors().duration(motion::FAST)
            }
        })
        .thumb_style(move |s| {
            let s = s
                .size(px(w.thumb_off))
                .rounded_full()
                .bg(if w.ink_thumb_off {
                    c::muted_fg()
                } else {
                    c::switch_thumb()
                })
                .translate_x(px(off_x));
            let s = if apple {
                s.border_1().border_color(c::input())
            } else {
                s
            };
            let s = if soft_shadow { s.shadow_sm() } else { s };
            let s = s.checked(move |s| {
                let s = s
                    .size(px(w.thumb_on))
                    .translate_x(px(on_x))
                    .bg(c::switch_thumb());
                if apple {
                    s.border_color(c::switch_thumb())
                } else {
                    s
                }
            });
            if reduced {
                s
            } else {
                s.transition_all().duration(motion::FAST)
            }
        })
}

/// A settings row with a switch at its end.
pub fn toggle_row(
    title: impl Into<SharedString>,
    subtitle: Option<&'static str>,
    on: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(bool) + 'static,
) -> AnyElement {
    toggle_row_with(None, title, subtitle, on, on_change, || false)
}

/// [`toggle_row`] leading with an icon tile, as [`Row::icon`] rows do
/// (put it in a [`group`](super::group), whose separators start under
/// icon rows' titles).
pub fn toggle_row_icon(
    glyph: Icon,
    title: impl Into<SharedString>,
    subtitle: Option<&'static str>,
    on: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(bool) + 'static,
) -> AnyElement {
    toggle_row_with(Some(glyph), title, subtitle, on, on_change, || false)
}

/// The full toggle row: an optional leading icon tile, and `disabled`,
/// which dims the row and stops the switch (a setting another one turns
/// off; say why in the subtitle or a footnote).
pub fn toggle_row_with(
    glyph: Option<Icon>,
    title: impl Into<SharedString>,
    subtitle: Option<&'static str>,
    on: impl Fn() -> bool + Copy + 'static,
    on_change: impl Fn(bool) + 'static,
    disabled: impl Fn() -> bool + Copy + 'static,
) -> AnyElement {
    let title: SharedString = title.into();
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .min_h(px(super::parts::row_height()))
        .py_2()
        .child(glyph.map(|g| {
            super::parts::icon_tile(g).opacity(move || if disabled() { 0.45_f32 } else { 1.0 })
        }))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .opacity(move || if disabled() { 0.45_f32 } else { 1.0 })
                .child(
                    text(title.clone())
                        .text_size(px(font::body()))
                        .number_of_lines(2),
                )
                .child(subtitle.map(|s| {
                    text(s)
                        .text_size(px(font::SMALL))
                        .line_height(px(18.0))
                        .text_color(move || c::muted_fg())
                        .number_of_lines(3)
                })),
        )
        .child(toggle(title, on, on_change).disabled(disabled))
        .into_element()
}

/// A segmented control: one silhouette, the selection a raised segment
/// (lighter than the recessed track in both themes, with a hairline edge
/// and a soft shadow).
pub fn segmented<T: Copy + PartialEq + 'static>(
    options: Vec<(T, &'static str)>,
    value: impl Fn() -> T + Copy + 'static,
    on_change: impl Fn(T) + Copy + 'static,
) -> View {
    let sp = spec();
    // Apple: a capsule track; Material: outlined segmented buttons, the
    // chosen one on the secondary container; Fluent and Adwaita: a
    // recessed track with a raised segment in their own corners.
    let material = sp.skin == Skin::Material;
    let track = match sp.skin {
        Skin::Apple | Skin::Material => FULL,
        _ => sp.radius.control + 2.0,
    };
    let segment = match sp.skin {
        Skin::Apple => FULL,
        Skin::Material => 0.0,
        _ => sp.radius.control,
    };
    let count = options.len();
    view()
        .flex_row()
        .p(px(if material { 0.0 } else { 3.0 }))
        .gap(px(if material { 0.0 } else { 2.0 }))
        .rounded(px(track))
        .overflow_hidden()
        .bg(move || {
            if material {
                AdaptiveColor::TRANSPARENT
            } else {
                c::secondary()
            }
        })
        .when(material, |v| v.border_1().border_color(move || c::input()))
        .role(AccessibilityRole::Tablist)
        .children(options.into_iter().enumerate().map(move |(i, (v, label))| {
            let selected = move || value() == v;
            button()
                .flex_1()
                .on_press(move || on_change(v))
                .selected(selected)
                .accessibility_label(label)
                .h(px(match (material, is_desktop()) {
                    (true, true) => 32.0,
                    (true, false) => 40.0,
                    (false, true) => 28.0,
                    (false, false) => 34.0,
                }))
                .flex_row()
                .gap_1()
                .items_center()
                .justify_center()
                .rounded(px(segment))
                .when(material && i + 1 < count, |b| {
                    b.border_r_1().border_color(move || c::input())
                })
                .when(!material, |b| {
                    b.bg(move || {
                        if selected() {
                            c::raised()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .border(hairline())
                    .border_color(move || {
                        if selected() {
                            c::border()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .style_when(selected, |st| st.shadow_sm())
                })
                .when(material, |b| {
                    b.bg(move || {
                        if selected() {
                            c::accent()
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                })
                .child(material.then(|| {
                    icon(Icon::Check, 16.0)
                        .text_color(move || c::on_accent())
                        .style_when(
                            move || !selected(),
                            |st| st.hidden().border_color(AdaptiveColor::TRANSPARENT),
                        )
                }))
                .cursor_pointer()
                .transition_colors()
                .duration(motion::BASE)
                .child(
                    text(label)
                        .text_size(px(font::SMALL + 1.0))
                        .font_weight(FontWeight::MEDIUM)
                        .style_when(selected, |st| st.font_weight(FontWeight::SEMIBOLD))
                        .text_color(move || match (selected(), material) {
                            (true, true) => c::on_accent(),
                            (true, false) => c::foreground(),
                            _ => c::muted_fg(),
                        }),
                )
        }))
}

// ---------------------------------------------------------------- pages

/// A pushed page: the system header on iOS and Android, the app's header
/// on desktops. `right` holds header actions. On wide windows it sits in a
/// floating pane (the conversation pane, or the info pane), on its
/// surface.
pub fn page(title: impl Into<SharedString>, content: impl IntoElement) -> Screen {
    page_with(title, None, content)
}

/// [`page`] with header actions.
pub fn page_with(
    title: impl Into<SharedString>,
    right: Option<AnyElement>,
    content: impl IntoElement,
) -> Screen {
    let title: SharedString = title.into();
    nav_page()
        .key(title.clone())
        .title(title.clone())
        .header(app_header(title, true, right))
        .child(
            view()
                .flex_1()
                .bg(move || if wide() { c::pane() } else { c::background() })
                .child(content),
        )
}

/// A title in a skin's type.
fn title_text(title: SharedString, t: TitleType) -> Text {
    text(title)
        .number_of_lines(1)
        .text_size(px(t.size))
        .line_height(px((t.size * 1.25).round()))
        .font_weight(t.weight)
        .letter_spacing(px(t.tracking))
        .role(AccessibilityRole::Header)
}

/// Liquid Glass behind a group of header controls (Apple): the native
/// material on iOS 26 / macOS 26, a translucent blur before and elsewhere.
pub(crate) fn glass_capsule(content: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .rounded_full()
        .px(px(2.0))
        .glass_effect(nsh_core::GlassEffect::Regular)
        .glass_tint_color(Some(c::card().opacity(0.35)))
        .backdrop_blur(px(18.0))
        .bg(move || c::card().opacity(0.55))
        .border(hairline())
        .border_color(move || c::foreground().opacity(0.08))
        .shadow_sm()
        .shadow_color(c::foreground().opacity(0.08))
        .child(content)
}

/// A section root's title in a wide window's list pane (`REDESIGN.md`:
/// the destination's title at 20pt): semibold on every skin, so the pane
/// keeps to two weights. Apple tightens it as its 20pt titles are.
fn pane_title_type() -> TitleType {
    let tracking = if spec().skin == Skin::Apple {
        -0.2
    } else {
        0.0
    };
    TitleType {
        size: font::HEADING,
        weight: FontWeight::SEMIBOLD,
        tracking,
    }
}

/// The header the app draws: back, title, actions, in the skin's shape.
///
/// - A section root (`back` false) in a wide window is the list pane's
///   header row: on the pane's surface, no rule, the 20pt title on the
///   16pt gutter and the actions on a capsule ([`floating::action_capsule`])
///   8pt from the pane's edge, in the band the conversation's header
///   capsule floats in ([`floating::header_band`]), so the two pane tops
///   read as one line.
/// - A section root on a phone gets the large title where the platform
///   has one (Apple's 34pt bold under its glass toolbar, Material's
///   expressive headline under its top app bar, Fluent's 28pt page title;
///   Adwaita draws a header bar with the title centred), its actions on
///   the skin's capsule as everywhere in the Floating design.
/// - A pushed page gets the skin's page header. Built for the info pane
///   ([`crate::nav::in_info_pane`]) its back becomes a close (×) that
///   closes the pane.
pub fn app_header(title: impl Into<SharedString>, back: bool, right: Option<AnyElement>) -> View {
    let title: SharedString = title.into();
    let sp = spec();
    let is_wide = wide();
    let in_info = back && crate::nav::in_info_pane();
    let large = !back && sp.large_title_now(is_wide);
    let centred = !large && sp.centred_now(is_wide);
    let apple = sp.skin == Skin::Apple;
    let adwaita = sp.skin == Skin::Adwaita;
    let back_glyph = if sp.back_arrow {
        Icon::ArrowLeft
    } else {
        Icon::Back
    };
    let back_btn = back.then(|| {
        let b = if in_info {
            icon_btn(Icon::Close, "Close info", crate::nav::close_info)
        } else {
            icon_btn(back_glyph, "Back", crate::nav::pop)
        };
        // Apple groups header controls on glass (iOS 26 / macOS 26 toolbars).
        if apple {
            glass_capsule(b).into_element()
        } else {
            b.into_element()
        }
    });

    if !back && is_wide {
        // The list pane's header row: the 8pt inset, then a capsule-tall
        // row. The title centres on the same line as the conversation's
        // header capsule; the actions' capsule ends 8pt from the pane's
        // edge, as the capsules over the conversation do.
        return view()
            .bg(move || c::pane())
            .z_index(1)
            .pt(px(floating::CAPSULE_INSET))
            .child(
                view()
                    .h(px(floating::capsule_height()))
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .pl_4()
                    .pr(px(floating::CAPSULE_INSET))
                    .child(title_text(title, pane_title_type()).flex_1())
                    .child(right.map(floating::action_capsule)),
            );
    }

    // A section root's actions float on the skin's capsule; a pushed
    // page's stay as the platform draws them (Apple: on glass).
    let right = right.map(|r| {
        if !back {
            floating::action_capsule(r).into_element()
        } else if apple {
            glass_capsule(r).into_element()
        } else {
            r
        }
    });
    let type_ = if large {
        sp.large_title
    } else if back {
        sp.page_title
    } else if is_wide {
        pane_title_type()
    } else {
        sp.page_title
    };
    // Header bars that are a surface of their own: Adwaita's (lighter
    // than the window, with a shade under it). The others sit on the page,
    // which on wide windows is a pane.
    let bar_bg = move || {
        if adwaita {
            c::card()
        } else if wide() {
            c::pane()
        } else {
            c::background()
        }
    };
    let base = view().bg(bar_bg).z_index(1).when(adwaita, |v| {
        v.border_b(hairline()).border_color(move || c::border())
    });
    if large {
        // Fluent: one row, the 28pt title beside the actions.
        if sp.skin == Skin::Fluent {
            return base
                .h(px(56.0))
                .flex_row()
                .items_center()
                .gap_2()
                .pl_4()
                .pr_2()
                .child(title_text(title, type_).flex_1())
                .child(right);
        }
        // Apple and Material: the toolbar row, then the title under it.
        let bar_h = if apple { 44.0 } else { 56.0 };
        return base
            .child(
                view()
                    .h(px(bar_h))
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    // Apple's glass group sits on the 16pt content gutter,
                    // flush with the large title and the list below.
                    .px(px(if apple { 16.0 } else { 8.0 }))
                    .child(right),
            )
            .child(
                title_text(title, type_)
                    .px_4()
                    .pb(px(if apple { 6.0 } else { 12.0 })),
            );
    }
    if centred {
        // The title centred on the bar, whatever the two sides hold.
        return base
            .h(px(sp.header_h))
            .flex_row()
            .items_center()
            .px(px(if adwaita { 6.0 } else { 8.0 }))
            .child(
                view()
                    .absolute()
                    .left(px(72.0))
                    .right(px(72.0))
                    .top_0()
                    .bottom_0()
                    .items_center()
                    .justify_center()
                    .pointer_events(PointerEvents::None)
                    .child(title_text(title, type_).text_center()),
            )
            .child(back_btn)
            .child(view().flex_1())
            .child(right);
    }
    // Spines: the title starts on the 16pt gutter when there is no back
    // button; the back button's frame sits 8pt in, so its chevron (whose
    // stroke starts a third into its box) lands near the gutter. The
    // trailing actions' frames end 8pt in, so their glyphs line up with the
    // content's trailing edge as the back glyph does with the leading one.
    base.h(px(sp.header_h))
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        // A visible edge in both themes where content scrolls beneath (a
        // little stronger than row separators), drawn over the content.
        // Material's and Apple's bars have none (they tint or blur on
        // scroll); Fluent's page header has none either.
        .when(sp.elevation == Elevation::Shadow && back, |v| {
            v.border_b(hairline()).border_color(move || c::border())
        })
        .child(back_btn)
        .child(title_text(title, type_).flex_1().when(!back, |t| t.pl_2()))
        .child(right)
}

// ---------------------------------------------------------------- sheets

/// A bottom sheet on phones, a centred panel on desktops. Always in the
/// tree; `open` drives it.
pub fn app_sheet(
    open: Signal<bool>,
    title: impl Into<SharedString>,
    content: impl IntoElement + 'static,
) -> nsh_base::SheetElement {
    let title: SharedString = title.into();
    let content = std::cell::RefCell::new(Some(content.into_element()));
    let sp = spec();
    // Fluent shows every sheet as a centred dialog; the others centre a
    // panel on wide windows and rise from the bottom on phones.
    let panel = wide() || sp.dialogs_centred;
    let r = sp.radius;
    let centred_title = matches!(sp.skin, Skin::Apple | Skin::Adwaita);
    let title_type = match sp.skin {
        Skin::Material => TitleType {
            size: 22.0,
            weight: FontWeight::NORMAL,
            tracking: 0.0,
        },
        _ => sp.page_title,
    };
    let material = sp.skin == Skin::Material;
    sheet()
        .open(move || Some(open.get()))
        // The scrim: the page dims behind the sheet (the sheet itself
        // draws no backdrop colour).
        .backdrop_style(|s| s.bg(c::overlay()))
        // Wide windows: the panel floats in the middle of the window, all
        // four corners visible; the full-height layer around it lets
        // presses through to the backdrop.
        .when(panel, |sh| {
            sh.popup_style(|s| {
                s.top_0()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .pointer_events(PointerEvents::BoxNone)
            })
        })
        .on_open_change(move |e: &SheetEvent| {
            if let SheetEvent::OpenChange(next, _) = e {
                open.set(*next);
            }
        })
        .child({
            view()
                .bg(move || c::popover())
                .rounded_t(px(r.sheet))
                .shadow_lg()
                .when(panel, |v| {
                    v.rounded(px(r.dialog))
                        .w_full()
                        .max_w(px(440.0))
                        .pt_2()
                        // Adwaita's dialogs carry no outline, only their shadow.
                        .when(sp.skin != Skin::Adwaita, |v| {
                            v.border(hairline()).border_color(move || c::border())
                        })
                        .when(sp.elevation == Elevation::Shadow, |v| v.shadow_xl())
                })
                .when(panel, |v| v.pb_4())
                .when(!panel, |v| v.pb_safe_area_plus(px(16.0)))
                // The grabber says "drag me down": phones only, the
                // desktop panel closes with Escape, the backdrop or Close.
                // Material's drag handle is 32×4.
                .child((!panel).then(|| {
                    view()
                        .items_center()
                        .pt(px(if material { 16.0 } else { 6.0 }))
                        .pb(px(if material { 8.0 } else { 0.0 }))
                        .child(
                            view()
                                .w(px(if material { 32.0 } else { 36.0 }))
                                .h(px(if material { 4.0 } else { 5.0 }))
                                .rounded_full()
                                .bg(move || {
                                    if material {
                                        c::muted_fg().opacity(0.4)
                                    } else {
                                        c::muted_fg().opacity(0.35)
                                    }
                                }),
                        )
                }))
                // Header: 16pt leading (the content's gutter), the close
                // button's frame 8pt from the trailing edge so its glyph
                // sits on the same optical edge as the title's start.
                // Apple and Adwaita centre the title between the edges.
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .pl_4()
                        .pr_2()
                        .pt(px(if panel { 4.0 } else { 2.0 }))
                        .pb_2()
                        .gap_2()
                        .child(centred_title.then(|| view().w(px(32.0))))
                        .child(
                            text(title)
                                .flex_1()
                                .number_of_lines(2)
                                .text_size(px(title_type.size))
                                .line_height(px((title_type.size * 1.3).round()))
                                .font_weight(title_type.weight)
                                .when(centred_title, |t| t.text_center())
                                .role(AccessibilityRole::Header),
                        )
                        .child(icon_btn(Icon::Close, "Close", move || open.set(false))),
                )
                .child(content.borrow_mut().take())
        })
}

/// A native confirm. `on_yes` runs when the destructive action is chosen.
pub fn confirm(
    title: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
    on_yes: impl FnOnce() + 'static,
) {
    let title = title.into();
    let message = message.into();
    let action = action.into();
    spawn(async move {
        let answer = alert_dialog()
            .title(title)
            .description(message)
            .cancel("cancel", "Cancel")
            .destructive("yes", action)
            .show()
            .await;
        if matches!(answer, Ok(r) if r.action.as_deref() == Some("yes")) {
            on_yes();
        }
    });
}

/// A native question with a plain (not destructive) action: "never by
/// surprise" confirmations. `on_yes` runs when the action is chosen.
pub fn ask_first(
    title: impl Into<String>,
    message: impl Into<String>,
    action: impl Into<String>,
    on_yes: impl FnOnce() + 'static,
) {
    let title = title.into();
    let message = message.into();
    let action = action.into();
    spawn(async move {
        let answer = alert_dialog()
            .title(title)
            .description(message)
            .cancel("cancel", "Cancel")
            .action("yes", action)
            .show()
            .await;
        if matches!(answer, Ok(r) if r.action.as_deref() == Some("yes")) {
            on_yes();
        }
    });
}

// ---------------------------------------------------------------- toasts

/// The one toast showing (a short message that leaves by itself).
#[derive(Default, Clone, PartialEq)]
pub struct ToastState {
    pub message: Option<SharedString>,
    pub serial: u64,
}

/// Shows `message` for a few seconds, and tells screen readers.
pub fn toast(message: impl Into<SharedString>) {
    let message: SharedString = message.into();
    announce_for_accessibility(message.to_string());
    let state = global::<ToastState>();
    let serial = state.with_untracked(|s| s.serial) + 1;
    state.set(ToastState {
        message: Some(message),
        serial,
    });
    set_timeout(Duration::from_millis(2600), move || {
        state.update(|s| {
            if s.serial == serial {
                s.message = None;
            }
        });
    });
}

/// How far above the safe area a toast floats on phones: 12pt over the
/// skin's section bar.
fn toast_lift_phone() -> f32 {
    spec().tab_bar_h() + 12.0
}

/// How far above the bottom a toast floats over an open conversation: 12pt
/// over a composer of up to 72pt (one line plus its padding).
const TOAST_LIFT_COMPOSER: f32 = 84.0;

/// Extra room a screen asks the toast to leave at the bottom (a composer,
/// an action bar), in points; 0 when none. See [`set_toast_inset`].
#[derive(Default, Clone, Copy, PartialEq)]
pub struct ToastInset(pub f32);

/// Asks toasts to float at least `px` points above the window's bottom
/// (above the safe area), for a screen with its own bottom bar. Set it
/// when the screen shows and back to 0 when it goes, e.g.
/// `ui::set_toast_inset(96.0); on_cleanup(|| ui::set_toast_inset(0.0));`.
/// An open conversation is already cleared without it.
pub fn set_toast_inset(px: f32) {
    global::<ToastInset>().set(ToastInset(px.max(0.0)));
}

/// Whether a conversation (and so its composer) is at the bottom of the
/// window now: the desktop detail pane's open chat with nothing pushed
/// over it, or a chat page on top of the phone stack.
fn conversation_showing() -> bool {
    let top_is_chat =
        crate::nav::routes().with(|r| r.0.last().map(|route| route.key.starts_with("chat:")));
    if wide() {
        top_is_chat.unwrap_or_else(|| {
            crate::nav::tab().get() == crate::nav::Tab::Chats
                && crate::nav::open_chat().get().0.is_some()
        })
    } else {
        top_is_chat.unwrap_or(false)
    }
}

/// Where the toast floats: above the tab bar on phones, near the bottom
/// on wide windows, and above the composer whenever a conversation shows
/// (it must never cover the last message or the field being typed in).
fn toast_lift() -> f32 {
    let base = if conversation_showing() {
        TOAST_LIFT_COMPOSER
    } else if wide() {
        24.0
    } else {
        toast_lift_phone()
    };
    let asked = global::<ToastInset>().get().0;
    if asked > 0.0 {
        base.max(asked + 12.0)
    } else {
        base
    }
}

/// The toast layer, mounted once at the root.
pub fn toast_layer() -> View {
    let state = global::<ToastState>();
    // Material's snackbar has 4pt corners, Fluent's 8; Apple and
    // Adwaita float a capsule.
    let corner = match spec().skin {
        Skin::Material => 4.0,
        Skin::Fluent => 8.0,
        _ => FULL,
    };
    let shown = move || state.with(|s| s.message.is_some());
    view()
        .absolute()
        .left_0()
        .right_0()
        .bottom(move || px(toast_lift()))
        .pb_safe_area()
        .px_4()
        .items_center()
        .pointer_events(PointerEvents::None)
        .accessibility_elements_hidden(true)
        .child(
            view()
                .px_4()
                .py(px(10.0))
                .max_w(px(420.0))
                .rounded(px(corner))
                .bg(move || c::foreground())
                .shadow_md()
                .opacity(move || if shown() { 1.0_f32 } else { 0.0 })
                .translate_y(move || {
                    if shown() || motion::reduced() {
                        px(0.0)
                    } else {
                        px(8.0)
                    }
                })
                .transition_all()
                .duration(motion::BASE)
                .ease_out()
                .child(
                    text(move || state.with(|s| s.message.clone().unwrap_or_default()))
                        .number_of_lines(2)
                        .text_center()
                        .text_size(px(font::SMALL + 1.0))
                        .line_height(px(20.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || c::background()),
                ),
        )
}

/// Hands `text` to the system's share sheet (iOS, Android, macOS; NSH
/// `share`). Where the platform has none (Windows, Linux), it is copied
/// instead and the toast says so.
pub fn share_text(text: impl Into<String>) {
    let text = text.into();
    Owner::detached().run(|| {
        spawn(async move {
            let request = nsh_core::share::ShareRequest::new().text(text.clone());
            if !matches!(nsh_core::prelude::share(request).await, Ok(_)) {
                set_clipboard(text);
                toast("Copied. Paste it into any app to share it.");
            }
        })
    });
}

/// Asks where to save `bytes` (the system's save dialog) and writes them
/// there. On platforms without a save dialog the file goes to the app's
/// documents folder and the toast says where.
pub fn save_file(suggested: impl Into<String>, bytes: Vec<u8>) {
    let suggested = suggested.into();
    Owner::detached().run(|| {
        spawn(async move {
            let dialog = nsh_core::file_dialog::SaveDialog::new().file_name(suggested.clone());
            let path = match nsh_core::prelude::pick_save_path(dialog).await {
                Ok(Some(p)) => p,
                Ok(None) => return,
                Err(_) => match data_dir() {
                    Some(d) => d.join(&suggested),
                    None => {
                        toast("This device has no place to save files.");
                        return;
                    }
                },
            };
            match std::fs::write(&path, &bytes) {
                Ok(()) => toast(format!(
                    "Saved {}",
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                )),
                Err(_) => toast("The file couldn't be saved there."),
            }
        })
    });
}
