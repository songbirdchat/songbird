//! The user's own presence (Prompt 9, M6; PROTOCOL §10.5, D-0104).
//!
//! One [`Presencer`] (`songbird_content::presence`) per app holds the
//! sender's rules: who may see what, debouncing, never the same state
//! twice, and **invisible sends nothing at all**. It is built from the
//! user's settings (online and last seen audiences, the presence choice),
//! hears the app come to the foreground and go to the background from NSH's
//! `lifecycle()`, and the choice from `Store::set_presence`.
//!
//! What it decides to send goes as one sealed batch to every allowed
//! contact's devices (`PUT /v1/ephemeral/sealed`, no device signature, the
//! XOR of the recipients' access keys in `Unidentified-Access-Key`), and
//! only while the account is live.
//!
//! The contacts (their accounts, devices and access keys) and the sealed-sender encryption of
//! each payload come from app-core (`Messenger::presence_peers`, `Messenger::seal_for`): only
//! contacts who shared their profile key are addressed, and nothing leaves in the clear.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    time::Duration,
};

use nsh_core::{platform::HttpRequest, platform::Lifecycle, prelude::*, time::unix_ms};
use songbird_content::presence::{Audience, Choice, Outgoing, Presence, Presencer};
use songbird_proto::ephemeral::SealedBatch;
use songbird_types::DeviceId;

use crate::store::{self, Settings, SettingsExt, Store};

/// A contact the backend can reach: their account, devices and the access
/// key derived from their profile key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub account: [u8; 16],
    pub devices: Vec<[u8; 16]>,
    pub access_key: [u8; 16],
}

thread_local! {
    /// The contacts presence reaches, as app-core last listed them.
    static PEERS: RefCell<Vec<Peer>> = const { RefCell::new(Vec::new()) };
}

/// The contacts presence reaches (who shared their profile key), as last read.
pub fn peers(_store: Store) -> Vec<Peer> {
    PEERS.with_borrow(Clone::clone)
}

/// Reads the reachable contacts from app-core again (when it opens, and when a chat with a new
/// contact begins), and tells the presence rules who they are.
pub fn refresh(s: Store) {
    super::run(async move {
        let got = {
            let lock = super::chat::core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else {
                return Ok(());
            };
            m.presence_peers().await.unwrap_or_default()
        };
        let list: Vec<Peer> = got
            .into_iter()
            .map(|(account, devices, access_key)| Peer {
                account,
                devices,
                access_key,
            })
            .collect();
        let to: BTreeSet<[u8; 16]> = list.iter().map(|p| p.account).collect();
        PEERS.with_borrow_mut(|p| *p = list);
        LANE.with(|l| {
            if let Some(lane) = l.borrow_mut().as_mut() {
                lane.set_contacts(to);
            }
        });
        pump(s);
        Ok(())
    });
}

/// A contact's presence arrived: shown on them, and answered once if they asked (§10.5).
pub fn received(account: [u8; 16], p: &Presence) {
    let Some(s) = store::try_store() else { return };
    let contact = super::chat::ui_id(&account);
    let shown = match songbird_content::presence::State::try_from(p.state) {
        Ok(songbird_content::presence::State::Online) => store::Presence::Online,
        Ok(songbird_content::presence::State::Away) => store::Presence::Away,
        Ok(songbird_content::presence::State::Busy) => store::Presence::Busy,
        _ if p.last_seen_ms > 0 => store::Presence::LastSeen(Some(
            (i64::try_from(p.last_seen_ms).unwrap_or(0) - unix_ms() as i64).div_euclid(60_000),
        )),
        _ => store::Presence::LastSeen(None),
    };
    s.contacts.update(|cs| {
        if let Some(c) = cs.iter_mut().find(|c| c.id == contact) {
            c.presence = shown;
        }
    });
    let reply = LANE.with(|l| {
        l.borrow_mut()
            .as_mut()
            .and_then(|lane| lane.presencer.received(account, p, now_ms()))
    });
    if let Some(out) = reply {
        send(s, &out);
    }
}

/// The user's choice as the presence rules see it.
pub fn choice_of(p: &store::Presence) -> Choice {
    match p {
        store::Presence::Away => Choice::Away,
        store::Presence::Busy => Choice::Busy,
        store::Presence::Invisible => Choice::Invisible,
        store::Presence::Online | store::Presence::LastSeen(_) => Choice::Online,
    }
}

fn audience(a: store::Audience) -> Audience {
    match a {
        store::Audience::Nobody => Audience::Nobody,
        // Only contacts hold the access keys: "Everyone" reaches contacts.
        store::Audience::Everyone | store::Audience::Contacts => Audience::Contacts,
    }
}

/// (online, last seen) audiences from the privacy settings; "Same as last
/// seen" makes online follow last seen.
pub fn audiences(s: &Settings, x: &SettingsExt) -> (Audience, Audience) {
    let last = audience(s.last_seen);
    let online = if x.online_same_as_last_seen {
        last
    } else {
        audience(s.online)
    };
    (online, last)
}

