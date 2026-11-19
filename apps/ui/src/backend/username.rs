//! The username link (PROTOCOL §4.3, D-0140): the username encrypted under a
//! key that lives only in the link, uploaded as bytes the server cannot read
//! (`PUT /v1/username/link`), and found again by its handle. Resetting it
//! makes a new key and handle; the old link stops working. Changing the
//! username deletes the link on the server, so a new one is made.

use nsh_core::storage::Storage;
use songbird_proto::username_link::UsernameLink;

use super::{Problem, live, problem, wire};

const K_LINK: &str = "sb.username_link";

/// This account's username link, once made.
pub fn my_link() -> Option<String> {
    Storage::get_item(K_LINK)
}

/// Makes (or remakes) the username link for `username`.
pub async fn make_link(username: &str) -> Result<String, Problem> {
    let (key, ciphertext) = songbird_app_core::username_link::seal_username(username)
        .map_err(|_| problem("The link couldn't be made."))?;
    let (status, body) = super::signed("PUT", "/v1/username/link", Some(ciphertext), false).await?;
    let v = super::ok_json(status, &body)?;
    let handle: [u8; 16] = v["handle"]
        .as_str()
        .and_then(wire::unhex)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| problem("The server's answer was incomplete."))?;
    let link = UsernameLink { handle, key }.to_link();
    if let Err(e) = Storage::set_item(K_LINK, &link) {
        log::warn!("username link storage: {e}");
    }
    Ok(link)
}

/// Changes the username (the server holds the old one for 30 days), then
/// makes a new link for it.
pub async fn change(username: &str) -> Result<String, Problem> {
    super::claim_username(username).await?;
    Storage::remove_item(K_LINK);
    make_link(username).await
}

/// Resets the link: the old one stops working.
pub async fn reset() -> Result<String, Problem> {
    let name = live()
        .with_untracked(|l| l.username.clone())
        .ok_or_else(|| problem("Set a username first."))?;
    make_link(&name).await
}

/// Opens someone's username link: fetches the ciphertext, opens it with the
/// key in the link, and finds the account (the handle lets a link work even
/// when its owner isn't findable by username).
pub async fn open(link: &str) -> Result<(String, [u8; 16]), Problem> {
    let l = UsernameLink::parse(link.trim())
        .ok_or_else(|| problem("That isn't a Songbird username link."))?;
    let path = format!("/v1/username/link/{}", wire::hex(&l.handle));
    let (status, body) = super::signed("GET", &path, None, false).await?;
    if status == 404 {
        return Err(problem(
            "This link no longer works. Ask them for a new one.",
        ));
    }
    if status != 200 {
        return Err(super::explain(status, &body));
    }
    let name = songbird_app_core::username_link::open_username(&l.key, &body)
        .map_err(|_| problem("This link is damaged."))?;
    Ok((name, l.handle))
}
