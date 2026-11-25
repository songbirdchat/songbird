//! The device's network route (NSH 0.7 `network()`, D-0188): which auto-download choice
//! applies on the link in use, and a delivery connection made again at once when the route
//! changes, instead of a dead socket held until the client's idle limit.

use nsh_core::{
    platform::{Network, NetworkKind},
    prelude::*,
};

/// Whether the mobile-data auto-download choice applies on `link` (Settings → Storage and
/// data): a link that costs the user (cellular data, a phone's hotspot, a metered network) or
/// one they asked to save data on (Low Data Mode, Data Saver). Wi-Fi and wired links take the
/// Wi-Fi choice. Where the platform doesn't say (Windows and Linux report no route), phones keep
/// the mobile choice and desktops the Wi-Fi one, as before NSH could tell them apart.
pub(crate) fn mobile_choice(link: Network, desktop: bool) -> bool {
    if link.expensive || link.constrained {
        return true;
    }
    match link.kind {
        NetworkKind::Cellular => true,
        NetworkKind::Wifi | NetworkKind::Wired => false,
        _ => !desktop,
    }
}

/// Whether going from `before` to `now` calls for a new delivery connection: back online, or
/// online over another kind of link (Wi-Fi handing over to cellular), whose old socket is dead
/// even though nothing has said so yet.
pub(crate) fn reconnects(before: Network, now: Network) -> bool {
    now.online()
        && (!before.online() || (before.kind != now.kind && before.kind != NetworkKind::Unknown))
}

/// Makes the delivery connection again when [`reconnects`] says so. Called once at the root:
/// its effect belongs to it.
pub(crate) fn follow() {
    let last = std::cell::Cell::new(None::<Network>);
    effect(move || {
        let now = network();
        untracked(|| {
            let before = last.replace(Some(now));
            if let Some(before) = before
                && reconnects(before, now)
                && super::is_live()
                && super::registered()
            {
                super::connect();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(kind: NetworkKind) -> Network {
        Network::new(kind)
    }

    #[test]
    fn the_link_picks_the_choice() {
        for desktop in [false, true] {
            assert!(!mobile_choice(link(NetworkKind::Wifi), desktop));
            assert!(!mobile_choice(link(NetworkKind::Wired), desktop));
            assert!(mobile_choice(link(NetworkKind::Cellular), desktop));
            // A hotspot over Wi-Fi costs the user; Low Data Mode asks to save.
            assert!(mobile_choice(
                link(NetworkKind::Wifi).expensive(true),
                desktop
            ));
            assert!(mobile_choice(
                link(NetworkKind::Wifi).constrained(true),
                desktop
            ));
        }
        // Unknown: the rule from before NSH 0.7.
        assert!(mobile_choice(link(NetworkKind::Unknown), false));
        assert!(!mobile_choice(link(NetworkKind::Unknown), true));
    }

    #[test]
    fn a_new_route_reconnects() {
        let (wifi, cell, none, unknown) = (
            link(NetworkKind::Wifi),
            link(NetworkKind::Cellular),
            link(NetworkKind::None),
            link(NetworkKind::Unknown),
        );
        assert!(reconnects(none, wifi));
        assert!(reconnects(wifi, cell));
        assert!(!reconnects(wifi, wifi));
        assert!(!reconnects(wifi, none));
        // The first answer of the platform's monitor is no change of route.
        assert!(!reconnects(unknown, wifi));
        // Only cost or Low Data Mode changed: the same socket still works.
        assert!(!reconnects(wifi, wifi.constrained(true)));
    }
}
