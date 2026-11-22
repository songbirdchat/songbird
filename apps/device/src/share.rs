//! Sharing the phone's screen in a call (D-0180). The system asks the person and captures; the
//! frames come here and the app sends them as the call's screen-share track, encrypted like the
//! camera. Nothing is kept or sent anywhere else.
//!
//! - **Android**: MediaProjection, after the system's "Start recording or casting?" prompt,
//!   inside a foreground service whose notification says the screen is shared.
//! - **iOS**: a ReplayKit broadcast extension (`apps/ios/ScreenShare`), started from the
//!   system's broadcast sheet. Its frames come over a socket in the app group's container,
//!   which only Songbird's own processes can reach.
//!
//! Desktops share with the call media's own capturer.

#![forbid(unsafe_code)]

/// A frame's pixels.
#[derive(Debug, Clone, Copy)]
pub enum Pixels<'a> {
    /// 32-bit RGBA (R, G, B, A in memory), `stride` bytes per row (Android).
    Rgba {
        /// The rows.
        data: &'a [u8],
        /// Bytes per row.
        stride: u32,
    },
    /// NV12: a luma plane and an interleaved `CbCr` plane of half the rows (iOS).
    Nv12 {
        /// The luma rows.
        luma: &'a [u8],
        /// Bytes per luma row.
        luma_stride: u32,
        /// The interleaved `CbCr` rows.
        chroma: &'a [u8],
        /// Bytes per `CbCr` row.
        chroma_stride: u32,
    },
}

/// One captured frame.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// Clockwise degrees to turn it upright (the phone held sideways), 0, 90, 180 or 270.
    pub rotation: u16,
    /// The pixels.
    pub pixels: Pixels<'a>,
}

/// Each frame, on a capture thread.
pub type OnFrame = Box<dyn Fn(Frame<'_>) + Send + Sync>;
/// Whether capture began (`false`: refused, cancelled, or not possible here).
pub type OnStarted = Box<dyn FnOnce(bool) + Send>;
/// Capture ended outside the app (the person stopped it from the system).
pub type OnEnded = Box<dyn FnOnce() + Send>;

/// Whether this platform shares its screen through here.
#[must_use]
pub fn supported() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
}

/// Asks the person and starts capturing.
pub fn start(on_frame: OnFrame, started: OnStarted, ended: OnEnded) {
    #[cfg(target_os = "android")]
    crate::android::share_start(on_frame, started, ended);
    #[cfg(target_os = "ios")]
    crate::apple::share::start(on_frame, started, ended);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        drop((on_frame, ended));
        started(false);
    }
}

/// Stops capturing (the system's indicator goes away).
pub fn stop() {
    #[cfg(target_os = "android")]
    crate::android::share_stop();
    #[cfg(target_os = "ios")]
    crate::apple::share::stop();
}
