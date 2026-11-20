//! Linux: the vault key in the Secret Service (GNOME Keyring, KWallet), over D-Bus with the
//! service's encrypted session (D-0172). No `unsafe`.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use secret_service::{EncryptionType, blocking::SecretService};
use zeroize::Zeroizing;

use crate::secure::{Error, fresh};

fn attrs() -> HashMap<&'static str, &'static str> {
    HashMap::from([("application", "chat.songbird.app"), ("name", "vault")])
}

pub(crate) fn vault_key() -> Result<Zeroizing<[u8; 32]>, Error> {
    // D-Bus calls block: never on the caller's runtime.
    std::thread::spawn(|| {
        let ss = SecretService::connect(EncryptionType::Dh).map_err(|_| Error::Unavailable)?;
        let found = ss.search_items(attrs()).map_err(|_| Error::Unavailable)?;
        let item = found.unlocked.into_iter().next().or_else(|| {
            let locked = found.locked.into_iter().next()?;
            locked.unlock().ok()?;
            Some(locked)
        });
        if let Some(item) = item {
            let s = Zeroizing::new(item.get_secret().map_err(|_| Error::Refused)?);
            let k: [u8; 32] = s.as_slice().try_into().map_err(|_| Error::Refused)?;
            return Ok(Zeroizing::new(k));
        }
        let k = fresh()?;
        let collection = ss
            .get_default_collection()
            .map_err(|_| Error::Unavailable)?;
        if collection.is_locked().unwrap_or(true) {
            collection.unlock().map_err(|_| Error::Unavailable)?;
        }
        collection
            .create_item(
                "Songbird",
                attrs(),
                &k[..],
                true,
                "application/octet-stream",
            )
            .map_err(|_| Error::Unavailable)?;
        Ok(k)
    })
    .join()
    .unwrap_or(Err(Error::Unavailable))
}

// ---------------------------------------------------------------- location (D-0174)

/// One position from GeoClue (street accuracy), with the app's desktop ID; GeoClue's agent
/// asks the person. Gives up after 30 seconds.
pub(crate) fn location(done: crate::location::Done) {
    use crate::location::{Failure, Position};
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedObjectPath;
    fn un<E>(_: E) -> Failure {
        Failure::Unavailable
    }
    std::thread::spawn(move || {
        let run = || -> Result<Position, Failure> {
            let conn = Connection::system().map_err(un)?;
            let manager = Proxy::new(
                &conn,
                "org.freedesktop.GeoClue2",
                "/org/freedesktop/GeoClue2/Manager",
                "org.freedesktop.GeoClue2.Manager",
            )
            .map_err(un)?;
            let client_path: OwnedObjectPath = manager.call("GetClient", &()).map_err(un)?;
            let client = Proxy::new(
                &conn,
                "org.freedesktop.GeoClue2",
                client_path,
                "org.freedesktop.GeoClue2.Client",
            )
            .map_err(un)?;
            client
                .set_property("DesktopId", "chat.songbird.app")
                .map_err(un)?;
            // 6: street level.
            client
                .set_property("RequestedAccuracyLevel", 6u32)
                .map_err(un)?;
            let mut updates = client.receive_signal("LocationUpdated").map_err(un)?;
            client
                .call_method("Start", &())
                .map_err(|_| Failure::Denied)?;
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                if let Some(m) = updates.next() {
                    let _ = tx.send(m);
                }
            });
            let msg = rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .map_err(un)?;
            let (_old, new): (OwnedObjectPath, OwnedObjectPath) =
                msg.body().deserialize().map_err(un)?;
            let loc = Proxy::new(
                &conn,
                "org.freedesktop.GeoClue2",
                new,
                "org.freedesktop.GeoClue2.Location",
            )
            .map_err(un)?;
            let p = Position {
                latitude: loc.get_property("Latitude").map_err(un)?,
                longitude: loc.get_property("Longitude").map_err(un)?,
                accuracy_m: loc.get_property("Accuracy").map_err(un)?,
            };
            let _ = client.call_method("Stop", &());
            Ok(p)
        };
        done(run());
    });
}
