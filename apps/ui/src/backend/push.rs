//! This device's push channel (D-0175): the system's token, registered with Songbird's api
//! (`PUT /v1/push`, sealed there to the push service's key, PROTOCOL §9). A push only wakes the
//! app to fetch its mail over the connection; it carries no message.

use std::cell::Cell;

use nsh_core::prelude::*;
use songbird_device::push::{self, Token};

use super::wire;

thread_local! {
    static ASKED: Cell<bool> = const { Cell::new(false) };
}

/// Asks for the token once per run (when the account is connected) and registers each token
/// the system gives.
pub fn install() {
    if ASKED.replace(true) {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Token>();
    let asked = push::register(Box::new(move |t| {
        let _ = tx.send(t);
    }));
    if !asked {
        return;
    }
    Owner::detached().run(|| {
        spawn(async move {
            while let Some(t) = rx.recv().await {
                let body = match &t {
                    Token::Apns(token) => serde_json::json!({ "apns": { "token": wire::hex(token) } }),
                };
                if let Err(p) =
                    super::signed("PUT", "/v1/push", Some(body.to_string().into_bytes()), true)
                        .await
                        .and_then(|(st, b)| super::ok_json(st, &b).map(|_| ()))
                {
                    log::info!("push registration: {p}");
                }
            }
        });
    });
}
