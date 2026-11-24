//! Extra state and intents for invites, plans, support, WhatsApp chats
//! and the desktop (A12, A13, A15). `Store` is shared, so this state lives
//! in one surface global (`Extras`) and is changed only through the
//! `impl Store` intents below.
//!
//! A real account's purchases, invites and support requests reach billing and
//! the server (`crate::backend::plans`); WhatsApp linking waits for its bridge
//! (Prompt 14) and says so. Only the demo fixtures simulate them (D-0184).

use std::time::Duration;

use nsh_core::{prelude::*, storage::Storage};

use super::{Plan, Store};

/// Storage key: the user hid the tray / menu bar icon.
const TRAY_HIDDEN_KEY: &str = "tray-hidden";

/// Monthly or yearly billing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Billing {
    #[default]
    Monthly,
    Yearly,
}

/// One invite code of the user's. Only whether it was used is known to
/// the UI; who used it is never shown (the server does not tell us either).
#[derive(Debug, Clone, PartialEq)]
pub struct InviteCode {
    pub code: String,
    pub used: bool,
}

/// A seat of a Family plan.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilySeat {
    pub id: u64,
    pub name: String,
    /// Invited, not joined yet.
    pub pending: bool,
    /// The seat's link to share (real backend only; it holds the seat's secret).
    pub link: Option<String>,
}

/// The account's standing (D-0131), as the server reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Standing {
    #[default]
    Good,
    /// New conversations slowed for a while; lifts by itself.
    Limited,
    /// Cannot send or call until lifted; receiving, export, deletion and appeals work.
    Suspended,
}

/// How the WhatsApp link stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhatsAppLink {
    #[default]
    Idle,
    /// Waiting for the code to be entered (or the QR scanned) on the phone.
    Pairing,
}

/// The extras' state, one per surface.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extras {
    /// Show WhatsApp chats in their own filter instead of in All chats.
    pub whatsapp_separate: bool,
    pub whatsapp_link: WhatsAppLink,
    pub billing: Billing,
    /// The paid plan ends at the end of the period instead of renewing.
    pub cancels_at_period_end: bool,
    pub family: Vec<FamilySeat>,
    /// The tray / menu bar icon is hidden by the user.
    pub tray_hidden: bool,
    /// Support requests the server accepted this session.
    pub support_sent: u32,
    /// Purchase in progress (the store sheet is busy).
    pub purchasing: bool,
    /// The account's standing.
    pub standing: Standing,
    /// The last UTC day the paid plan holds (real backend).
    pub last_day: Option<u32>,
    /// The account's invite codes and their status (real backend).
    pub invites_live: Option<Vec<InviteCode>>,
}

/// The real backend is on and this device is registered.
fn live_ready() -> bool {
    crate::backend::is_live() && crate::backend::registered()
}

/// The extras' global.
pub fn extras() -> Signal<Extras> {
    global::<Extras>()
}

/// The user's invite code number `i`: 24 Crockford base32 symbols in six
/// groups of four, as the server issues them (proto
/// `registration::invite`). Pure, deterministic per seed.
pub fn invite_code(seed: u64, i: u32) -> String {
    use super::ext_onboarding::{INVITE_GROUP, INVITE_LEN};
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut x = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(u64::from(i).wrapping_mul(1_442_695_040_888_963_407))
        ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = String::new();
    for n in 0..INVITE_LEN {
        if n > 0 && n % INVITE_GROUP == 0 {
            out.push('-');
        }
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 29;
        out.push(ALPHABET[(x % ALPHABET.len() as u64) as usize] as char);
    }
    out
}

/// The personal invite link: opens a chat with the user after signup.
pub fn personal_link(username: &str) -> String {
    format!(
        "https://songbird.chat/i/{}",
        username.trim_start_matches('@').to_lowercase()
    )
}

const ANNOUNCEMENT_KEY: &str = "announcement-dismissed";

impl Store {
    // ------------------------------------------------------------ invites

    /// The user's invite codes, used ones first. The counts come from the
    /// session (`invites_used`, `invites_left`).
    /// A real account's, as the server lists them; the demo's, made from its seed.
    pub fn invite_codes(&self) -> Vec<InviteCode> {
        if let Some(live) = extras().with(|e| e.invites_live.clone()) {
            // Used ones first.
            let mut v = live;
            v.sort_by_key(|c| !c.used);
            return v;
        }
        // A real account's codes come only from the server (`refresh_invites`).
        if !super::demo() {
            return Vec::new();
        }
        let (used, left) = self.session.with(|s| (s.invites_used, s.invites_left));
        let seed = self.me.with(|m| m.seed);
        (0..used + left)
            .map(|i| InviteCode {
                code: invite_code(seed, i),
                used: i < used,
            })
            .collect()
    }

