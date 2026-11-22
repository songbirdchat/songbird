//! Storage and data: manage storage (per chat, largest files first),
//! network usage, calls on less data, the proxy, media auto-download and
//! upload quality.

use std::collections::HashSet;

use nsh_core::prelude::*;

use crate::features::groups::page_scroll;

use crate::{
    features::groups::{
        bottom_bar, card_rows, chat_avatar_for, check_circle, glide, icon_toggle_row,
    },
    nav,
    store::{
        ChatId, Settings,
        ext_groups::{
            FileKind, StoredFile, chat_totals, format_bytes, groups_state, largest_first,
        },
        store, when_label,
    },
    ui::*,
};

/// Storage and data, pushed (You → Storage and data).
pub fn storage_and_data() {
    nav::push("storage-and-data", move || {
        let s = store();
        let used = move || s.stored_files().iter().map(|f| f.bytes).sum::<u64>();
        let traffic = move || groups_state().with(|g| g.usage.total());
        let proxy = move || {
            s.settings
                .with(|x| x.proxy.clone())
                .unwrap_or_else(|| "Off".into())
        };
        page(
            "Storage and data",
            page_scroll(
                view()
                    .pt_4()
                    .child(group([
                        view()
                            .child(dynamic(move || {
                                let u = used();
                                untracked(move || {
                                    Row::new("Manage storage")
                                        .icon(Icon::HardDrive)
                                        .subtitle(format!("{} used by Songbird", format_bytes(u)))
                                        .chevron()
                                        .on_press(manage_storage)
                                        .build()
                                })
                            }))
                            .into_element(),
                        view()
                            .child(dynamic(move || {
                                let t = traffic();
                                untracked(move || {
                                    Row::new("Network usage")
                                        .icon(Icon::Wifi)
                                        .subtitle(format!("{} sent and received", format_bytes(t)))
                                        .chevron()
                                        .on_press(network_usage)
                                        .build()
                                })
                            }))
                            .into_element(),
                    ]))
                    .child(section_title("Calls and connection"))
                    .child(group([
                        icon_toggle_row(Icon::Phone, "Use less data for calls",
                            Some("Calls use less data, at a slightly lower quality."),
                            move || s.settings.with(|x| x.low_data_calls),
                            move |v| s.settings_update(|x| x.low_data_calls = v),
                        ),
                        Row::new("Proxy")
                            .icon(Icon::Globe)
                            .subtitle("Change it in You, under Proxy.")
                            .trailing(
                                text(proxy)
                                    .max_w(px(160.0))
                                    .number_of_lines(1)
                                    .text_size(px(font::SMALL + 1.0))
                                    .text_color(move || c::muted_fg()),
                            )
                            .build(),
                    ]))
                    .child(section_title("Media auto-download"))
                    .child(auto_download())
                    .child(footnote("Turn off what you don't need on mobile data. Media that isn't downloaded waits in the chat until you tap it."))
                    .child(section_title("Media upload quality"))
                    .child(view().px_4().child(segmented(
                        vec![(false, "Standard"), (true, "HD")],
                        move || s.settings.with(|x| x.hd_uploads),
                        move |v| s.settings_update(|x| x.hd_uploads = v),
                    )))
                    .child(footnote("HD photos and videos look sharper, but they're larger and take longer to send.")),
            ),
        )
    });
}

/// One media kind's auto-download switches.
struct AutoKind {
    title: &'static str,
    glyph: Icon,
    get: fn(&Settings) -> (bool, bool),
    set: fn(&mut Settings, (bool, bool)),
}

const AUTO: [AutoKind; 4] = [
    AutoKind {
        title: "Photos",
        glyph: Icon::Image,
        get: |s| s.auto_photos,
        set: |s, v| s.auto_photos = v,
    },
    AutoKind {
        title: "Audio",
        glyph: Icon::Mic,
        get: |s| s.auto_audio,
        set: |s, v| s.auto_audio = v,
    },
    AutoKind {
        title: "Videos",
        glyph: Icon::Video,
        get: |s| s.auto_video,
        set: |s, v| s.auto_video = v,
    },
    AutoKind {
        title: "Documents",
        glyph: Icon::File,
        get: |s| s.auto_docs,
        set: |s, v| s.auto_docs = v,
    },
];

