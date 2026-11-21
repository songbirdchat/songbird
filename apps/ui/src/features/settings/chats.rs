//! Chats: theme and wallpaper with a live preview, animations, default
//! reactions, chat behaviour, backup and transfer, export, and archiving
//! or clearing everything.

use nsh_core::prelude::*;

use super::{danger_tile, foot, links, nav_row, section};
use crate::{store::store, ui::*};

/// Wallpaper 0 is the paper chat background; 1–8 tint it with an avatar
/// hue (data colours from tokens).
pub const WALLPAPERS: u8 = 9;

pub fn wallpaper_name(i: u8) -> &'static str {
    [
        "Default", "Coral", "Amber", "Meadow", "Lagoon", "Sky", "Iris", "Orchid", "Rose",
    ][(i as usize).min(8)]
}

/// The tint of wallpaper `i` over the chat background.
pub fn wallpaper_tint(i: u8) -> AdaptiveColor {
    if i == 0 {
        AdaptiveColor::TRANSPARENT
    } else {
        c::avatar((i - 1) as u64).opacity(0.22)
    }
}

const EMOJI_CHOICES: [&str; 32] = [
    "👍", "❤️", "😂", "😮", "😢", "🙏", "🔥", "🎉", "👏", "😍", "🤔", "😅", "👀", "💯", "✅", "🙌",
    "😊", "🥰", "😎", "🤣", "😭", "😡", "🤝", "💪", "🌻", "☕", "🍕", "🐦", "⭐", "💚", "👌", "🫶",
];

pub fn chats() {
    super::push("Chats", content);
}

fn content() -> AnyElement {
    let s = store();
    let settings = s.settings;
    let picking = signal(None::<usize>);
    let exporting = signal(false);
    view()
        .child(section_title("Theme and wallpaper"))
        .child(wallpaper_picker())
        .child(view().pt_4().child(group(vec![toggle_row(
            "Animations",
            Some("Animate emoji, stickers and reactions."),
            move || settings.with(|x| x.animations),
            move |on| s.settings_update(|x| x.animations = on),
        )])))
        .child(section_title("Default reactions"))
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
                .justify_center()
                .gap_3()
                .children((0..6).map(move |i| {
                    let e = move || settings.with(|x| x.default_reactions.get(i).cloned().unwrap_or_default());
                    tile(
                        format!("Reaction {}, change", i + 1),
                        if is_desktop() { 40.0 } else { 46.0 },
                        move || picking.get() == Some(i),
                        move || picking.set(Some(i)),
                        text(e).text_size(px(font::HEADING)),
                    )
                })),
        )
        .child(foot("Choose one to replace it. These show first when you react to a message."))
        .child(section(
            "Chat settings",
            vec![
                toggle_row_icon(
                    Icon::Keyboard,
                    "Enter is send",
                    Some("The Enter key sends your message. Use Shift+Enter for a new line."),
                    move || settings.with(|x| x.enter_sends),
                    move |on| s.settings_update(|x| x.enter_sends = on),
                ),
                toggle_row_icon(
                    Icon::Archive,
                    "Keep chats archived",
                    Some("Archived chats stay archived when a new message arrives."),
                    move || settings.with(|x| x.keep_archived),
                    move |on| s.settings_update(|x| x.keep_archived = on),
                ),
                toggle_row_icon(
                    Icon::Image,
                    "Save to photos",
                    Some(if is_desktop() {
                        "Save photos and videos you receive to your Pictures folder."
                    } else {
                        "Save photos and videos you receive to your Photos."
                    }),
                    move || settings.with(|x| x.save_to_photos),
                    move |on| s.settings_update(|x| x.save_to_photos = on),
                ),
                toggle_row_icon(
                    Icon::Volume,
                    "Play sound stickers",
                    Some("Sound stickers play their sound when you open the chat."),
                    move || settings.with(|x| x.sound_stickers_autoplay),
                    move |on| s.settings_update(|x| x.sound_stickers_autoplay = on),
                ),
                nav_row("Stickers and packs", Icon::Sticker, links::packs),
            ],
        ))
        .child(section(
            "Chat history",
            [
                Some(nav_row("Chat backup", Icon::Cloud, links::backups)),
                // Phone to phone only.
                (!is_desktop()).then(|| nav_row("Transfer chats", Icon::Smartphone, links::transfer)),
            ]
            .into_iter()
            .flatten()
            .chain([
                Row::new("Export chat").icon(Icon::Share).chevron().on_press(move || exporting.set(true)).build(),
                Row::new("Archive all chats")
                    .icon(Icon::Archive)
                    .on_press(move || {
                        confirm("Archive all chats?", "You can find them in Archived at the top of Chats.", "Archive all", move || {
                            s.archive_all();
                            toast("All chats archived");
                        })
                    })
                    .build(),
                Row::new("Clear all chats")
                    .leading(danger_tile(Icon::Trash))
                    .danger()
                    .on_press(move || {
                        confirm(
                            "Clear all chats?",
                            "Every message is deleted from this device. Your chats stay in the list, empty. This can't be undone.",
                            "Clear all",
                            move || {
                                s.clear_all();
                                toast("All chats cleared");
                            },
                        )
                    })
                    .build(),
            ])
            .collect(),
        ))
        .child(reaction_sheet(picking))
        .child(export_sheet(exporting))
        .into_element()
}

