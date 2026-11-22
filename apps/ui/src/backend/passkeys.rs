//! Passkeys, the account's second factor (PROTOCOL §4.7, D-0177): made by the system's own
//! passkey sheet (`songbird_device::passkeys`), registered with the server, listed and removed
//! there; and, on a device being linked, an assertion over the link's transcript.

use std::cell::RefCell;

use songbird_device::passkeys::{self, Failure};

use super::{Problem, ok_json, problem, signed, wire};

thread_local! {
    /// The account's passkeys as the server lists them (credential IDs, hex).
    static IDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The relying party for this server: its web domain, the api's host without its first
/// label (`api.songbird.chat` → `songbird.chat`), as `[api.keys.devices] rp_id` names it.
#[must_use]
pub fn rp_for(api_name: &str) -> String {
    let host = api_name.split(':').next().unwrap_or(api_name);
    match host.split_once('.') {
        Some((_, rest)) if rest.contains('.') => rest.to_owned(),
        _ => host.to_owned(),
    }
}

fn explain(f: Failure) -> Problem {
    problem(match f {
        Failure::Cancelled => "Cancelled.",
        Failure::NotAllowed => {
            "The system refused the passkey. This version of Songbird may not be set up for passkeys yet."
        }
        Failure::Unavailable => "This device can't make or use passkeys.",
    })
}

/// Whether this device can make and use passkeys.
#[must_use]
pub fn available() -> bool {
    !cfg!(test) && passkeys::available()
}

/// The account's passkeys (credential IDs, hex), from the server.
///
/// # Errors
/// A [`Problem`].
pub async fn list() -> Result<Vec<String>, Problem> {
    let (st, body) = signed("GET", "/v1/account/factors", None, false).await?;
    let v = ok_json(st, &body)?;
    let ids: Vec<String> = v["passkeys"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    IDS.with_borrow_mut(|i| i.clone_from(&ids));
    Ok(ids)
}

/// Makes a passkey on this device and registers it (the main device only).
///
/// # Errors
/// A [`Problem`] saying what failed.
pub async fn add(account: [u8; 16], name: String) -> Result<(), Problem> {
    let (st, body) = signed("POST", "/v1/account/passkeys/challenge", None, false).await?;
    let v = ok_json(st, &body)?;
    let challenge = v["challenge"]
        .as_str()
        .and_then(wire::unhex)
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let rp = v["rp_id"]
        .as_str()
        .ok_or_else(|| problem("This server doesn't take passkeys."))?
        .to_owned();
    let (tx, rx) = tokio::sync::oneshot::channel();
    passkeys::create(
        &rp,
        &account,
        &name,
        &challenge,
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    let created = rx
        .await
        .unwrap_or(Err(Failure::Unavailable))
        .map_err(explain)?;
    let req = serde_json::json!({
        "client_data_json": wire::hex(&created.client_data_json),
        "attestation_object": wire::hex(&created.attestation_object),
    });
    let (st, body) = signed(
        "PUT",
        "/v1/account/passkeys",
        Some(req.to_string().into_bytes()),
        true,
    )
    .await?;
    if st == 409 {
        return Err(problem(
            "That passkey can't be added: the account has the most it can, or has it already.",
        ));
    }
    ok_json(st, &body)?;
    list().await.map(|_| ())
}

/// Removes passkey `id` (hex) from the account.
///
/// # Errors
/// A [`Problem`].
pub async fn remove(id: String) -> Result<(), Problem> {
    let req = serde_json::json!({ "id": id });
    let (st, body) = signed(
        "DELETE",
        "/v1/account/passkeys",
        Some(req.to_string().into_bytes()),
        true,
    )
    .await?;
    if !(200..300).contains(&st) {
        return Err(super::explain(st, &body));
    }
    list().await.map(|_| ())
}

/// An assertion over `challenge` from any of the account's passkeys on this device.
///
/// # Errors
/// A [`Problem`].
pub async fn assert(rp: &str, challenge: &[u8]) -> Result<passkeys::Assertion, Problem> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    passkeys::assert(
        rp,
        challenge,
        &[],
        Box::new(move |r| {
            let _ = tx.send(r);
        }),
    );
    rx.await
        .unwrap_or(Err(Failure::Unavailable))
        .map_err(explain)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_relying_party_is_the_servers_web_domain() {
        assert_eq!(super::rp_for("api.songbird.chat"), "songbird.chat");
        assert_eq!(super::rp_for("api.test:8443"), "api.test");
        assert_eq!(super::rp_for("localhost"), "localhost");
    }
}
