//! Billing against a real SurrealDB and emulated providers (Prompt 11, M3, D-0129): App Store,
//! Google Play and Mollie purchases verified; replayed, duplicated and reordered notifications
//! change nothing twice; forged chains, wrong apps and environments refused; token caps under
//! concurrency; and the database holds no account and no provider identifier in the clear.
//! Runs when `SONGBIRD_TEST_SURREALDB` (+ `_USER`, `_PASS`) is set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::unused_self,
    clippy::needless_pass_by_value,
    reason = "tests"
)]

mod emulators;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_lc_rs::signature::{KeyPair as _, RsaKeyPair};
use emulators::{ApplePki, Shared, World, p256, pem, rs256, rsa};
use serde_json::{Value, json};
use songbird_accounts::Db;
use songbird_billing::mollie::{Mollie, Prices};
use songbird_billing::{
    Billing, Env, ProductSetting, Products, RangeBudget, apple, google, http, jose, store,
};
use songbird_surreal::Value as Sv;
use songbird_tokens::token::{self, Info, Keys, Kind};
use songbird_types::DeviceId;

const SECRET: [u8; 32] = [0x77; 32];
const BUNDLE: &str = "chat.songbird.app";
const AUDIENCE: &str = "https://billing.test/v1/billing/google";
const PUSHER: &str = "pubsub@songbird-test.iam.gserviceaccount.com";

struct Rig {
    billing: Arc<Billing>,
    addr: SocketAddr,
    world: Shared,
    pki: ApplePki,
    oidc: RsaKeyPair,
    client: http::Client,
    pinned: [u8; 32],
    _stop: tokio::sync::watch::Sender<bool>,
}

fn now_ms() -> u64 {
    songbird_billing::now_ms()
}

