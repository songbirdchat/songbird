//! Incoming calls on iOS (D-0176): PushKit and CallKit.
//!
//! A ring arrives as a VoIP push carrying nothing (PROTOCOL §9: the push service sends one
//! constant wake-up), and iOS requires every VoIP push to show a call at once. So each push
//! reports a CallKit call from "Songbird" right away and tells the app ([`Event::Ring`]); the
//! app connects, receives the encrypted offer, and names the caller ([`update`]), or ends the
//! call ([`ended`]) when no offer comes. Answer, end and mute on the system's call screen come
//! back as events.
//!
//! WebRTC's audio waits for CallKit while a CallKit call runs (its manual audio), as Apple
//! requires: started when CallKit activates the audio session, stopped when it deactivates.
//! Calls the app rings itself (in front) are unchanged.

#![forbid(unsafe_code)]

/// What the system tells the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The PushKit (VoIP) token, again whenever iOS issues a new one.
    Token(Vec<u8>),
    /// A ring woke the app and a call from "Songbird" shows: fetch the offer.
    Ring([u8; 16]),
    /// The person answered on the system's call screen.
    Answer([u8; 16]),
    /// The person declined or hung up there.
    End([u8; 16]),
    /// Mute set there.
    Mute([u8; 16], bool),
}

/// Where events go (on the main thread).
pub type OnEvent = Box<dyn Fn(Event) + Send + Sync>;

/// Why a call ended, for the system's recents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It couldn't connect (no offer came).
    Failed,
    /// The other side hung up.
    Remote,
    /// Nobody answered.
    Unanswered,
    /// Answered on another of the user's devices, or in the app itself.
    AnsweredElsewhere,
    /// Declined on another device.
    DeclinedElsewhere,
}

/// Starts PushKit and CallKit. `false` off iOS. Call it on the main thread, once.
#[must_use]
pub fn install(on: OnEvent) -> bool {
    #[cfg(target_os = "ios")]
    return crate::apple::callkit::install(on);
    #[allow(unreachable_code, reason = "iOS only")]
    {
        drop(on);
        false
    }
}

/// Names the caller of a call the system shows.
#[allow(unused_variables, reason = "iOS only")]
pub fn update(id: [u8; 16], caller: &str, video: bool) {
    #[cfg(target_os = "ios")]
    crate::apple::callkit::update(id, caller, video);
}

/// The call is over: the system's screen goes away.
#[allow(unused_variables, reason = "iOS only")]
pub fn ended(id: [u8; 16], why: Ended) {
    #[cfg(target_os = "ios")]
    crate::apple::callkit::ended(id, why);
}
