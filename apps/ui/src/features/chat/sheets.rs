//! The conversation's sheets: emoji, attachments and their mock pickers
//! (poll and event creators, contacts, stickers), forward (up to five
//! chats), delete, more reactions, disappearing messages.

use nsh_core::prelude::*;

use super::{Conv, logic};
use crate::{
    store::{Body, ChatId, ChatKind, Disappear},
    ui::*,
};

/// Every sheet, mounted once (each opens from its signal).
pub(crate) fn all(cv: Conv) -> AnyElement {
    view()
        .child(emoji_sheet(cv))
        .child(attach_sheet(cv))
        .child(poll_sheet(cv))
        .child(event_sheet(cv))
        .child(contact_sheet(cv))
        .child(sticker_sheet(cv))
        .child(crate::features::maps::pick(
            cv.location_open,
            move |lat, lon| {
                cv.send_body(Body::Location {
                    label: format!("{lat:.5}, {lon:.5}"),
                    live: false,
                    at: Some((lat, lon)),
                });
            },
        ))
        .child(camera_sheet(cv))
        .child(forward_sheet(cv))
        .child(delete_sheet(cv))
        .child(reactions_sheet(cv))
        .child(timer_sheet(cv.s, cv.chat, cv.timer_open))
        .into_element()
}

const EMOJI: [&str; 48] = [
    "😀", "😂", "🥲", "😊", "😍", "🥰", "😘", "😉", "😎", "🤔", "😴", "😅", "😭", "😡", "😮", "🙄",
    "👍", "👎", "👏", "🙏", "🙌", "👋", "💪", "🤝", "❤️", "🧡", "💛", "💚", "💙", "💜", "🔥", "✨",
    "🎉", "🎂", "☕", "🍷", "🍕", "🌻", "🏡", "🚴", "⚽", "📚", "📞", "📷", "✅", "❌", "⏰", "🐦",
];

/// A grid of emoji as buttons; `pick` gets the one chosen.
fn emoji_grid(
    set: &'static [&'static str],
    size: f32,
    pick: impl Fn(&'static str) + Copy + 'static,
) -> View {
    let cell = if is_desktop() { 44.0 } else { 48.0 };
    view()
        .flex_row()
        .flex_wrap()
        .px_3()
        .pb_2()
        .justify_center()
        .children(set.iter().map(move |&e| {
            pressable()
                .on_press(move || pick(e))
                .accessibility_label(e)
                .size(px(cell))
                .items_center()
                .justify_center()
                .rounded(move || radius::control())
                .hover(|st| st.bg(move || c::accent()))
                .child(text(e).text_size(px(size)))
        }))
}

fn emoji_sheet(cv: Conv) -> AnyElement {
    let open = cv.emoji_open;
    app_sheet(
        open,
        "Emoji",
        emoji_grid(&EMOJI, 26.0, move |e| {
            cv.draft.update(|d| d.push_str(e));
            if cv.editing.get_untracked().is_none() {
                cv.s.set_draft(cv.chat, cv.draft.get_untracked());
            }
        }),
    )
    .into_element()
}

fn reactions_sheet(cv: Conv) -> AnyElement {
    let open = cv.reactions_open;
    app_sheet(
        open,
        "React",
        emoji_grid(&EMOJI, 26.0, move |e| {
            if let Some(&id) = cv.acting_on.get_untracked().first() {
                cv.react(id, e);
            }
            open.set(false);
        }),
    )
    .into_element()
}

/// One tile of the attachment menu: a tonal violet disc with the glyph,
/// the label under it (one brand hue for all; the glyph tells them apart).
fn tile(glyph: Icon, label: &'static str, f: impl Fn() + 'static) -> AnyElement {
    let disc = if is_desktop() { 44.0 } else { 52.0 };
    pressable()
        .on_press(f)
        .accessibility_label(label)
        .w(relative(0.25))
        .py_2()
        .gap(px(6.0))
        .items_center()
        .rounded(move || radius::lg())
        .hover(|st| st.bg(move || c::accent()))
        .child(
            view()
                .size(px(disc))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(move || c::accent())
                .border(hairline())
                .border_color(move || c::primary().opacity(0.18))
                .text_color(move || c::primary())
                .child(icon(glyph, if is_desktop() { 20.0 } else { 22.0 })),
        )
        .child(
            text(label)
                .number_of_lines(1)
                .text_size(px(if is_desktop() {
                    font::CAPTION
                } else {
                    font::SMALL
                }))
                .text_center()
                .text_color(move || c::foreground()),
        )
        .into_element()
}

