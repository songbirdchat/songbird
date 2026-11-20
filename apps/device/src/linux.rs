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
