//! A4: the first run. A new person goes welcome → invite code → username
//! → profile → recovery key (→ backups) → notifications → "Who do you
//! want to talk to?" and lands in a real conversation, in under two
//! minutes. A returning person restores a backup, links to their phone,
//! or reads what to do without their recovery key.
//!
//! One full-screen surface with its own step history (Back, Escape),
//! full-bleed on phones and a centred 440pt card on desktops.

mod returning;
mod steps;

use nsh_core::prelude::*;

use crate::{
    features::backups::FlowHost,
    store::{ChatId, Store, store},
    ui::*,
};

/// Where the first run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Welcome,
    Invite,
    Username,
    Creating,
    /// The account exists but its username was claimed first: pick another.
    ClaimUsername,
    Profile,
    Recovery,
    BackupOffer,
    BackupSetup,
    Notifications,
    Connect,
    Returning,
    Restore,
    LinkDevice,
    LostKey,
}

/// The new-account steps the progress shows.
pub(crate) const STEPS: usize = 6;

impl Step {
    /// Its place in the new-account progress (`None`: not shown).
    pub(crate) fn progress(self) -> Option<usize> {
        match self {
            Step::Invite => Some(0),
            Step::Username | Step::Creating | Step::ClaimUsername => Some(1),
            Step::Profile => Some(2),
            Step::Recovery | Step::BackupOffer | Step::BackupSetup => Some(3),
            Step::Notifications => Some(4),
            Step::Connect => Some(5),
            _ => None,
        }
    }

    /// Whether Back leaves it. Not while the account is being made, and
    /// not back over it once it exists.
    pub(crate) fn can_go_back(self) -> bool {
        !matches!(
            self,
            Step::Welcome | Step::Creating | Step::ClaimUsername | Step::Profile
        )
    }
}

/// The first run's state: the step history and the drafts that outlive a
/// step (Back keeps what was typed).
#[derive(Clone, Copy)]
pub(crate) struct Flow {
    pub history: Signal<Vec<Step>>,
    pub forward: Stored<bool>,
    pub welcome_page: Signal<usize>,
    pub invite: Signal<String>,
    pub username: Signal<String>,
    pub name: Signal<String>,
    pub key_typed: Signal<String>,
    /// The invite code registration refused (shown on the invite step
    /// until it is edited).
    pub invite_refused: Signal<Option<String>>,
    /// The username someone claimed first.
    pub taken: Signal<Option<String>>,
    /// The backup wizard or restore shown inside a step, sharing Back.
    pub host: FlowHost,
}

impl Flow {
    fn new() -> Self {
        Self {
            history: signal(vec![Step::Welcome]),
            forward: stored(true),
            welcome_page: signal(0),
            invite: signal(String::new()),
            username: signal(String::new()),
            name: signal(String::new()),
            key_typed: signal(String::new()),
            invite_refused: signal(None),
            taken: signal(None),
            host: FlowHost::new(),
        }
    }

    pub(crate) fn step(&self) -> Step {
        self.history
            .with(|h| h.last().copied().unwrap_or(Step::Welcome))
    }

    pub(crate) fn go(&self, step: Step) {
        self.host.reset();
        self.forward.with(|f| *f = true);
        self.history.update(|h| h.push(step));
    }

    /// Goes on without keeping the current step in the history.
    pub(crate) fn replace(&self, step: Step) {
        self.host.reset();
        self.forward.with(|f| *f = true);
        self.history.update(|h| {
            h.pop();
            h.push(step);
        });
    }

    /// Goes back to `step`, an earlier one, dropping the steps after it
    /// (registration refused the invite code).
    pub(crate) fn rewind_to(&self, step: Step) {
        self.host.reset();
        self.forward.with(|f| *f = false);
        self.history.update(|h| {
            while h.len() > 1 && h.last() != Some(&step) {
                h.pop();
            }
        });
    }

    /// Whether Back shows: the step allows it and an inner flow does not
    /// hide it.
    pub(crate) fn back_shown(&self) -> bool {
        self.step().can_go_back()
            && self.history.with(|h| h.len() > 1)
            && !self.host.hide_back.get()
    }

    pub(crate) fn back(&self) {
        if !self.host.hide_back.get_untracked() {
            // An inner flow (the backup wizard) steps back inside itself first.
            if let Some(inner) = self.host.back.get() {
                if inner() {
                    return;
                }
            }
        }
        if self.history.with_untracked(|h| h.len() > 1)
            && self.step().can_go_back()
            && !self.host.hide_back.get_untracked()
        {
            self.host.reset();
            self.forward.with(|f| *f = false);
            self.history.update(|h| {
                h.pop();
            });
        }
    }
}

/// Leaves the first run into `chat`, with a welcome.
/// Ends the first run: opens `chat` where it exists (the demo's welcome chat; a real account
/// starts on its chat list, D-0183: it opened a chat that wasn't there).
pub(crate) fn finish(s: Store, chat: ChatId, message: String) {
    if s.chat(chat).is_some() {
        crate::features::chat::open(chat);
    }
    toast(message);
    s.finish_onboarding();
}

