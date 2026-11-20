//! The profile page (photo, name, about, status, username with its QR
//! code and link, links) and the "Set a status" sheet.

use nsh_core::prelude::*;

use crate::{
    store::{StatusExpiry, now, store, until_label},
    ui::*,
};

const STATUS_EMOJI: [&str; 8] = ["💬", "📅", "💼", "🏠", "🚗", "🎧", "🌴", "🤒"];
const STATUS_PRESETS: [(&str, &str); 5] = [
    ("📅", "In a meeting"),
    ("💼", "At work"),
    ("🚗", "Commuting"),
    ("🌴", "On holiday"),
    ("🤒", "Off sick"),
];

/// The profile page, pushed.
pub fn profile() {
    super::push("Profile", content);
}

fn content() -> AnyElement {
    let s = store();
    let me = s.me;
    let status_open = signal(false);
    let name = signal(me.with_untracked(|p| p.name.clone()));
    let about = signal(me.with_untracked(|p| p.about.clone()));
    let username = signal(me.with_untracked(|p| p.username.clone()));
    // Writes go through the profile intent as the user types.
    effect(move || {
        let n = name.get();
        if !n.trim().is_empty() {
            untracked(|| s.profile_update(|p| p.name = n.trim().to_string()));
        }
    });
    effect(move || {
        let a = about.get();
        untracked(|| s.profile_update(|p| p.about = a));
    });
    let name_invalid = move || name.with(|n| n.trim().is_empty());
    let candidate = move || username.with(|u| u.trim().trim_start_matches('@').to_lowercase());
    let unchanged = move || me.with(|p| p.username == candidate());
    // Taken or not is the server's answer to the claim; here only the format is checked.
    let available =
        move || unchanged() || songbird_keys::username::Username::parse(&candidate()).is_ok();
    // The real username link (PROTOCOL §4.3): made on first visit, reset on demand.
    let link_url = signal(crate::backend::username::my_link().unwrap_or_default());
    if link_url.get_untracked().is_empty() && crate::backend::registered() {
        let name = me.with_untracked(|p| p.username.clone());
        if !name.is_empty() {
            crate::backend::run(async move {
                let l = crate::backend::username::make_link(&name).await?;
                let _ = link_url.try_set(l);
                Ok(())
            });
        }
    }
    let link = move || link_url.get().trim_start_matches("https://").to_string();
    let seed = me.with_untracked(|p| p.seed);

    view()
        .child(
            view()
                .items_center()
                .pt_6()
                .pb_2()
                .gap_3()
                .child(
                    view()
                        .size(px(112.0))
                        .child(avatar(me.with_untracked(|p| p.name.clone()), seed, 112.0))
                        .child(
                            view()
                                .absolute()
                                .right(px(-4.0))
                                .bottom(px(-4.0))
                                .rounded_full()
                                .bg(move || c::primary())
                                .text_color(move || c::on_primary())
                                .border_2()
                                .border_color(move || c::background())
                                // The system picker; the photo is encrypted on this device.
                                .child(icon_btn(Icon::Camera, "Change profile photo", move || {
                                    crate::backend::avatar::choose(s)
                                })),
                        ),
                )
                .child(
                    view().px_4().max_w(px(392.0)).child(
                        footnote("Your name, photo and about are end\u{2011}to\u{2011}end encrypted. Only your contacts can see them.")
                            .px_0()
                            .text_center(),
                    ),
                ),
        )
        .child(
            view()
                .px_4()
                .pt_4()
                .gap_4()
                .child(labelled_field(
                    "Name",
                    field("Your name", name).max_length(25),
                    move || {
                        if name_invalid() { "Add a name so people know it's you." } else { "Shown to your contacts and in groups." }
                    },
                    name_invalid,
                ))
                .child(labelled_field(
                    "About",
                    field("About", about).max_length(139),
                    "A few words about you, such as what you do or what you love.",
                    || false,
                )),
        )
        .child(super::section(
            "Status",
            vec![super::value_row(
                "Custom status",
                Some(Icon::Smile),
                move || {
                    me.with(|p| p.status_line.clone()).map_or("Set a status".into(), |l| {
                        let e = s.ext().with(|x| x.status_emoji.clone());
                        format!("{e} {l}")
                    })
                },
                move || status_open.set(true),
            )],
        ))
        .child(view().h(px(24.0)))
        .child(
            view().px_4().gap_3().child(labelled_field(
                "Username",
                view()
                    .flex_row()
                    .gap_2()
                    .child(field("username", username).flex_1().auto_capitalize(AutoCapitalize::None))
                    .child(
                        btn("Save", Variant::Secondary, BtnSize::Md, move || {
                            let u = candidate();
                            crate::backend::run(async move {
                                let l = crate::backend::username::change(&u).await?;
                                s.profile_update(|p| p.username = u.clone());
                                let _ = link_url.try_set(l);
                                toast(format!("Your username is now @{u}"));
                                Ok(())
                            });
                        })
                        .disabled(move || unchanged() || !available()),
                    ),
                move || {
                    let u = candidate();
                    if unchanged() {
                        "People can find you by your username. Change it any time.".to_string()
                    } else if u.len() < 3 {
                        "Use at least 3 letters or numbers.".to_string()
                    } else if available() {
                        format!("Save to claim @{u}. If someone has it, you'll be told.")
                    } else {
                        "3 to 32 letters, numbers, _ or ., starting with a letter.".to_string()
                    }
                },
                move || !unchanged() && !available(),
            )),
        )
        .child(
            view().px_4().pt_4().child(
                card()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .child(dynamic(move || qr_code(&link_url.get(), 112.0).into_element()))
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_2()
                            .child(text("Your username link").text_size(px(font::body())).font_weight(FontWeight::SEMIBOLD))
                            .child(muted(link).number_of_lines(1))
                            .child(
                                view()
                                    .flex_row()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(btn_with_icon("Copy link", Some(Icon::Copy), Variant::Outline, BtnSize::Sm, move || {
                                        set_clipboard(link_url.get_untracked());
                                        toast("Link copied");
                                    }))
                                    .child(btn_with_icon("Reset", Some(Icon::Refresh), Variant::Outline, BtnSize::Sm, move || {
                                        confirm(
                                            "Reset your username link?",
                                            "Your old link and QR code will stop working.",
                                            "Reset",
                                            move || crate::backend::run(async move {
                                                let l = crate::backend::username::reset().await?;
                                                let _ = link_url.try_set(l);
                                                toast("New link made. The old one no longer works.");
                                                Ok(())
                                            }),
                                        )
                                    })),
                            ),
                    ),
            ),
        )
        .child(super::foot("Anyone with your link or QR code can start a chat with you. Reset it if it gets around."))
        .child(links_section())
        .child(status_sheet(status_open))
        .into_element()
}

