//! The parent's rules, enforced on the child's own app (PROTOCOL §10.6, D-0166).
//!
//! The server enforces the policy's flags (no message requests, group approval, hidden). The
//! rest, sealed under the link secret, only the two linked devices read, and the child's app
//! enforces here:
//!
//! - **Each new contact needs approval.** Starting a chat with someone new asks for the
//!   parent's approval in person: this phone shows a code with the contact, the parent's phone
//!   scans it and shows eight digits (`approval_code`, from the link secret), typed here.
//!   Nothing goes through the server; approvals are kept on this device until supervision ends.
//! - **Calls only from contacts:** an offer from someone who isn't a contact is answered busy.
//! - **Nudges off:** none shows and none can be sent.
//! - **Quiet hours:** no notifications between the two times, in this device's local time.
//!
//! The signed policy is kept on this device (checked against the key scanned at linking at
//! every read), so the rules hold offline and across restarts; it is fetched again at start and
//! every 30 minutes. A policy that ends supervision lifts everything.

use std::cell::RefCell;

use super::vault::Storage;
use nsh_core::prelude::*;
use songbird_content::supervision::{ClientRules, approval_matches, open_rules};
use songbird_proto::supervision::{Policy, UNLINK};

use super::{Problem, problem, supervision as sup, wire};

/// What starting a chat answers when the contact needs the parent's approval.
pub const NEEDS_APPROVAL: &str = "Your parent's approval is needed to message someone new.";

const K_POLICY: &str = "sb.supervision_policy";
const K_APPROVED: &str = "sb.supervision_approved";
/// Wrong approval codes allowed before a wait.
const TRIES: u32 = 5;

thread_local! {
    /// Wrong codes in a row, and when typing is allowed again (ms).
    static WRONG: RefCell<(u32, u64)> = const { RefCell::new((0, 0)) };
    /// The contact waiting for approval: (account, username).
    static PENDING: RefCell<Option<([u8; 16], String)>> = const { RefCell::new(None) };
}

fn now_ms() -> u64 {
    nsh_core::time::unix_ms() as u64
}

/// The policy in force here, checked: `None` when this isn't a supervised child's device or
/// supervision has ended.
fn policy() -> Option<(Policy, sup::ChildLink)> {
    let link = sup::child_link()?;
    let bytes = wire::unhex(&Storage::get_item(K_POLICY)?)?;
    let p = Policy::decode(&bytes).ok()?;
    if !sup::verify(&p, Some(&link.policy_key)) || p.flags & UNLINK != 0 {
        return None;
    }
    if p.unlink_at_s > 0 && now_ms() / 1000 >= p.unlink_at_s {
        return None;
    }
    Some((p, link))
}

/// The client rules in force, opened.
#[must_use]
pub fn in_force() -> Option<ClientRules> {
    let (p, link) = policy()?;
    open_rules(
        &link.secret,
        p.child.as_bytes(),
        p.policy_version,
        &p.client_rules,
    )
    .ok()
}

/// Keeps a verified policy (a newer one only).
pub fn keep(p: &Policy) {
    let Some(link) = sup::child_link() else {
        return;
    };
    if !sup::verify(p, Some(&link.policy_key)) {
        return;
    }
    let newer = policy().is_none_or(|(old, _)| p.policy_version >= old.policy_version);
    if newer {
        let _ = Storage::set_item(K_POLICY, wire::hex(&p.encode()));
    }
    if p.flags & UNLINK != 0 {
        Storage::remove_item(K_APPROVED);
    }
}

/// Forgets the rules and approvals (the child unlinked, or the account is gone).
pub fn forget() {
    Storage::remove_item(K_POLICY);
    Storage::remove_item(K_APPROVED);
}

async fn refresh() {
    if sup::child_link().is_none() || !super::is_live() || !super::registered() {
        return;
    }
    if let Ok(Some(p)) = sup::fetch().await {
        keep(&p);
    }
}

/// Fetches the policy a minute after start and every 30 minutes. Called once at the root.
pub fn install() {
    Owner::detached().run(|| {
        set_timeout(std::time::Duration::from_secs(60), || {
            spawn(refresh());
        });
        set_interval(std::time::Duration::from_secs(30 * 60), || {
            spawn(refresh());
        });
    });
}

// ---------------------------------------------------------------- the rules

/// Whether calls from people who aren't contacts are declined.
#[must_use]
pub fn unknown_calls_blocked() -> bool {
    in_force().is_some_and(|r| r.block_unknown_calls)
}

/// Whether nudges are off.
#[must_use]
pub fn nudges_off() -> bool {
    in_force().is_some_and(|r| r.nudges_off)
}

/// Whether now is inside quiet hours (this device's local time).
#[must_use]
pub fn quiet_now() -> bool {
    use chrono::Timelike as _;
    in_force().is_some_and(|r| {
        let t = chrono::Local::now();
        r.quiet(t.hour() * 60 + t.minute())
    })
}

