//! Songbird, the app: an NSH Rust UI over app-core.
//!
//! - `ui`: the design system — tokens, icons and every styled control.
//! - `store`: the UI's view of app-core and its intents (mocked until A2).
//! - `nav` and `shell`: navigation and the phone and desktop layouts.
//! - `features`: the screens, one module per area of the prompt set.
//! - `camera`: the camera's sessions and states (D-0110); its platform code, and all `unsafe`,
//!   live in `songbird-camera`, so the app forbids `unsafe`.

#![forbid(unsafe_code)]

pub mod backend;
pub mod camera;
pub mod features;
pub mod nav;
pub mod shell;
pub mod sounds;
pub mod store;
pub mod theming;
pub mod ui;

use nsh_core::prelude::*;

use crate::store::Store;

/// A developer or preview build: debug, or built with `SONGBIRD_PREVIEW_BUILD=1` exactly (for
/// screenshots and notification checks on emulators and devices). Preview builds skip the first
/// run on request, show the Calls tab's test rows and honour `SONGBIRD_DEMO_ARRIVALS`; a release
/// build does none of it, whatever else the variable holds (D-0100).
pub const PREVIEW_BUILD: bool =
    cfg!(debug_assertions) || is_one(option_env!("SONGBIRD_PREVIEW_BUILD"));

const fn is_one(v: Option<&str>) -> bool {
    match v {
        Some(s) => s.len() == 1 && s.as_bytes()[0] == b'1',
        None => false,
    }
}

#[cfg(test)]
mod preview_tests {
    #[test]
    fn only_exactly_one_marks_a_preview_build() {
        assert!(super::is_one(Some("1")));
        for v in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some("11"),
            Some(" 1"),
        ] {
            assert!(!super::is_one(v), "{v:?}");
        }
    }
}

/// The app's UI set (`nsh.json` `"rust"`).
#[native_module("songbird/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The camera's live preview, `songbird/camera` (D-0110): AVFoundation on macOS.
#[native_components(macos)]
fn camera_components_macos(components: &nsh_platform_macos::Components) {
    songbird_camera::apple::register(components);
}

/// The camera's live preview on iOS, in the packaged app.
#[native_components(ios)]
fn camera_components_ios(components: &nsh_platform_ios::Components) {
    songbird_camera::apple::register(components);
}

/// The camera's live preview on Windows (Media Foundation frames drawn with Direct2D).
#[native_components(windows)]
fn camera_components_windows(components: &nsh_platform_windows::Components) {
    songbird_camera::windows::register(components);
}

/// The camera's live preview on Linux (V4L2 frames in a GTK picture).
#[native_components(linux)]
fn camera_components_linux(components: &nsh_platform_linux::Components) {
    songbird_camera::linux::register(components);
}

/// Songbird's root: the store, the theme the user chose, the first run
/// or the app, and the layers over everything (calls, toasts).
#[component]
pub fn App() -> impl IntoElement {
    let s = Store::new();
    store::install(s);
    // Settings saved on this device, and the encrypted records synced from
    // the account's other devices (Prompt 9); before the connection, which
    // reads the proxy setting.
    backend::records::restore(s);
    // A status whose real expiry passed while the app was closed goes now.
    s.schedule_status_expiry();
    camera::install_for_app();
    backend::restore();
    backend::avatar::restore(
        crate::store::try_store().map_or(42, |s| s.me.with_untracked(|m| m.seed)),
    );
    backend::records::periodic(s);
    // Prompt 9, M6: the encrypted profile follows the user's edits, and
    // presence follows the app's lifecycle and the privacy settings.
    backend::profile::install(s);
    backend::presence::install(s);
    s.refresh_server_settings();
    // Prompt 11: the plan and standing from the server; a kept token or a seat redeemed.
    s.refresh_plan();
    features::lock::install(s);
    theming::follow_settings(s);
    features::desktop::install_shortcuts();
    features::notify::install(features::desktop::current_chat);
    features::notify::demo_from_env();
    let onboarded = move || s.session.with(|x| x.onboarded);
    view()
        .flex_1()
        .bg(move || ui::c::background())
        .text_color(move || ui::c::foreground())
        .child(
            view()
                .flex_1()
                // Screen readers skip the app while it's locked (Prompt 9).
                .accessibility_elements_hidden(move || features::lock::locked().get().0)
                .child(
                    when(onboarded, || {
                        when(ui::wide, shell::desktop).otherwise(shell::phone)
                    })
                    .otherwise(features::onboarding::flow),
                ),
        )
        .child(features::desktop::jump::layer())
        .child(features::lock::layer())
        .child(features::calls::call_layer())
        .child(ui::toast_layer())
}