/// A small table: one row per kind, a Wi-Fi and a mobile data column.
fn auto_download() -> View {
    let s = store();
    let lane = |label: &'static str| {
        text(label)
            .w(px(72.0))
            .text_center()
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || c::muted_fg())
    };
    let cell = move |k: &'static AutoKind, mobile: bool| {
        let name = if mobile {
            format!("{} on mobile data", k.title)
        } else {
            format!("{} on Wi-Fi", k.title)
        };
        view().w(px(72.0)).items_center().child(toggle(
            name,
            move || {
                s.settings
                    .with(|x| if mobile { (k.get)(x).1 } else { (k.get)(x).0 })
            },
            move |v| {
                s.settings_update(|x| {
                    let (mut wifi, mut cell) = (k.get)(x);
                    if mobile {
                        cell = v
                    } else {
                        wifi = v
                    }
                    (k.set)(x, (wifi, cell));
                })
            },
        ))
    };
    view()
        .mx_4()
        .rounded(move || radius::lg())
        .overflow_hidden()
        .bg(move || c::card())
        .border_1()
        .border_color(move || c::border())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .pt_3()
                .pb_1()
                // The same gaps as the rows below, so each heading centres
                // over its column of switches.
                .child(view().flex_1())
                .child(lane("Wi-Fi"))
                .child(lane(if is_desktop() {
                    "Metered"
                } else {
                    "Mobile data"
                })),
        )
        .children(AUTO.iter().enumerate().map(move |(i, k)| {
            view()
                .child((i > 0).then(|| separator_inset(INSET_ICON)))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .min_h(px(if is_desktop() { 48.0 } else { 56.0 }))
                        .child(icon_tile(k.glyph))
                        .child(text(k.title).flex_1().text_size(px(font::body())))
                        .child(cell(k, false))
                        .child(cell(k, true)),
                )
        }))
}

fn kind_glyph(kind: FileKind) -> Icon {
    match kind {
        FileKind::Photo => Icon::Image,
        FileKind::Video => Icon::Video,
        FileKind::Voice => Icon::Mic,
        FileKind::Document => Icon::File,
    }
}

/// The kinds in the bar, with their tokens (a legend names each, so the
/// colour is never the only cue).
const KINDS: [FileKind; 4] = [
    FileKind::Video,
    FileKind::Photo,
    FileKind::Document,
    FileKind::Voice,
];

/// Four clearly different hues from the palette: the brand violet, then
/// the avatar hues furthest from it and from each other (teal, raspberry,
/// blue), each tuned for light and dark by the theme.
fn kind_color(kind: FileKind) -> AdaptiveColor {
    match kind {
        FileKind::Video => c::primary(),
        FileKind::Photo => c::avatar(7),
        FileKind::Document => c::avatar(4),
        FileKind::Voice => c::avatar(0),
    }
}

fn kind_plural(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Photo => "Photos",
        FileKind::Video => "Videos",
        FileKind::Voice => "Voice messages",
        FileKind::Document => "Documents",
    }
}

/// Manage storage: the total, what it's made of, and chats by size.
pub fn manage_storage() {
    nav::push("manage-storage", move || {
        let s = store();
        page(
            "Manage storage",
            view().flex_1().child(dynamic(move || {
                let files = s.stored_files();
                untracked(move || storage_overview(s, files))
            })),
        )
    });
}

