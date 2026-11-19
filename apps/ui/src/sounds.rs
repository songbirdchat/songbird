//! Songbird's own sounds (A1 sound design): original tones synthesized by
//! `assets/sounds/make_sounds.py`, bundled in the binary for previews and
//! shipped as assets for the system to play with notifications.

use std::cell::RefCell;

use nsh_core::prelude::*;

/// (name in Settings, file stem, bytes).
const TONES: [(&str, &str, &[u8]); 10] = [
    (
        "Chirp",
        "chirp",
        include_bytes!("../assets/sounds/chirp.wav"),
    ),
    (
        "Tweet",
        "tweet",
        include_bytes!("../assets/sounds/tweet.wav"),
    ),
    (
        "Flutter",
        "flutter",
        include_bytes!("../assets/sounds/flutter.wav"),
    ),
    ("Pip", "pip", include_bytes!("../assets/sounds/pip.wav")),
    (
        "Songbird",
        "songbird",
        include_bytes!("../assets/sounds/songbird.wav"),
    ),
    (
        "Dawn chorus",
        "dawn_chorus",
        include_bytes!("../assets/sounds/dawn_chorus.wav"),
    ),
    (
        "Robin",
        "robin",
        include_bytes!("../assets/sounds/robin.wav"),
    ),
    ("Wren", "wren", include_bytes!("../assets/sounds/wren.wav")),
    (
        "Skylark",
        "skylark",
        include_bytes!("../assets/sounds/skylark.wav"),
    ),
    (
        "Nightingale",
        "nightingale",
        include_bytes!("../assets/sounds/nightingale.wav"),
    ),
];

thread_local! {
    /// The sound playing now (a new preview stops it).
    static PLAYING: RefCell<Option<Sound>> = const { RefCell::new(None) };
}

fn find(name: &str) -> Option<(&'static str, &'static [u8])> {
    TONES
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, f, b)| (*f, *b))
}

/// The asset path the system plays with a notification (`None`: "None").
pub fn asset(name: &str) -> Option<String> {
    find(name).map(|(f, _)| format!("sounds/{f}.wav"))
}

/// Plays tone `name` once (the picker's preview, an incoming call's ring).
pub fn play(name: &str) {
    stop();
    let Some((_, bytes)) = find(name) else { return };
    spawn(async move {
        if let Ok(sound) = load_sound(bytes).await {
            sound.play(1.0, 1.0);
            PLAYING.with_borrow_mut(|p| *p = Some(sound));
        }
    });
}

/// Stops what [`play`] started.
pub fn stop() {
    if let Some(s) = PLAYING.with_borrow_mut(Option::take) {
        s.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_in_settings_has_a_sound() {
        for t in crate::store::MESSAGE_TONES
            .iter()
            .chain(crate::store::RINGTONES.iter())
        {
            assert!(*t == "None" || find(t).is_some(), "{t}");
        }
        assert!(find("Chirp").is_some_and(|(_, b)| b.starts_with(b"RIFF")));
    }
}
