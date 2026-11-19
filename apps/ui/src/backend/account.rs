//! Settings only the server can enforce (Prompt 9, M2; PROTOCOL §10.3):
//! being found by username, who may add the account to groups, message
//! requests, the inactivity period. The app shows its last known copy
//! (saved on the device) and writes changes through these calls when the
//! account is live; a linked device may tighten them, only the main phone
//! loosens them, and a parent's policy may lock them.

use serde_json::Value;

use super::{Problem, explain, ok_json, problem, signed};

/// What the server holds for the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSettings {
    pub discoverable: bool,
    /// Group adds need the access key (contacts only).
    pub groups_contacts: bool,
    pub requests_everyone: bool,
    pub inactivity_months: u8,
    pub supervised: bool,
}

/// `GET /v1/account/settings`.
pub async fn get() -> Result<ServerSettings, Problem> {
    let (status, body) = signed("GET", "/v1/account/settings", None, false).await?;
    let v = ok_json(status, &body)?;
    Ok(ServerSettings {
        discoverable: v["discoverable"].as_bool().unwrap_or(true),
        groups_contacts: v["groups"].as_str() == Some("contacts"),
        requests_everyone: v["requests"].as_str() != Some("nobody"),
        inactivity_months: v["inactivity_months"]
            .as_u64()
            .and_then(|m| u8::try_from(m).ok())
            .unwrap_or(12),
        supervised: v["supervised"].as_bool().unwrap_or(false),
    })
}

async fn put(path: &str, body: Value) -> Result<(), Problem> {
    let (status, answer) = signed("PUT", path, Some(body.to_string().into_bytes()), true).await?;
    if (200..300).contains(&status) {
        Ok(())
    } else if status == 403 && answer.windows(10).any(|w| w == b"supervised") {
        Err(problem("A parent's rules keep this setting as it is."))
    } else {
        Err(explain(status, &answer))
    }
}

/// `PUT /v1/account/discoverable`.
pub async fn set_discoverable(on: bool) -> Result<(), Problem> {
    put("/v1/account/discoverable", serde_json::json!({ "on": on })).await
}

/// `PUT /v1/account/groups`: "contacts" or "everyone".
pub async fn set_groups(contacts: bool) -> Result<(), Problem> {
    let from = if contacts { "contacts" } else { "everyone" };
    put("/v1/account/groups", serde_json::json!({ "from": from })).await
}

/// `PUT /v1/account/requests`.
pub async fn set_requests(everyone: bool) -> Result<(), Problem> {
    let from = if everyone { "everyone" } else { "nobody" };
    put("/v1/account/requests", serde_json::json!({ "from": from })).await
}

/// `PUT /v1/account/inactivity`.
pub async fn set_inactivity(months: u8) -> Result<(), Problem> {
    put(
        "/v1/account/inactivity",
        serde_json::json!({ "months": months }),
    )
    .await
}

/// `PUT /v1/account/pin` (the main phone): 6 to 12 digits; the server keeps
/// only a salted Argon2id verifier under its pepper.
pub async fn set_pin(pin: String) -> Result<(), Problem> {
    put("/v1/account/pin", serde_json::json!({ "pin": pin })).await
}

/// `DELETE /v1/account/pin`.
pub async fn clear_pin() -> Result<(), Problem> {
    let (status, answer) = signed("DELETE", "/v1/account/pin", None, false).await?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(explain(status, &answer))
    }
}

/// `GET /v1/account/export` (the main phone): what the server holds about
/// the account, as it answers it.
pub async fn export() -> Result<Value, Problem> {
    let (status, body) = signed("GET", "/v1/account/export", None, false).await?;
    ok_json(status, &body)
}

/// The account report in a few plain lines: what the server holds, and
/// that it can't read the encrypted parts.
#[must_use]
pub fn summarize(v: &Value) -> String {
    let a = &v["account"];
    let devices = v["devices"].as_array().map_or(0, Vec::len);
    let store = &v["settings_store"];
    format!(
        "Account created {}.\n{} device(s).\nUsername: {}.\nTwo-step PIN: {}.\nEncrypted settings: {} record(s), {} bytes, unreadable to the server.\nDeleted after {} months unused.",
        a["created"].as_str().unwrap_or("?"),
        devices,
        if a["username_hash"].is_null() {
            "none"
        } else {
            "set (only its hash)"
        },
        if a["pin_set"].as_bool() == Some(true) {
            "on"
        } else {
            "off"
        },
        store["records"].as_u64().unwrap_or(0),
        store["record_bytes"].as_u64().unwrap_or(0),
        a["inactivity_months"].as_u64().unwrap_or(12),
    )
}

/// The account's server-side report as a file the user keeps (pretty JSON:
/// what the server holds, none of it message content).
pub async fn report_file() -> Result<Vec<u8>, Problem> {
    let v = export().await?;
    serde_json::to_vec_pretty(&v).map_err(|_| super::problem("The report couldn't be written."))
}
