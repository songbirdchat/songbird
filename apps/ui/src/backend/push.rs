//! This device's push channel (D-0175): the system's token, registered with Songbird's api
//! (`PUT /v1/push`, sealed there to the push service's key, PROTOCOL §9). A push only wakes the
//! app to fetch its mail over the connection; it carries no message.

use std::cell::Cell;

use nsh_core::prelude::*;
use songbird_device::push::{self, Token};

use super::wire;

thread_local! {
    static ASKED: Cell<bool> = const { Cell::new(false) };
    /// The tokens this run has: APNs alert, PushKit (iOS).
    static TOKENS: std::cell::RefCell<(Option<Vec<u8>>, Option<Vec<u8>>)> =
        const { std::cell::RefCell::new((None, None)) };
}

/// The PushKit token (iOS calls), registered with the alert token (D-0176).
pub fn voip_token(t: Vec<u8>) {
    TOKENS.with_borrow_mut(|x| x.1 = Some(t));
    put_apns();
}

/// `PUT /v1/push` with both Apple tokens, once the alert token is known.
fn put_apns() {
    let Some((alert, voip)) = TOKENS.with_borrow(|(a, v)| a.clone().map(|a| (a, v.clone()))) else {
        return;
    };
    let mut apns = serde_json::json!({ "token": wire::hex(&alert) });
    if let Some(v) = voip {
        apns["voip"] = serde_json::Value::String(wire::hex(&v));
    }
    let body = serde_json::json!({ "apns": apns }).to_string().into_bytes();
    spawn(async move {
        if let Err(p) = super::signed("PUT", "/v1/push", Some(body), true)
            .await
            .and_then(|(st, b)| super::ok_json(st, &b).map(|_| ()))
        {
            log::info!("push registration: {p}");
        }
    });
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
                match t {
                    Token::Apns(token) => {
                        TOKENS.with_borrow_mut(|x| x.0 = Some(token));
                        put_apns();
                    }
                    Token::WebPush {
                        endpoint,
                        p256dh,
                        auth,
                    } => {
                        let body = serde_json::json!({ "webpush": {
                            "endpoint": endpoint,
                            "p256dh": wire::hex(&p256dh),
                            "auth": wire::hex(&auth),
                        } })
                        .to_string()
                        .into_bytes();
                        if let Err(p) = super::signed("PUT", "/v1/push", Some(body), true)
                            .await
                            .and_then(|(st, b)| super::ok_json(st, &b).map(|_| ()))
                        {
                            log::info!("push registration: {p}");
                        }
                    }
                }
            }
        });
    });
}
