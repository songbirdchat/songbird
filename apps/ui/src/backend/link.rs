//! Linking a device to the account, both sides (PROTOCOL §4.6; Prompt 3's
//! server, wired in the app by Prompt 9 so a linked device gets the storage
//! key in the provisioning message, version 2).
//!
//! **The new device** opens a provisioning channel, draws an X25519 key, and
//! shows both as a QR code: `SONGBIRD:LINK:<channel hex>:<key hex>`
//! (`features::devices::codes`, D-0111). The main phone reads it only from
//! the live camera, and asks before linking (D-0110).
//! It waits for the sealed provisioning message, opens it, makes its own
//! keys, proves them (Ed25519 and XEdDSA over the transcript, and the
//! confirmation tag under the key only the main phone sent), and links.
//!
//! **The main phone** reads the code, gets a link token, seals the
//! provisioning message (the account, its identity key, the token, the
//! reserved device ID, a fresh confirmation key, the server's name and the
//! storage key) to the new device's key, and waits for the new device to
//! appear, pending. It checks the confirmation tag (the server never saw
//! that key, so it cannot slip in keys of its own) and signs a device list
//! naming it.
//!
//! **Removing a device** signs a list without it. The storage key is rotated
//! when the main phone is left alone; with other devices still linked, the
//! new key must reach them end to end, which app-core's sync messages do
//! (A2), so until then the key stays (D-0106).

use std::time::Duration;

use super::vault::Storage;
use nsh_core::{platform::HttpRequest, prelude::*};
use serde_json::Value;
use songbird_keys::provisioning::{self, ProvisioningKey};
use songbird_keys::xeddsa::Signer;
use songbird_proto::devices::{
    DeviceList, Factor, LINK_AUTH_CONTEXT, LINK_IDENTITY_CONTEXT, LinkRequest, ListedDevice,
    Provision, link_transcript,
};
use songbird_types::{AccountId, DeviceId};

use crate::features::devices::codes;

use super::{Problem, endpoint, explain, live, ok_json, problem, put, send, signed, wire};

const K_DEVICE_IDENTITY: &str = "sb.device_identity_seed";

// ---------------------------------------------------------------- the new device

/// A provisioning channel opened by this (new) device.
#[derive(Debug)]
pub struct Waiting {
    pub channel: Vec<u8>,
    key: ProvisioningKey,
}

impl Waiting {
    /// The code to show.
    #[must_use]
    pub fn code(&self) -> String {
        codes::link_code(&self.channel, &self.key.public())
    }
}

/// `POST /v1/provisioning`: opens a channel.
pub async fn open_channel() -> Result<Waiting, Problem> {
    let ep = endpoint();
    let (status, body) = send(HttpRequest::post(
        format!("{}/v1/provisioning", ep.api),
        Vec::new(),
    ))
    .await?;
    let v = ok_json(status, &body)?;
    let channel = v["channel"]
        .as_str()
        .and_then(wire::unhex)
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let key =
        ProvisioningKey::generate().map_err(|_| problem("This device couldn't make a key."))?;
    Ok(Waiting { channel, key })
}

impl Waiting {
    /// Waits for the sealed message on this channel (up to its ten-minute
    /// life) and opens it with this device's provisioning key.
    async fn receive(&self) -> Result<Vec<u8>, Problem> {
        let ep = endpoint();
        let path = format!("/v1/provisioning/{}", wire::hex(&self.channel));
        // Each read waits up to 25 s on the server.
        for _ in 0..24 {
            let (status, body) = send(HttpRequest::new("GET", format!("{}{path}", ep.api))).await?;
            match status {
                200 => {
                    return provisioning::open(&self.key, &body)
                        .map_err(|_| problem("The message from your phone didn't open."));
                }
                204 => {}
                _ => return Err(explain(status, &body)),
            }
        }
        Err(problem("The code expired. Show a new one."))
    }
}

/// A link waiting for its second factor: the request is signed and the
/// token is still unspent (the server checks the factor before spending it).
pub struct Pending {
    provision: Provision,
    auth_seed: [u8; 32],
    identity_seed: [u8; 32],
    request: LinkRequest,
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending").finish_non_exhaustive()
    }
}

/// Where linking stands on the new device.
#[derive(Debug)]
pub enum Step {
    /// Linked: the account, the device and the storage key are saved here.
    Linked,
    /// The account has a two-step PIN: ask for it ([`with_pin`]).
    NeedsPin(Pending),
    /// The PIN given wasn't right; ask again.
    WrongPin(Pending),
}