/// Runs the share sheet's chosen action once, under no owner (a picker's task outlives the sheet).
fn run_chosen(chosen: Signal<Option<fn(Conv)>>, cv: Conv) {
    if let Some(f) = chosen.try_get().flatten() {
        chosen.set(None);
        Owner::detached().run(|| f(cv));
    }
}

/// Photo & video, Camera, Document, Location, Contact, Poll, Event,
/// Sticker: four to a row. Each picker is a mock that sends a realistic
/// body. A sheet on phones; on desktops a popover above the paperclip.
fn attach_sheet(cv: Conv) -> AnyElement {
    let open = cv.attach_open;
    // A tile's choice runs once the sheet has gone (`app_sheet_then`): on a phone the next sheet
    // or the system picker can't show while this one is still leaving, and a picker's task
    // started under the tile was cancelled with it (D-0183).
    let chosen: Signal<Option<fn(Conv)>> = signal(None);
    let then = move |f: fn(Conv)| {
        move || {
            if cv.wide {
                open.set(false);
                f(cv);
            } else {
                chosen.set(Some(f));
                open.set(false);
                // Where the sheet tells no `Close` (no exit to wait for), the choice still runs.
                Owner::detached().run(|| {
                    set_timeout(std::time::Duration::from_millis(450), move || run_chosen(chosen, cv));
                });
            }
        }
    };
    let whatsapp =
        cv.s.chat(cv.chat)
            .is_some_and(|c| matches!(c.kind, ChatKind::WhatsApp(_)));
    let tiles = move || {
        let tiles = vec![
            // The system photo picker; app-core encrypts and uploads (D-0141).
            tile(
                Icon::Image,
                "Photos",
                then(|cv| {
                    if crate::backend::chat::owns(cv.chat) {
                        let (s, chat) = (cv.s, cv.chat);
                        let hd = s.settings.with_untracked(|x| x.hd_uploads);
                        spawn(async move {
                            let mut r = nsh_core::picker::PickRequest::new(true, 10);
                            // Standard quality scales photos down; HD keeps them.
                            if !hd {
                                r.max_dimension = Some(DevicePixels(1600));
                            }
                            let files = pick_images(r).await;
                            crate::backend::chat::send_picked(s, chat, files, true);
                        });
                        return;
                    }
                    cv.send_body(Body::Image {
                        caption: String::new(),
                        alt: "A photo from your library: a street in the old town at dusk".into(),
                        w: 4,
                        h: 3,
                        hd: cv.s.settings.with_untracked(|x| x.hd_uploads),
                    })
                }),
            ),
            tile(Icon::Camera, "Camera", then(|cv| cv.camera_open.set(true))),
            // A photo seen once, then deleted on every device (real chats).
            tile(
                Icon::Eye,
                "View once",
                then(|cv| {
                    let (s, chat) = (cv.s, cv.chat);
                    if !crate::backend::chat::owns(chat) {
                        toast("View once works in chats with real contacts.");
                        return;
                    }
                    spawn(async move {
                        let mut r = nsh_core::picker::PickRequest::new(true, 1);
                        r.max_dimension = Some(DevicePixels(1600));
                        if let Some(f) = pick_images(r).await.into_iter().next() {
                            crate::backend::chat::send_view_once(s, chat, f);
                        }
                    });
                }),
            ),
            // The system's document picker; app-core encrypts and uploads.
            tile(
                Icon::File,
                "Document",
                then(|cv| {
                    if crate::backend::chat::owns(cv.chat) {
                        let (s, chat) = (cv.s, cv.chat);
                        spawn(async move {
                            let r = nsh_core::picker::FilePickRequest::new(true, 10)
                                .max_size(songbird_app_core::attachments::MAX_FILE as u64);
                            let files = pick_files(r).await;
                            crate::backend::chat::send_picked(s, chat, files, false);
                        });
                        return;
                    }
                    cv.send_body(Body::File {
                        name: "Meeting notes.pdf".into(),
                        size: "312 KB".into(),
                        kind: "PDF".into(),
                    })
                }),
            ),
            // The map picker on self-hosted OSM tiles (D-0142).
            tile(
                Icon::MapPin,
                "Location",
                then(|cv| cv.location_open.set(true)),
            ),
            tile(
                Icon::Contact,
                "Contact",
                then(|cv| cv.contact_open.set(true)),
            ),
            tile(Icon::Poll, "Poll", then(|cv| cv.poll_open.set(true))),
            tile(Icon::Calendar, "Event", then(|cv| cv.event_open.set(true))),
            tile(
                Icon::Sticker,
                "Sticker",
                then(|cv| cv.sticker_open.set(true)),
            ),
        ];
        if whatsapp {
            // WhatsApp's bridge carries no polls or events from Songbird yet.
            tiles
                .into_iter()
                .enumerate()
                .filter(|(i, _)| *i != 5 && *i != 6)
                .map(|(_, t)| t)
                .collect()
        } else {
            tiles
        }
    };
    let content = move || {
        view()
            .gap_3()
            .child(view().flex_row().flex_wrap().children(tiles()))
            // Features WhatsApp chats lack are explained, never silently gone (A15).
            .child(
                whatsapp.then(|| crate::features::whatsapp::unsupported_note("Polls and events")),
            )
            .child(footnote("Files and photos are end-to-end encrypted too.").text_center())
    };
    if !cv.wide {
        return app_sheet_then(open, "Share", content().px_3().pb_3(), move || {
            run_chosen(chosen, cv);
        })
        .into_element();
    }
    // Desktops: a popover anchored above the paperclip, its leading edge
    // on the button's, kept 8pt inside the window; Escape or a press
    // outside closes it.
    const W: f32 = 344.0;
    dynamic(move || {
        if !open.get() {
            return None;
        }
        let (x, y, _, _) =
            cv.attach_at
                .get_untracked()
                .unwrap_or((24.0, window().height.0 - 72.0, 32.0, 32.0));
        let win = window();
        let left = x.min(win.width.0 - W - 8.0).max(8.0).round();
        let bottom = (win.height.0 - y + 8.0).max(8.0).round();
        let pop = if motion::reduced() {
            motion()
        } else {
            motion().entering(
                MotionSpec::new(super::timing(motion::FAST, Easing::EaseOut))
                    .values(MotionChannel::Opacity, [0.0, 1.0])
                    .values(MotionChannel::Scale, [0.97, 1.0])
                    .values(MotionChannel::TranslateY, [6.0, 0.0]),
            )
        };
        Some(untracked(|| {
            view()
                .child(
                    overlay().absolute().inset_0().child(
                        pressable()
                            .absolute()
                            .inset_0()
                            .accessibility_label("Close")
                            .on_press(move || open.set(false)),
                    ),
                )
                .child(
                    overlay()
                        .absolute()
                        .left(px(left))
                        .bottom(px(bottom))
                        .child(
                            view()
                                .bind_motion(&pop)
                                .w(px(W))
                                .p_2()
                                .pt_3()
                                .rounded(move || radius::lg())
                                .bg(move || c::popover())
                                .border_1()
                                .border_color(move || c::border())
                                .shadow_lg()
                                .role(AccessibilityRole::Dialog)
                                .label("Share")
                                .child(content()),
                        ),
                )
        }))
    })
    .into_element()
}