/// The preview bubble pair on the chosen wallpaper, and the swatches.
fn wallpaper_picker() -> View {
    let s = store();
    let settings = s.settings;
    let chosen = move || settings.with(|x| x.chat_wallpaper);
    view()
        .mx_4()
        .rounded(move || radius::lg())
        .overflow_hidden()
        .border_1()
        .border_color(move || c::border())
        .bg(move || c::card())
        .child(
            view().bg(move || c::chat_bg()).child(
                view()
                    .p_4()
                    .gap_2()
                    .bg(move || wallpaper_tint(chosen()))
                    .transition_colors()
                    .duration(motion::BASE)
                    .label(move || format!("Preview, {} wallpaper", wallpaper_name(chosen())))
                    .child(bubble("Are we still on for Saturday? 🌻", false))
                    .child(bubble("Yes! I'll bring the cake 🎂", true)),
            ),
        )
        .child(
            view()
                .p_3()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .justify_center()
                .role(AccessibilityRole::Radiogroup)
                .label("Wallpaper")
                .children((0..WALLPAPERS).map(move |i| {
                    swatch(
                        wallpaper_name(i),
                        move || {
                            if i == 0 {
                                c::chat_bg()
                            } else {
                                c::avatar((i - 1) as u64)
                            }
                        },
                        move || chosen() == i,
                        move || s.settings_update(|x| x.chat_wallpaper = i),
                    )
                })),
        )
}

fn bubble(line: &'static str, mine: bool) -> View {
    view().flex_row().when(mine, |v| v.justify_end()).child(
        view()
            .max_w(px(260.0))
            .px_3()
            .py_2()
            .rounded(move || radius::bubble())
            .bg(move || {
                if mine {
                    c::bubble_out()
                } else {
                    c::bubble_in()
                }
            })
            .child(text(line).text_size(px(font::body())).text_color(move || {
                if mine {
                    c::on_bubble_out()
                } else {
                    c::on_bubble_in()
                }
            })),
    )
}

/// Replace one default reaction.
fn reaction_sheet(picking: Signal<Option<usize>>) -> nsh_base::SheetElement {
    let s = store();
    let open = signal(false);
    effect(move || open.set(picking.get().is_some()));
    effect(move || {
        if !open.get() {
            untracked(|| picking.set(None));
        }
    });
    let content = view()
        .px_4()
        .pb_2()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .justify_center()
        .children(EMOJI_CHOICES.iter().map(move |e| {
            let e: &'static str = e;
            tile(
                format!("Use {e}"),
                if is_desktop() { 40.0 } else { 48.0 },
                move || {
                    s.settings
                        .with(|x| x.default_reactions.iter().any(|r| r == e))
                },
                move || {
                    if let Some(i) = picking.get_untracked() {
                        s.settings_update(|x| {
                            if let Some(slot) = x.default_reactions.get_mut(i) {
                                *slot = e.to_string();
                            }
                        });
                    }
                    picking.set(None);
                },
                text(e).text_size(px(font::HEADING)),
            )
        }));
    app_sheet(open, "Choose a reaction", content)
}

/// Export a chat: pick one, get its messages on this device as a text file (the messages this
/// device holds; the file is the user's, outside Songbird's protection).
fn export_sheet(open: Signal<bool>) -> nsh_base::SheetElement {
    let s = store();
    let chats = s.inbox();
    app_sheet(
        open,
        "Export chat",
        view()
            .child(foot("The export is a file on this device. Once you share it, it's no longer protected by Songbird.").pb_3())
            .child(
                scroll().max_h(px(360.0)).child(
                    group_with_inset(chats.into_iter().take(12).map(|chat| {
                        let id = chat.id;
                        Row::new(chat.title.clone())
                            .leading(avatar(chat.title.clone(), chat.seed, 36.0))
                            .on_press(move || {
                                open.set(false);
                                let text = store().export_text(id);
                                save_file("songbird-chat.txt", text.into_bytes());
                            })
                            .build()
                    }), INSET_AVATAR)
                    .mb_2(),
                ),
            ),
    )
}
