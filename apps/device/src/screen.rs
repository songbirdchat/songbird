//! Keeping the app out of screenshots and screen recording (D-0173).
//!
//! - **Android**: `FLAG_SECURE` on the activity's window (no screenshots, recordings show black,
//!   the recents thumbnail is blank); the thumbnail alone can also be hidden (Android 13+).
//! - **macOS**: the app's windows are left out of screen capture (`NSWindowSharingNone`).
//! - **Windows**: the app's windows are excluded from capture (`WDA_EXCLUDEFROMCAPTURE`).
//! - **iOS** lets no app block screenshots; it tells when the screen is recorded or mirrored,
//!   and the app covers itself then ([`captured`]).
//! - **Linux** has no such thing ([`can_block`] is false).

#![forbid(unsafe_code)]

/// Whether this platform can keep the app out of screenshots.
#[must_use]
pub fn can_block() -> bool {
    cfg!(any(
        target_os = "android",
        target_os = "macos",
        target_os = "windows"
    ))
}

/// Keeps the app's windows out of screenshots and recordings (or lets them in again). Call it
/// on the UI thread, and again when the app opens a window.
#[allow(unused_variables, reason = "a no-op where the platform can't")]
pub fn protect(on: bool) {
    #[cfg(target_os = "macos")]
    crate::apple::protect_windows(on);
    #[cfg(target_os = "windows")]
    crate::windows::protect_windows(on);
    #[cfg(target_os = "android")]
    crate::android::set_secure(on);
}

/// Android 13 and later: a blank thumbnail in the recent apps, screenshots still allowed.
#[allow(unused_variables, reason = "Android only")]
pub fn hide_in_recents(on: bool) {
    #[cfg(target_os = "android")]
    crate::android::hide_in_recents(on);
}

/// Whether the screen is being recorded or mirrored right now (iOS; elsewhere false). Call it
/// on the UI thread.
#[must_use]
pub fn captured() -> bool {
    #[cfg(target_os = "ios")]
    return crate::apple::captured();
    #[allow(unreachable_code, reason = "other platforms")]
    false
}