/// The presence rules with what the app last told them.
pub struct Lane {
    pub(crate) presencer: Presencer,
    audiences: (Audience, Audience),
}

impl Lane {
    pub fn new(choice: Choice, audiences: (Audience, Audience), to: BTreeSet<[u8; 16]>) -> Self {
        Self {
            presencer: Presencer::new(choice, audiences.0, audiences.1, to),
            audiences,
        }
    }

    pub fn foreground(&mut self, now_ms: u64) {
        self.presencer.foreground(now_ms);
    }

    pub fn background(&mut self, now_ms: u64) {
        self.presencer.background(now_ms);
    }

    pub fn set_choice(&mut self, choice: Choice, now_ms: u64) {
        self.presencer.set_choice(choice, now_ms);
    }

    /// New audiences; the same ones again change nothing (an effect that
    /// re-runs on an unrelated setting).
    pub fn set_audiences(&mut self, a: (Audience, Audience), now_ms: u64) {
        if a != self.audiences {
            self.audiences = a;
            self.presencer.set_audiences(a.0, a.1, now_ms);
        }
    }

    pub fn set_contacts(&mut self, to: BTreeSet<[u8; 16]>) {
        self.presencer.set_contacts(to);
    }

    /// What to send now, and when to look again.
    pub fn poll(&mut self, now_ms: u64) -> (Option<Outgoing>, Option<u64>) {
        let out = self.presencer.poll(now_ms);
        (out, self.presencer.next_poll())
    }
}

/// One sealed batch for `out`: a payload per device of every addressed
/// peer that `seal` could seal for, and the combined access key. `None`
/// when no device could be addressed (nothing to send).
pub fn batch(
    out: &Outgoing,
    peers: &[Peer],
    seal: impl Fn(&Peer, &[u8; 16], &Presence) -> Option<Vec<u8>>,
) -> Option<(SealedBatch, [u8; 16])> {
    let mut events = Vec::new();
    let mut keys = Vec::new();
    for peer in peers.iter().filter(|p| out.to.contains(&p.account)) {
        let before = events.len();
        for d in &peer.devices {
            if let Some(payload) = seal(peer, d, &out.presence) {
                events.push((DeviceId::from_bytes(*d), payload));
            }
        }
        if events.len() > before {
            keys.push(peer.access_key);
        }
    }
    if events.is_empty() {
        return None;
    }
    let key = songbird_keys::sealed::combine(keys.iter());
    Some((SealedBatch { events }, key))
}

thread_local! {
    static LANE: RefCell<Option<Lane>> = const { RefCell::new(None) };
    /// A poll is already waiting on a timer.
    static WAITING: Cell<bool> = const { Cell::new(false) };
}

fn now_ms() -> u64 {
    unix_ms().max(0.0) as u64
}

/// Builds the rules from the settings and follows the app's lifecycle and
/// the privacy settings. Called once at the root (its effects belong to it).
pub fn install(s: Store) {
    let choice = s.me.with_untracked(|p| choice_of(&p.presence));
    let a = audiences(&s.settings.get_untracked(), &s.ext().get_untracked());
    let to = peers(s).iter().map(|p| p.account).collect();
    LANE.with(|l| *l.borrow_mut() = Some(Lane::new(choice, a, to)));
    effect(move || {
        let state = lifecycle();
        untracked(|| {
            let now = now_ms();
            LANE.with(|l| {
                if let Some(lane) = l.borrow_mut().as_mut() {
                    match state {
                        Lifecycle::Active => lane.foreground(now),
                        Lifecycle::Background => lane.background(now),
                        Lifecycle::Inactive => {}
                    }
                }
            });
            pump(s);
        });
    });
    let ext = s.ext();
    effect(move || {
        let a = s.settings.with(|st| ext.with(|x| audiences(st, x)));
        untracked(|| {
            LANE.with(|l| {
                if let Some(lane) = l.borrow_mut().as_mut() {
                    lane.set_audiences(a, now_ms());
                }
            });
            pump(s);
        });
    });
}

/// The user chose online, away, busy or invisible.
pub fn set_choice(s: Store, p: &store::Presence) {
    let choice = choice_of(p);
    let known = LANE.with(|l| {
        l.borrow_mut()
            .as_mut()
            .map(|lane| lane.set_choice(choice, now_ms()))
            .is_some()
    });
    if known {
        pump(s);
    }
}

/// Sends what is due, and comes back when the debounce allows the next.
fn pump(s: Store) {
    let now = now_ms();
    let (out, next) = LANE.with(|l| {
        l.borrow_mut()
            .as_mut()
            .map_or((None, None), |lane| lane.poll(now))
    });
    if let Some(out) = out {
        send(s, &out);
    }
    if let Some(at) = next
        && !WAITING.with(Cell::get)
    {
        WAITING.with(|w| w.set(true));
        set_timeout(
            Duration::from_millis(at.saturating_sub(now).max(1)),
            move || {
                WAITING.with(|w| w.set(false));
                pump(s);
            },
        );
    }
}

