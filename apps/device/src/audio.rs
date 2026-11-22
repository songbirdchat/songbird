//! A call's audio on a phone (D-0176). WebRTC records and plays; this sets what the system
//! needs around it: the microphone's permission (Android asks at run time; iOS asks when WebRTC
//! first opens the microphone), the communication mode (Android), and where the sound goes
//! (the earpiece, or the loudspeaker with "Speaker").

#![forbid(unsafe_code)]

/// The answer to asking for the microphone.
pub type Done = Box<dyn FnOnce(bool) + Send>;

/// Whether the app may record: asks the person first where the system wants that now
/// (Android). Elsewhere `true` (iOS asks itself when the microphone opens; computers don't ask).
pub fn microphone(done: Done) {
    #[cfg(target_os = "android")]
    return crate::android::microphone(done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(true);
}

/// A call starts: the system's communication mode (Android).
pub fn call_started() {
    #[cfg(target_os = "android")]
    crate::android::audio_mode(true);
}

/// The sound of the call through the loudspeaker (`true`) or the earpiece and headsets.
#[allow(unused_variables, reason = "phones only")]
pub fn speaker(on: bool) {
    #[cfg(target_os = "ios")]
    crate::apple::speaker(on);
    #[cfg(target_os = "android")]
    crate::android::speaker(on);
}

/// The call ended: everything back as it was.
pub fn call_ended() {
    #[cfg(target_os = "ios")]
    crate::apple::speaker(false);
    #[cfg(target_os = "android")]
    {
        crate::android::speaker(false);
        crate::android::audio_mode(false);
    }
}