fn poll_sheet(cv: Conv) -> AnyElement {
    let open = cv.poll_open;
    let question = signal(String::new());
    let options: [Signal<String>; 4] = std::array::from_fn(|_| signal(String::new()));
    let multiple = signal(false);
    let filled = move || {
        options
            .iter()
            .filter(|o| o.with(|t| !t.trim().is_empty()))
            .count()
    };
    let ready = move || question.with(|q| !q.trim().is_empty()) && filled() >= 2;
    let send = move || {
        let opts: Vec<(String, u32)> = options
            .iter()
            .map(|o| o.get_untracked().trim().to_string())
            .filter(|t| !t.is_empty())
            .map(|t| (t, 0))
            .collect();
        cv.send_body(Body::Poll {
            question: question.get_untracked().trim().to_string(),
            options: opts,
            mine: Vec::new(),
            multiple: multiple.get_untracked(),
            closed: false,
        });
        batch(|| {
            question.set(String::new());
            for o in &options {
                o.set(String::new());
            }
            multiple.set(false);
            open.set(false);
        });
    };
    app_sheet(
        open,
        "Create poll",
        view()
            .px_4()
            .pb_3()
            .gap_3()
            .child(labelled_field(
                "Question",
                field("Ask a question", question),
                "",
                || false,
            ))
            .child(
                view()
                    .gap_2()
                    .child(section_title("Options").px_0().pt_0().pb_0())
                    .children(
                        options
                            .iter()
                            .enumerate()
                            .map(|(i, &o)| field(format!("Option {}", i + 1), o)),
                    ),
            )
            .child(toggle_row(
                "Allow multiple answers",
                None,
                move || multiple.get(),
                move |v| multiple.set(v),
            ))
            .child(
                btn("Send poll", Variant::Primary, BtnSize::Md, send).disabled(move || !ready()),
            ),
    )
    .into_element()
}