/// The whole first run, shown full screen until it finishes
/// (`Store::finish_onboarding`).
pub fn flow() -> AnyElement {
    let s = store();
    let f = Flow::new();
    on_key("escape", move |_| f.back());
    #[cfg(debug_assertions)]
    preview_keys(s);
    view()
        .flex_1()
        .child(when(wide, move || desktop_frame(s, f)).otherwise(move || phone_frame(s, f)))
        .into_element()
}

fn phone_frame(s: Store, f: Flow) -> AnyElement {
    view()
        .flex_1()
        .bg(move || c::background())
        .p_safe_area()
        .child(top_bar(f))
        .child(step_region(s, f))
        .into_element()
}

fn desktop_frame(s: Store, f: Flow) -> AnyElement {
    view()
        .flex_1()
        .bg(move || c::chat_bg())
        .items_center()
        .justify_center()
        .p_6()
        .child(
            view()
                .w(px(440.0))
                .h(move || px((window().height.0 - 64.0).clamp(480.0, 700.0)))
                .rounded(move || radius::sheet())
                .overflow_hidden()
                .bg(move || c::card())
                .border_1()
                .border_color(move || c::border())
                .shadow_lg()
                .pt_2()
                .child(top_bar(f))
                .child(step_region(s, f)),
        )
        .into_element()
}

/// Back and the progress. Back keeps its slot when hidden, so the
/// progress never moves.
fn top_bar(f: Flow) -> View {
    let step = computed(move || f.step());
    let back_shown = move || f.back_shown();
    let side = if is_desktop() { 32.0 } else { 44.0 };
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .pl_3()
        .pr_3()
        .h(px(52.0))
        .child(
            view().size(px(side)).child(
                icon_btn(Icon::Back, "Back", move || f.back())
                    .style_when(move || !back_shown(), |s| s.hidden()),
            ),
        )
        .child(
            view()
                .flex_1()
                .flex_row()
                .pl_2()
                .style_when(move || step.get().progress().is_none(), |s| s.hidden())
                .child(step_segments(STEPS, move || {
                    step.get().progress().unwrap_or(0)
                })),
        )
        // The progress ends on the content's trailing edge (24pt in):
        // the bar's 12pt inset, the 8pt gap and this 4pt.
        .child(view().w(px(4.0)))
}

/// The current step, sliding in from the side it comes from.
fn step_region(s: Store, f: Flow) -> View {
    let step = computed(move || f.step());
    view().flex_1().child(dynamic(move || {
        let current = step.get();
        untracked(move || {
            let dir = if f.forward.get() { 1.0 } else { -1.0 };
            let enter = motion().entering(
                MotionSpec::new(KeyframeTiming::new(motion::BASE).easing(Easing::EaseOut))
                    .values(MotionChannel::Opacity, [0.0, 1.0])
                    .values(MotionChannel::TranslateX, [24.0 * dir, 0.0]),
            );
            let body = match current {
                Step::Welcome => steps::welcome(f),
                Step::Invite => steps::invite(f),
                Step::Username => steps::username(s, f),
                Step::Creating => steps::creating(s, f),
                Step::ClaimUsername => steps::claim_username(s, f),
                Step::Profile => steps::profile(s, f),
                Step::Recovery => steps::recovery(s, f),
                Step::BackupOffer => steps::backup_offer(f),
                Step::BackupSetup => steps::backup_setup(f),
                Step::Notifications => steps::notifications(f),
                Step::Connect => steps::connect(s, f),
                Step::Returning => returning::returning(f),
                Step::Restore => returning::restore(s, f),
                Step::LinkDevice => returning::link_device(s, f),
                Step::LostKey => returning::lost_key(f),
            };
            view()
                .flex_1()
                .bind_motion(&enter)
                .child(body)
                .into_element()
        })
    }))
}

/// Debug builds: jump straight to the pages this module owns, for
/// screenshots and review (mod+alt+1 … 7). Not in release builds.
#[cfg(debug_assertions)]
fn preview_keys(s: Store) {
    use crate::features::{backups, devices};
    let open = move |page: fn()| {
        page();
        s.finish_onboarding();
    };
    on_key("mod+alt+1", move |_| open(|| devices::safety_number(1)));
    on_key("mod+alt+2", move |_| open(devices::linked_devices));
    on_key("mod+alt+3", move |_| open(devices::account_security));
    on_key("mod+alt+4", move |_| open(backups::backup_settings));
    on_key("mod+alt+5", move |_| open(backups::backup_wizard));
    on_key("mod+alt+6", move |_| open(backups::transfer));
    on_key("mod+alt+7", move |_| {
        s.set_key_changed(2, true);
        open(|| devices::safety_number(2));
    });
}
