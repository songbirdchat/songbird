//! This device's push channel (D-0175): the system's token, registered with Songbird's api
//! (`PUT /v1/push`, sealed there to the push service's key, PROTOCOL §9). A push only wakes the
//! app to fetch its mail over the connection; it carries no message.

use std::cell::Cell;

use nsh_core::prelude::*;
use songbird_device::push::Token;

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
    #[cfg(target_os = "ios")]
    ask_apns(tx);
    #[cfg(not(target_os = "ios"))]
    if !songbird_device::push::register(Box::new(move |t| {
        let _ = tx.send(t);
    })) {
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

/// iOS: the APNs token through NSH 0.7's push module (`nsh-extra`, D-0188), which hears it from
/// NSH's own app delegate; every registration reports it, and so does a new one the system
/// issues later.
#[cfg(target_os = "ios")]
fn ask_apns(tx: tokio::sync::mpsc::UnboundedSender<Token>) {
    use nsh_extra::push::{ForegroundPresentation, PushEvent};
    // The wake-up the Notification Service Extension shows (thread `songbird`, D-0099) says
    // nothing the open app doesn't have already over its connection, where its own notifier
    // decides what to show: in front it neither shows nor sounds.
    nsh_extra::push::set_thread_presentation(
        "songbird",
        Some(
            ForegroundPresentation::default()
                .banner(false)
                .list(false)
                .sound(false),
        ),
    );
    nsh_extra::push::on_event(move |event| {
        if let PushEvent::Token(token) = event {
            let _ = tx.send(Token::Apns(token.as_bytes().to_vec()));
        }
    });
    Owner::detached().run(|| {
        spawn(async {
            if let Err(e) = nsh_extra::push::register().await {
                log::info!("push registration: {e}");
            }
        });
    });
}