fn event_sheet(cv: Conv) -> AnyElement {
    let open = cv.event_open;
    let title = signal(String::new());
    let when_ = signal(String::new());
    let place = signal(String::new());
    let ready =
        move || title.with(|t| !t.trim().is_empty()) && when_.with(|t| !t.trim().is_empty());
    let send = move || {
        cv.send_body(Body::Event {
            title: title.get_untracked().trim().into(),
            when: when_.get_untracked().trim().into(),
            place: place.get_untracked().trim().into(),
            going: 1,
            mine: Some(true),
        });
        batch(|| {
            title.set(String::new());
            when_.set(String::new());
            place.set(String::new());
            open.set(false);
        });
    };
    app_sheet(
        open,
        "Create event",
        view()
            .px_4()
            .pb_3()
            .gap_3()
            .child(labelled_field(
                "Event name",
                field("Dinner at Mama's", title),
                "",
                || false,
            ))
            .child(labelled_field(
                "When",
                field("Sun 22 Nov, 13:00", when_),
                "",
                || false,
            ))
            .child(labelled_field(
                "Where (optional)",
                field("Add a place", place),
                "",
                || false,
            ))
            .child(
                btn("Send event", Variant::Primary, BtnSize::Md, send).disabled(move || !ready()),
            ),
    )
    .into_element()
}

/// Share a contact: the user's contacts.
fn contact_sheet(cv: Conv) -> AnyElement {
    let open = cv.contact_open;
    let s = cv.s;
    let people = s.contacts.with_untracked(|cs| {
        cs.iter()
            .filter(|c| !c.blocked)
            .cloned()
            .collect::<Vec<_>>()
    });
    app_sheet(
        open,
        "Share contact",
        scroll()
            .max_h(px(420.0))
            .child(view().pb_2().children(people.into_iter().map(move |p| {
                let (name, username, seed) = (p.name.clone(), p.username.clone(), p.id);
                Row::new(p.name.clone())
                    .leading(avatar(p.name.clone(), seed, 36.0))
                    .subtitle(format!("@{}", p.username))
                    .on_press(move || {
                        open.set(false);
                        cv.send_body(Body::Contact {
                            name: name.clone(),
                            username: username.clone(),
                        });
                    })
                    .build()
            }))),
    )
    .into_element()
}