    /// Reads this account's invite codes and which are used (real backend).
    pub fn refresh_invites(&self) {
        if !live_ready() {
            return;
        }
        let s = *self;
        crate::backend::run(async move {
            let codes = crate::backend::plans::invite_codes().await?;
            let used = codes.iter().filter(|c| c.1).count() as u32;
            let left = codes.len() as u32 - used;
            s.session.update(|x| {
                x.invites_used = used;
                x.invites_left = left;
            });
            extras().update(|e| {
                e.invites_live = Some(
                    codes
                        .into_iter()
                        .map(|(code, used)| InviteCode { code, used })
                        .collect(),
                );
            });
            Ok(())
        });
    }

    // ------------------------------------------------------------ plans

    /// The current plan.
    pub fn plan(&self) -> Plan {
        self.settings.with(|s| s.plan)
    }

    /// Buys `plan` through the platform's store: StoreKit 2 or Play Billing on phones and the
    /// Mac, the EU provider's checkout in the browser elsewhere; billing then issues the
    /// anonymous tokens this account redeems (D-0129, D-0178).
    pub fn purchase(&self, plan: Plan, billing: Billing, done: impl FnOnce() + 'static) {
        let s = *self;
        extras().update(|e| e.purchasing = true);
        if live_ready() && crate::backend::plans::in_store() {
            let p = if plan == Plan::Family {
                "family"
            } else {
                "plus"
            };
            let i = if billing == Billing::Yearly {
                "year"
            } else {
                "month"
            };
            extras().update(|e| e.billing = billing);
            crate::backend::run(async move {
                match crate::backend::plans::buy_in_store(p, i).await {
                    Ok(true) => s.await_store(0, Box::new(done)),
                    Ok(false) => extras().update(|x| x.purchasing = false),
                    Err(e) => {
                        extras().update(|x| x.purchasing = false);
                        return Err(e);
                    }
                }
                Ok(())
            });
            return;
        }
        if live_ready() {
            // The EU provider in the browser: the app keeps only its subscriber secret, then
            // waits for the payment and fetches its anonymous tokens (D-0129).
            let (p, i) = (
                if plan == Plan::Family {
                    "family"
                } else {
                    "plus"
                },
                if billing == Billing::Yearly {
                    "year"
                } else {
                    "month"
                },
            );
            extras().update(|e| e.billing = billing);
            crate::backend::run(async move {
                match crate::backend::plans::checkout(p, i).await {
                    Ok(url) => drop(open_url(url)),
                    // Already paying on this device: fetch this month's tokens instead.
                    Err(e) if e.0.contains("already has a subscription") => {}
                    Err(e) => {
                        extras().update(|x| x.purchasing = false);
                        return Err(e);
                    }
                }
                s.await_payment(0, Box::new(done));
                Ok(())
            });
            return;
        }
        // Only the demo grants a plan without a store or a payment provider. A real account
        // that isn't registered yet has nothing to buy it for.
        if !super::demo() {
            extras().update(|e| e.purchasing = false);
            crate::ui::toast("Finish creating your account first.");
            return;
        }
        set_timeout(Duration::from_millis(900), move || {
            s.settings_update(|x| x.plan = plan);
            extras().update(|e| {
                e.purchasing = false;
                e.billing = billing;
                e.cancels_at_period_end = false;
                if plan == Plan::Family && e.family.is_empty() {
                    e.family = vec![FamilySeat {
                        id: 1,
                        name: "Ana Popescu".into(),
                        pending: false,
                        link: None,
                    }];
                }
            });
            done();
        });
    }

