//! You → Server connection: turn the real Songbird backend on, point the
//! app at it, register this device, and watch the delivery connection.
//! What runs live and what stays on this device is said plainly.

use nsh_core::prelude::*;

use crate::{
    backend::{self, Endpoint, Link, live},
    nav,
    ui::*,
};

/// Pushes the Server connection page.
pub fn server_connection() {
    nav::push("server", || page("Server connection", body()));
}

fn link_label(link: Link) -> (Tone, Icon, String) {
    match link {
        Link::Off => (Tone::Info, Icon::WifiOff, "Not connected".into()),
        Link::Connecting => (Tone::Info, Icon::Refresh, "Connecting…".into()),
        Link::Connected => (
            Tone::Secure,
            Icon::ShieldCheck,
            "Connected. Signed in with this device's key.".into(),
        ),
        Link::Retrying => (
            Tone::Warning,
            Icon::Refresh,
            "Connection lost. Trying again…".into(),
        ),
        Link::Refused(code) => (
            Tone::Danger,
            Icon::Alert,
            format!("The server refused this device (code {code})."),
        ),
    }
}

fn body() -> impl IntoElement {
    let state = live();
    let on = move || state.with(|l| l.enabled);
    let registered = move || state.with(|l| l.device.is_some());
    let ep = state.with_untracked(|l| l.endpoint.clone());
    let api = signal(ep.api.clone());
    let api_name = signal(ep.api_name.clone());
    let ws_addr = signal(ep.ws_addr.clone());
    let ws_name = signal(ep.ws_name.clone());
    let billing = signal(ep.billing.clone());
    let invite = signal(String::new());
    let username = signal(String::new());
    let devices = signal(Vec::<backend::RemoteDevice>::new());
    let advanced = signal(false);
    let save = move || {
        backend::set_endpoint(Endpoint {
            api: api.get_untracked().trim().trim_end_matches('/').to_string(),
            api_name: api_name.get_untracked().trim().to_string(),
            ws_addr: ws_addr.get_untracked().trim().to_string(),
            ws_name: ws_name.get_untracked().trim().to_string(),
            billing: billing
                .get_untracked()
                .trim()
                .trim_end_matches('/')
                .to_string(),
        });
        toast("Server address saved");
    };
    let register = move || {
        save();
        let code = invite.get_untracked();
        let name = username.get_untracked();
        backend::run(async move {
            backend::register(Some(code).filter(|c| !c.trim().is_empty()), Some(name)).await?;
            toast("This device is registered");
            Ok(())
        });
    };
    let refresh_devices = move || {
        backend::run(async move {
            devices.set(backend::devices().await?);
            Ok(())
        });
    };

    page_scroll(
        view()
            .gap_1()
            .max_w(px(760.0))
            .w(relative(1.0))
            .self_center()
            .child(view().pt_4().child(callout(
                Icon::Server,
                "Songbird's server",
                "Your messages, calls, groups and profile go through Songbird's server, end-to-end \
                 encrypted: it carries them but can't read them. It knows your account, your \
                 devices and a hash of your username.",
                crate::features::settings::privacy_centre,
            )))
            .child(section_title("Connection"))
            // Only the demo fixtures can run without the server: for a real account it is always
            // on, so no screen of it falls back to the sample data (D-0184).
            .child(crate::store::demo().then(|| {
                group_plain([toggle_row(
                    "Connect to Songbird's server",
                    Some("When it's off, Songbird runs on this device with sample chats."),
                    on,
                    backend::set_enabled,
                )])
            }))
            .child(view().px_4().pt_3().child(dynamic(move || {
                let (tone, glyph, words) = link_label(state.with(|l| l.link));
                banner(tone, glyph, words).into_element()
            })))
            // Only takes space while there is something to say.
            .child(dynamic(move || {
                match state.with(|l| l.problem.clone()) {
                    Some(p) => view().px_4().pt_2().child(banner(Tone::Danger, Icon::Alert, p)).into_element(),
                    None => view().into_element(),
                }
            }))
            // Only takes space while there is something to say.
            .child(dynamic(move || {
                match state.with(|l| l.busy.clone()) {
                    Some(b) => view().px_4().pt_2().child(banner(Tone::Info, Icon::Clock, b)).into_element(),
                    None => view().into_element(),
                }
            }))
            .child(view().pt_4().child(group_plain([Row::new("Advanced")
                .subtitle("Server addresses, for your own server")
                .trailing(
                    view()
                        .size(px(18.0))
                        .text_color(move || c::muted_fg())
                        .child(
                            when(move || advanced.get(), || icon(Icon::ChevronDown, 18.0))
                                .otherwise(|| icon(Icon::ChevronRight, 18.0)),
                        ),
                )
                .on_press(move || advanced.update(|a| *a = !*a))
                .build()])))
            .child(when(
                move || advanced.get(),
                move || {
                    view()
                        .px_4()
                        .pt_3()
                        .gap_3()
                        .child(labelled_field(
                            "Server address",
                            field("https://api.songbird.chat", api),
                            "Change it only to use a server you or your organisation run.",
                            || false,
                        ))
                        .child(labelled_field(
                            "Server name",
                            field("api.songbird.chat", api_name),
                            "The name on the server's certificate.",
                            || false,
                        ))
                        .child(labelled_field(
                            "Message delivery address",
                            field("delivery.songbird.chat:443", ws_addr),
                            "Where new messages arrive from.",
                            || false,
                        ))
                        .child(labelled_field(
                            "Message delivery name",
                            field("delivery.songbird.chat", ws_name),
                            "The name on the delivery service's certificate.",
                            || false,
                        ))
                        .child(labelled_field(
                            "Payments address",
                            field("https://billing.songbird.chat", billing),
                            "Where desktop payments are checked. It never learns who you are.",
                            || false,
                        ))
                        .child(view().flex_row().child(btn("Save addresses", Variant::Secondary, BtnSize::Md, save)))
                        .child(run_your_own())
                },
            ))
            .child(section_title("This device"))
            .child(dynamic(move || {
                if registered() {
                    let (account, device, name, waiting) = state.with(|l| {
                        (l.account.clone().unwrap_or_default(), l.device.clone().unwrap_or_default(), l.username.clone(), l.waiting)
                    });
                    view()
                        .gap_3()
                        .child(group_plain([
                            Row::new("Account").value(short(&account)).build(),
                            Row::new("This device").value(short(&device)).build(),
                            Row::new("Username").value(name.unwrap_or_else(|| "Not chosen yet".into())).build(),
                            Row::new("Messages waiting")
                                .subtitle("Encrypted messages wait on the server until this device can open them.")
                                .value(waiting.to_string())
                                .build(),
                        ]))
                        .child(
                            view()
                                .px_4()
                                .flex_row()
                                .gap_2()
                                .child(btn("Reconnect", Variant::Secondary, BtnSize::Md, backend::connect))
                                .child(btn("Show devices", Variant::Secondary, BtnSize::Md, refresh_devices))
                                .child(btn("Forget on this device", Variant::Ghost, BtnSize::Md, || {
                                    confirm(
                                        "Forget this account here?",
                                        "The account stays on the server. This device loses its keys and can't sign in again.",
                                        "Forget",
                                        backend::forget,
                                    )
                                })),
                        )
                        .child(dynamic(move || {
                            let list = devices.get();
                            if list.is_empty() {
                                return view().into_element();
                            }
                            group_plain(list.into_iter().map(|d| {
                                let mut words = Vec::new();
                                if d.this_device { words.push("This device"); }
                                if d.primary { words.push("Main"); }
                                if d.pending { words.push("Waiting for approval"); }
                                let row = Row::new(short(&d.id)).subtitle(words.join(" · "));
                                if d.this_device {
                                    return row.build();
                                }
                                // The main phone logs another device out: a signed list
                                // without it (and a new storage key once it is alone).
                                let id = d.id.clone();
                                row.chevron()
                                    .on_press(move || {
                                        let id = id.clone();
                                        confirm(
                                            format!("Log out device {}?", short(&id)),
                                            "It stops getting messages, and can't read your settings from now on.",
                                            "Log out",
                                            move || {
                                                let id = id.clone();
                                                backend::run(async move {
                                                    backend::link::remove(&id).await?;
                                                    devices.set(backend::devices().await?);
                                                    toast("The device was logged out");
                                                    Ok(())
                                                });
                                            },
                                        )
                                    })
                                    .build()
                            }))
                            .into_element()
                        }))
                        .into_element()
                } else {
                    view()
                        .px_4()
                        .gap_3()
                        .child(labelled_field(
                            "Invite code",
                            field("XXXX-XXXX-XXXX-XXXX-XXXX-XXXX", invite),
                            move || {
                                let code = invite.get();
                                if code.trim().is_empty() || backend::wire::invite_well_formed(&code) {
                                    SharedString::from("24 letters and numbers. Leave empty if the server isn't invite-only.")
                                } else {
                                    SharedString::from("Codes are 24 letters and numbers, in groups of four.")
                                }
                            },
                            move || invite.with(|c| !c.trim().is_empty() && !backend::wire::invite_well_formed(c)),
                        ))
                        .child(labelled_field(
                            "Username",
                            field("radu", username),
                            "3 to 32 letters, numbers, _ or ., starting with a letter. Only a hash of it reaches the server.",
                            || false,
                        ))
                        .child(view().flex_row().child(
                            btn("Register this device", Variant::Primary, BtnSize::Md, register)
                                // Screen readers hear that registering runs (NSH 0.7 `busy`, D-0188).
                                .busy(move || state.with(|l| l.busy.is_some()))
                                .disabled(move || !on() || state.with(|l| l.busy.is_some())),
                        ))
                        .child(crate::features::settings::foot(
                            "Signing up solves a short puzzle on this device, so the server can stop bulk \
                             sign-ups without knowing who you are.",
                        ))
                        .into_element()
                }
            }))
    )
}

/// Running a local server: developer notes, debug builds only.
fn run_your_own() -> Option<View> {
    #[cfg(debug_assertions)]
    {
        Some(
            view().gap_1().pt_2().child(tag("Developer preview")).child(
                crate::features::settings::foot(
                    "From the repository: cargo xtask compose up, then scripts/dev-app-backend.sh. \
                     Mint an invite with scripts/dev-app-backend.sh invite.",
                )
                .px_0(),
            ),
        )
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// The first 8 hex of an id, for display.
fn short(id: &str) -> String {
    if id.len() > 8 {
        format!("{}…", &id[..8])
    } else {
        id.to_string()
    }
}