/// The profile's links (a website, a portfolio).
fn links_section() -> View {
    let s = store();
    let ext = s.ext();
    let draft = signal(String::new());
    let valid = move || draft.with(|d| d.contains('.') && !d.contains(' ') && d.len() > 3);
    view()
        .child(section_title("Links"))
        .child(dynamic(move || {
            let links = ext.with(|x| x.links.clone());
            untracked(move || {
                group(links.into_iter().map(|l| {
                    let gone = l.clone();
                    Row::new(l.clone())
                        .icon(Icon::Link)
                        .trailing(
                            icon_btn(Icon::Close, format!("Remove {l}"), move || {
                                let gone = gone.clone();
                                s.ext_update(|x| x.links.retain(|x| *x != gone));
                            })
                            .text_color(move || c::muted_fg()),
                        )
                        .build()
                }))
                .style_when(move || ext.with(|x| x.links.is_empty()), |st| st.hidden())
            })
        }))
        .child(
            view()
                .px_4()
                .pt_3()
                .flex_row()
                .gap_2()
                .child(
                    field("Add a link", draft)
                        .flex_1()
                        .auto_capitalize(AutoCapitalize::None)
                        .keyboard_type(KeyboardType::Url),
                )
                .child(
                    btn("Add", Variant::Secondary, BtnSize::Md, move || {
                        let l = draft.get_untracked().trim().to_string();
                        s.ext_update(|x| x.links.push(l));
                        draft.set(String::new());
                    })
                    .disabled(move || !valid()),
                ),
        )
}