/// `PUT /v1/ephemeral/sealed` with the batch for `out`, when live: each addressed device's
/// payload sealed by app-core first.
fn send(s: Store, out: &Outgoing) {
    if !(super::is_live() && super::registered()) {
        return;
    }
    let peers = peers(s);
    let out = out.clone();
    let api = super::endpoint().api;
    spawn(async move {
        let Ok(content) =
            songbird_app_core::messenger::Messenger::<super::chat::AppNet>::presence_content(
                &out.presence,
            )
        else {
            return;
        };
        let mut sealed: std::collections::HashMap<([u8; 16], [u8; 16]), Vec<u8>> =
            std::collections::HashMap::new();
        {
            let lock = super::chat::core();
            let mut guard = lock.lock().await;
            let Some(m) = guard.as_mut() else { return };
            for p in peers.iter().filter(|p| out.to.contains(&p.account)) {
                for d in &p.devices {
                    if let Ok(b) = m.seal_for(&p.account, d, &content).await {
                        sealed.insert((p.account, *d), b);
                    }
                }
            }
        }
        let Some((batch, key)) = batch(&out, &peers, |p, d, _| {
            sealed.get(&(p.account, *d)).cloned()
        }) else {
            return;
        };
        let Ok(body) = batch.encode() else {
            return;
        };
        let request = HttpRequest::new("PUT", format!("{api}/v1/ephemeral/sealed"))
            .header("unidentified-access-key", super::wire::hex(&key))
            .header("content-type", "application/octet-stream")
            .header("content-length", body.len().to_string())
            .body(body);
        // Best effort, like every ephemeral event: always 204, or the whole
        // batch refused; nothing to retry.
        if let Err(p) = super::send(request).await {
            log::debug!("presence: {p}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANA: [u8; 16] = [1; 16];

    fn lane(choice: Choice) -> Lane {
        Lane::new(
            choice,
            (Audience::Contacts, Audience::Contacts),
            BTreeSet::from([ANA]),
        )
    }

    /// Runs the lane like the app's pump: polls at each time and counts sends.
    fn sends(lane: &mut Lane, times: &[u64]) -> Vec<Outgoing> {
        times.iter().filter_map(|t| lane.poll(*t).0).collect()
    }

    #[test]
    fn invisible_sends_nothing_ever() {
        let mut l = lane(Choice::Invisible);
        l.foreground(0);
        assert!(sends(&mut l, &[0, 6_000]).is_empty());
        l.background(10_000);
        l.set_audiences((Audience::Contacts, Audience::Nobody), 11_000);
        assert!(sends(&mut l, &[10_000, 20_000, 60_000]).is_empty());
        // Turning invisible drops what was due.
        let mut l = lane(Choice::Online);
        l.foreground(0);
        l.set_choice(Choice::Invisible, 1);
        assert!(sends(&mut l, &[1, 10_000]).is_empty());
    }

    #[test]
    fn choice_changes_inside_the_debounce_make_one_send_of_the_latest() {
        let mut l = lane(Choice::Online);
        l.foreground(0);
        let first = sends(&mut l, &[0]);
        assert_eq!(first.len(), 1);
        l.set_choice(Choice::Away, 1_000);
        l.set_choice(Choice::Busy, 2_000);
        let (none, next) = l.poll(2_000);
        assert!(none.is_none());
        assert_eq!(next, Some(5_000));
        let after = sends(&mut l, &[5_000, 6_000, 20_000]);
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].presence.state,
            i32::from(songbird_content::presence::State::Busy)
        );
        assert_eq!(after[0].to, BTreeSet::from([ANA]));
    }

    #[test]
    fn the_same_audiences_again_change_nothing() {
        let mut l = lane(Choice::Online);
        l.foreground(0);
        assert_eq!(sends(&mut l, &[0]).len(), 1);
        l.set_audiences((Audience::Contacts, Audience::Contacts), 10_000);
        assert!(sends(&mut l, &[10_000]).is_empty());
    }

    #[test]
    fn a_batch_addresses_only_sealed_devices_and_combines_their_keys() {
        let out = Outgoing {
            presence: Presence {
                state: 1,
                last_seen_ms: 0,
                wants_reply: false,
            },
            to: BTreeSet::from([ANA, [2; 16]]),
        };
        let peers = [
            Peer {
                account: ANA,
                devices: vec![[10; 16], [11; 16]],
                access_key: [0xAA; 16],
            },
            Peer {
                account: [2; 16],
                devices: vec![[20; 16]],
                access_key: [0x0F; 16],
            },
            // Not addressed.
            Peer {
                account: [3; 16],
                devices: vec![[30; 16]],
                access_key: [0xFF; 16],
            },
        ];
        let (b, key) = batch(&out, &peers, |_, _, _| Some(vec![7; 8])).expect("a batch");
        assert_eq!(b.events.len(), 3);
        assert_eq!(key, [0xAA ^ 0x0F; 16]);
        assert!(b.encode().is_ok());
        assert!(batch(&out, &[], |_, _, _| Some(vec![1])).is_none());
    }
}