    /// Asks the store (or, off the stores, the EU provider) for a subscription bought on another
    /// device or before a reinstall, and fetches this month's tokens on it. Returns whether
    /// this account has a plan now; the refresh that follows shows a restored one.
    pub fn restore_purchases(&self) -> bool {
        if live_ready() && crate::backend::plans::in_store() {
            let s = *self;
            crate::backend::run(async move {
                if crate::backend::plans::restore_from_store().await? {
                    crate::backend::plans::renew().await?;
                } else {
                    crate::ui::toast("This store account has no Songbird subscription.");
                }
                s.refresh_plan();
                Ok(())
            });
        } else if live_ready() {
            let s = *self;
            crate::backend::run(async move {
                if crate::backend::plans::subscription().await?.is_some() {
                    crate::backend::plans::renew().await?;
                }
                s.refresh_plan();
                Ok(())
            });
        }
        self.plan() != Plan::Free
    }

    /// After a store purchase: fetches this month's tokens, retrying every 5 s (for up to
    /// 15 minutes) while billing hasn't confirmed the purchase with the store yet.
    fn await_store(&self, attempt: u32, done: Box<dyn FnOnce()>) {
        let s = *self;
        crate::backend::run(async move {
            match crate::backend::plans::renew().await {
                Ok(()) => {
                    extras().update(|e| e.purchasing = false);
                    s.refresh_plan();
                    done();
                }
                Err(e) if crate::backend::plans::not_paid_yet(&e) && attempt < 180 => {
                    set_timeout(Duration::from_secs(5), move || {
                        s.await_store(attempt + 1, done)
                    });
                }
                Err(e) => {
                    extras().update(|x| x.purchasing = false);
                    return Err(e);
                }
            }
            Ok(())
        });
    }