fn storage_overview(s: crate::store::Store, files: Vec<StoredFile>) -> AnyElement {
    if files.is_empty() {
        return empty_state(
            Icon::HardDrive,
            "Nothing stored",
            "Photos, videos and files you get will show here.",
            None,
        )
        .into_element();
    }
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let by_kind: Vec<(FileKind, u64)> = KINDS
        .iter()
        .map(|&k| {
            (
                k,
                files.iter().filter(|f| f.kind == k).map(|f| f.bytes).sum(),
            )
        })
        .collect();
    let totals = chat_totals(&files);
    page_scroll(
        view()
            .pt_4()
            .child(
                view().px_4().child(
                    card()
                        .gap_3()
                        .child(
                            view()
                                .flex_row()
                                .items_end()
                                .gap_2()
                                .child(text(format_bytes(total)).text_size(px(font::DISPLAY)).font_weight(FontWeight::BOLD).tabular_nums())
                                .child(muted("used by Songbird").pb_1()),
                        )
                        .child(
                            view()
                                .flex_row()
                                .h(px(10.0))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(move || c::muted())
                                .gap(px(2.0))
                                .accessibility_elements_hidden(true)
                                // Shares of the free width (after the 2pt gaps), so
                                // the segments and gaps add up to exactly the bar;
                                // a tiny share still shows as a 4pt sliver.
                                .children(by_kind.iter().filter(|(_, b)| *b > 0).map(|&(k, b)| {
                                    view()
                                        .h_full()
                                        .w(px(0.0))
                                        .min_w(px(4.0))
                                        .flex_grow(b as f32 / total as f32)
                                        .bg(move || kind_color(k))
                                })),
                        )
                        .child(view().flex_row().flex_wrap().gap_x(px(16.0)).gap_y(px(6.0)).children(by_kind.iter().map(|&(k, b)| {
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(view().size(px(10.0)).rounded(px(3.0)).bg(move || kind_color(k)))
                                .child(muted(format!("{} {}", kind_plural(k), format_bytes(b))).tabular_nums())
                        })))
                        .child(footnote("Deleting files frees space on this device. The messages stay, and you can download the files again while the sender's copy lasts.").px(px(0.0))),
                ),
            )
            .child(section_title("Chats, largest first"))
            // Rules start on the text column (16 + 36 + 12).
            .child(card_rows(totals.into_iter().filter_map(|(chat, bytes)| {
                let c = s.chat(chat)?;
                Some(
                    Row::new(c.title.clone())
                        .leading(chat_avatar_for(&c, 36.0))
                        .value(format_bytes(bytes))
                        .on_press(move || chat_storage(chat))
                        .build(),
                )
            }), 64.0)),
    )
    .into_element()
}

/// One chat's files, largest first, to select and delete.
fn chat_storage(chat: ChatId) {
    nav::push(format!("chat-storage:{chat}"), move || {
        let s = store();
        let files = computed(move || largest_first(&s.stored_files(), chat));
        let selected = signal(HashSet::<u64>::new());
        let chosen_bytes = move || {
            selected.with(|sel| {
                files.with(|f| {
                    f.iter()
                        .filter(|x| sel.contains(&x.id))
                        .map(|x| x.bytes)
                        .sum::<u64>()
                })
            })
        };
        let all = move || files.with(|f| !f.is_empty() && selected.with(|s| s.len() == f.len()));
        let title = s.chat(chat).map(|c| c.title).unwrap_or_default();
        let delete = move || {
            let ids: Vec<u64> = selected.get_untracked().into_iter().collect();
            if ids.is_empty() {
                return;
            }
            let n = ids.len();
            let bytes = chosen_bytes();
            confirm(
                if n == 1 {
                    "Delete 1 file?".to_string()
                } else {
                    format!("Delete {n} files?")
                },
                format!(
                    "This frees {} on this device. The messages stay in the chat.",
                    format_bytes(bytes)
                ),
                "Delete",
                move || {
                    glide();
                    s.delete_files(&ids);
                    selected.set(HashSet::new());
                    toast(format!("Freed {}", format_bytes(bytes)));
                },
            );
        };
        page_with(
            title,
            Some(
                btn(
                    move || if all() { "Deselect all" } else { "Select all" },
                    Variant::Ghost,
                    BtnSize::Sm,
                    move || {
                        if all() {
                            selected.set(HashSet::new());
                        } else {
                            selected.set(files.with(|f| f.iter().map(|x| x.id).collect()));
                        }
                    },
                )
                .into_element(),
            ),
            view()
                .flex_1()
                .child(
                    virtual_list(
                        move || files.with(Vec::len),
                        move |at| {
                            let f = computed(move || files.with(|l| l.get(at.index()).cloned()));
                            let id = move || f.with(|f| f.as_ref().map_or(0, |f| f.id));
                            let chosen = move || selected.with(|s| s.contains(&id()));
                            let name = move || {
                                f.with(|f| f.as_ref().map(|f| f.name.clone()).unwrap_or_default())
                            };
                            let kind =
                                move || f.with(|f| f.as_ref().map_or(FileKind::Photo, |f| f.kind));
                            press_surface(Press::Row, move || {
                                let i = id();
                                selected.update(|s| {
                                    if !s.remove(&i) {
                                        s.insert(i);
                                    }
                                })
                            })
                            .selected(chosen)
                            .accessibility_label(move || Some(SharedString::from(name())))
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .min_h(px(if is_desktop() { 52.0 } else { 64.0 }))
                            .child(check_circle(chosen))
                            .child(
                                view()
                                    .size(px(40.0))
                                    .rounded(move || radius::sm())
                                    .items_center()
                                    .justify_center()
                                    .bg(move || c::accent())
                                    .text_color(move || c::primary())
                                    .child(dynamic(move || {
                                        let k = kind();
                                        untracked(move || icon(kind_glyph(k), 18.0).into_element())
                                    })),
                            )
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(2.0))
                                    .child(
                                        text(name).text_size(px(font::body())).number_of_lines(1),
                                    )
                                    .child(
                                        text(move || {
                                            f.with(|f| {
                                                f.as_ref()
                                                    .map(|f| {
                                                        format!(
                                                            "{} · {}",
                                                            f.kind.label(),
                                                            when_label(f.at)
                                                        )
                                                    })
                                                    .unwrap_or_default()
                                            })
                                        })
                                        .text_size(px(font::SMALL))
                                        .text_color(move || c::muted_fg()),
                                    ),
                            )
                            .child(
                                text(move || {
                                    f.with(|f| {
                                        f.as_ref()
                                            .map(|f| format_bytes(f.bytes))
                                            .unwrap_or_default()
                                    })
                                })
                                .text_size(px(font::SMALL + 1.0))
                                .tabular_nums()
                                .text_color(move || c::muted_fg()),
                            )
                        },
                    )
                    .keys(move || files.with(|f| f.iter().map(|x| x.id).collect::<Vec<_>>()))
                    .estimated_item_size(px(60.0))
                    .separator(|| separator_inset(124.0))
                    .empty(empty_state(
                        Icon::HardDrive,
                        "Nothing stored",
                        "This chat has no files on this device.",
                        None,
                    ))
                    .flex_1(),
                )
                .child(bottom_bar(
                    btn(
                        move || match selected.with(HashSet::len) {
                            0 => "Select files to delete".to_string(),
                            1 => format!("Delete 1 file · {}", format_bytes(chosen_bytes())),
                            n => format!("Delete {n} files · {}", format_bytes(chosen_bytes())),
                        },
                        Variant::Danger,
                        BtnSize::Lg,
                        delete,
                    )
                    .disabled(move || selected.with(HashSet::is_empty)),
                )),
        )
    });
}

