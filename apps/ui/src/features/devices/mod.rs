//! A4 identity and devices: safety numbers, linked devices and account
//! security (username link, two-step PIN, passkeys, accounts, deletion).

mod account;
pub mod codes;

use nsh_core::prelude::*;
use songbird_camera::Purpose;

pub use account::{account_security, account_switcher};

use crate::{
    nav,
    store::{ContactId, Device, DeviceKind, Store, ext_onboarding::safety_groups, store},
    ui::*,
};

/// The most devices linked to one account, besides the phone.
pub const MAX_LINKED: usize = 4;

// ---------------------------------------------------------------- safety number

/// The safety-number screen for a contact, pushed.
pub fn safety_number(contact: ContactId) {
    nav::push(format!("safety:{contact}"), move || {
        page("Safety number", safety_body(contact))
    });
}

fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

fn safety_body(contact: ContactId) -> AnyElement {
    let s = store();
    let Some(person) = s.contact(contact) else {
        return empty_state(
            Icon::User,
            "Contact not found",
            "They may have been removed.",
            None,
        )
        .into_element();
    };
    let me = s.me.get_untracked();
    let first = first_name(&person.name);
    let verified = move || s.contact(contact).is_some_and(|c| c.verified);
    let key_changed = move || s.contact(contact).is_some_and(|c| c.key_changed);
    let scanning = signal(false);
    let groups = safety_groups(&person.safety_number);
    let spoken = groups.join(" ");
    let wide_layout = wide();

    let number = view()
        .flex_row()
        .flex_wrap()
        .justify_center()
        .gap_x_4()
        .gap_y_2()
        .max_w(px(320.0))
        .role(AccessibilityRole::Text)
        .label(format!("Safety number: {spoken}"))
        .children(groups.into_iter().map(|g| {
            text(g)
                .w(px(64.0))
                .text_center()
                .font_family(MONO)
                .tabular_nums()
                .text_size(px(font::TITLE))
                .letter_spacing(px(1.0))
                .accessibility_elements_hidden(true)
        }));

    let changed_note = format!(
        "{first}'s safety number changed. This usually means they reinstalled Songbird or got a new phone. Compare the new number to be sure."
    );
    let first2 = first.clone();
    let first3 = first.clone();
    let content = view()
        .w_full()
        .max_w(px(560.0))
        .self_center()
        .px_4()
        .pt_6()
        .gap_5()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_center()
                .gap_3()
                .child(avatar(me.name.clone(), me.seed, 56.0))
                .child(
                    view()
                        .size(px(32.0))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(move || c::accent())
                        .text_color(move || c::primary())
                        .accessibility_elements_hidden(true)
                        .child(icon(Icon::Lock, 16.0)),
                )
                .child(avatar(person.name.clone(), person.id, 56.0)),
        )
        .child(
            text(format!("Verify end\u{2011}to\u{2011}end encryption with {}", person.name))
                .text_size(px(font::HEADING))
                .font_weight(FontWeight::SEMIBOLD)
                .text_center()
                .role(AccessibilityRole::Header),
        )
        .child(when(key_changed, move || banner(Tone::Info, Icon::Info, changed_note.clone())))
        .child(
            card()
                .gap_5()
                .items_center()
                .py_5()
                .child(number)
                .child(
                    view()
                        .role(AccessibilityRole::Image)
                        .label(format!("Code for {first2} to scan"))
                        .child(qr_code(&codes::safety_code(&person.safety_number), if wide_layout { 168.0 } else { 152.0 })),
                )
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .child(btn_with_icon("Scan code", Some(Icon::Scan), Variant::Secondary, BtnSize::Md, move || {
                            scanning.set(true)
                        }))
                        .child(btn_with_icon("Copy", Some(Icon::Copy), Variant::Ghost, BtnSize::Md, {
                            let number = person.safety_number.clone();
                            move || {
                                set_clipboard(number.clone());
                                toast("Safety number copied");
                            }
                        })),
                ),
        )
        .child(
            group([toggle_row(
                "Mark as verified",
                Some("Turn on after the numbers match. You'll see a shield next to their name."),
                verified,
                move |v| {
                    s.verify(contact, v);
                    if v {
                        toast(format!("{first3} is verified"));
                    }
                },
            )])
            .mx_0(),
        )
        .child(
            view()
                .gap_2()
                .child(muted(format!(
                    "To check that no one is listening in, compare these numbers with {first} in person or on a call, or scan each other's code. If they match, your messages and calls are end\u{2011}to\u{2011}end encrypted with {first} and no one else."
                )))
                .child(view().flex_row().child(
                    // The ghost button's own inset pulled back, so its label
                    // sits on the paragraph's edge.
                    btn("Learn more", Variant::Ghost, BtnSize::Sm, || {
                        // An external page: opens in the browser.
                        let _ = open_url("https://songbird.chat/help/safety-numbers");
                    })
                    .ml(px(-12.0)),
                )),
        );

    view()
        .flex_1()
        .child(page_scroll(content))
        .child(scan_sheet(
            s,
            contact,
            scanning,
            first,
            person.safety_number.clone(),
        ))
        .into_element()
}

