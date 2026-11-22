//! Appearance: the theme (switches the app's tokens live through
//! `theming.rs`), the app icon (Plus), text size and motion notes.

use nsh_core::{element::ImageSrc, prelude::*};

use super::{links, section};
use crate::{
    backend::icon as app_icons_sys,
    store::{Plan, Store, ThemeChoice, store},
    ui::*,
};

pub fn appearance() {
    super::push("Appearance", content);
}

/// The app icons: the feather on four grounds drawn from tokens, as the real icons are
/// (`assets/icon`, D-0179).
const ICONS: [&str; 4] = app_icons_sys::NAMES;

fn content() -> AnyElement {
    let s = store();
    let settings = s.settings;
    view()
        .child(section_title("Theme"))
        // A list of choices, not a segmented control: four labels with
        // "High contrast" don't fit one line on a phone, and each choice
        // says what it does.
        .child(group_plain(
            [
                (ThemeChoice::System, "System", "Follows your device's light or dark setting."),
                (ThemeChoice::Light, "Light", "Always light, whatever your device uses."),
                (ThemeChoice::Dark, "Dark", "Always dark, easier on the eyes at night."),
                (ThemeChoice::HighContrast, "High contrast", "Stronger text, borders and colours, in light or dark."),
            ]
            .into_iter()
            .map(move |(choice, title, line)| {
                radio_row(title, Some(line), move || settings.with(|x| x.theme == choice), move || {
                    s.settings_update(|x| x.theme = choice)
                })
            }),
        )
        .role(AccessibilityRole::Radiogroup)
        .label("Theme"))
        // Linux has no app icon an app may change (D-0179): the choice isn't offered there.
        .children(app_icons_sys::supported().then(|| app_icons(s)))
        .child(section(
            "Display",
            vec![
                Row::new("Text size")
                    .icon(Icon::Sliders)
                    .subtitle("Songbird follows your device's text size. Change it in your device's settings.")
                    .build(),
                Row::new("Reduce motion")
                    .icon(Icon::Zap)
                    .subtitle(if motion::reduced() {
                        "On in your device's settings: Songbird keeps movement to fades."
                    } else {
                        "Songbird follows your device's reduce motion setting."
                    })
                    .build(),
            ],
        ))
        .into_element()
}

/// The app icon choice: the main icon for everyone, the others with Plus. The system changes
/// the icon (iOS confirms it with its own alert); the tile shows the new one once it has.
fn app_icons(s: Store) -> View {
    let settings = s.settings;
    let ext = s.ext();
    let plus = move || settings.with(|x| x.plan != Plan::Free);
    let changing = signal(false);
    view()
        // The Plus tag sits right after the title it qualifies.
        .child(
            view()
                .flex_row()
                .items_end()
                .child(section_title("App icon").pr_0())
                .child(when(move || !plus(), || view().pl_2().pb_2().child(tag("Plus")))),
        )
        .child(
            view()
                .mx_4()
                .p_3()
                .rounded(move || radius::lg())
                .bg(move || c::card())
                .border_1()
                .border_color(move || c::border())
                .flex_row()
                .flex_wrap()
                .gap_3()
                .role(AccessibilityRole::Radiogroup)
                .label("App icon")
                .children(ICONS.iter().enumerate().map(move |(i, name)| {
                    let i = i as u8;
                    let name: &'static str = name;
                    view()
                        .items_center()
                        .gap_1()
                        .child(tile(
                            format!("{name} app icon"),
                            64.0,
                            move || ext.with(|x| x.app_icon == i),
                            move || {
                                if i > 0 && !plus() {
                                    toast("App icons come with Songbird Plus");
                                    links::subscription();
                                } else if !changing.get_untracked()
                                    && ext.with_untracked(|x| x.app_icon != i)
                                {
                                    changing.set(true);
                                    crate::backend::run(async move {
                                        let ok = app_icons_sys::apply(i).await;
                                        changing.set(false);
                                        if ok {
                                            s.ext_update(|x| x.app_icon = i);
                                        } else {
                                            toast("Your device didn't change Songbird's icon. Try again.");
                                        }
                                        Ok(())
                                    });
                                }
                            },
                            view()
                                .relative()
                                .child(app_icon(i))
                                // A lock on the icons Plus unlocks: not colour alone.
                                .child(when(move || i > 0 && !plus(), || {
                                    view()
                                        .absolute()
                                        .right(px(-6.0))
                                        .bottom(px(-6.0))
                                        .size(px(20.0))
                                        .rounded_full()
                                        .items_center()
                                        .justify_center()
                                        .bg(move || c::card())
                                        .border_1()
                                        .border_color(move || c::border())
                                        .text_color(move || c::muted_fg())
                                        .accessibility_elements_hidden(true)
                                        .child(icon(Icon::Lock, 11.0))
                                })),
                        ))
                        .child(text(name).text_size(px(font::CAPTION)).text_color(move || c::muted_fg()))
                }))
                .child(when(move || !plus(), || {
                    muted("Choose another icon with Songbird Plus.").w_full()
                }))
                .child(when(move || plus() && app_icons_sys::while_running(), || {
                    muted(if cfg!(target_os = "macos") {
                        "Shows in the Dock while Songbird is open."
                    } else {
                        "Shows on the taskbar while Songbird is open."
                    })
                    .w_full()
                })),
        )
}

/// The app icon itself, small: its SVG master (`assets/icon`), the picture the system shows.
fn app_icon(i: u8) -> View {
    const MASTERS: [&str; 4] = [
        include_str!("../../../assets/icon/songbird.svg"),
        include_str!("../../../assets/icon/night.svg"),
        include_str!("../../../assets/icon/paper.svg"),
        include_str!("../../../assets/icon/lavender.svg"),
    ];
    let master = MASTERS[usize::from(i).min(MASTERS.len() - 1)];
    view()
        .size(px(44.0))
        .rounded(move || radius::control())
        .overflow_hidden()
        .border_1()
        .border_color(move || c::border())
        .accessibility_elements_hidden(true)
        .child(svg(ImageSrc::svg_markup(master)).size(px(44.0)))
}
