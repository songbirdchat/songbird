//! The app's icon (D-0179): the person's choice in Appearance, a Plus feature, made real by
//! the platform (`songbird_device::icon`): iOS's alternate icons, Android's launcher aliases,
//! the Dock on macOS and the taskbar on Windows. Linux has none, and the setting isn't shown.
//!
//! The choice is a synced setting (`s/app_icon`); each device applies it at start-up, and the
//! main icon comes back when Plus ends.

use super::vault::Storage;
use crate::store::{Plan, Store};

/// The icons, in Appearance's order; the first is the main one.
pub const NAMES: [&str; 4] = ["Songbird", "Night", "Paper", "Lavender"];

/// Their pictures for the desktop's Dock and taskbar (512 × 512, the macOS shape).
const PNGS: [&[u8]; 4] = [
    include_bytes!("../../assets/icon/songbird-512.png"),
    include_bytes!("../../assets/icon/night-512.png"),
    include_bytes!("../../assets/icon/paper-512.png"),
    include_bytes!("../../assets/icon/lavender-512.png"),
];

/// The icon the system shows now (phones keep it between runs).
const K_APPLIED: &str = "sb.icon_applied";

/// Whether this platform can change the app's icon.
pub fn supported() -> bool {
    songbird_device::icon::supported()
}

/// Whether the change lasts only while Songbird runs (the desktop).
pub fn while_running() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Makes icon `i` the app's. `false` when the system refused.
pub async fn apply(i: u8) -> bool {
    let i = usize::from(i).min(NAMES.len() - 1);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let name = (i > 0).then_some(NAMES[i]);
    songbird_device::icon::set(
        name,
        PNGS[i],
        Box::new(move |ok| {
            // Nobody waits any more when the screen that asked has gone.
            let _ = tx.send(ok);
        }),
    );
    let ok = rx.await.unwrap_or(false);
    if ok {
        super::put(K_APPLIED, &i.to_string());
    }
    ok
}

/// At start-up: the chosen icon, or the main one when the account has no Plus.
pub fn on_start(s: Store) {
    if !supported() {
        return;
    }
    let chosen = s.ext().with_untracked(|x| x.app_icon);
    let want = if s.settings.with_untracked(|x| x.plan) == Plan::Free {
        0
    } else {
        chosen
    };
    let applied = Storage::get_item(K_APPLIED)
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0);
    // Phones keep the icon; the desktop starts with the bundle's, so a chosen one is set again.
    if want == applied && !(while_running() && want != 0) {
        return;
    }
    super::run(async move {
        if apply(want).await && want != chosen {
            s.ext_update(|x| x.app_icon = want);
        }
        Ok(())
    });
}
