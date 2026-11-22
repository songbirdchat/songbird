//! The app's icon, chosen by the person (D-0179).
//!
//! - **iOS**: the system's alternate icons (`setAlternateIconName`), compiled into the app's
//!   asset catalog by `apps/ios/alternate-icons.sh`; iOS tells the person it changed.
//! - **Android**: one launcher alias of the activity per icon (`chat.songbird.app.Icon<Name>`),
//!   the chosen one enabled and the rest disabled.
//! - **macOS**: the Dock's icon while Songbird runs (`NSApplication.applicationIconImage`); the
//!   app sets it again at every start. Finder keeps the bundle's icon: changing it would break
//!   the app's signature.
//! - **Windows**: the icon of Songbird's windows, so its taskbar button (a pinned shortcut keeps
//!   its own); set again at every start.
//! - **Linux**: none. Desktops take the icon from the launcher entry, which an app shouldn't
//!   rewrite, so the setting isn't offered there.

#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

/// Whether this platform can change the app's icon.
#[must_use]
pub fn supported() -> bool {
    #[cfg(target_os = "ios")]
    let yes = crate::apple::icon::supported();
    #[cfg(not(target_os = "ios"))]
    let yes = cfg!(any(
        target_os = "android",
        target_os = "macos",
        target_os = "windows"
    ));
    yes
}

/// The answer: whether the icon changed.
pub type Done = Box<dyn FnOnce(bool) + Send>;

/// Makes `name` the app's icon (`None` is the main one). `name` is the icon's name in the
/// iOS catalog and the Android alias (`Night`, `Paper`, `Lavender`); `png` is its picture, for
/// the desktop (512 × 512).
#[allow(unused_variables, reason = "each platform uses what it needs")]
pub fn set(name: Option<&str>, png: &'static [u8], done: Done) {
    #[cfg(target_os = "ios")]
    return crate::apple::icon::set(name, done);
    #[cfg(target_os = "macos")]
    return crate::apple::icon::set(png, done);
    #[cfg(target_os = "android")]
    return done(crate::android::set_icon(name.unwrap_or("Songbird")));
    #[cfg(target_os = "windows")]
    return done(crate::windows::set_icon(png));
    #[allow(unreachable_code, reason = "Linux")]
    done(false);
}
