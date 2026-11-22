//! Incoming calls through the system's call screen on iOS (PushKit and CallKit, D-0176).
//!
//! A ring's VoIP push shows a call from "Songbird" at once (iOS requires it); this module ties it
//! to the offer when the connection brings it ([`bind`], naming the caller), answers or ends the
//! call from the system's screen, and ends the system's call when no offer came within 30
//! seconds, or when the call ends in the app.

use std::{cell::RefCell, time::Duration};

use nsh_core::prelude::*;
use songbird_device::callkit::{self, Ended, Event};

/// How long a ring's push waits for its offer.
const OFFER_WAIT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct State {
    /// A call the system shows, its offer not here yet; answered there already?
    pending: Option<([u8; 16], bool)>,
    /// The system's call and the app's call it is.
    bound: Option<(u64, [u8; 16])>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Starts PushKit and CallKit (iOS; once per run).
pub fn install() {
    thread_local! {
        static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if ON.replace(true) {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    if !callkit::install(Box::new(move |e| {
        let _ = tx.send(e);
    })) {
        return;
    }
    Owner::detached().run(|| {
        spawn(async move {
            while let Some(e) = rx.recv().await {
                handle(e);
            }
        });
    });
}

fn handle(e: Event) {
    match e {
        Event::Token(t) => super::push::voip_token(t),
        Event::Ring(id) => {
            STATE.with_borrow_mut(|s| s.pending = Some((id, false)));
            set_timeout(OFFER_WAIT, move || {
                let stale = STATE.with_borrow_mut(|s| {
                    let stale = s.pending.is_some_and(|(p, _)| p == id);
                    if stale {
                        s.pending = None;
                    }
                    stale
                });
                if stale {
                    callkit::ended(id, Ended::Failed);
                }
            });
        }
        Event::Answer(id) => {
            let bound = STATE.with_borrow_mut(|s| {
                if let Some((p, answered)) = s.pending.as_mut().filter(|(p, _)| *p == id) {
                    let _ = p;
                    *answered = true;
                }
                s.bound.filter(|(_, b)| *b == id).map(|(ui, _)| ui)
            });
            if let Some(ui) = bound {
                crate::features::calls::live::system_answer(ui);
            }
        }
        Event::End(id) => {
            let bound = STATE.with_borrow_mut(|s| {
                if s.pending.is_some_and(|(p, _)| p == id) {
                    s.pending = None;
                }
                let b = s.bound.filter(|(_, b)| *b == id).map(|(ui, _)| ui);
                if b.is_some() {
                    s.bound = None;
                }
                b
            });
            if let Some(ui) = bound {
                crate::features::calls::live::system_end(ui);
            }
        }
        Event::Mute(id, muted) => {
            let bound = STATE.with_borrow(|s| s.bound.filter(|(_, b)| *b == id).map(|(ui, _)| ui));
            if let Some(ui) = bound {
                crate::features::calls::live::system_mute(ui, muted);
            }
        }
    }
}

/// An offer rang the app's call `ui`: if the system shows a call for a ring, it becomes this
/// call, named; answered there already, it is answered now.
pub fn bind(ui: u64) {
    let Some((id, answered)) = STATE.with_borrow_mut(|s| {
        let p = s.pending.take()?;
        s.bound = Some((ui, p.0));
        Some(p)
    }) else {
        return;
    };
    let (title, video) =
        crate::features::calls::live::with(ui, |c| (c.title.clone(), c.video)).unwrap_or_default();
    callkit::update(id, &title, video);
    if answered {
        crate::features::calls::live::system_answer(ui);
    }
}

/// Call `ui` was answered in the app: the system's screen goes away.
pub fn answered_in_app(ui: u64) {
    if let Some(id) = take(ui) {
        callkit::ended(id, Ended::AnsweredElsewhere);
    }
}

/// Call `ui` ended in the app (or the other side ended it).
pub fn call_closed(ui: u64, unanswered: bool) {
    if let Some(id) = take(ui) {
        callkit::ended(
            id,
            if unanswered {
                Ended::Unanswered
            } else {
                Ended::Remote
            },
        );
    }
}

fn take(ui: u64) -> Option<[u8; 16]> {
    STATE.with_borrow_mut(|s| {
        let id = s.bound.filter(|(u, _)| *u == ui).map(|(_, id)| id)?;
        s.bound = None;
        Some(id)
    })
}