/// The "Set a status" sheet: a line, an emoji, when it clears.
pub fn status_sheet(open: Signal<bool>) -> nsh_base::SheetElement {
    let s = store();
    let me = s.me;
    let line = signal(me.with_untracked(|p| p.status_line.clone().unwrap_or_default()));
    let emoji = signal(s.ext().with_untracked(|x| x.status_emoji.clone()));
    let expiry = signal(StatusExpiry::Hour);
    // Opening shows the status as it is now.
    effect(move || {
        if open.get() {
            untracked(|| {
                line.set(me.with(|p| p.status_line.clone().unwrap_or_default()));
                emoji.set(s.ext().with(|x| x.status_emoji.clone()));
            });
        }
    });
    let content = view()
        .px_4()
        .gap_4()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    view()
                        .size(px(if is_desktop() { 36.0 } else { 48.0 }))
                        .items_center()
                        .justify_center()
                        .rounded(move || radius::control())
                        .bg(move || c::secondary())
                        .accessibility_elements_hidden(true)
                        .child(text(move || emoji.get()).text_size(px(font::TITLE))),
                )
                .child(field("What's happening?", line).flex_1().max_length(60)),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .role(AccessibilityRole::Radiogroup)
                .label("Emoji")
                .children(STATUS_EMOJI.iter().map(move |e| {
                    let e: &'static str = e;
                    tile(format!("Emoji {e}"), if is_desktop() { 36.0 } else { 44.0 }, move || emoji.with(|x| x == e), move || emoji.set(e.into()), text(e).text_size(px(font::TITLE)))
                })),
        )
        .child(
            view()
                .gap_2()
                .child(text("Suggestions").text_size(px(font::SMALL)).font_weight(FontWeight::SEMIBOLD).text_color(move || c::muted_fg()))
                .child(view().flex_row().flex_wrap().gap_2().children(STATUS_PRESETS.iter().map(move |(e, l)| {
                    let (e, l): (&'static str, &'static str) = (e, l);
                    chip(format!("{e} {l}"), move || line.with(|x| x == l), move || {
                        line.set(l.into());
                        emoji.set(e.into());
                    })
                }))),
        )
        .child(
            view()
                .gap_2()
                .child(text("Clear after").text_size(px(font::SMALL)).font_weight(FontWeight::SEMIBOLD).text_color(move || c::muted_fg()))
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .role(AccessibilityRole::Radiogroup)
                        .label("Clear after")
                        .children(StatusExpiry::ALL.into_iter().map(move |x| {
                            chip(x.label(), move || expiry.get() == x, move || expiry.set(x))
                        })),
                )
                .child(muted(move || until_label(expiry.get().until(now()), now()))),
        )
        .child(
            view()
                .gap_2()
                .pt_2()
                .child(
                    btn("Set status", Variant::Primary, BtnSize::Lg, move || {
                        s.set_status(line.get_untracked(), emoji.get_untracked(), expiry.get_untracked());
                        open.set(false);
                        toast("Status set. Contacts you allow will see it.");
                    })
                    .disabled(move || line.with(|l| l.trim().is_empty())),
                )
                .child(when(
                    move || me.with(|p| p.status_line.is_some()),
                    move || {
                        btn("Clear status", Variant::Ghost, BtnSize::Md, move || {
                            s.clear_status();
                            line.set(String::new());
                            open.set(false);
                        })
                    },
                )),
        )
        .child(super::foot("Your status is end\u{2011}to\u{2011}end encrypted, like your profile. Change who sees it in Privacy.").px_0());
    app_sheet(open, "Set a status", content)
}
