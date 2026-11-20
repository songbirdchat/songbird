//! App lock (Prompt 9, M5): a six-digit passcode kept only on this device
//! (`backend::lock`). Locked at start when the lock is on, and again when
//! the app comes back after the delay the user chose; the lock screen sits
//! over everything until the passcode is right. Calls still ring over it
//! (the call layer is above). With "Unlock with Face ID" on (D-0172), the
//! system's biometric prompt shows at once and on a button; a missed match
//! counts nothing against the passcode's tries.
//!
//! Motion: the lock screen fades in over `motion::FAST` (immediately, with
//! reduced motion) and fades out when unlocked; a wrong passcode turns the
//! cells' borders to the destructive token (`code_cells`' own transition).

use std::time::Duration;

use nsh_core::{platform::Lifecycle, prelude::*, time::unix_ms};

use crate::{
    backend::lock,
    nav,
    store::{LockAfter, Store, store},
    ui::*,
};

/// Whether the lock screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Locked(pub bool);

pub fn locked() -> Signal<Locked> {
    global::<Locked>()
}

/// Locks at start if the lock is on, and follows the app going away and
/// coming back. Called once at the root.
pub fn install(s: Store) {
    if s.settings.with_untracked(|x| x.app_lock) && lock::is_set() {
        locked().set(Locked(true));
    }
    // When the app went away, on both clocks: the wall clock (counts while the phone sleeps)
    // and the monotonic one (can't be set). Either reaching the delay locks, and so does the
    // wall clock going backwards (D-0108).
    let left_at = stored(None::<(f64, nsh_core::time::Instant)>);
    effect(move || {
        let state = lifecycle();
        untracked(|| match state {
            Lifecycle::Background => {
                left_at.replace(Some((unix_ms(), nsh_core::time::now())));
            }
            Lifecycle::Active => {
                let Some((t, mono)) = left_at.replace(None) else {
                    return;
                };
                let (on, after) = (
                    s.settings.with(|x| x.app_lock),
                    s.ext().with(|x| x.app_lock_after),
                );
                let wait_ms = match after {
                    LockAfter::Immediately => 0.0,
                    LockAfter::Minute => 60_000.0,
                    LockAfter::Hour => 3_600_000.0,
                };
                let wall = unix_ms() - t;
                let elapsed = wall.max(mono.elapsed().as_secs_f64() * 1000.0);
                if on && lock::is_set() && (wall < 0.0 || elapsed >= wait_ms) {
                    locked().set(Locked(true));
                }
            }
            Lifecycle::Inactive => {}
        });
    });
}

/// The lock screen, over the app while locked.
pub fn layer() -> AnyElement {
    when(move || locked().get().0, lock_screen).into_element()
}