/// Waits for the main phone's message, then links this device with keys of
/// its own (or asks for the account's PIN first).
pub async fn complete(w: Waiting) -> Result<Step, Problem> {
    let ep = endpoint();
    let plain = w.receive().await?;
    let p = Provision::decode(&plain)
        .map_err(|_| problem("The message from your phone was damaged."))?;
    if p.server_name != ep.api_name {
        return Err(problem(
            "Your phone uses another Songbird server. Check Server connection on both.",
        ));
    }
    let auth_seed: [u8; 32] = wire::random();
    let identity_seed: [u8; 32] = wire::random();
    let request = link_request(&p, &auth_seed, &identity_seed, &ep.api_name)?;
    submit(Pending {
        provision: p,
        auth_seed,
        identity_seed,
        request,
    })
    .await
}

/// Links with the account's two-step PIN (6 to 12 digits).
pub async fn with_pin(mut pending: Pending, pin: String) -> Result<Step, Problem> {
    pending.request.factor = Factor::Pin(pin);
    submit(pending).await
}

async fn submit(pending: Pending) -> Result<Step, Problem> {
    let ep = endpoint();
    let body = pending.request.encode();
    let req = HttpRequest::post(format!("{}/v1/devices/link", ep.api), body.clone())
        .header("content-type", "application/octet-stream")
        .header("content-length", body.len().to_string());
    let (status, answer) = send(req).await?;
    if status == 403 && answer.windows(6).any(|w| w == b"factor") {
        return Ok(if matches!(pending.request.factor, Factor::None) {
            Step::NeedsPin(pending)
        } else {
            Step::WrongPin(pending)
        });
    }
    ok_json(status, &answer)?;
    let Pending {
        provision: p,
        auth_seed,
        identity_seed,
        ..
    } = pending;
    put(super::K_DEVICE_SEED, &wire::hex(&auth_seed));
    put(K_DEVICE_IDENTITY, &wire::hex(&identity_seed));
    let account = p.account.to_hex_unredacted();
    let device = p.device.to_hex_unredacted();
    put(super::K_ACCOUNT, &account);
    put(super::K_DEVICE, &device);
    if let Some(k) = p.storage_key {
        super::records::adopt_key(k);
    }
    live().update(|l| {
        l.account = Some(account);
        l.device = Some(device);
    });
    super::connect();
    Ok(Step::Linked)
}

/// The link request for provisioning `p`, signed by the new keys.
fn link_request(
    p: &Provision,
    auth_seed: &[u8; 32],
    identity_seed: &[u8; 32],
    server: &str,
) -> Result<LinkRequest, Problem> {
    let auth =
        wire::ed25519(auth_seed).ok_or_else(|| problem("This device couldn't make a key."))?;
    let ident = Signer::new(identity_seed);
    let mut identity_key = [0u8; 33];
    identity_key[0] = 0x05;
    identity_key[1..].copy_from_slice(&ident.public());
    let mut r = LinkRequest {
        token: p.token,
        device: p.device,
        auth_key: wire::public(&auth),
        identity_key,
        auth_sig: [0; 64],
        identity_sig: [0; 64],
        confirm: [0; 32],
        factor: Factor::None,
    };
    let tr = |context: &[u8], r: &LinkRequest| {
        link_transcript(
            context,
            server,
            &r.token,
            &r.device,
            &r.auth_key,
            &r.identity_key,
        )
        .map_err(|_| problem("The server's name is too long."))
    };
    let sig = auth.sign(&tr(LINK_AUTH_CONTEXT, &r)?);
    r.auth_sig.copy_from_slice(sig.as_ref());
    r.identity_sig = ident.sign(&tr(LINK_IDENTITY_CONTEXT, &r)?, &wire::random());
    r.confirm = provisioning::confirm_tag(&p.confirm_key, &tr(LINK_IDENTITY_CONTEXT, &r)?);
    Ok(r)
}

// ---------------------------------------------------------------- the main phone

/// A device as `GET /v1/devices` lists it.
#[derive(Debug, Clone)]
struct Listed {
    device: [u8; 16],
    auth_key: [u8; 32],
    identity_key: [u8; 33],
    primary: bool,
    pending: bool,
    confirm: Option<[u8; 32]>,
}