/// Network usage since the last reset.
fn network_usage() {
    nav::push("network-usage", move || {
        let s = store();
        let usage = move || groups_state().with(|g| g.usage.clone());
        let line =
            move |title: &'static str,
                  glyph: Icon,
                  f: fn(&crate::store::ext_groups::NetworkUsage) -> (u64, u64)| {
                view()
                    .child(dynamic(move || {
                        let (sent, got) = f(&usage());
                        untracked(move || {
                            Row::new(title)
                                .icon(glyph)
                                .subtitle(format!(
                                    "Sent {} · Received {}",
                                    format_bytes(sent),
                                    format_bytes(got)
                                ))
                                .build()
                        })
                    }))
                    .into_element()
            };
        page(
            "Network usage",
            page_scroll(
                view()
                    .pt_4()
                    .gap_4()
                    .child(
                        view().px_4().child(
                            card()
                                .gap_1()
                                .child(
                                    text(move || format_bytes(usage().total()))
                                        .text_size(px(font::DISPLAY))
                                        .font_weight(FontWeight::BOLD)
                                        .tabular_nums(),
                                )
                                .child(muted(move || {
                                    format!("Sent and received since {}", usage().since)
                                })),
                        ),
                    )
                    .child(group([
                        line("Messages", Icon::Chats, |u| {
                            (u.messages_sent, u.messages_received)
                        }),
                        line("Media", Icon::Image, |u| (u.media_sent, u.media_received)),
                        line("Calls", Icon::Phone, |u| (u.calls_sent, u.calls_received)),
                    ]))
                    .child(view().px_4().child(btn(
                        "Reset statistics",
                        Variant::Outline,
                        BtnSize::Md,
                        move || {
                            confirm(
                                "Reset network usage?",
                                "The counts start again from zero. Nothing else changes.",
                                "Reset",
                                move || s.reset_network_usage(),
                            )
                        },
                    )))
                    .child(footnote(
                        "Counted on this device only. Songbird doesn't collect usage statistics.",
                    )),
            ),
        )
    });
}