async fn rig(env: Env, apple_api: bool) -> Option<Rig> {
    let Ok(addr) = std::env::var("SONGBIRD_TEST_SURREALDB") else {
        assert!(
            std::env::var("SONGBIRD_REQUIRE_SURREALDB").is_err(),
            "SONGBIRD_REQUIRE_SURREALDB is set but SONGBIRD_TEST_SURREALDB is not"
        );
        return None;
    };
    let db = Db::connect(songbird_surreal::Config {
        addr,
        host: "localhost".into(),
        tls: None,
        user: std::env::var("SONGBIRD_TEST_SURREALDB_USER").unwrap_or_else(|_| "root".into()),
        pass: std::env::var("SONGBIRD_TEST_SURREALDB_PASS").unwrap(),
        namespace: "test".into(),
        database: format!(
            "billing_{}",
            DeviceId::random().unwrap().to_hex_unredacted()
        ),
        timeout: Duration::from_secs(10),
        max_message_bytes: 16 << 20,
    })
    .await
    .unwrap();
    let world: Shared = Arc::new(Mutex::new(World::default()));
    let base = emulators::serve(Arc::clone(&world)).await;
    let client = http::Client::new(Duration::from_secs(10)).unwrap();
    let pki = ApplePki::new(true, true);
    // Apple's API key.
    let (apple_key, apple_der) = p256();
    world.lock().unwrap().apple_key = apple_key.public_key().as_ref().to_vec();
    let apple = apple::Apple::new(
        jose::AppleTrust::apple(pki.root.clone()),
        BUNDLE.into(),
        env,
        apple_api.then(|| apple::ServerApi {
            base: base.clone(),
            issuer: "issuer-1".into(),
            key_id: "KEY1".into(),
            key: jose::Es256::from_pkcs8(&apple_der).unwrap(),
        }),
        client.clone(),
    );
    // Google's service account and OIDC key.
    let (_, sa_der, n, e) = rsa();
    world.lock().unwrap().google_sa = (n, e);
    let sa_json = json!({
        "client_email": "play@songbird-test.iam.gserviceaccount.com",
        "private_key": pem("PRIVATE KEY", &sa_der),
        "token_uri": format!("{base}/token"),
    });
    let (oidc, _, on, oe) = rsa();
    world.lock().unwrap().google_jwks = vec![("kid-1".into(), on, oe)];
    let google = google::Google::new(
        BUNDLE.into(),
        env,
        google::ServiceAccount::from_json(sa_json.to_string().as_bytes()).unwrap(),
        base.clone(),
        google::PushAuth {
            audience: AUDIENCE.into(),
            email: PUSHER.into(),
            certs_url: format!("{base}/certs"),
        },
        client.clone(),
    );
    let mollie = Mollie::new(
        client.clone(),
        "test_key".into(),
        format!("{base}/v2"),
        "https://billing.test/v1/billing/mollie/webhook".into(),
        "https://songbird.test/thanks".into(),
        Prices {
            currency: "EUR".into(),
            plus_month: "2.99".into(),
            plus_year: "29.99".into(),
            family_month: "4.99".into(),
            family_year: "49.99".into(),
        },
    );
    let products = Products::new(&[
        ProductSetting {
            id: "plus.monthly".into(),
            plan: "plus".into(),
        },
        ProductSetting {
            id: "family.yearly".into(),
            plan: "family".into(),
        },
        ProductSetting {
            id: "plus_sub".into(),
            plan: "plus".into(),
        },
    ])
    .unwrap();
    let keys = Keys::derive(&[0x11; 32], &[1]).unwrap();
    let pinned = keys.public()[0].1;
    let billing = Arc::new(Billing {
        store: store::Store::connect(db, &SECRET).await.unwrap(),
        keys,
        products,
        apple: Some(apple),
        google: Some(google),
        mollie: Some(mollie),
        budget: RangeBudget::new(600, 1_000),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(songbird_billing::serve(
        listener,
        None,
        Arc::clone(&billing),
        256,
        rx,
    ));
    Some(Rig {
        billing,
        addr,
        world,
        pki,
        oidc,
        client,
        pinned,
        _stop: stop,
    })
}

impl Rig {
    async fn post(&self, path: &str, body: &[u8], headers: &[(&str, &str)]) -> (u16, Value) {
        let r = self
            .client
            .send(
                "POST",
                &format!("http://{}{path}", self.addr),
                headers,
                body.to_vec(),
            )
            .await
            .unwrap();
        (
            r.status,
            serde_json::from_slice(&r.body).unwrap_or_default(),
        )
    }

    /// Requests `n` tokens for `period` with `proof`; finishes them against the pinned key.
    async fn tokens(&self, proof: Value, period: u32, n: usize) -> (u16, Value) {
        let (pending, req) = token::request(
            1,
            Info {
                kind: Kind::Plan,
                level: 1,
                period,
            },
            n,
        )
        .unwrap();
        let (st, v) = self
            .post(
                "/v1/billing/tokens",
                json!({ "proof": proof, "request": req })
                    .to_string()
                    .as_bytes(),
                &[],
            )
            .await;
        if st == 200 {
            let resp: token::IssueResponse = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(pending.finish(&self.pinned, &resp).unwrap().len(), n);
        }
        (st, v)
    }

    fn txn(
        &self,
        pki: &ApplePki,
        original: &str,
        product: &str,
        expires_ms: u64,
        extra: Value,
    ) -> String {
        let mut p = json!({
            "originalTransactionId": original,
            "transactionId": format!("{original}-{expires_ms}"),
            "productId": product,
            "bundleId": BUNDLE,
            "environment": "Sandbox",
            "type": "Auto-Renewable Subscription",
            "expiresDate": expires_ms,
            "signedDate": now_ms(),
        });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        pki.sign(&p)
    }

    fn renewal(&self, auto: bool) -> String {
        self.pki.sign(&json!({
            "autoRenewStatus": u8::from(auto),
            "environment": "Sandbox",
        }))
    }

    fn notification(
        &self,
        uuid: &str,
        kind: &str,
        signed_ms: u64,
        txn: &str,
        renewal: &str,
    ) -> Vec<u8> {
        let p = self.pki.sign(&json!({
            "notificationType": kind,
            "notificationUUID": uuid,
            "version": "2.0",
            "signedDate": signed_ms,
            "data": {
                "bundleId": BUNDLE,
                "environment": "Sandbox",
                "signedTransactionInfo": txn,
                "signedRenewalInfo": renewal,
            },
        }));
        json!({ "signedPayload": p }).to_string().into_bytes()
    }

    fn oidc_token(&self, claims: Value) -> String {
        rs256(
            &self.oidc,
            &json!({ "alg": "RS256", "kid": "kid-1", "typ": "JWT" }),
            &claims,
        )
    }

    fn good_claims(&self) -> Value {
        json!({
            "iss": "https://accounts.google.com",
            "aud": AUDIENCE,
            "email": PUSHER,
            "email_verified": true,
            "exp": now_ms() / 1_000 + 600,
        })
    }

    fn rtdn(&self, id: &str, kind: &str, purchase: &str) -> Vec<u8> {
        let data = json!({
            "version": "1.0",
            "packageName": BUNDLE,
            "eventTimeMillis": now_ms().to_string(),
            kind: { "version": "1.0", "notificationType": 13, "purchaseToken": purchase },
        });
        json!({
            "message": { "data": jose::b64_encode(data.to_string().as_bytes()), "messageId": id },
            "subscription": "projects/songbird-test/subscriptions/rtdn",
        })
        .to_string()
        .into_bytes()
    }
}

fn months() -> (u32, u32) {
    let m = token::month_of(songbird_billing::today());
    (m, m + 1)
}

const DAY: u64 = 86_400_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn apple_purchases_are_verified_capped_and_forgeries_refused() {
    let Some(r) = rig(Env::Sandbox, true).await else {
        return;
    };
    let (month, next) = months();
    let expires = now_ms() + 45 * DAY;
    let t = r.txn(&r.pki, "1000001", "plus.monthly", expires, json!({}));
    r.world
        .lock()
        .unwrap()
        .apple
        .insert("1000001".into(), (1, t.clone(), r.renewal(true)));
    let proof = json!({ "apple": t });
    // Two this month (one and a spare), then the cap; next month has its own.
    assert_eq!(r.tokens(proof.clone(), month, 1).await.0, 200);
    assert_eq!(r.tokens(proof.clone(), month, 1).await.0, 200);
    let (st, v) = r.tokens(proof.clone(), month, 1).await;
    assert_eq!((st, v["error"].as_str()), (429, Some("cap")));
    assert_eq!(r.tokens(proof.clone(), next, 2).await.0, 200);
    let (st, v) = r.tokens(proof.clone(), next + 1, 1).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("period")));
    assert!(
        r.world.lock().unwrap().apple_calls >= 4,
        "the API was asked each time"
    );

    // Apple says it was revoked (status 5): nothing more, whatever the client's JWS says.
    let t2 = r.txn(&r.pki, "1000002", "plus.monthly", expires, json!({}));
    r.world
        .lock()
        .unwrap()
        .apple
        .insert("1000002".into(), (5, t2.clone(), r.renewal(false)));
    let (st, v) = r.tokens(json!({ "apple": t2 }), month, 1).await;
    assert_eq!((st, v["error"].as_str()), (402, Some("not_paid")));
    // Expired.
    let t3 = r.txn(&r.pki, "1000003", "plus.monthly", now_ms() - DAY, json!({}));
    r.world
        .lock()
        .unwrap()
        .apple
        .insert("1000003".into(), (2, t3.clone(), r.renewal(false)));
    assert_eq!(r.tokens(json!({ "apple": t3 }), month, 1).await.0, 402);
    // Unknown to Apple.
    let t4 = r.txn(&r.pki, "1000004", "plus.monthly", expires, json!({}));
    assert_eq!(r.tokens(json!({ "apple": t4 }), month, 1).await.0, 403);

    // Forgeries: another root; a leaf or an intermediate without Apple's marker (a developer
    // certificate from the same intermediate); another app; the other environment; an account
    // token; a product we do not sell; a non-subscription; a tampered payload.
    let other = ApplePki::new(true, true);
    let no_leaf = ApplePki::new(true, false);
    let no_inter = ApplePki::new(false, true);
    let mut forged = vec![
        r.txn(&other, "1000001", "plus.monthly", expires, json!({})),
        r.txn(&no_leaf, "1000001", "plus.monthly", expires, json!({})),
        r.txn(&no_inter, "1000001", "plus.monthly", expires, json!({})),
        r.txn(
            &r.pki,
            "1000001",
            "plus.monthly",
            expires,
            json!({ "bundleId": "com.other" }),
        ),
        r.txn(
            &r.pki,
            "1000001",
            "plus.monthly",
            expires,
            json!({ "environment": "Production" }),
        ),
        r.txn(
            &r.pki,
            "1000001",
            "plus.monthly",
            expires,
            json!({ "appAccountToken": "6c0a7d3e-0000-0000-0000-000000000000" }),
        ),
        r.txn(&r.pki, "1000001", "gems.100", expires, json!({})),
        r.txn(
            &r.pki,
            "1000001",
            "plus.monthly",
            expires,
            json!({ "type": "Consumable" }),
        ),
    ];
    let mut parts: Vec<String> = t.split('.').map(str::to_owned).collect();
    let mut payload: Value =
        serde_json::from_slice(&jose::b64url_decode(&parts[1]).unwrap()).unwrap();
    payload["expiresDate"] = json!(expires + 365 * DAY);
    parts[1] = jose::b64url_encode(payload.to_string().as_bytes());
    forged.push(parts.join("."));
    for f in forged {
        let (st, v) = r.tokens(json!({ "apple": f }), month, 1).await;
        assert_eq!((st, v["error"].as_str()), (403, Some("proof")));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn apple_notifications_replayed_or_reordered_change_nothing_twice() {
    // No Server API: the notifications themselves are the state.
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let (month, _) = months();
    let t0 = now_ms();
    let expires = t0 + 45 * DAY;
    let txn = r.txn(&r.pki, "2000001", "plus.monthly", expires, json!({}));
    let renew = r.renewal(true);
    let sub = r.notification("u-1", "SUBSCRIBED", t0, &txn, &renew);
    assert_eq!(r.post("/v1/billing/apple", &sub, &[]).await.0, 200);
    let proof = json!({ "apple": txn.clone() });
    assert_eq!(r.tokens(proof.clone(), month, 1).await.0, 200);
    // A refund, then its replay, then an older renewal delivered late: still refunded.
    let refunded = r.txn(
        &r.pki,
        "2000001",
        "plus.monthly",
        expires,
        json!({ "revocationDate": t0 + 2_000 }),
    );
    let refund = r.notification("u-2", "REFUND", t0 + 2_000, &refunded, &r.renewal(false));
    assert_eq!(r.post("/v1/billing/apple", &refund, &[]).await.0, 200);
    assert_eq!(r.post("/v1/billing/apple", &refund, &[]).await.0, 200);
    let late = r.notification("u-0", "DID_RENEW", t0 + 1_000, &txn, &renew);
    assert_eq!(r.post("/v1/billing/apple", &late, &[]).await.0, 200);
    let key = r.billing.store.key("apple", b"2000001");
    let row = r.billing.store.get(&key).await.unwrap().unwrap();
    assert!(row.state.revoked);
    let (st, _) = r
        .tokens(json!({ "apple": refunded.clone() }), month, 1)
        .await;
    assert_eq!(st, 402);
    // A later renewal (a new period, signed later) applies.
    let renewed = r.txn(
        &r.pki,
        "2000001",
        "plus.monthly",
        expires + 30 * DAY,
        json!({}),
    );
    let again = r.notification("u-3", "DID_RENEW", t0 + 3_000, &renewed, &renew);
    assert_eq!(r.post("/v1/billing/apple", &again, &[]).await.0, 200);
    assert_eq!(r.tokens(json!({ "apple": renewed }), month, 1).await.0, 200);
    // Not Apple's, not 2.0, malformed: refused.
    let other = ApplePki::new(true, true);
    let forged = other.sign(&json!({
        "notificationType": "DID_RENEW", "notificationUUID": "u-9", "version": "2.0",
        "signedDate": t0 + 9_000, "data": { "bundleId": BUNDLE, "environment": "Sandbox" },
    }));
    for bad in [
        json!({ "signedPayload": forged }).to_string().into_bytes(),
        b"{}".to_vec(),
        b"not json".to_vec(),
    ] {
        assert_eq!(r.post("/v1/billing/apple", &bad, &[]).await.0, 400);
    }
}

fn play(state: &str, product: &str, expiry_ms: u64, extra: Value) -> Value {
    let secs = expiry_ms / 1_000;
    let day = u32::try_from(secs / 86_400).unwrap();
    let (y, m, d) = token::civil_from_days(i64::from(day));
    let rem = secs % 86_400;
    let mut v = json!({
        "kind": "androidpublisher#subscriptionPurchaseV2",
        "subscriptionState": state,
        "acknowledgementState": "ACKNOWLEDGEMENT_STATE_PENDING",
        "lineItems": [{
            "productId": product,
            "expiryTime": format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3_600, rem % 3_600 / 60, rem % 60),
            "autoRenewingPlan": { "autoRenewEnabled": true },
        }],
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn google_purchases_and_notifications() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let (month, _) = months();
    let expires = now_ms() + 45 * DAY;
    {
        let mut w = r.world.lock().unwrap();
        w.google.insert(
            "tok-active".into(),
            play("SUBSCRIPTION_STATE_ACTIVE", "plus_sub", expires, json!({})),
        );
        w.google.insert(
            "tok-expired".into(),
            play(
                "SUBSCRIPTION_STATE_EXPIRED",
                "plus_sub",
                now_ms() - DAY,
                json!({}),
            ),
        );
        w.google.insert(
            "tok-onhold".into(),
            play("SUBSCRIPTION_STATE_ON_HOLD", "plus_sub", expires, json!({})),
        );
        w.google.insert("tok-linked".into(), play("SUBSCRIPTION_STATE_ACTIVE", "plus_sub", expires,
            json!({ "externalAccountIdentifiers": { "obfuscatedExternalAccountId": "acct-1" } })));
        w.google.insert(
            "tok-gems".into(),
            play("SUBSCRIPTION_STATE_ACTIVE", "gems", expires, json!({})),
        );
    }
    assert_eq!(
        r.tokens(json!({ "google": "tok-active" }), month, 1)
            .await
            .0,
        200
    );
    assert_eq!(
        r.world.lock().unwrap().google_acks,
        vec!["tok-active".to_owned()],
        "acknowledged once"
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-active" }), month, 1)
            .await
            .0,
        200
    );
    assert_eq!(
        r.world.lock().unwrap().google_acks.len(),
        1,
        "not acknowledged again"
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-expired" }), month, 1)
            .await
            .0,
        402
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-onhold" }), month, 1)
            .await
            .0,
        402
    );
    for refused in ["tok-linked", "tok-gems", "tok-unknown"] {
        assert_eq!(
            r.tokens(json!({ "google": refused }), month, 1).await.0,
            403,
            "{refused}"
        );
    }

    // Notifications: only Google's OIDC token for our push subscription.
    let body = r.rtdn("m-1", "voidedPurchaseNotification", "tok-active");
    let good = r.oidc_token(r.good_claims());
    let mut bad_claims = Vec::new();
    for (k, v) in [
        ("aud", json!("https://other.test/")),
        ("email", json!("someone@else.test")),
        ("email_verified", json!(false)),
        ("iss", json!("https://evil.test")),
        ("exp", json!(now_ms() / 1_000 - 10)),
    ] {
        let mut c = r.good_claims();
        c[k] = v;
        bad_claims.push(r.oidc_token(c));
    }
    let (stranger, _, _, _) = rsa();
    bad_claims.push(rs256(
        &stranger,
        &json!({ "alg": "RS256", "kid": "kid-1" }),
        &r.good_claims(),
    ));
    bad_claims.push(String::new());
    for b in bad_claims {
        let h = format!("Bearer {b}");
        assert_eq!(
            r.post(
                "/v1/billing/google",
                &body,
                &[("authorization", h.as_str())]
            )
            .await
            .0,
            401
        );
    }
    let h = format!("Bearer {good}");
    assert_eq!(
        r.post(
            "/v1/billing/google",
            &body,
            &[("authorization", h.as_str())]
        )
        .await
        .0,
        200
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-active" }), month, 1)
            .await
            .0,
        402,
        "voided"
    );

    // A subscription notification fetches the state; its replay fetches nothing.
    let n = r.rtdn("m-2", "subscriptionNotification", "tok-expired");
    let gets = r.world.lock().unwrap().google_gets;
    assert_eq!(
        r.post("/v1/billing/google", &n, &[("authorization", h.as_str())])
            .await
            .0,
        200
    );
    assert_eq!(r.world.lock().unwrap().google_gets, gets + 1);
    assert_eq!(
        r.post("/v1/billing/google", &n, &[("authorization", h.as_str())])
            .await
            .0,
        200
    );
    assert_eq!(
        r.world.lock().unwrap().google_gets,
        gets + 1,
        "a replay changes nothing"
    );
    // Another package's notification is refused.
    let other = json!({
        "message": { "data": jose::b64_encode(br#"{"packageName":"com.other","testNotification":{}}"#), "messageId": "m-3" },
    });
    assert_eq!(
        r.post(
            "/v1/billing/google",
            other.to_string().as_bytes(),
            &[("authorization", h.as_str())]
        )
        .await
        .0,
        400
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_refuses_test_purchases() {
    let Some(r) = rig(Env::Production, false).await else {
        return;
    };
    let (month, _) = months();
    r.world.lock().unwrap().google.insert(
        "tok-test".into(),
        play(
            "SUBSCRIPTION_STATE_ACTIVE",
            "plus_sub",
            now_ms() + 45 * DAY,
            json!({ "testPurchase": {} }),
        ),
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-test" }), month, 1).await.0,
        403
    );
    // A sandbox Apple transaction in production.
    let t = r.txn(
        &r.pki,
        "3000001",
        "plus.monthly",
        now_ms() + 45 * DAY,
        json!({}),
    );
    assert_eq!(r.tokens(json!({ "apple": t }), month, 1).await.0, 403);
}

/// Mollie's `paidAt` for a UTC day.
fn paid_at(day: u32) -> String {
    let (y, m, d) = token::civil_from_days(i64::from(day));
    format!("{y:04}-{m:02}-{d:02}T10:20:31+00:00")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mollie_checkout_webhooks_and_cancel() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let (month, next) = months();
    let secret = hex::encode([0x5a; 32]);
    let (st, v) = r
        .post(
            "/v1/billing/mollie/checkout",
            json!({ "plan": "family", "interval": "month", "subscriber": secret })
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!(st, 200);
    assert!(
        v["checkout"]
            .as_str()
            .unwrap()
            .starts_with("https://mollie.test/checkout/tr_")
    );
    let proof = json!({ "mollie": secret });
    assert_eq!(
        r.tokens(proof.clone(), month, 1).await.0,
        402,
        "not paid yet"
    );
    // The payment is paid; Mollie calls twice at once: one subscription.
    {
        let mut w = r.world.lock().unwrap();
        let p = w.payments.get_mut("tr_1").unwrap();
        p["status"] = json!("paid");
        p["paidAt"] = json!(paid_at(songbird_billing::today()));
    }
    let (a, b) = tokio::join!(
        r.post("/v1/billing/mollie/webhook", b"id=tr_1", &[]),
        r.post("/v1/billing/mollie/webhook", b"id=tr_1", &[])
    );
    assert_eq!((a.0, b.0), (200, 200));
    assert_eq!(
        r.post("/v1/billing/mollie/webhook", b"id=tr_1", &[])
            .await
            .0,
        200
    );
    assert_eq!(r.world.lock().unwrap().subscriptions.len(), 1);
    // Family: seven a month, then the cap.
    assert_eq!(r.tokens(proof.clone(), month, 7).await.0, 200);
    assert_eq!(r.tokens(proof.clone(), month, 1).await.1["error"], "cap");
    assert_eq!(
        r.tokens(proof.clone(), next, 7).await.0,
        200,
        "paid past the next month's first day"
    );
    let (st, s) = r
        .post(
            "/v1/billing/mollie/state",
            json!({ "subscriber": secret }).to_string().as_bytes(),
            &[],
        )
        .await;
    assert_eq!(st, 200);
    assert_eq!(
        (s["plan"].as_str(), s["renews"].as_bool()),
        (Some("family"), Some(true))
    );
    assert!(s["paid_through"].as_u64().unwrap() >= u64::from(songbird_billing::today() + 28));
    // A second checkout for a running subscription is refused.
    let (st, _) = r
        .post(
            "/v1/billing/mollie/checkout",
            json!({ "plan": "plus", "interval": "year", "subscriber": secret })
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!(st, 409);
    // A recurring payment, its webhook replayed: the period moves once, from the day it was paid
    // (D-0134).
    let paid_before = s["paid_through"].as_u64().unwrap();
    r.world.lock().unwrap().payments.insert(
        "tr_80".into(),
        json!({
            "id": "tr_80", "status": "paid", "sequenceType": "recurring", "subscriptionId": "sub_1",
            "customerId": "cst_1", "amount": { "currency": "EUR", "value": "4.99" },
            "paidAt": paid_at(songbird_billing::today() + 30),
        }),
    );
    for _ in 0..3 {
        assert_eq!(
            r.post("/v1/billing/mollie/webhook", b"id=tr_80", &[])
                .await
                .0,
            200
        );
    }
    let (_, s2) = r
        .post(
            "/v1/billing/mollie/state",
            json!({ "subscriber": secret }).to_string().as_bytes(),
            &[],
        )
        .await;
    let moved = s2["paid_through"].as_u64().unwrap();
    assert!(
        moved > paid_before && moved <= paid_before + 31,
        "{paid_before} -> {moved}"
    );
    // A recurring payment charged back: revoked.
    r.world.lock().unwrap().payments.insert(
        "tr_90".into(),
        json!({
            "id": "tr_90", "status": "paid", "sequenceType": "recurring", "subscriptionId": "sub_1",
            "customerId": "cst_1", "amount": { "currency": "EUR", "value": "4.99" },
            "amountChargedBack": { "currency": "EUR", "value": "4.99" },
        }),
    );
    assert_eq!(
        r.post("/v1/billing/mollie/webhook", b"id=tr_90", &[])
            .await
            .0,
        200
    );
    assert_eq!(r.tokens(proof.clone(), next, 1).await.0, 402);
    // Replaying the first payment's webhook changes nothing now: it was counted once.
    assert_eq!(
        r.post("/v1/billing/mollie/webhook", b"id=tr_1", &[])
            .await
            .0,
        200
    );
    assert_eq!(r.tokens(proof.clone(), next, 1).await.0, 402);
    // A revoked row takes no new checkout (which would clear the revocation without paying).
    let (st, v) = r
        .post(
            "/v1/billing/mollie/checkout",
            json!({ "plan": "plus", "interval": "month", "subscriber": secret })
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!((st, v["error"].as_str()), (409, Some("revoked")));
    assert_eq!(r.tokens(proof.clone(), month, 1).await.0, 402);
    // Unknown and malformed webhooks are acknowledged and change nothing.
    for junk in [&b"id=tr_404"[..], b"id=../../x", b"", b"id=sub_1"] {
        assert_eq!(r.post("/v1/billing/mollie/webhook", junk, &[]).await.0, 200);
    }
    // Cancel: Mollie is told, renewal off.
    assert_eq!(
        r.post(
            "/v1/billing/mollie/cancel",
            json!({ "subscriber": secret }).to_string().as_bytes(),
            &[]
        )
        .await
        .0,
        204
    );
    assert_eq!(r.world.lock().unwrap().cancelled, vec!["sub_1".to_owned()]);
    let (_, s) = r
        .post(
            "/v1/billing/mollie/state",
            json!({ "subscriber": secret }).to_string().as_bytes(),
            &[],
        )
        .await;
    assert_eq!(s["renews"], false);
    // A secret nobody checked out with.
    let (st, _) = r
        .post(
            "/v1/billing/mollie/state",
            json!({ "subscriber": hex::encode([1u8; 32]) })
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!(st, 404);
}

/// D-0169: a cancelled Mollie subscription renews again while its paid period lasts: a new
/// subscription on the same customer, its first charge when the paid period would have renewed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mollie_renewal_resumes_after_cancel() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let secret = hex::encode([0x5b; 32]);
    let body = || json!({ "subscriber": secret }).to_string();
    let (st, _) = r
        .post(
            "/v1/billing/mollie/checkout",
            json!({ "plan": "plus", "interval": "month", "subscriber": secret })
                .to_string()
                .as_bytes(),
            &[],
        )
        .await;
    assert_eq!(st, 200);
    // Nothing to resume before anything is paid (no renewal was ever on; not paid either).
    assert_eq!(
        r.post("/v1/billing/mollie/resume", body().as_bytes(), &[])
            .await
            .0,
        409
    );
    {
        let mut w = r.world.lock().unwrap();
        let p = w.payments.get_mut("tr_1").unwrap();
        p["status"] = json!("paid");
        p["paidAt"] = json!(paid_at(songbird_billing::today()));
    }
    assert_eq!(
        r.post("/v1/billing/mollie/webhook", b"id=tr_1", &[])
            .await
            .0,
        200
    );
    // Renewing already: nothing to resume.
    let (st, v) = r
        .post("/v1/billing/mollie/resume", body().as_bytes(), &[])
        .await;
    assert_eq!((st, v["error"].as_str()), (409, Some("renews")));
    assert_eq!(
        r.post("/v1/billing/mollie/cancel", body().as_bytes(), &[])
            .await
            .0,
        204
    );
    assert_eq!(
        r.post("/v1/billing/mollie/state", body().as_bytes(), &[])
            .await
            .1["renews"],
        false
    );
    // Resume: a second subscription, starting when the paid period renews; renewal on again.
    assert_eq!(
        r.post("/v1/billing/mollie/resume", body().as_bytes(), &[])
            .await
            .0,
        204
    );
    let s = r
        .post("/v1/billing/mollie/state", body().as_bytes(), &[])
        .await
        .1;
    assert_eq!(s["renews"], true);
    {
        let w = r.world.lock().unwrap();
        assert_eq!(w.subscriptions.len(), 2);
        let paid = u32::try_from(s["paid_through"].as_u64().unwrap()).unwrap();
        let start = songbird_billing::ymd(paid - songbird_billing::MOLLIE_SLACK_DAYS);
        assert_eq!(
            w.subscriptions[1].2, start,
            "the first charge when the period would renew"
        );
    }
    // A second tap: nothing more.
    assert_eq!(
        r.post("/v1/billing/mollie/resume", body().as_bytes(), &[])
            .await
            .0,
        409
    );
    assert_eq!(r.world.lock().unwrap().subscriptions.len(), 2);
    // Cancel and resume again: the new subscription is the one cancelled, then a third starts.
    assert_eq!(
        r.post("/v1/billing/mollie/cancel", body().as_bytes(), &[])
            .await
            .0,
        204
    );
    assert_eq!(
        r.world.lock().unwrap().cancelled.last().map(String::as_str),
        Some("sub_2")
    );
    assert_eq!(
        r.post("/v1/billing/mollie/resume", body().as_bytes(), &[])
            .await
            .0,
        204
    );
    assert_eq!(r.world.lock().unwrap().subscriptions.len(), 3);
    // An unknown subscriber.
    let other = json!({ "subscriber": hex::encode([2u8; 32]) }).to_string();
    assert_eq!(
        r.post("/v1/billing/mollie/resume", other.as_bytes(), &[])
            .await
            .0,
        404
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn caps_hold_under_concurrency() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let (month, _) = months();
    let t = r.txn(
        &r.pki,
        "4000001",
        "family.yearly",
        now_ms() + 300 * DAY,
        json!({}),
    );
    let r = Arc::new(r);
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let (r, t) = (Arc::clone(&r), t.clone());
        tasks.push(tokio::spawn(async move {
            r.tokens(json!({ "apple": t }), month, 1).await.0
        }));
    }
    let mut ok = 0;
    for t in tasks {
        match t.await.unwrap() {
            200 => ok += 1,
            429 => {}
            s => panic!("unexpected {s}"),
        }
    }
    assert_eq!(ok, 7, "exactly the Family cap");
}

/// Walks a value, collecting field names and text.
fn walk(v: &Sv, names: &mut Vec<String>, texts: &mut Vec<String>) {
    match v {
        Sv::Text(t) => texts.push(t.clone()),
        Sv::Array(a) => a.iter().for_each(|x| walk(x, names, texts)),
        Sv::Map(m) => {
            for (k, x) in m {
                names.push(k.clone());
                walk(x, names, texts);
            }
        }
        Sv::Tag(_, x) => walk(x, names, texts),
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn billing_keeps_no_account_and_no_store_identifier() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let (month, _) = months();
    let t = r.txn(
        &r.pki,
        "5000001",
        "plus.monthly",
        now_ms() + 45 * DAY,
        json!({}),
    );
    assert_eq!(r.tokens(json!({ "apple": t }), month, 1).await.0, 200);
    r.world.lock().unwrap().google.insert(
        "tok-secret-purchase".into(),
        play(
            "SUBSCRIPTION_STATE_ACTIVE",
            "plus_sub",
            now_ms() + 45 * DAY,
            json!({}),
        ),
    );
    assert_eq!(
        r.tokens(json!({ "google": "tok-secret-purchase" }), month, 1)
            .await
            .0,
        200
    );
    let db = r.billing.store.db();
    let info = db.query1("INFO FOR DB;", Vec::new()).await.unwrap();
    let mut names = Vec::new();
    let mut texts = Vec::new();
    walk(info.get("tables").unwrap(), &mut names, &mut texts);
    names.sort();
    assert_eq!(names, vec!["notif", "sub", "subtok"], "billing's tables");
    // No row records when a token request came: times are whole days, stamps the provider's
    // (D-0134), so issuance cannot be joined to a redemption by the clock.
    let times = db
        .query1("SELECT VALUE <string> at FROM sub;", Vec::new())
        .await
        .unwrap();
    let times: Vec<String> = times
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t.as_text().map(str::to_owned))
        .collect();
    assert_ne!(times.len(), 0);
    assert!(times.iter().all(|t| t.contains("T00:00:00")), "{times:?}");
    let stamps = db
        .query1("SELECT VALUE stamp FROM sub;", Vec::new())
        .await
        .unwrap();
    let now = i64::try_from(now_ms()).unwrap();
    assert!(
        stamps
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x.as_i64().is_none_or(|v| (now - v).abs() > 60_000)),
        "a stamp near now"
    );
    let mut names = Vec::new();
    let mut texts = Vec::new();
    for table in ["sub", "subtok", "notif"] {
        let v = db
            .query1(&format!("SELECT * FROM {table};"), Vec::new())
            .await
            .unwrap();
        walk(&v, &mut names, &mut texts);
    }
    names.sort();
    names.dedup();
    for n in &names {
        assert!(
            [
                "at", "id", "interval", "mcust", "msub", "n", "paid", "plan", "provider", "renews",
                "revoked", "stamp", "until"
            ]
            .contains(&n.as_str()),
            "unexpected field {n}"
        );
    }
    for t in &texts {
        assert!(
            !t.contains("5000001") && !t.contains("tok-secret-purchase"),
            "a store identifier in the clear"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lapsed_subscriptions_and_old_marks_are_swept() {
    let Some(r) = rig(Env::Sandbox, false).await else {
        return;
    };
    let today = songbird_billing::today();
    let st = &r.billing.store;
    let mk = |p: u32, renews: bool| store::State {
        provider: "apple",
        plan: "plus",
        paid: p,
        revoked: false,
        renews,
    };
    let old = st.key("apple", b"old");
    let renewing = st.key("apple", b"renewing");
    let recent = st.key("apple", b"recent");
    st.apply(
        &old,
        &mk(today - store::LAPSED_DAYS - 1, false),
        Some(1),
        true,
    )
    .await
    .unwrap();
    st.apply(
        &renewing,
        &mk(today - store::LAPSED_DAYS - 1, true),
        Some(1),
        true,
    )
    .await
    .unwrap();
    st.apply(&recent, &mk(today - 30, false), Some(1), true)
        .await
        .unwrap();
    st.spend(&recent, 1, 1, 2, today - 1).await.unwrap();
    assert!(st.first_time(&[9; 32], today - 1).await.unwrap());
    assert_eq!(st.sweep(today).await.unwrap(), 3);
    assert!(st.get(&old).await.unwrap().is_none());
    assert!(st.get(&renewing).await.unwrap().is_some());
    assert!(st.get(&recent).await.unwrap().is_some());
    assert!(
        st.first_time(&[9; 32], today + 1).await.unwrap(),
        "the mark went"
    );
}