    /// Polls billing every 5 s (for up to 15 minutes) until the payment is confirmed, then
    /// redeems this month's token and refreshes the plan.
    fn await_payment(&self, attempt: u32, done: Box<dyn FnOnce()>) {
        let s = *self;
        set_timeout(Duration::from_secs(5), move || {
            crate::backend::run(async move {
                let today = crate::backend::plans::day_of(nsh_core::time::unix_ms() as u64);
                let paid = crate::backend::plans::subscription()
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|x| !x.revoked && x.paid_through >= today);
                if paid {
                    let r = crate::backend::plans::renew().await;
                    extras().update(|e| e.purchasing = false);
                    r?;
                    s.refresh_plan();
                    done();
                } else if attempt < 180 && extras().with_untracked(|e| e.purchasing) {
                    s.await_payment(attempt + 1, done);
                } else {
                    extras().update(|e| e.purchasing = false);
                }
                Ok(())
            });
        });
    }

    /// Reads the plan and standing from the server (real backend); redeems a token kept for
    /// this month and a Family seat's token first.
    pub fn refresh_plan(&self) {
        if !live_ready() {
            return;
        }
        let s = *self;
        crate::backend::run(async move {
            use crate::backend::plans;
            plans::redeem_kept().await?;
            // Vault day passes, fetched ahead and away from any backup run (D-0134).
            if s.settings.with_untracked(|x| {
                x.backup_on && x.backup_destination == super::BackupDestination::SongbirdServers
            }) {
                let _ = crate::backend::backup::prefetch_passes().await;
            }
            if plans::has_seat() {
                plans::claim_seat().await?;
            }
            let e = plans::entitlement().await?;
            let plan = match e.level.as_str() {
                "plus" if plans::family().is_some() => Plan::Family,
                "plus" => Plan::Plus,
                _ => Plan::Free,
            };
            s.settings_update(|x| x.plan = plan);
            extras().update(|x| {
                x.last_day = e.last_day;
                x.standing = match e.standing.as_str() {
                    "limited" => Standing::Limited,
                    "suspended" => Standing::Suspended,
                    _ => Standing::Good,
                };
                if let Some(f) = plans::family() {
                    x.family = f
                        .seats
                        .iter()
                        .enumerate()
                        .map(|(i, seat)| FamilySeat {
                            id: i as u64 + 1,
                            name: seat.name.clone(),
                            pending: false,
                            link: Some(seat.link.clone()),
                        })
                        .collect();
                }
            });
            Ok(())
        });
    }

    /// Joins someone's family with the link they sent (real backend).
    pub fn join_family(&self, link: &str) {
        let s = *self;
        let link = link.to_owned();
        crate::backend::run(async move {
            crate::backend::plans::join_family(&link)?;
            s.refresh_plan();
            Ok(())
        });
    }

    /// Leaves the family this device holds a seat in.
    pub fn leave_family(&self) {
        let s = *self;
        crate::backend::run(async move {
            crate::backend::plans::leave_family().await?;
            s.refresh_plan();
            Ok(())
        });
    }

    /// Turns off renewal: the plan stays until the end of the period (billing's `cancel`).
    pub fn cancel_renewal(&self) {
        if live_ready() {
            crate::backend::run(async move {
                crate::backend::plans::cancel().await?;
                extras().update(|e| e.cancels_at_period_end = true);
                Ok(())
            });
            return;
        }
        if super::demo() {
            extras().update(|e| e.cancels_at_period_end = true);
        }
    }

    /// Turns renewal back on while the paid period lasts (D-0169): billing starts a new
    /// subscription whose first charge is the day the period would have renewed.
    pub fn resume_renewal(&self) {
        if live_ready() {
            crate::backend::run(async move {
                crate::backend::plans::resume().await?;
                extras().update(|e| e.cancels_at_period_end = false);
                crate::ui::toast(
                    "Renewal is back on. Nothing is charged before your plan would renew.",
                );
                Ok(())
            });
            return;
        }
        if super::demo() {
            extras().update(|e| e.cancels_at_period_end = false);
        }
    }

    /// Invites someone to the Family plan (up to six people in all): billing makes the seat
    /// and its link to share. The demo adds a pending seat.
    pub fn family_invite(&self, name: String) {
        if live_ready() {
            let s = *self;
            crate::backend::run(async move {
                crate::backend::plans::family_invite(&name).await?;
                s.refresh_plan();
                Ok(())
            });
            return;
        }
        if !super::demo() {
            return;
        }
        let id = nsh_core::time::unix_ms() as u64;
        extras().update(|e| {
            if e.family.len() < 5 {
                e.family.push(FamilySeat {
                    id,
                    name,
                    pending: true,
                    link: None,
                });
            }
        });
    }

    pub fn family_remove(&self, id: u64) {
        let link = extras().with_untracked(|e| {
            e.family
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.link.clone())
        });
        if let (true, Some(link)) = (live_ready(), link) {
            let s = *self;
            crate::backend::run(async move {
                crate::backend::plans::family_remove(&link).await?;
                s.refresh_plan();
                Ok(())
            });
            return;
        }
        if super::demo() {
            extras().update(|e| e.family.retain(|m| m.id != id));
        }
    }

    // ------------------------------------------------------------ support

    /// Sends a support request, sealed to the support team (`backend::plans::open_ticket`).
    /// Logs are attached only when the user said so, and only the scrubbed form they
    /// previewed. `done` hears whether the server has it; nothing is said sent before.
    pub fn send_support(
        &self,
        topic: &str,
        message: &str,
        logs: Option<String>,
        attach_account: bool,
        done: impl FnOnce(bool) + 'static,
    ) {
        if !live_ready() {
            // The demo pretends; a real account not registered yet has no one to send to.
            let demo = super::demo();
            if demo {
                extras().update(|e| e.support_sent += 1);
            }
            done(demo);
            return;
        }
        let kind = if topic == "Billing" {
            "billing"
        } else {
            "help"
        };
        let text = format!("[{topic}] {message}");
        crate::backend::run(async move {
            let r =
                crate::backend::plans::open_ticket(kind, &text, logs.as_deref(), attach_account)
                    .await;
            if r.is_ok() {
                extras().update(|e| e.support_sent += 1);
            }
            done(r.is_ok());
            r.map(drop)
        });
    }

    /// Appeals a limit or suspension: a ticket of kind `appeal`, signed (it names the account,
    /// which the appeal is about).
    /// `done` hears whether the server has it.
    pub fn appeal(&self, message: &str, done: impl FnOnce(bool) + 'static) {
        if !live_ready() {
            let demo = super::demo();
            if demo {
                extras().update(|e| e.support_sent += 1);
            }
            done(demo);
            return;
        }
        let text = message.to_owned();
        crate::backend::run(async move {
            let r = crate::backend::plans::open_ticket("appeal", &text, None, true).await;
            if r.is_ok() {
                extras().update(|e| e.support_sent += 1);
            }
            done(r.is_ok());
            r.map(drop)
        });
    }

    // ------------------------------------------------------------ announcement

    /// Whether the announcement `id` was dismissed (kept across runs).
    pub fn announcement_dismissed(&self, id: &str) -> bool {
        Storage::get_item(ANNOUNCEMENT_KEY).is_some_and(|v| v == id)
    }

    pub fn dismiss_announcement(&self, id: &str) {
        if let Err(e) = Storage::set_item(ANNOUNCEMENT_KEY, id) {
            log::warn!("{e}");
        }
    }

    // ------------------------------------------------------------ WhatsApp

    pub fn whatsapp_linked(&self) -> bool {
        self.settings.with(|s| s.whatsapp_linked)
    }

    /// The demo's linking: a pairing code and QR show until the "phone" accepts after a few
    /// seconds. There is no WhatsApp bridge yet (Prompt 14), so a real account never links
    /// (its WhatsApp page says so, D-0184).
    pub fn begin_whatsapp_link(&self, done: impl FnOnce() + 'static) {
        if !super::demo() {
            return;
        }
        let s = *self;
        extras().update(|e| e.whatsapp_link = WhatsAppLink::Pairing);
        set_timeout(Duration::from_millis(3500), move || {
            if extras().with_untracked(|e| e.whatsapp_link == WhatsAppLink::Pairing) {
                s.settings_update(|x| x.whatsapp_linked = true);
                extras().update(|e| e.whatsapp_link = WhatsAppLink::Idle);
                done();
            }
        });
    }

    pub fn cancel_whatsapp_link(&self) {
        extras().update(|e| e.whatsapp_link = WhatsAppLink::Idle);
    }

    /// Unlinks the demo's WhatsApp at once (a real account has nothing linked).
    pub fn unlink_whatsapp(&self) {
        self.settings_update(|x| x.whatsapp_linked = false);
    }

    /// Whether WhatsApp chats show in All chats (else in their own filter).
    pub fn whatsapp_in_all(&self) -> bool {
        !extras().with(|e| e.whatsapp_separate)
    }

    pub fn set_whatsapp_separate(&self, separate: bool) {
        extras().update(|e| e.whatsapp_separate = separate);
    }

    // ------------------------------------------------------------ desktop

    /// The user's own "Open at login" switch: the system's login item
    /// (NSH 0.6: `SMAppService` on macOS, the Run key on Windows, an XDG
    /// autostart entry on Linux). Shown at once; put back, with a toast, if
    /// the system refuses. On macOS the system may then wait for the user's
    /// approval in Login Items.
    pub fn set_launch_at_login(&self, on: bool) {
        self.settings_update(|x| x.launch_at_login = on);
        let s = *self;
        spawn(async move {
            if nsh_core::element::desktop::set_launch_at_login(on)
                .await
                .is_err()
            {
                s.settings_update(|x| x.launch_at_login = !on);
                crate::ui::toast(if on {
                    "Songbird could not be set to open at login"
                } else {
                    "Songbird could not be removed from login items"
                });
            }
        });
    }

    /// Reads the login item as the system has it now (the user may change
    /// it in the system's settings): call when the switch shows.
    pub fn refresh_launch_at_login(&self) {
        let s = *self;
        spawn(async move {
            let on = nsh_core::element::desktop::launch_at_login().await;
            s.settings_update(|x| x.launch_at_login = on);
        });
    }

    /// Shows or hides the tray / menu bar icon. While it shows, closing the
    /// window keeps Songbird running there (D-0098). Remembered.
    pub fn set_tray_hidden(&self, hidden: bool) {
        extras().update(|e| e.tray_hidden = hidden);
        if hidden {
            if let Err(e) = Storage::set_item(TRAY_HIDDEN_KEY, "1") {
                log::warn!("tray choice not saved: {e}");
            }
        } else {
            Storage::remove_item(TRAY_HIDDEN_KEY);
        }
    }

    /// The tray choice saved by an earlier run.
    pub fn restore_tray_choice(&self) {
        let hidden = Storage::get_item(TRAY_HIDDEN_KEY).is_some();
        extras().update(|e| e.tray_hidden = hidden);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_are_stable_and_readable() {
        let a = invite_code(42, 0);
        assert_eq!(a, invite_code(42, 0));
        assert_ne!(a, invite_code(42, 1));
        assert_eq!(a.len(), "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX".len());
        // What the invite step accepts, unchanged by its cleaning.
        let clean = super::super::ext_onboarding::normalize_invite(&a);
        assert_eq!(super::super::ext_onboarding::format_invite(&clean), a);
        for ch in clean.chars() {
            assert!(!"ILOU".contains(ch), "{a} is not Crockford");
        }
    }

    #[test]
    fn personal_link_is_lowercase_without_at() {
        assert_eq!(personal_link("@Radu"), "https://songbird.chat/i/radu");
    }
}
