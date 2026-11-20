//! Network usage (You → Storage and data → Network usage): every byte this device sends and
//! receives, counted where it crosses the network, by what it was for, and kept on this device
//! (NSH storage) until the user resets it.
//!
//! - **Messages**: the delivery connection's frames (messages, receipts, typing, presence,
//!   nudges, group messages) and every other `api` request and answer.
//! - **Media**: attachments, packs, GIFs, map tiles and avatars (their `api` paths and the
//!   storage URLs they redirect to).
//! - **Calls**: what WebRTC counted on each call's transports, added when the call ends.

use super::vault::Storage;

use crate::store::ext_groups::{NetworkUsage, groups_state};

const K_USAGE: &str = "sb.network_usage";

/// What traffic was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Messages,
    Media,
    Calls,
}

/// The lane of an HTTP request, by its URL.
#[must_use]
pub fn lane_of(url: &str) -> Lane {
    let media = [
        "/v1/attachments",
        "/v1/packs",
        "/v1/gifs",
        "/v1/maps",
        "/v1/profile/avatar",
        "/v1/media",
    ];
    if media.iter().any(|p| url.contains(p)) || !url.contains("/v1/") {
        Lane::Media
    } else {
        Lane::Messages
    }
}

fn today() -> String {
    let days = (nsh_core::time::unix_ms() / 86_400_000.0).floor();
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    #[allow(clippy::cast_possible_truncation, reason = "a day count")]
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let name = usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("");
    format!("{d} {name} {y}")
}

fn zero() -> NetworkUsage {
    NetworkUsage {
        messages_sent: 0,
        messages_received: 0,
        media_sent: 0,
        media_received: 0,
        calls_sent: 0,
        calls_received: 0,
        since: today(),
    }
}

/// What was counted on this device (from its storage).
#[must_use]
pub fn load() -> NetworkUsage {
    let Some(v) = Storage::get_item(K_USAGE) else {
        return zero();
    };
    let mut parts = v.splitn(7, '|');
    let mut n = || {
        parts
            .next()
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let (ms, mr, ds, dr, cs, cr) = (n(), n(), n(), n(), n(), n());
    let since = v.rsplit('|').next().unwrap_or_default().to_string();
    NetworkUsage {
        messages_sent: ms,
        messages_received: mr,
        media_sent: ds,
        media_received: dr,
        calls_sent: cs,
        calls_received: cr,
        since: if since.is_empty() { today() } else { since },
    }
}

fn save(u: &NetworkUsage) {
    let v = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        u.messages_sent,
        u.messages_received,
        u.media_sent,
        u.media_received,
        u.calls_sent,
        u.calls_received,
        u.since
    );
    let _ = Storage::set_item(K_USAGE, v);
}

/// Counts traffic in `lane` (only for a real account: the demo shows its own figures).
pub fn count(lane: Lane, sent: usize, received: usize) {
    if crate::store::demo() || (sent == 0 && received == 0) {
        return;
    }
    let (s, r) = (sent as u64, received as u64);
    let state = groups_state();
    state.update(|g| {
        let u = &mut g.usage;
        match lane {
            Lane::Messages => {
                u.messages_sent += s;
                u.messages_received += r;
            }
            Lane::Media => {
                u.media_sent += s;
                u.media_received += r;
            }
            Lane::Calls => {
                u.calls_sent += s;
                u.calls_received += r;
            }
        }
    });
    state.with_untracked(|g| save(&g.usage));
}

/// Starts counting again from today.
pub fn reset() -> NetworkUsage {
    let z = zero();
    save(&z);
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_fall_in_their_lanes() {
        assert_eq!(lane_of("https://api.x/v1/attachments/ab"), Lane::Media);
        assert_eq!(lane_of("https://s3.x/bucket/obj?sig=1"), Lane::Media);
        assert_eq!(lane_of("https://api.x/v1/keys/fetch"), Lane::Messages);
        assert!(today().split(' ').count() == 3);
    }
}