/// The camera (D-0110): the live preview, flip and the shutter. The photo comes back as a JPEG
/// in memory with its metadata already stripped, and is never written to the photo library.
fn camera_sheet(cv: Conv) -> AnyElement {
    let open = cv.camera_open;
    let busy = signal(false);
    app_sheet(
        open,
        "Camera",
        view()
            .px_4()
            .pb_4()
            .gap_4()
            .items_center()
            .child(dynamic(move || {
                let is_open = open.get();
                untracked(move || {
                    if !is_open {
                        busy.set(false);
                        return view().into_element();
                    }
                    let session = crate::camera::use_session(
                        songbird_camera::Purpose::Photo,
                        || true,
                        |_| {},
                    );
                    let shown = session.shown;
                    let running = move || matches!(shown.get(), crate::camera::Shown::Running(_));
                    let (flip, shoot) = (session.clone(), session.clone());
                    view()
                        .gap_4()
                        .items_center()
                        .child(crate::camera::camera_view(&session, 280.0, ""))
                        .child(when(running, move || {
                            let (flip, shoot) = (flip.clone(), shoot.clone());
                            view()
                                .flex_row()
                                .gap_3()
                                .child(btn_with_icon(
                                    "Flip",
                                    Some(Icon::FlipCamera),
                                    Variant::Ghost,
                                    BtnSize::Md,
                                    move || {
                                        flip.flip();
                                    },
                                ))
                                .child(
                                    btn_with_icon(
                                        "Take photo",
                                        Some(Icon::Camera),
                                        Variant::Primary,
                                        BtnSize::Md,
                                        move || {
                                            busy.set(true);
                                            shoot.take_photo(move |result| {
                                                let _ = busy.try_set(false);
                                                match result {
                                                    Ok(still) => {
                                                        open.set(false);
                                                        // mock: app-core encrypts the JPEG with the attachment
                                                        // format (Prompt 6) and uploads it; until then only its
                                                        // size reaches the message, and the bytes are dropped.
                                                        cv.send_body(Body::Image {
                                                            caption: String::new(),
                                                            alt: "A photo you took".into(),
                                                            w: still.width.max(1),
                                                            h: still.height.max(1),
                                                            hd: false,
                                                        });
                                                    }
                                                    Err(_) => toast("The photo couldn't be taken"),
                                                }
                                            });
                                        },
                                    )
                                    .disabled(move || busy.get()),
                                )
                        }))
                        .into_element()
                })
            })),
    )
    .into_element()
}

const STICKERS: [&str; 12] = [
    "🦉", "🐦", "🦊", "🐻", "🐼", "🐸", "🌻", "🍀", "🎈", "🧁", "☕", "🚲",
];

fn sticker_sheet(cv: Conv) -> AnyElement {
    let open = cv.sticker_open;
    // Installed sound-sticker packs follow the built-in stickers.
    let sound_packs = computed(|| {
        crate::store::ext_groups::groups_state().with(|g| {
            g.packs
                .iter()
                .filter(|p| p.kind == crate::store::ext_groups::PackKind::SoundStickers)
                .map(|p| (p.name.clone(), p.items.clone()))
                .collect::<Vec<_>>()
        })
    });
    let cell = if is_desktop() { 44.0 } else { 48.0 };
    // GIFs come through the privacy proxy, in real chats only (they are sent as photos).
    let gifs_here = crate::backend::chat::owns(cv.chat);
    let tab = signal(false);
    app_sheet(
        open,
        "Stickers",
        view()
            .child(gifs_here.then(|| {
                view().px_4().pb_2().child(segmented(
                    vec![(false, "Stickers"), (true, "GIFs")],
                    move || tab.get(),
                    move |v| tab.set(v),
                ))
            }))
            .child(when(
                move || tab.get(),
                move || gif_grid(cv, open).into_element(),
            ))
            .child(when(
                move || !tab.get(),
                move || stickers_grid(cv, open, sound_packs, cell).into_element(),
            )),
    )
    .into_element()
}

