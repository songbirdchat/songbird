//! The device's push token (D-0175): what the push service needs to wake this device. The
//! token goes to Songbird's api sealed to the push service's key (PROTOCOL §9); what a push
//! carries is nothing but "fetch your mail".
//!
//! - **iOS**: APNs, through NSH 0.7's `nsh-extra` push module, which the app calls (D-0188); it
//!   hears the token from NSH's app delegate. [`register`] answers `false` there.
//! - **Android**: UnifiedPush through the user's own distributor (no Google services): its Web
//!   Push endpoint and the keys the connector made.

#![forbid(unsafe_code)]

/// A push token.
#[derive(Clone, PartialEq, Eq)]
pub enum Token {
    /// APNs: the device token for alert pushes.
    Apns(Vec<u8>),
    /// UnifiedPush (Android): the distributor's Web Push endpoint and this device's keys.
    WebPush {
        /// The endpoint URL.
        endpoint: String,
        /// The P-256 public key, uncompressed.
        p256dh: [u8; 65],
        /// The authentication secret.
        auth: [u8; 16],
    },
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Apns(_) => f.write_str("Apns(..)"),
            Self::WebPush { .. } => f.write_str("WebPush(..)"),
        }
    }
}

/// Where tokens go: called on the UI thread, again whenever the system issues a new one.
pub type OnToken = Box<dyn Fn(Token) + Send + Sync>;

/// Asks the system for this device's push token. Call it on the UI thread, once notifications
/// are allowed. `false` where this platform's push isn't reached this way.
#[must_use]
pub fn register(on_token: OnToken) -> bool {
    #[cfg(target_os = "android")]
    return crate::android::register_push(on_token);
    #[allow(unreachable_code, reason = "other platforms")]
    {
        drop(on_token);
        false
    }
}