async fn listed() -> Result<(Vec<Listed>, u64), Problem> {
    let (status, body) = signed("GET", "/v1/devices", None, false).await?;
    let v = ok_json(status, &body)?;
    let hexed = |x: &Value| x.as_str().and_then(wire::unhex);
    let devices = v["devices"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    Some(Listed {
                        device: hexed(&d["device"])?.try_into().ok()?,
                        auth_key: hexed(&d["auth_key"])?.try_into().ok()?,
                        identity_key: hexed(&d["identity_key"])?.try_into().ok()?,
                        primary: d["primary"].as_bool().unwrap_or(false),
                        pending: d["pending"].as_bool().unwrap_or(false),
                        confirm: hexed(&d["confirm"]).and_then(|c| c.try_into().ok()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((devices, v["list_version"].as_u64().unwrap_or(0)))
}

/// Signs and stores a device list naming `keep` (the main phone first).
async fn put_list(keep: &[Listed], version: u64) -> Result<(), Problem> {
    let identity = Storage::get_item(super::K_IDENTITY)
        .and_then(|h| wire::unhex(&h))
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|seed| wire::ed25519(&seed))
        .ok_or_else(|| problem("Only your main phone can change your devices."))?;
    let account = live()
        .with_untracked(|l| l.account.clone())
        .and_then(|a| wire::unhex(&a))
        .and_then(|b| <[u8; 16]>::try_from(b).ok())
        .ok_or_else(|| problem("This device isn't registered yet."))?;
    let mut list = DeviceList {
        account: AccountId::from_bytes(account),
        list_version: version,
        issued_ms: nsh_core::time::unix_ms() as u64,
        devices: keep
            .iter()
            .map(|d| ListedDevice {
                device: DeviceId::from_bytes(d.device),
                auth_key: d.auth_key,
                identity_key: d.identity_key,
                primary: d.primary,
            })
            .collect(),
        signature: [0; 64],
    };
    let sig = identity.sign(&list.signed_bytes());
    list.signature.copy_from_slice(sig.as_ref());
    let (status, answer) = signed("PUT", "/v1/devices/list", Some(list.encode()), false).await?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(explain(status, &answer))
    }
}

/// On a device that just became the main phone (an account recovery, Prompt 10): signs a list
/// naming only itself, above the stored version.
pub(crate) async fn sign_self_list() -> Result<(), Problem> {
    let me = live()
        .with_untracked(|l| l.device.clone())
        .and_then(|d| wire::unhex(&d))
        .ok_or_else(|| problem("This device isn't registered yet."))?;
    let (devices, version) = listed().await?;
    let mine: Vec<Listed> = devices
        .into_iter()
        .filter(|d| d.device.as_slice() == me.as_slice())
        .map(|mut d| {
            d.primary = true;
            d
        })
        .collect();
    if mine.is_empty() {
        return Err(problem("The server doesn't list this device yet."));
    }
    put_list(&mine, version + 1).await
}

/// On the main phone: links the device whose code `text` is.
pub async fn link_from_code(text: &str) -> Result<(), Problem> {
    let Some(codes::Scanned::Link {
        channel,
        key: public,
    }) = codes::parse(text)
    else {
        return Err(problem("That isn't a Songbird link code."));
    };
    let ep = endpoint();
    // A link token and the device ID it reserves.
    let (status, body) = signed("POST", "/v1/devices/link-token", None, false).await?;
    let v = ok_json(status, &body)?;
    let token: [u8; 32] = v["token"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let device: [u8; 16] = v["device"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let identity_public = Storage::get_item(super::K_IDENTITY)
        .and_then(|h| wire::unhex(&h))
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|seed| wire::ed25519(&seed))
        .map(|k| wire::public(&k))
        .ok_or_else(|| problem("Only your main phone can link devices."))?;
    let account = live()
        .with_untracked(|l| l.account.clone())
        .and_then(|a| wire::unhex(&a))
        .and_then(|b| <[u8; 16]>::try_from(b).ok())
        .ok_or_else(|| problem("This device isn't registered yet."))?;
    let confirm_key: [u8; 32] = wire::random();
    let message = Provision {
        account: AccountId::from_bytes(account),
        account_identity: identity_public,
        token,
        device: DeviceId::from_bytes(device),
        confirm_key,
        server_name: ep.api_name.clone(),
        storage_key: super::records::storage_key(),
    };
    let plain = message
        .encode()
        .map_err(|_| problem("The link message couldn't be built."))?;
    let sealed =
        provisioning::seal(&public, &plain).map_err(|_| problem("That code's key isn't valid."))?;
    let path = format!("/v1/provisioning/{}", wire::hex(&channel));
    let (status, answer) = signed("PUT", &path, Some(sealed), false).await?;
    if !(200..300).contains(&status) {
        return Err(explain(status, &answer));
    }
    // The new device links within the channel's life: watch for it, pending.
    for _ in 0..120 {
        let (devices, version) = listed().await?;
        if let Some(new) = devices.iter().find(|d| d.device == device && d.pending) {
            let transcript = link_transcript(
                LINK_IDENTITY_CONTEXT,
                &ep.api_name,
                &token,
                &DeviceId::from_bytes(device),
                &new.auth_key,
                &new.identity_key,
            )
            .map_err(|_| problem("The server's name is too long."))?;
            let honest = new
                .confirm
                .is_some_and(|tag| provisioning::confirm_ok(&confirm_key, &transcript, &tag));
            if !honest {
                return Err(problem(
                    "The new device's keys don't match what this phone sent. It was not linked.",
                ));
            }
            let mut keep: Vec<Listed> = devices
                .into_iter()
                .filter(|d| !d.pending || d.device == device)
                .collect();
            keep.sort_by_key(|d| !d.primary);
            return put_list(&keep, version + 1).await;
        }
        timer(Duration::from_secs(5)).await;
    }
    Err(problem(
        "The new device didn't finish linking. Show a new code and try again.",
    ))
}

/// On the main phone: logs a device out (a list without it). When the
/// main phone is left alone, the settings' storage key is rotated.
pub async fn remove(device_hex: &str) -> Result<(), Problem> {
    let gone: [u8; 16] = wire::unhex(device_hex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("That device isn't known."))?;
    let (devices, version) = listed().await?;
    let mut keep: Vec<Listed> = devices
        .into_iter()
        .filter(|d| d.device != gone && !d.pending)
        .collect();
    keep.sort_by_key(|d| !d.primary);
    put_list(&keep, version + 1).await?;
    if keep.len() == 1
        && let Err(e) = super::records::rotate_key().await
    {
        // The device is logged out either way; the next removal tries again.
        log::warn!("storage key rotation: {e:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_device_shows_a_link_code_the_main_phone_reads() {
        let key = ProvisioningKey::generate().expect("key");
        let public = key.public();
        let w = Waiting {
            channel: vec![1, 2, 3],
            key,
        };
        assert_eq!(
            codes::parse(&w.code()),
            Some(codes::Scanned::Link {
                channel: vec![1, 2, 3],
                key: public
            })
        );
    }

    /// The request the new device builds verifies as the server and the
    /// main phone check it: both signatures and the confirmation tag.
    #[test]
    fn a_link_request_proves_its_keys_and_the_confirmation_key() {
        let p = Provision {
            account: AccountId::from_bytes([1; 16]),
            account_identity: [2; 32],
            token: [3; 32],
            device: DeviceId::from_bytes([4; 16]),
            confirm_key: [5; 32],
            server_name: "api.test".into(),
            storage_key: Some([6; 32]),
        };
        let r = link_request(&p, &[7; 32], &[8; 32], "api.test").expect("request");
        let tr = |c: &[u8]| {
            link_transcript(
                c,
                "api.test",
                &r.token,
                &r.device,
                &r.auth_key,
                &r.identity_key,
            )
            .expect("transcript")
        };
        assert!(songbird_keys::ed25519_verify(
            &r.auth_key,
            &tr(LINK_AUTH_CONTEXT),
            &r.auth_sig
        ));
        let mut x = [0u8; 32];
        x.copy_from_slice(&r.identity_key[1..]);
        assert!(songbird_keys::xeddsa::verify(
            &x,
            &tr(LINK_IDENTITY_CONTEXT),
            &r.identity_sig
        ));
        assert!(provisioning::confirm_ok(
            &[5; 32],
            &tr(LINK_IDENTITY_CONTEXT),
            &r.confirm
        ));
        assert!(!provisioning::confirm_ok(
            &[6; 32],
            &tr(LINK_IDENTITY_CONTEXT),
            &r.confirm
        ));
    }
}