/// The scanner for a contact's safety-number code: the camera runs while the sheet is open and
/// nothing is read yet; a code for another number says so, anything else is ignored.
fn scan_sheet(
    s: Store,
    contact: ContactId,
    open: Signal<bool>,
    first: String,
    number: String,
) -> nsh_base::SheetElement {
    // None: scanning; Some(true) the numbers match; Some(false) they don't.
    let result = signal(None::<bool>);
    let caption = format!("Point your camera at {first}'s code.");
    app_sheet(
        open,
        "Scan code",
        view()
            .px_4()
            .pb_4()
            .gap_4()
            .items_center()
            .child(dynamic(move || {
                let is_open = open.get();
                let caption = caption.clone();
                let number = number.clone();
                let first = first.clone();
                untracked(move || {
                    if !is_open {
                        result.set(None);
                        return view().into_element();
                    }
                    let theirs = number.clone();
                    let session = crate::camera::use_session(
                        Purpose::ScanCode,
                        move || result.get().is_none(),
                        move |text| {
                            if let Some(codes::Scanned::Safety(digits)) = codes::parse(&text) {
                                result.set(Some(codes::safety_matches(&digits, &theirs)));
                            }
                        },
                    );
                    // mock: the demo camera of a preview build shows their code (app-core
                    // compares fingerprints).
                    crate::camera::demo_shows(codes::safety_code(&number));
                    dynamic(move || {
                        let r = result.get();
                        let session = session.clone();
                        let caption = caption.clone();
                        let first = first.clone();
                        untracked(move || match r {
                            Some(true) => view()
                                .items_center()
                                .gap_3()
                                .py_4()
                                .child(hero_icon(Icon::ShieldCheck))
                                .child(
                                    text("The numbers match")
                                        .text_size(px(font::TITLE))
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(muted("Your chat is end\u{2011}to\u{2011}end encrypted. Mark it as verified?").text_center())
                                .child(
                                    view()
                                        .flex_row()
                                        .gap_2()
                                        .child(btn("Not now", Variant::Ghost, BtnSize::Md, move || open.set(false)))
                                        .child(btn("Mark as verified", Variant::Primary, BtnSize::Md, move || {
                                            s.verify(contact, true);
                                            open.set(false);
                                            toast("Verified");
                                        })),
                                )
                                .into_element(),
                            Some(false) => view()
                                .items_center()
                                .gap_3()
                                .py_4()
                                .child(hero_icon(Icon::Alert))
                                .child(
                                    text("The numbers don't match")
                                        .text_size(px(font::TITLE))
                                        .font_weight(FontWeight::SEMIBOLD),
                                )
                                .child(
                                    muted(format!(
                                        "This is not {first}'s current code. Make sure you scan the code on their screen now, in person."
                                    ))
                                    .text_center(),
                                )
                                .child(btn("Scan again", Variant::Secondary, BtnSize::Md, move || result.set(None)))
                                .into_element(),
                            None => crate::camera::camera_view(&session, 240.0, caption).into_element(),
                        })
                    })
                    .into_element()
                })
            })),
    )
}

// ---------------------------------------------------------------- linked devices

/// Linked devices, pushed.
pub fn linked_devices() {
    nav::push("linked-devices", || page("Linked devices", devices_body()));
}

fn device_icon(kind: DeviceKind) -> Icon {
    match kind {
        DeviceKind::Phone => Icon::Smartphone,
        DeviceKind::Tablet => Icon::Tablet,
        DeviceKind::Desktop => Icon::Laptop,
    }
}

/// Which listed device is the one the app runs on. The store's flag wins
/// when its kind fits the platform (a computer on desktop, a phone or tablet
/// elsewhere); otherwise the first device of the platform's kind is this one,
/// so a desktop never marks the phone as "This device".
pub fn this_device_id(devices: &[Device]) -> Option<u64> {
    let fits = |d: &Device| (d.kind == DeviceKind::Desktop) == is_desktop();
    devices
        .iter()
        .find(|d| d.this_device && fits(d))
        .or_else(|| devices.iter().find(|d| fits(d)))
        .or_else(|| devices.iter().find(|d| d.this_device))
        .map(|d| d.id)
}

/// The devices with `this_device` following the platform (see
/// [`this_device_id`]).
fn devices_here(devices: Vec<Device>) -> Vec<Device> {
    let this = this_device_id(&devices);
    devices
        .into_iter()
        .map(|mut d| {
            d.this_device = Some(d.id) == this;
            // The seed says "This device" on the phone; elsewhere that is
            // the computer, so the phone is simply active.
            if d.this_device || d.last_active == "This device" {
                d.last_active = "Active now".into();
            }
            d
        })
        .collect()
}

fn devices_body() -> AnyElement {
    let s = store();
    // The account's real devices, from the server's list.
    crate::backend::refresh_device_list(s);
    let link_open = signal(false);
    let selected = signal(None::<Device>);
    let detail_open = signal(false);
    let linked = move || s.devices.with(|d| d.len().saturating_sub(1));
    let full = move || linked() >= MAX_LINKED;

    let header = view()
        .items_center()
        .gap_3()
        .px_4()
        .pt_6()
        .pb_2()
        .child(hero_icon(Icon::Laptop))
        .child(
            muted("Use Songbird on your computers and tablets, even when your phone is off.")
                .text_center()
                .max_w(px(360.0)),
        )
        .child(
            btn_with_icon(
                "Link a device",
                Some(Icon::QrCode),
                Variant::Primary,
                BtnSize::Md,
                move || link_open.set(true),
            )
            .disabled(full),
        )
        .child(when(full, || {
            muted("You've linked 4 devices. Log one out to link another.").text_center()
        }));

    let list = dynamic(move || {
        let devices = devices_here(s.devices.get());
        untracked(move || {
            group(devices.into_iter().map(move |d| {
                let leading = icon_tile(device_icon(d.kind));
                let subtitle = if d.this_device {
                    "Active now".to_string()
                } else {
                    d.last_active.clone()
                };
                let row = Row::new(d.name.clone()).leading(leading).subtitle(subtitle);
                let row = if d.this_device {
                    row.trailing(tag("This device"))
                } else {
                    row.chevron()
                };
                row.on_press(move || {
                    selected.set(Some(d.clone()));
                    detail_open.set(true);
                })
                .build()
            }))
            .into_element()
        })
    });

    view()
        .flex_1()
        .child(page_scroll(
            view()
                .w_full()
                .max_w(px(760.0))
                .self_center()
                .child(header)
                .child(section_title("Devices"))
                .child(list)
                .child(crate::features::settings::foot(
                    "You can link up to 4 devices. Messages stay end\u{2011}to\u{2011}end encrypted on every one.",
                )),
        ))
        .child(link_sheet(s, link_open))
        .child(device_sheet(s, detail_open, selected))
        .into_element()
}

/// Linking (PROTOCOL §4.6): the steps and the camera; a scanned link code asks for
/// confirmation before anything is sent. Link codes are read only from the live camera
/// (D-0110): never from a photo, a pasted text or a link.
fn link_sheet(s: Store, open: Signal<bool>) -> nsh_base::SheetElement {
    let found = signal(None::<String>);
    // The main phone is sealing and waiting for the new device.
    let linking = signal(false);
    app_sheet(
        open,
        "Link a device",
        view().px_4().pb_4().gap_4().child(dynamic(move || {
            let is_open = open.get();
            untracked(move || {
                if !is_open {
                    found.set(None);
                    return view().into_element();
                }
                let session = crate::camera::use_session(
                    Purpose::ScanCode,
                    move || found.get().is_none(),
                    move |text| {
                        if matches!(codes::parse(&text), Some(codes::Scanned::Link { .. })) {
                            found.set(Some(text));
                        }
                    },
                );
                // mock: the demo camera of a preview build shows a new device's code.
                crate::camera::demo_shows(codes::link_code(&[0x5B; 16], &[0xC3; 32]));
                let pin_note = move || {
                    when(
                        move || s.settings.with(|x| x.two_step_pin),
                        || muted("You may be asked for your two-step PIN.").text_center(),
                    )
                };
                dynamic(move || {
                    let code = found.get();
                    let session = session.clone();
                    untracked(move || match code {
                        Some(code) => view()
                            .gap_4()
                            .items_center()
                            .child(hero_icon(Icon::Laptop))
                            .child(
                                text("Link a new device?")
                                    .text_size(px(font::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                muted(
                                    "It will be able to read and send all your messages. Only continue if you are setting up your own device right now, and nobody asked you to scan this code.",
                                )
                                .text_center()
                                .max_w(px(360.0)),
                            )
                            .child(pin_note())
                            .child(
                                view()
                                    .flex_row()
                                    .gap_2()
                                    .child(btn("Cancel", Variant::Ghost, BtnSize::Md, move || open.set(false)))
                                    .child(
                                        btn("Link device", Variant::Primary, BtnSize::Md, move || {
                                            if !(crate::backend::is_live() && crate::backend::registered()) {
                                                toast("Sign up on this device first, then link others to it.");
                                                return;
                                            }
                                            linking.set(true);
                                            let code = code.clone();
                                            spawn(async move {
                                                // Seals the account and the storage key to the
                                                // new device, then names it in a signed list once
                                                // its keys prove the confirmation key.
                                                let result = crate::backend::link::link_from_code(&code).await;
                                                let _ = linking.try_set(false);
                                                match result {
                                                    Ok(()) => {
                                                        let _ = open.try_set(false);
                                                        toast("Your new device is linked");
                                                    }
                                                    Err(p) => toast(p.to_string()),
                                                }
                                            });
                                        })
                                        .disabled(move || linking.get()),
                                    ),
                            )
                            .into_element(),
                        None => view()
                            .gap_4()
                            .child(
                                view()
                                    .gap_3()
                                    .child(numbered_step(1, "On the new device, open Songbird."))
                                    .child(numbered_step(2, "Choose \"I already have an account\", then \"Link to another device\"."))
                                    .child(numbered_step(3, "Point this device's camera at the code it shows.")),
                            )
                            .child(crate::camera::camera_view(&session, 220.0, "Looking for a code…"))
                            .into_element(),
                    })
                })
                .into_element()
            })
        })),
    )
}

/// A device's sheet: rename it, or log it out.
fn device_sheet(
    s: Store,
    open: Signal<bool>,
    selected: Signal<Option<Device>>,
) -> nsh_base::SheetElement {
    let name = signal(String::new());
    app_sheet(
        open,
        "Device",
        view().px_4().pb_4().gap_4().child(dynamic(move || {
            let device = selected.get();
            let is_open = open.get();
            untracked(move || {
                let Some(d) = device.filter(|_| is_open) else { return view().into_element() };
                name.set(d.name.clone());
                let id = d.id;
                let this = d.this_device;
                let original = d.name.clone();
                let save = move || {
                    let n = name.get_untracked().trim().to_string();
                    if !n.is_empty() {
                        s.rename_device(id, n);
                        open.set(false);
                        toast("Device renamed");
                    }
                };
                view()
                    .gap_4()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(icon_tile(device_icon(d.kind)))
                            .child(
                                view()
                                    .flex_1()
                                    .child(text(d.name.clone()).text_size(px(font::body())).font_weight(FontWeight::SEMIBOLD))
                                    .child(muted(if this { "This device".to_string() } else { d.last_active.clone() })),
                            ),
                    )
                    .child(labelled_field(
                        "Name",
                        field("Device name", name).on_submit(move |_: &str| save()).max_length(40),
                        "Only you see this name.",
                        || false,
                    ))
                    .child(
                        btn("Save name", Variant::Primary, BtnSize::Md, save)
                            .disabled(move || name.with(|n| n.trim().is_empty())),
                    )
                    .child(if this {
                        muted("This is the device you're using. To stop using Songbird here, delete the app or your account.")
                            .into_element()
                    } else {
                        btn_with_icon("Log out device", Some(Icon::LogOut), Variant::Danger, BtnSize::Md, move || {
                            let device_name = original.clone();
                            confirm(
                                format!("Log out {device_name}?"),
                                "It stops getting messages, and its chats are deleted from it.",
                                "Log out",
                                move || {
                                    s.remove_device(id);
                                    open.set(false);
                                    toast(format!("{device_name} was logged out"));
                                },
                            )
                        })
                        .into_element()
                    })
                    .into_element()
            })
        })),
    )
}

#[cfg(test)]
mod camera_flow {
    //! Linking by camera (D-0110, D-0111): only a link code read by the live camera leads to
    //! the confirmation, the camera stops while it shows, and only "Link device" links.

    use std::sync::{Arc, LazyLock};

    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;
    use songbird_camera::fake::FakeCamera;

    use super::*;

    static CAM: LazyLock<FakeCamera> = LazyLock::new(FakeCamera::new);

    #[component]
    fn LinkProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        crate::camera::install(Some(Arc::new(CAM.clone())), false);
        let open = signal(true);
        view()
            .child(text(move || {
                format!("devices: {}", s.devices.with(Vec::len))
            }))
            .child(link_sheet(s, open))
    }

    #[test]
    fn a_link_code_asks_first_and_anything_else_is_ignored() -> Result<(), String> {
        let mut app = Headless::start(EngineConfig::new(Root::of::<LinkProbe>()))
            .map_err(|e| e.to_string())?;
        app.wait_for("Looking for a code…")?;
        let before = app
            .screen()
            .texts()
            .iter()
            .find(|t| t.starts_with("devices: "))
            .map(ToString::to_string);
        // A group invite link and garbage: nothing happens.
        CAM.show_code("https://songbird.chat/g/0123456789abcdef#k=abc");
        CAM.show_code("hello");
        app.settle();
        assert!(!app.shows("Link a new device?"));
        assert_eq!(CAM.running(), 1);
        // A link code: the confirmation, with the camera stopped.
        CAM.show_code(&codes::link_code(&[0x11; 16], &[0x22; 32]));
        app.wait_for("Link a new device?")?;
        app.wait_for("text:/read and send all your messages/")?;
        assert_eq!(CAM.running(), 0, "{:?}", CAM.log());
        let after_scan = app
            .screen()
            .texts()
            .iter()
            .find(|t| t.starts_with("devices: "))
            .map(ToString::to_string);
        assert_eq!(before, after_scan, "nothing is linked before the press");
        // This test device has no account: nothing is invented, and it says why.
        app.press("Link device")?;
        app.settle();
        let after = app
            .screen()
            .texts()
            .iter()
            .find(|t| t.starts_with("devices: "))
            .map(ToString::to_string);
        assert_eq!(before, after, "no device without an account");
        Ok(())
    }
}