/// GIF search: trending until something is typed; a GIF picked is fetched through the proxy
/// and sent as an encrypted photo.
fn gif_grid(cv: Conv, open: Signal<bool>) -> View {
    let query = signal(String::new());
    let results = signal(Vec::<crate::backend::Gif>::new());
    let state = signal(0u8); // 0 loading, 1 shown, 2 failed
    let serial = signal(0u64);
    effect(move || {
        let q = query.get();
        let n = untracked(move || {
            serial.update(|x| *x += 1);
            serial.get()
        });
        // A short pause after typing, so each key does not search.
        set_timeout(std::time::Duration::from_millis(350), move || {
            if serial.get_untracked() != n {
                return;
            }
            state.set(0);
            spawn(async move {
                let got = crate::backend::gifs(&q).await;
                if serial.get_untracked() != n {
                    return;
                }
                match got {
                    Ok(list) => {
                        results.set(list);
                        state.set(1);
                    }
                    Err(_) => state.set(2),
                }
            });
        });
    });
    let pick = move |g: crate::backend::Gif| {
        open.set(false);
        toast("Sending GIF…");
        let (s, chat) = (cv.s, cv.chat);
        spawn(async move {
            match crate::backend::unsigned("GET", &g.gif).await {
                Ok((200, bytes)) if !bytes.is_empty() => {
                    let file = nsh_core::picker::PickedFile::new(
                        format!("{}.gif", g.id),
                        "image/gif",
                        nsh_core::image::ImageData::new(bytes),
                    );
                    crate::backend::chat::send_picked(s, chat, vec![file], true);
                }
                _ => toast("The GIF couldn't be fetched."),
            }
        });
    };
    view()
        .px_3()
        .pb_3()
        .gap_2()
        .child(search_field("Search GIFs", query))
        .child(dynamic(move || {
            let st = state.get();
            let list = results.get();
            untracked(move || match st {
                0 => view().py_6().items_center().child(muted("Loading…")).into_element(),
                2 => view()
                    .py_6()
                    .items_center()
                    .child(muted("GIFs couldn't be loaded. Try again in a moment."))
                    .into_element(),
                _ if list.is_empty() => view().py_6().items_center().child(muted("No GIFs found.")).into_element(),
                _ => scroll()
                    .max_h(px(360.0))
                    .child(
                        view().flex_row().flex_wrap().gap_1().children(list.into_iter().map(move |g| {
                            let ratio = g.width.max(1) as f32 / g.height.max(1) as f32;
                            let still = g.still.clone();
                            pressable()
                                .on_press({
                                    let g = g.clone();
                                    move || pick(g.clone())
                                })
                                .accessibility_label("GIF")
                                .h(px(96.0))
                                .w(px((96.0 * ratio).clamp(64.0, 200.0)))
                                .rounded(move || radius::sm())
                                .overflow_hidden()
                                .bg(move || c::secondary())
                                .child(image(still).w_full().h_full().object_fit(ResizeMode::Cover))
                        })),
                    )
                    .into_element(),
            })
        }))
        .child(footnote("Searches go through Songbird's privacy proxy: the GIF provider never sees who you are."))
}

