//! The delivery connection (`crates/client`): authenticated with this
//! device's key, reconnecting with backoff, its state shown live.
//!
//! Envelopes are end-to-end ciphertext: app-core (`super::chat`, D-0139)
//! decrypts and stores each one, in order, and only then is it acknowledged.
//! One app-core can't store is left on the server and comes again.

use std::{cell::RefCell, rc::Rc};

use nsh_core::prelude::*;
use songbird_client::{Client, ClientConfig, DeviceKey, Event, ResumeState};
use songbird_types::DeviceId;

use super::{Link, device_key, live};

thread_local! {
    /// The running client (dropping it stops it).
    static CLIENT: RefCell<Option<Rc<Client>>> = const { RefCell::new(None) };
    /// The task that starts the client and reads its events.
    static READER: RefCell<Option<Spawned<()>>> = const { RefCell::new(None) };
}

/// Connects (again) with this device's key. Does nothing when the device
/// has no account.
pub fn connect() {
    disconnect();
    let Some((device, seed)) = device_key() else {
        return;
    };
    let endpoint = live().with_untracked(|l| l.endpoint.clone());
    let Ok(key) = DeviceKey::from_seed(DeviceId::from_bytes(device), &seed) else {
        live().update(|l| l.problem = Some("This device's key is damaged.".into()));
        return;
    };
    live().update(|l| {
        l.link = Link::Connecting;
        l.waiting = 0;
    });
    let plain = is_loopback(&endpoint.ws_addr);
    let mut cfg = ClientConfig::new(endpoint.ws_addr, endpoint.ws_name);
    // The proxy setting (Prompt 9): TLS to the proxy, the server's own TLS
    // inside it. A proxy only carries TLS, so the endpoint is reached over TLS.
    let proxy =
        crate::store::try_store().and_then(|s| s.settings.with_untracked(|x| x.proxy.clone()));
    if let Some(host) = proxy {
        let route =
            songbird_types::proxy_link::ProxyLink::parse(&format!("songbird://proxy#{host}"), &[])
                .ok();
        match (route, songbird_client::web_tls()) {
            (Some(link), Some(tls)) => {
                cfg.proxy = Some(songbird_client::ProxyRoute::from_link(&link, tls.clone()));
                cfg.tls = Some(tls);
            }
            _ => {
                live().update(|l| {
                    l.problem = Some("The proxy address isn't valid. Check it in Proxy.".into())
                });
                return;
            }
        }
    } else if !plain {
        // TLS 1.3 to anything but this machine: the device signs the TLS
        // exporter (D-0018), so delivery itself must terminate it.
        match songbird_client::web_tls() {
            Some(tls) => cfg.tls = Some(tls),
            None => {
                live().update(|l| {
                    l.link = Link::Off;
                    l.problem = Some("This device can't set up a secure connection.".into());
                });
                return;
            }
        }
    }
    // Right after registration delivery's device directory may lag a poll.
    cfg.new_device_grace = std::time::Duration::from_secs(10);
    // No owner: the connection lives until `disconnect`, not with the view that
    // started it (the on/off switch re-renders its page).
    let task = Owner::detached().run(|| {
        spawn(async move {
            // The client spawns on tokio; NSH's `background` runs on its tokio
            // runtime, and the client's tasks stay there.
            let Ok((client, mut events)) =
                background(async move { Client::start(cfg, key, ResumeState::default()) }).await
            else {
                live().update(|l| l.link = Link::Off);
                return;
            };
            CLIENT.with_borrow_mut(|c| *c = Some(Rc::new(client)));
            while let Some(event) = events.recv().await {
                match event {
                    Event::Connected { .. } => {
                        live().update(|l| {
                            l.link = Link::Connected;
                            l.problem = None;
                        });
                        if let Err(p) = super::chat::start().await {
                            live().update(|l| l.problem = Some(p.0));
                        } else {
                            // What failed for want of a connection goes now, by itself.
                            super::chat::resend_waiting();
                        }
                    }
                    // Stored by app-core, then acknowledged; in `seq` order.
                    Event::Deliver { seq, envelope } => {
                        super::usage::count(super::usage::Lane::Messages, 0, envelope.len() + 16);
                        if super::chat::receive(&envelope).await {
                            if let Some(c) = self::client() {
                                let _ = c.ack(seq).await;
                            }
                        } else {
                            live().update(|l| l.waiting += 1);
                            retry_waiting();
                        }
                    }
                    // Typing and other ephemeral events: decrypted, never stored or acknowledged.
                    Event::Ephemeral { payload } => {
                        super::usage::count(super::usage::Lane::Messages, 0, payload.len() + 8);
                        let _ = super::chat::receive(&payload).await;
                    }
                    // Nudges: taken only from the attention class (§10.5).
                    Event::Attention { payload } => {
                        super::usage::count(super::usage::Lane::Messages, 0, payload.len() + 8);
                        super::chat::receive_attention(&payload).await;
                    }
                    Event::Disconnected { .. } => live().update(|l| l.link = Link::Retrying),
                    Event::Failed { code } => {
                        live().update(|l| {
                            l.link = Link::Refused(code);
                            l.problem = Some(format!(
                                "The server closed the connection for this device (code {code})."
                            ));
                        });
                        break;
                    }
                    _ => {}
                }
            }
        })
    });
    READER.with_borrow_mut(|r| *r = Some(task));
}

/// Whether `host:port` names this machine, the only place plain TCP is allowed
/// (development against `scripts/dev-app-backend.sh`).
fn is_loopback(addr: &str) -> bool {
    let host = addr.rsplit_once(':').map_or(addr, |(h, _)| h);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The running delivery client.
pub(crate) fn client() -> Option<Rc<Client>> {
    CLIENT.with_borrow(Clone::clone)
}

/// Closes the connection.
/// A delivery that couldn't be stored stays on the server, which sends it again on the next
/// connection: reconnect once, quietly, after a pause (several failures, one reconnect), so a
/// passing failure doesn't wait for the user's Reconnect.
fn retry_waiting() {
    thread_local! {
        static PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if PENDING.replace(true) {
        return;
    }
    Owner::detached().run(|| {
        set_timeout(std::time::Duration::from_secs(20), || {
            PENDING.set(false);
            if live().with_untracked(|l| l.waiting > 0 && l.link == Link::Connected) {
                connect();
            }
        });
    });
}

pub fn disconnect() {
    if let Some(t) = READER.with_borrow_mut(Option::take) {
        t.cancel();
    }
    drop(CLIENT.with_borrow_mut(Option::take));
    if live().with_untracked(|l| l.link != Link::Off) {
        live().update(|l| l.link = Link::Off);
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback;

    #[test]
    fn plain_tcp_only_to_this_machine() {
        for a in [
            "127.0.0.1:9201",
            "localhost:9201",
            "LOCALHOST:1",
            "[::1]:9201",
            "127.9.9.9:1",
        ] {
            assert!(is_loopback(a), "{a}");
        }
        for a in [
            "ws.songbird.chat:443",
            "162.55.161.212:443",
            "[2a01:4f8::1]:443",
            "10.0.0.1:9201",
            "localhost.evil.com:443",
        ] {
            assert!(!is_loopback(a), "{a}");
        }
    }
}