fn lock_screen() -> AnyElement {
    let code = signal(String::new());
    // Saved on the device: closing the app doesn't reset the count or the wait. The wait is
    // measured on the monotonic clock from this screen's start (`mono`), never the wall clock.
    let base = nsh_core::time::now();
    let mono = move || base.elapsed().as_secs_f64() * 1000.0;
    let (fails, left) = lock::failures();
    let wrong = signal(fails);
    let wait_until = signal(left as f64);
    if left > 0 {
        tick_until(wait_until, mono);
    }
    let checking = signal(false);
    let shake = signal(false);
    let message = move || {
        let left = ((wait_until.get() - mono()) / 1000.0).ceil();
        if left > 0.0 {
            format!("Too many tries. Try again in {left:.0} seconds.")
        } else if wrong.get() > 0 {
            "That passcode isn't right.".into()
        } else {
            "Enter your passcode to open Songbird.".into()
        }
    };
    let submit = move || {
        if checking.get_untracked() || mono() < wait_until.get_untracked() {
            return;
        }
        let entered = code.get_untracked();
        if entered.len() != 6 {
            return;
        }
        checking.set(true);
        spawn(async move {
            let ok = lock::check(entered).await;
            checking.set(false);
            code.set(String::new());
            if ok {
                lock::reset_failures();
                wrong.set(0);
                locked().set(Locked(false));
            } else {
                let (n, left) = lock::record_failure();
                wrong.set(n);
                shake.set(true);
                if left > 0 {
                    wait_until.set(mono() + left as f64);
                    // The countdown's sentence follows the clock.
                    tick_until(wait_until, mono);
                }
            }
        });
    };
    // Biometrics (D-0172): asked at once, and on the button. Not while the passcode's wait runs.
    let bio = s_bio();
    let try_bio = move || {
        if bio.is_none() || checking.get_untracked() || mono() < wait_until.get_untracked() {
            return;
        }
        checking.set(true);
        spawn(async move {
            let ok = crate::backend::biometric::verify("open Songbird")
                .await
                .is_ok();
            checking.set(false);
            if ok {
                lock::reset_failures();
                locked().set(Locked(false));
            }
        });
    };
    if bio.is_some() && left == 0 {
        set_timeout(Duration::from_millis(250), try_bio);
    }
    // A full passcode is checked at once.
    effect(move || {
        if code.with(|c| c.len() == 6) {
            untracked(submit);
        }
    });
    let fade = motion().entering(
        MotionSpec::new(crate::features::chat::timing(
            if motion::reduced() { 0.0 } else { motion::FAST },
            Easing::EaseOut,
        ))
        .values(MotionChannel::Opacity, [0.0, 1.0]),
    );
    view()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .bg(move || c::background())
        .items_center()
        .justify_center()
        .bind_motion(&fade)
        .child(
            view()
                .w_full()
                .max_w(px(360.0))
                .px_6()
                .gap_4()
                .items_center()
                .child(hero_icon(Icon::Lock))
                .child(flow_title("Songbird is locked").text_center())
                .child(
                    passcode_cells(
                        "Passcode",
                        code,
                        move || wrong.get() > 0 && code.with(String::is_empty),
                        submit,
                    )
                    .mt_2(),
                )
                .child(status_line(
                    move || {
                        if wrong.get() > 0 {
                            StatusTone::Problem
                        } else {
                            StatusTone::Muted
                        }
                    },
                    move || {
                        if wrong.get() > 0 {
                            Icon::Alert
                        } else {
                            Icon::Lock
                        }
                    },
                    message,
                ))
                .child(when(
                    move || bio.is_some(),
                    move || {
                        btn(
                            format!("Use {}", bio.map_or("biometrics", |k| k.label())),
                            Variant::Ghost,
                            BtnSize::Md,
                            try_bio,
                        )
                        .into_element()
                    },
                )),
        )
        .into_element()
}

/// The biometric that opens the lock, when the user turned it on and the device offers one.
fn s_bio() -> Option<songbird_device::biometrics::Kind> {
    let on = store().ext().with_untracked(|x| x.app_lock_biometrics);
    on.then(crate::backend::biometric::kind).flatten()
}

/// Rewrites the countdown every second until `until` (on the screen's
/// monotonic clock `mono`), saving what is left so a restart resumes it.
fn tick_until(until: Signal<f64>, mono: impl Fn() -> f64 + Copy + 'static) {
    fn step(until: Signal<f64>, mono: impl Fn() -> f64 + Copy + 'static) {
        let Some(end) = until.try_get() else {
            return;
        };
        let left = (end - mono()).max(0.0);
        lock::wait_left(left as u64);
        if left > 0.0 {
            until.update(|_| {});
            set_timeout(Duration::from_secs(1), move || step(until, mono));
        }
    }
    step(until, mono);
}

/// Turning the lock on: a passcode typed twice.
pub fn set_passcode_page() {
    crate::features::settings::push("Set a passcode", || {
        let s = store();
        let first = signal(String::new());
        let second = signal(String::new());
        let confirming = signal(false);
        let mismatch = signal(false);
        let saving = signal(false);
        let done = move || {
            if !confirming.get_untracked() {
                if first.with_untracked(|c| c.len() == 6) {
                    confirming.set(true);
                }
                return;
            }
            let (a, b) = (first.get_untracked(), second.get_untracked());
            if a != b {
                mismatch.set(true);
                second.set(String::new());
                return;
            }
            saving.set(true);
            spawn(async move {
                if lock::set(a).await {
                    s.settings_update(|x| x.app_lock = true);
                    locked().set(Locked(false));
                    nav::pop();
                    toast("App lock is on.");
                } else {
                    saving.set(false);
                    toast("The passcode couldn't be saved. Try again.");
                }
            });
        };
        effect(move || {
            let full = if confirming.get() {
                second.with(|c| c.len() == 6)
            } else {
                first.with(|c| c.len() == 6)
            };
            if full {
                untracked(done);
            }
        });
        view()
            .px_6()
            .pt_6()
            .gap_4()
            .items_center()
            .child(hero_icon(Icon::Lock))
            .child(
                flow_lead(move || -> SharedString {
                    if confirming.get() {
                        "Type it again to confirm.".into()
                    } else {
                        "Choose six digits. Songbird asks for them when you open it. They stay on this device.".into()
                    }
                })
                .text_center(),
            )
            .child(when(move || !confirming.get(), move || {
                passcode_cells("New passcode", first, || false, done)
            }))
            .child(when(move || confirming.get(), move || {
                passcode_cells("Confirm passcode", second, move || mismatch.get(), done)
            }))
            .child(when(move || mismatch.get(), || {
                status_line(|| StatusTone::Problem, || Icon::Alert, || "They don't match. Type the new passcode again.".into())
            }))
            .into_element()
    });
}