/// Installed sticker packs (real chats): their artwork, sent as stickers.
fn installed_stickers(cv: Conv, open: Signal<bool>) -> View {
    let packs = computed(|| {
        crate::store::ext_groups::groups_state().with(|g| {
            g.packs
                .iter()
                .filter(|p| {
                    p.kind == crate::store::ext_groups::PackKind::Stickers && !p.objects.is_empty()
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    view().child(dynamic(move || {
        let list = packs.get();
        untracked(move || {
            if !crate::backend::chat::owns(cv.chat) {
                return view().into_element();
            }
            view()
                .children(list.into_iter().map(move |p| {
                    let (pid, name) = (p.id, p.name.clone());
                    let label = format!("Sticker from {}", p.name);
                    view().child(section_title(p.name.clone())).child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .px_3()
                            .pb_2()
                            .justify_center()
                            .children(p.objects.iter().copied().zip(p.items.clone()).map(
                                move |(object, emoji)| {
                                    let name = name.clone();
                                    let src = crate::backend::packs::image(pid, object);
                                    pressable()
                                        .on_press(move || {
                                            open.set(false);
                                            crate::backend::chat::send_sticker(
                                                cv.s,
                                                cv.chat,
                                                Some(pid),
                                                object,
                                                emoji.clone(),
                                                name.clone(),
                                            );
                                        })
                                        .accessibility_label(label.clone())
                                        .size(px(64.0))
                                        .p_1()
                                        .rounded(move || radius::control())
                                        .hover(|st| st.bg(move || c::accent()))
                                        .child(src.map(|s| {
                                            image(s)
                                                .w_full()
                                                .h_full()
                                                .object_fit(ResizeMode::Contain)
                                        }))
                                },
                            )),
                    )
                }))
                .into_element()
        })
    }))
}

/// The built-in stickers and installed sound-sticker packs.
fn stickers_grid(
    cv: Conv,
    open: Signal<bool>,
    sound_packs: Memo<Vec<(String, Vec<String>)>>,
    cell: f32,
) -> View {
    view()
        .child(emoji_grid(&STICKERS, 40.0, move |e| {
            open.set(false);
            if crate::backend::chat::owns(cv.chat) {
                let object = STICKERS.iter().position(|x| *x == e).unwrap_or(0);
                crate::backend::chat::send_sticker(
                    cv.s,
                    cv.chat,
                    None,
                    u32::try_from(object).unwrap_or(0),
                    e.into(),
                    "Songbird friends".into(),
                );
                return;
            }
            cv.send_body(Body::Sticker {
                emoji: e.into(),
                pack: "Songbird friends".into(),
            });
        }))
        .child(installed_stickers(cv, open))
        .child(dynamic(move || {
            let packs = sound_packs.get();
            untracked(move || {
                view().children(packs.into_iter().map(move |(name, items)| {
                    view()
                        .child(section_title(format!("{name} · sound stickers")))
                        .child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .px_3()
                                .pb_2()
                                .justify_center()
                                .children(items.into_iter().map(move |e| {
                                    let (label, pick, pack) =
                                        (format!("{e}, plays a sound"), e.clone(), name.clone());
                                    pressable()
                                        .on_press(move || {
                                            open.set(false);
                                            cv.send_body(Body::Sticker {
                                                emoji: pick.clone(),
                                                pack: pack.clone(),
                                            });
                                        })
                                        .accessibility_label(label)
                                        .size(px(cell))
                                        .items_center()
                                        .justify_center()
                                        .rounded(move || radius::control())
                                        .hover(|st| st.bg(move || c::accent()))
                                        .child(text(e).text_size(px(40.0)))
                                })),
                        )
                }))
            })
        }))
}

/// Forward to up to five chats.
fn forward_sheet(cv: Conv) -> AnyElement {
    let open = cv.forward_open;
    let s = cv.s;
    let chosen = signal(Vec::<ChatId>::new());
    let chats = s.inbox();
    let send = move || {
        let to = chosen.get_untracked();
        let ids = cv.acting_on.get_untracked();
        for &target in &to {
            for &id in &ids {
                s.forward(cv.chat, id, target);
            }
        }
        batch(|| {
            chosen.set(Vec::new());
            open.set(false);
        });
        cv.end_select();
        toast(match to.len() {
            1 => "Forwarded".to_string(),
            n => format!("Forwarded to {n} chats"),
        });
    };
    app_sheet(
        open,
        "Forward to…",
        view()
            .pb_2()
            .child(
                footnote(format!(
                    "You can forward to up to {} chats at a time.",
                    logic::FORWARD_LIMIT
                ))
                .pb_2(),
            )
            .child(
                scroll()
                    .max_h(px(380.0))
                    .child(view().children(chats.into_iter().map(move |c| {
                        let id = c.id;
                        let on = move || chosen.with(|v| v.contains(&id));
                        let full =
                            move || !on() && !logic::can_forward_to_more(chosen.with(Vec::len));
                        pressable()
                            .on_press(move || {
                                chosen.update(|v| {
                                    if let Some(i) = v.iter().position(|x| *x == id) {
                                        v.remove(i);
                                    } else if logic::can_forward_to_more(v.len()) {
                                        v.push(id);
                                    }
                                })
                            })
                            .disabled(full)
                            .selected(on)
                            .accessibility_label(c.title.clone())
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .min_h(px(if is_desktop() { 48.0 } else { 56.0 }))
                            .hover(|st| st.bg(move || c::accent()))
                            .child(avatar(c.title.clone(), c.seed, 36.0))
                            .child(
                                text(c.title.clone())
                                    .flex_1()
                                    .number_of_lines(1)
                                    .text_size(px(font::body())),
                            )
                            .child(
                                view()
                                    .size(px(22.0))
                                    .rounded_full()
                                    .border_2()
                                    .items_center()
                                    .justify_center()
                                    .border_color(
                                        move || if on() { c::primary() } else { c::muted_fg() },
                                    )
                                    .bg(move || {
                                        if on() {
                                            c::primary()
                                        } else {
                                            AdaptiveColor::TRANSPARENT
                                        }
                                    })
                                    .text_color(move || c::on_primary())
                                    .child(
                                        icon(Icon::Check, 14.0)
                                            .opacity(move || if on() { 1.0_f32 } else { 0.0 }),
                                    ),
                            )
                    }))),
            )
            .child(
                view().px_4().pt_3().child(
                    btn(
                        move || match chosen.with(Vec::len) {
                            0 => "Forward".to_string(),
                            n => format!("Forward to {n}"),
                        },
                        Variant::Primary,
                        BtnSize::Md,
                        send,
                    )
                    .disabled(move || chosen.with(Vec::is_empty)),
                ),
            ),
    )
    .into_element()
}

/// Delete for me, or for everyone (only the user's own messages).
fn delete_sheet(cv: Conv) -> AnyElement {
    let open = cv.delete_open;
    let s = cv.s;
    let chat = cv.chat;
    let all_mine = move || {
        let ids = cv.acting_on.get();
        !ids.is_empty()
            && s.messages.with(|m| {
                m.get(&chat).is_some_and(|v| {
                    v.iter()
                        .filter(|x| ids.contains(&x.id))
                        .all(|x| x.mine() && !matches!(x.body, Body::Deleted | Body::System(_)))
                })
            })
    };
    let done = move |msg: &'static str| {
        open.set(false);
        cv.end_select();
        toast(msg);
    };
    app_sheet(
        open,
        "Delete message?",
        view()
            .px_4()
            .pb_3()
            .gap_2()
            .child(muted(move || {
                if all_mine() {
                    "Delete for everyone removes it from everyone's devices in this chat. They'll see that a message was deleted."
                } else {
                    "It will be removed from this device only."
                }
            }))
            .child(when(all_mine, move || {
                btn("Delete for everyone", Variant::Danger, BtnSize::Md, move || {
                    s.delete_messages_for_everyone(chat, &cv.acting_on.get_untracked());
                    done("Deleted for everyone");
                })
            }))
            .child(btn("Delete for me", Variant::Outline, BtnSize::Md, move || {
                s.delete_messages_for_me(chat, &cv.acting_on.get_untracked());
                done("Deleted for you");
            }))
            .child(btn("Cancel", Variant::Ghost, BtnSize::Md, move || open.set(false))),
    )
    .into_element()
}

/// Disappearing messages: Off, 24 hours, 7 days, 4 weeks (the protocol's longest). Shared with
/// chat info.
pub(crate) fn timer_sheet(s: crate::store::Store, chat: ChatId, open: Signal<bool>) -> AnyElement {
    let current = move || s.chat(chat).and_then(|c| c.disappearing);
    let choice = |value: Option<Disappear>, label: &'static str| {
        let on = move || current() == value;
        pressable()
            .on_press(move || {
                if current() != value {
                    s.set_disappearing(chat, value);
                }
                open.set(false);
            })
            .selected(on)
            .role(AccessibilityRole::Radio)
            .accessibility_label(label)
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .min_h(px(if is_desktop() { 44.0 } else { 56.0 }))
            .hover(|st| st.bg(move || c::accent()))
            .child(
                view()
                    .size(px(20.0))
                    .rounded_full()
                    .border_2()
                    .items_center()
                    .justify_center()
                    .border_color(move || if on() { c::primary() } else { c::muted_fg() })
                    .child(
                        view()
                            .size(px(10.0))
                            .rounded_full()
                            .bg(move || c::primary())
                            .opacity(move || if on() { 1.0_f32 } else { 0.0 }),
                    ),
            )
            .child(text(label).flex_1().text_size(px(font::body())))
            .into_element()
    };
    app_sheet(
        open,
        "Disappearing messages",
        view()
            .pb_3()
            .child(footnote("New messages in this chat will disappear after the time you choose. Anyone in the chat can still save or forward them before then.").pb_2())
            .child(view().role(AccessibilityRole::Radiogroup).children([
                choice(None, "Off"),
                choice(Some(Disappear::Day), "24 hours"),
                choice(Some(Disappear::Week), "7 days"),
                choice(Some(Disappear::FourWeeks), "4 weeks"),
            ])),
    )
    .into_element()
}

#[cfg(test)]
mod attach_tests {
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    use super::*;
    use crate::store::Store;

    #[component]
    fn AttachProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        view().flex_1().child(crate::features::chat::conversation(1))
    }

    /// Each picker the share sheet opens after closing itself shows (found on an iPhone: Poll
    /// closed the sheet and opened nothing, D-0183).
    #[test]
    fn the_share_sheet_opens_the_poll_editor() -> Result<(), String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<AttachProbe>()))
            .map_err(|e| e.to_string())?;
        app.wait_for("Attach")?;
        app.press("Attach")?;
        app.wait_for("text:Poll")?;
        app.press("Poll")?;
        app.wait_for("text:Create poll")?;
        Ok(())
    }
}
