//! Runs the Rust application directly on the machine's native backend.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    // The developer's console log, and the app's own for support requests (D-0184).
    nsh_app_songbird::backend::applog::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<nsh_app_songbird::App>()),
        platform::AppOptions {
            title: "Songbird".to_owned(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            app_id: "chat.songbird.app".to_owned(),
            size: (1180, 780),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!(
    "Run this binary on macOS, Windows or Linux; use the TSX entry with nsh run for iOS and Android."
);