/// Turning the lock off: the current passcode first.
pub fn turn_off_page() {
    crate::features::settings::push("Turn off app lock", || {
        let s = store();
        let code = signal(String::new());
        let wrong = signal(false);
        let submit = move || {
            let entered = code.get_untracked();
            if entered.len() != 6 {
                return;
            }
            spawn(async move {
                if lock::check(entered).await {
                    lock::clear();
                    s.settings_update(|x| x.app_lock = false);
                    nav::pop();
                    toast("App lock is off.");
                } else {
                    wrong.set(true);
                    code.set(String::new());
                }
            });
        };
        effect(move || {
            if code.with(|c| c.len() == 6) {
                untracked(submit);
            }
        });
        view()
            .px_6()
            .pt_6()
            .gap_4()
            .items_center()
            .child(hero_icon(Icon::Unlock))
            .child(flow_lead("Enter your passcode to turn off app lock.").text_center())
            .child(passcode_cells(
                "Passcode",
                code,
                move || wrong.get(),
                submit,
            ))
            .child(when(
                move || wrong.get(),
                || {
                    status_line(
                        || StatusTone::Problem,
                        || Icon::Alert,
                        || "That passcode isn't right.".into(),
                    )
                },
            ))
            .into_element()
    });
}

/// The lock on the real engine, headless.
#[cfg(test)]
mod headless {
    use nsh_core::prelude::*;

    use crate::store::Store;

    #[component]
    fn LockProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        crate::backend::records::restore(s);
        super::install(s);
        view().flex_1().child(text("the app")).child(super::layer())
    }

    #[test]
    fn the_app_opens_locked_and_only_the_passcode_opens_it() -> Result<(), String> {
        let storage = nsh_testing::MemoryStorage::new([
            (
                "sb.app_lock".to_string(),
                crate::backend::lock::stored_form("246810", crate::backend::wire::random()),
            ),
            (
                "sb.device_settings".to_string(),
                r#"{"app_lock":"1"}"#.to_string(),
            ),
        ]);
        let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<LockProbe>());
        config.storage = Some(storage);
        let mut app = nsh_testing::Headless::start(config).map_err(|e| e.to_string())?;
        app.wait_for("text:Songbird is locked")?;
        app.type_text("Passcode", "111111")?;
        app.wait_for("text:That passcode isn't right.")?;
        app.type_text("Passcode", "246810")?;
        app.wait_for_gone(
            "text:Songbird is locked",
            std::time::Duration::from_secs(10),
        )?;
        app.wait_for("text:the app")?;
        Ok(())
    }

    /// A wait earned before the app was closed still holds when it opens.
    #[test]
    fn closing_the_app_does_not_reset_the_wait() -> Result<(), String> {
        let storage = nsh_testing::MemoryStorage::new([
            (
                "sb.app_lock".to_string(),
                crate::backend::lock::stored_form("246810", crate::backend::wire::random()),
            ),
            (
                "sb.device_settings".to_string(),
                r#"{"app_lock":"1"}"#.to_string(),
            ),
            ("sb.app_lock_fails".to_string(), "5:600000".to_string()),
        ]);
        let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<LockProbe>());
        config.storage = Some(storage);
        let mut app = nsh_testing::Headless::start(config).map_err(|e| e.to_string())?;
        app.wait_for("text:Songbird is locked")?;
        app.wait_for("text:/^Too many tries/")?;
        // The right passcode is not even checked during the wait.
        app.type_text("Passcode", "246810")?;
        app.wait_for("text:Songbird is locked")?;
        app.wait_for("text:/^Too many tries/")?;
        Ok(())
    }
}
