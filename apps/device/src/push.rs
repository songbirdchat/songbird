//! The device's push token (D-0175): what the push service needs to wake this device. The
//! token goes to Songbird's api sealed to the push service's key (PROTOCOL §9); what a push
//! carries is nothing but "fetch your mail".
//!
//! - **iOS**: APNs. NSH's app delegate doesn't take the token callbacks, so they are added to its
//!   class at run time (`class_addMethod`: added, never replacing NSH's own), then UIKit is asked
//!   to register.

#![forbid(unsafe_code)]

/// A push token.
#[derive(Clone, PartialEq, Eq)]
pub enum Token {
    /// APNs: the device token for alert pushes.
    Apns(Vec<u8>),
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Apns(_) => f.write_str("Apns(..)"),
        }
    }
}

/// Where tokens go: called on the UI thread, again whenever the system issues a new one.
pub type OnToken = Box<dyn Fn(Token) + Send + Sync>;

/// Asks the system for this device's push token. Call it on the UI thread, once notifications
/// are allowed. `false` where this platform's push isn't reached this way.
#[must_use]
pub fn register(on_token: OnToken) -> bool {
    #[cfg(target_os = "ios")]
    return crate::apple::register_push(on_token);
    #[allow(unreachable_code, reason = "other platforms")]
    {
        drop(on_token);
        false
    }
}
