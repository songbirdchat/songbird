//! Screen protection (D-0173): "Block screenshots" keeps Songbird's windows out of screenshots
//! and recordings where the system allows it (`songbird_device::screen`), and a cover hides the
//! app where it can't be kept out:
//!
//! - on iOS and Android with "Hide content in app switcher", whenever the app isn't in front
//!   (the switcher's snapshot shows the cover);
//! - on iOS with "Block screenshots", while the screen is recorded, mirrored or shared.
//!
//! Motion: none, on purpose: the cover must be complete in the frame the system snapshots.

use std::time::Duration;

use nsh_core::platform::{Lifecycle, Os};
use nsh_core::prelude::*;

use crate::{store::Store, ui::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Captured(bool);

fn mobile() -> bool {
    matches!(os(), Os::Ios | Os::Android)
}

/// Applies the settings and follows them. Called once at the root.
pub fn install(s: Store) {
    effect(move || {
        let on = s.settings.with(|x| x.block_screenshots);
        untracked(|| songbird_device::screen::protect(on));
    });
    effect(move || {
        let hide = s.settings.with(|x| x.hide_in_switcher);
        untracked(|| songbird_device::screen::hide_in_recents(hide));
    });
    let captured = global::<Captured>();
    Owner::detached().run(|| {
        set_interval(Duration::from_secs(1), move || {
            let on = s.settings.with_untracked(|x| x.block_screenshots);
            if !on {
                if captured.get_untracked().0 {
                    captured.set(Captured(false));
                }
                return;
            }
            // Windows opened since (a chat in its own window) are kept out too.
            if !mobile() {
                songbird_device::screen::protect(true);
            }
            let now = Captured(songbird_device::screen::captured());
            if captured.get_untracked() != now {
                captured.set(now);
            }
        });
    });
}

/// The cover, over everything when it's needed.
pub fn layer(s: Store) -> AnyElement {
    let captured = global::<Captured>();
    let show = move || {
        let away =
            mobile() && s.settings.with(|x| x.hide_in_switcher) && lifecycle() != Lifecycle::Active;
        let recorded = s.settings.with(|x| x.block_screenshots) && captured.get().0;
        away || recorded
    };
    when(show, move || {
        let recorded = move || captured.get().0;
        view()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .bg(c::background)
            .items_center()
            .justify_center()
            .gap_3()
            .child(hero_icon(Icon::Lock))
            .child(flow_title("Songbird").text_center())
            .child(when(recorded, || {
                muted("Hidden while your screen is recorded or shared.")
                    .text_center()
                    .into_element()
            }))
            .into_element()
    })
    .into_element()
}