fn approved() -> Vec<String> {
    Storage::get_item(K_APPROVED)
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Whether this device may start a chat with `account`. When it may not, the contact waits
/// for approval ([`pending`]).
pub fn may_start(account: &[u8; 16], username: &str) -> bool {
    let needs = in_force().is_some_and(|r| r.approved_contacts_only);
    if !needs || approved().contains(&wire::hex(account)) {
        return true;
    }
    PENDING.with_borrow_mut(|p| *p = Some((*account, username.to_owned())));
    false
}

/// The contact waiting for the parent's approval: (account, username).
#[must_use]
pub fn pending() -> Option<([u8; 16], String)> {
    PENDING.with_borrow(Clone::clone)
}

/// Checks the digits the parent's phone showed for `account`; on a match the contact is
/// approved on this device. Five wrong codes in a row make the next wait a minute, doubling.
///
/// # Errors
/// A [`Problem`] saying what to do.
pub fn approve(account: &[u8; 16], typed: &str) -> Result<(), Problem> {
    let (wrong, until) = WRONG.with_borrow(|w| *w);
    if now_ms() < until {
        return Err(problem(format!(
            "Too many tries. Try again in {} s.",
            (until - now_ms()).div_ceil(1000)
        )));
    }
    let (p, link) = policy().ok_or_else(|| problem("Supervision isn't on for this phone."))?;
    if !approval_matches(&link.secret, p.child.as_bytes(), account, typed) {
        let wrong = wrong + 1;
        let wait = if wrong >= TRIES {
            60_000u64 << (wrong - TRIES).min(10)
        } else {
            0
        };
        WRONG.with_borrow_mut(|w| *w = (wrong, now_ms() + wait));
        return Err(problem(
            "Those digits don't match. Check them on your parent's phone.",
        ));
    }
    WRONG.with_borrow_mut(|w| *w = (0, 0));
    let mut list = approved();
    let hex = wire::hex(account);
    if !list.contains(&hex) {
        list.push(hex);
    }
    let _ = Storage::set_item(K_APPROVED, serde_json::to_string(&list).unwrap_or_default());
    PENDING.with_borrow_mut(|p| *p = None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ParentalRules;

    macro_rules! check {
        ($c:expr) => {
            if !$c {
                return Err(format!("line {}: {}", line!(), stringify!($c)));
            }
        };
    }

    /// A child's phone linked to a parent's: the parent's link.
    fn linked(rules: &ParentalRules, version: u64) -> sup::ParentLink {
        let l = sup::ParentLink {
            seed: [3; 32],
            secret: [4; 32],
            child: [5; 16],
            version,
            unlink_at_s: 0,
        };
        sup::save_child(&sup::ChildLink {
            policy_key: sup::public(&l).unwrap_or_default(),
            secret: l.secret,
            birth_year: 2014,
            birth_month: 1,
            country: "RO".into(),
        });
        if let Some(p) = sup::sign_policy(&l, rules, false, version) {
            keep(&p);
        }
        l
    }

    fn sign(l: &sup::ParentLink, r: &ParentalRules, unlink: bool, v: u64) -> Policy {
        sup::sign_policy(l, r, unlink, v).unwrap_or_else(|| panic!("signed"))
    }

    fn scenario() -> Result<(), String> {
        forget();
        let r = ParentalRules {
            block_unknown_calls: true,
            nudges: false,
            ..Default::default()
        };
        let l = linked(&r, 1);
        check!(unknown_calls_blocked() && nudges_off());
        let contact = [9u8; 16];
        check!(!may_start(&contact, "ana"));
        check!(pending() == Some((contact, "ana".to_owned())));
        check!(approve(&contact, "00000000").is_err());
        let code = |c: &[u8; 16]| {
            songbird_content::supervision::approval_code(&l.secret, &l.child, c).unwrap_or_default()
        };
        // Another contact's digits don't approve this one.
        check!(approve(&contact, &code(&[8; 16])).is_err());
        check!(approve(&contact, &code(&contact)).is_ok());
        check!(may_start(&contact, "ana") && pending().is_none());

        // A forged policy (another key) is ignored; an older one never replaces a newer one.
        let loose = ParentalRules {
            block_unknown_calls: false,
            ..Default::default()
        };
        let forged = sup::ParentLink {
            seed: [7; 32],
            ..l.clone()
        };
        keep(&sign(&forged, &loose, false, 9));
        check!(unknown_calls_blocked());
        keep(&sign(&l, &loose, false, 2));
        check!(!unknown_calls_blocked());
        keep(&sign(&l, &r, false, 1));
        check!(!unknown_calls_blocked());

        // Unlinking lifts every rule and forgets the approvals.
        keep(&sign(&l, &r, true, 3));
        check!(in_force().is_none() && !nudges_off());
        check!(approved().is_empty());

        // Five wrong codes make the next try wait.
        forget();
        let _l = linked(&ParentalRules::default(), 1);
        for _ in 0..TRIES {
            check!(approve(&[1; 16], "12345678").is_err());
        }
        check!(approve(&[1; 16], "12345678").is_err_and(|e| e.0.contains("Too many tries")));
        WRONG.with_borrow_mut(|w| *w = (0, 0));
        forget();
        Ok(())
    }

    #[component]
    fn RulesProbe() -> impl IntoElement {
        let out = match scenario() {
            Ok(()) => "rules: ok".to_owned(),
            Err(e) => format!("rules: failed {e}"),
        };
        text(out)
    }

    /// D-0166, on the real engine (device storage), headless.
    #[test]
    fn the_childs_app_enforces_the_rules_and_approvals_take_the_parents_digits()
    -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                RulesProbe,
            >()))
            .map_err(|e| e.to_string())?;
        app.wait_until("the result", std::time::Duration::from_secs(5), |s| {
            s.texts().iter().any(|t| t.starts_with("rules: "))
        })?;
        let shown = app
            .screen()
            .texts()
            .into_iter()
            .find(|t| t.starts_with("rules: "))
            .unwrap_or_default()
            .to_owned();
        if shown == "rules: ok" {
            Ok(())
        } else {
            Err(shown)
        }
    }
}
