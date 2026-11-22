//! Plans in `api` against real stores (Prompt 11; D-0127, D-0128): anonymous plan tokens
//! redeemed once into an entitlement, under their metadata and month only, with a budget; and
//! issuance and redemption that share no value. Runs when the `api_live` variables are set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "tests"
)]

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use songbird_accounts::push_token;
use songbird_accounts::{Db, Registry};
use songbird_api::client::{ApiClient, unsigned};
use songbird_api::config::TokenSettings;
use songbird_api::plans::{GRACE_DAYS, Plans, Redeemer};
use songbird_api::{ApiMetrics, App};
use songbird_jobs::{Queue, QueueConfig, Stream};
use songbird_tokens::token::{self, Info, Kind, Redemption};
use songbird_types::{AccountId, DeviceId, Secret, SecretRef};

const SERVER: &str = "api.test";
const SECRET: [u8; 32] = [0x5c; 32];
/// The support team's private key (staff's, offline in production).
const SUPPORT_PRIVATE: [u8; 32] = [0x42; 32];

fn env(name: &str) -> Option<String> {
    let v = std::env::var(name).ok();
    assert!(
        v.is_some() || std::env::var("SONGBIRD_REQUIRE_STACK").is_err(),
        "SONGBIRD_REQUIRE_STACK is set but {name} is not"
    );
    v
}

struct Stack {
    app: Arc<App>,
    addr: std::net::SocketAddr,
    _stop: tokio::sync::watch::Sender<bool>,
}

async fn stack(per_hour: u32, burst: u32) -> Option<Stack> {
    let surreal = env("SONGBIRD_TEST_SURREALDB")?;
    let hot = env("SONGBIRD_TEST_VALKEY")?;
    let jobs = env("SONGBIRD_TEST_VALKEY_JOBS")?;
    let s3 = env("SONGBIRD_TEST_RUSTFS")?;
    let db = Db::connect(songbird_surreal::Config {
        addr: surreal,
        host: "localhost".into(),
        tls: None,
        user: std::env::var("SONGBIRD_TEST_SURREALDB_USER").unwrap_or_else(|_| "root".into()),
        pass: std::env::var("SONGBIRD_TEST_SURREALDB_PASS").unwrap(),
        namespace: "test".into(),
        database: format!("plans_{}", DeviceId::random().unwrap().to_hex_unredacted()),
        // The registry defines its whole schema at connect: slow on a loaded CI runner.
        timeout: Duration::from_secs(30),
        max_message_bytes: 16 << 20,
    })
    .await
    .unwrap();
    let registry = Arc::new(Registry::connect(db).await.unwrap());
    let s3 = songbird_s3::Client::new(songbird_s3::Config::new(
        s3,
        "songbird-plans-test".into(),
        Secret::new(std::env::var("SONGBIRD_TEST_RUSTFS_ACCESS_KEY").unwrap()),
        Secret::new(std::env::var("SONGBIRD_TEST_RUSTFS_SECRET_KEY").unwrap()),
    ))
    .unwrap();
    let queue = Queue::connect(
        redis::Client::open(jobs).unwrap(),
        &[3; 32],
        QueueConfig::new("plans-test".into()),
    )
    .await
    .unwrap();
    queue.ensure_groups(&Stream::ALL).await.unwrap();
    let hot = songbird_valkey::ValkeyConn::connect(redis::Client::open(hot).unwrap())
        .await
        .unwrap();
    let (_, push_public) = push_token::key_pair(&[0x77; 32]).unwrap();
    // A secret per test run for the budget records (they live in the shared valkey-hot).
    let mut secret = SECRET;
    secret[31] = DeviceId::random().unwrap().as_bytes()[0];
    let redeemer = Redeemer::new(
        TokenSettings {
            secret: SecretRef::Env("SONGBIRD_UNUSED".into()),
            keys: vec![1, 2],
            redeems_per_hour: per_hour,
            redeem_burst: burst,
        },
        &secret,
    )
    .unwrap();
    let app = Arc::new(App {
        plans: Plans {
            redeemer: Some(Arc::new(redeemer)),
            ..Plans::default()
        },
        requests: None,
        reports: None,
        support: Some(Box::new(
            songbird_api::support::Support::new(
                songbird_keys::provisioning::ProvisioningKey::from_private(&SUPPORT_PRIVATE)
                    .unwrap()
                    .public(),
                &secret,
            )
            .unwrap(),
        )),
        calls: None,
        server_name: SERVER.into(),
        push_store: Arc::new(
            songbird_accounts::PushStore::beside(&registry)
                .await
                .unwrap(),
        ),
        registry,
        s3,
        queue: Arc::new(queue),
        hot,
        push_public,
        push_index_secret: [9; 32],
        limits: songbird_api::config::AttachmentLimits::default(),
        media: None,
        metrics: ApiMetrics::default(),
        messages: None,
        registration: None,
        usernames: None,
        keys: None,
        storage: None,
        vault: None,
        kt: None,
        profiles: None,
        devices: None,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(songbird_api::server::serve(
        listener,
        None,
        Arc::clone(&app),
        64,
        rx,
    ));
    Some(Stack {
        app,
        addr,
        _stop: stop,
    })
}

async fn device(s: &Stack) -> (AccountId, ApiClient) {
    let mut seed = [0u8; 32];
    aws_lc_rs::rand::fill(&mut seed).unwrap();
    let kp = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    let mut pk = [0u8; 32];
    pk.copy_from_slice(kp.public_key().as_ref());
    let (a, d) = (AccountId::random().unwrap(), DeviceId::random().unwrap());
    s.app.registry.create_account(&a, &[(d, pk)]).await.unwrap();
    (a, ApiClient::new(s.addr, SERVER, d, kp))
}

/// What the issuer and the redeemer each saw.
struct Issued {
    issuer_view: Vec<String>,
    tokens: Vec<Redemption>,
}

/// Tokens as billing issues them: requested and finished by the client against the key it
/// pinned from `GET /v1/tokens/keys`.
async fn issue(s: &Stack, info: Info, n: usize) -> Issued {
    let (st, b) = unsigned(s.addr, SERVER, "GET", "/v1/tokens/keys", None)
        .await
        .unwrap();
    assert_eq!(st, 200);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    let current = u8::try_from(v["current"].as_u64().unwrap()).unwrap();
    let pinned: [u8; 32] = hex::decode(
        v["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == current)
            .unwrap()["public"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
    .try_into()
    .unwrap();
    let keys = s.app.plans.redeemer.as_ref().unwrap().keys();
    let (pending, req) = token::request(current, info, n).unwrap();
    let resp = keys.issue(&req).unwrap();
    let mut issuer_view = req.blinded.clone();
    issuer_view.extend(resp.evaluated.iter().cloned());
    issuer_view.push(resp.proof.clone());
    Issued {
        issuer_view,
        tokens: pending.finish(&pinned, &resp).unwrap(),
    }
}

async fn redeem(c: &ApiClient, t: &Redemption) -> (u16, serde_json::Value) {
    let (st, b) = c
        .send(
            "POST",
            "/v1/entitlements/redeem",
            Some(&serde_json::to_vec(t).unwrap()),
        )
        .await
        .unwrap();
    (st, serde_json::from_slice(&b).unwrap_or_default())
}

fn plan(period: u32) -> Info {
    Info {
        kind: Kind::Plan,
        level: 1,
        period,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_token_redeems_once_for_its_month() {
    let Some(s) = stack(1_000, 100).await else {
        return;
    };
    let today = songbird_entitlements::today();
    let month = token::month_of(today);
    let (a, c) = device(&s).await;
    let issued = issue(&s, plan(month), 2).await;
    let (st, v) = redeem(&c, &issued.tokens[0]).await;
    assert_eq!(st, 200, "{v}");
    let end = token::last_day(month) + GRACE_DAYS;
    assert_eq!(
        (v["level"].as_str(), v["last_day"].as_u64()),
        (Some("plus"), Some(u64::from(end)))
    );
    // The snapshot follows the registry: the account is Plus.
    s.app
        .plans
        .live
        .store(songbird_entitlements::load(&s.app.registry).await.unwrap());
    let (st, b) = c.send("GET", "/v1/entitlements", None).await.unwrap();
    assert_eq!(st, 200);
    let e: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(e["level"], "plus");
    // Spent: refused, nothing changes, for this account or another.
    let (st, v) = redeem(&c, &issued.tokens[0]).await;
    assert_eq!((st, v["error"].as_str()), (409, Some("spent")));
    let (_, c2) = device(&s).await;
    assert_eq!(redeem(&c2, &issued.tokens[0]).await.0, 409);
    // The spare redeems on the second account (a reinstall, or a second account of one person).
    assert_eq!(redeem(&c2, &issued.tokens[1]).await.0, 200);
    // Next month's token extends; last month's and a later one's are refused.
    let next = issue(&s, plan(month + 1), 1).await;
    let (st, v) = redeem(&c, &next.tokens[0]).await;
    assert_eq!(st, 200);
    assert_eq!(
        v["last_day"].as_u64(),
        Some(u64::from(token::last_day(month + 1) + GRACE_DAYS))
    );
    for p in [month - 1, month + 2] {
        let t = issue(&s, plan(p), 1).await;
        let (st, v) = redeem(&c, &t.tokens[0]).await;
        assert_eq!((st, v["error"].as_str()), (409, Some("period")));
    }
    // Tampered tokens.
    let t = &issue(&s, plan(month), 1).await.tokens[0];
    let mut bad = t.clone();
    bad.output = issued.tokens[1].output.clone();
    assert_eq!(redeem(&c, &bad).await.1["error"], "invalid");
    let mut bad = t.clone();
    bad.kind = Kind::VaultDay.as_u8();
    assert_eq!(redeem(&c, &bad).await.0, 400);
    let mut bad = t.clone();
    bad.level = 0;
    assert_eq!(redeem(&c, &bad).await.0, 400);
    let mut bad = t.clone();
    bad.level = 9;
    assert_eq!(redeem(&c, &bad).await.0, 400);
    // Nothing but the nullifier stays of a token; the entitlement names no payment.
    let row = s.app.registry.entitlement_of(&a).await.unwrap().unwrap();
    assert_eq!(row.source.as_str(), "token");
    let n = token::nullifier(
        issued.tokens[0].key,
        &issued.tokens[0].info().unwrap(),
        &hex::decode(&issued.tokens[0].nonce)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!(s.app.registry.nullifier_held(&n).await.unwrap());
    // Unsigned redemptions are refused.
    let (st, _) = unsigned(
        s.addr,
        SERVER,
        "POST",
        "/v1/entitlements/redeem",
        Some(&serde_json::to_vec(t).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(st, 401);
}

/// Everything the registry keeps after issuances and redemptions shares no value with what the
/// issuer saw (blinded and evaluated elements, proofs): the two sides cannot be joined.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn issuance_and_redemption_share_no_value() {
    let Some(s) = stack(1_000, 100).await else {
        return;
    };
    let month = token::month_of(songbird_entitlements::today());
    let mut issuer_view = Vec::new();
    for _ in 0..5 {
        let (_, c) = device(&s).await;
        let issued = issue(&s, plan(month), 1).await;
        issuer_view.extend(issued.issuer_view);
        assert_eq!(redeem(&c, &issued.tokens[0]).await.0, 200);
    }
    let dump = s
        .app
        .registry
        .db()
        .query(
            "RETURN SELECT * FROM tnull; RETURN SELECT * FROM ent;",
            Vec::new(),
        )
        .await
        .unwrap();
    let dump = format!("{dump:?}");
    // `Debug` hides byte contents; read the raw values too.
    let raw = s
        .app
        .registry
        .db()
        .query(
            "RETURN (SELECT VALUE meta::id(id)[0] FROM tnull);",
            Vec::new(),
        )
        .await
        .unwrap();
    let held: Vec<String> = raw[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| hex::encode(v.as_bytes().unwrap()))
        .collect();
    assert_eq!(held.len(), 5);
    for x in &issuer_view {
        assert!(!dump.contains(x.as_str()));
        for h in &held {
            assert!(!h.contains(x.as_str()) && !x.contains(h.as_str()));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redemptions_have_a_budget() {
    let Some(s) = stack(1, 2).await else {
        return;
    };
    let month = token::month_of(songbird_entitlements::today());
    let (_, c) = device(&s).await;
    let issued = issue(&s, plan(month), 3).await;
    assert_eq!(redeem(&c, &issued.tokens[0]).await.0, 200);
    assert_eq!(redeem(&c, &issued.tokens[1]).await.0, 200);
    assert_eq!(redeem(&c, &issued.tokens[2]).await.0, 429);
}

/// A family (PROTOCOL §14.6, D-0130): the owner and five members each reach Plus through
/// sealed seats; a sixth seat is refused; a removed member gets no further month; a member can
/// leave; every miss is one 404; and the mailbox names no account.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_family_shares_plus_without_naming_anyone() {
    use songbird_tokens::family::{Invite, bearer_hash, manage_hash};
    let Some(s) = stack(1_000, 100).await else {
        return;
    };
    let month = token::month_of(songbird_entitlements::today());
    let (_, owner) = device(&s).await;
    let call = |method: &'static str,
                path: String,
                headers: Vec<(String, String)>,
                body: Option<Vec<u8>>| {
        let addr = s.addr;
        async move {
            let mut b = hyper::Request::builder()
                .method(method)
                .uri(&path)
                .header("host", SERVER);
            for (k, v) in &headers {
                b = b.header(k.as_str(), v.as_str());
            }
            let body = body.unwrap_or_default();
            b = b.header("content-length", body.len());
            let req = b
                .body(http_body_util::Full::new(bytes::Bytes::from(body)))
                .unwrap();
            let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
            let (mut send, conn) =
                hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(tcp))
                    .await
                    .unwrap();
            tokio::spawn(conn);
            let r = send.send_request(req).await.unwrap();
            let st = r.status().as_u16();
            let body = http_body_util::BodyExt::collect(r.into_body())
                .await
                .unwrap()
                .to_bytes();
            (
                st,
                serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default(),
            )
        }
    };
    // The owner's app: a management token, a family, five seats.
    let mut manage = [0u8; 32];
    aws_lc_rs::rand::fill(&mut manage).unwrap();
    let (st, v) = call(
        "POST",
        "/v1/family".into(),
        vec![],
        Some(
            serde_json::json!({ "manage": hex::encode(manage_hash(&manage)) })
                .to_string()
                .into_bytes(),
        ),
    )
    .await;
    assert_eq!(st, 200);
    let family: [u8; 16] = hex::decode(v["family"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let token_header = || vec![("x-songbird-family-token".to_owned(), hex::encode(manage))];
    let seats: Vec<Invite> = (0..5).map(|_| Invite::new(family).unwrap()).collect();
    // Family tokens for the month: one for the owner and one per seat.
    let issued = issue(&s, plan(month), 6).await;
    assert_eq!(redeem(&owner, &issued.tokens[0]).await.0, 200);
    let put = |seat: &Invite, t: &Redemption| {
        let sealed = seat.seal(&serde_json::to_vec(t).unwrap()).unwrap();
        call(
            "PUT",
            format!(
                "/v1/family/{}/slots/{}",
                hex::encode(family),
                hex::encode(seat.slot)
            ),
            token_header(),
            Some(
                serde_json::json!({
                    "auth": hex::encode(bearer_hash(&seat.bearer())),
                    "sealed": hex::encode(sealed),
                })
                .to_string()
                .into_bytes(),
            ),
        )
    };
    for (seat, t) in seats.iter().zip(&issued.tokens[1..]) {
        assert_eq!(put(seat, t).await.0, 204);
    }
    // A sixth seat: refused.
    let sixth = Invite::new(family).unwrap();
    assert_eq!(put(&sixth, &issued.tokens[1]).await.1["error"], "full");
    // Each member opens their link and redeems on their own account.
    let get = |seat: Invite| {
        call(
            "GET",
            format!(
                "/v1/family/{}/slots/{}",
                hex::encode(seat.family),
                hex::encode(seat.slot)
            ),
            vec![(
                "x-songbird-family-bearer".to_owned(),
                hex::encode(seat.bearer()),
            )],
            None,
        )
    };
    let mut members = Vec::new();
    for seat in &seats {
        let joined = Invite::parse(&seat.link()).unwrap();
        let (st, v) = get(joined.clone()).await;
        assert_eq!(st, 200);
        let t: Redemption = serde_json::from_slice(
            &joined
                .open(&hex::decode(v["sealed"].as_str().unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let (_, c) = device(&s).await;
        let (st, v) = redeem(&c, &t).await;
        assert_eq!((st, v["level"].as_str()), (200, Some("plus")));
        members.push(c);
    }
    // Misses are one 404: an unknown family, an unknown slot, a wrong bearer, no bearer.
    let mut wrong = seats[0].clone();
    wrong.family = [9; 16];
    assert_eq!(get(wrong).await.0, 404);
    assert_eq!(get(Invite::new(family).unwrap()).await.0, 404);
    let (st, _) = call(
        "GET",
        format!(
            "/v1/family/{}/slots/{}",
            hex::encode(family),
            hex::encode(seats[0].slot)
        ),
        vec![(
            "x-songbird-family-bearer".to_owned(),
            hex::encode([1u8; 32]),
        )],
        None,
    )
    .await;
    assert_eq!(st, 404);
    // A stranger without the management token can't put or remove.
    let (st, _) = call(
        "DELETE",
        format!(
            "/v1/family/{}/slots/{}",
            hex::encode(family),
            hex::encode(seats[1].slot)
        ),
        vec![("x-songbird-family-token".to_owned(), hex::encode([3u8; 32]))],
        None,
    )
    .await;
    assert_eq!(st, 404);
    // The owner removes member 2; member 3 leaves.
    let (st, _) = call(
        "DELETE",
        format!(
            "/v1/family/{}/slots/{}",
            hex::encode(family),
            hex::encode(seats[1].slot)
        ),
        token_header(),
        None,
    )
    .await;
    assert_eq!(st, 204);
    let (st, _) = call(
        "DELETE",
        format!(
            "/v1/family/{}/slots/{}",
            hex::encode(family),
            hex::encode(seats[2].slot)
        ),
        vec![(
            "x-songbird-family-bearer".to_owned(),
            hex::encode(seats[2].bearer()),
        )],
        None,
    )
    .await;
    assert_eq!(st, 204);
    // Next month: the owner seals tokens into the seats that remain; the removed and the
    // departed find nothing.
    let next = issue(&s, plan(month + 1), 3).await;
    for (seat, t) in [&seats[0], &seats[3], &seats[4]]
        .into_iter()
        .zip(&next.tokens)
    {
        assert_eq!(put(seat, t).await.0, 204);
    }
    assert_eq!(get(seats[1].clone()).await.0, 404);
    assert_eq!(get(seats[2].clone()).await.0, 404);
    assert_eq!(get(seats[3].clone()).await.0, 200);
    // Nothing in the mailbox names an account.
    let dump = s
        .app
        .registry
        .db()
        .query("SELECT * FROM fam; SELECT * FROM fam_slot;", Vec::new())
        .await
        .unwrap();
    let text = format!("{dump:?}");
    for f in ["account", "acct", "owner", "member", "device"] {
        assert!(!text.contains(f), "a field named {f}");
    }
    // The owner closes the family.
    let (st, _) = call(
        "DELETE",
        format!("/v1/family/{}", hex::encode(family)),
        token_header(),
        None,
    )
    .await;
    assert_eq!(st, 204);
    assert_eq!(get(seats[0].clone()).await.0, 404);
}

/// A raw request with headers; the status and the JSON answer.
async fn raw(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<Vec<u8>>,
) -> (u16, serde_json::Value) {
    let mut b = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header("host", SERVER);
    for (k, v) in headers {
        b = b.header(*k, v.as_str());
    }
    let body = body.unwrap_or_default();
    b = b.header("content-length", body.len());
    let req = b
        .body(http_body_util::Full::new(bytes::Bytes::from(body)))
        .unwrap();
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (mut send, conn) = hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(tcp))
        .await
        .unwrap();
    tokio::spawn(conn);
    let r = send.send_request(req).await.unwrap();
    let st = r.status().as_u16();
    let body = http_body_util::BodyExt::collect(r.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        st,
        serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default(),
    )
}

#[tokio::test]
async fn support_tickets_are_sealed_bearer_bound_and_gone_at_close() {
    use songbird_keys::provisioning::{Domain, ProvisioningKey, open_with, seal_with};
    let Some(s) = stack(1_000, 100).await else {
        return;
    };
    // The app fetches the support key and seals to it; only the team's private key opens it.
    let (st, v) = raw(s.addr, "GET", "/v1/support/key", &[], None).await;
    assert_eq!(st, 200);
    let support: [u8; 32] = hex::decode(v["key"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let staff = ProvisioningKey::from_private(&SUPPORT_PRIVATE).unwrap();
    assert_eq!(support, staff.public());
    let reply = ProvisioningKey::generate().unwrap();
    let sealed = seal_with(Domain::Support, &support, b"I cannot sign in").unwrap();
    assert!(open_with(Domain::Report, &staff, &sealed).is_err());
    assert!(open_with(Domain::Support, &reply, &sealed).is_err());
    let body = |kind: &str, sealed: &[u8]| {
        serde_json::json!({
            "kind": kind,
            "reply_key": hex::encode(reply.public()),
            "sealed": hex::encode(sealed),
        })
        .to_string()
        .into_bytes()
    };

    // Unsigned: nobody attached, no priority. An unsigned appeal is refused.
    let (st, v) = raw(
        s.addr,
        "POST",
        "/v1/support/tickets",
        &[],
        Some(body("help", &sealed)),
    )
    .await;
    assert_eq!(st, 201);
    assert_eq!(v["priority"], false);
    let ticket = v["ticket"].as_str().unwrap().to_owned();
    let bearer = v["bearer"].as_str().unwrap().to_owned();
    let (st, _) = raw(
        s.addr,
        "POST",
        "/v1/support/tickets",
        &[],
        Some(body("appeal", &sealed)),
    )
    .await;
    assert_eq!(st, 401);
    for bad in [
        body("other", &sealed),
        body("help", &sealed[..20]),
        b"{}".to_vec(),
    ] {
        let (st, _) = raw(s.addr, "POST", "/v1/support/tickets", &[], Some(bad)).await;
        assert_eq!(st, 400);
    }
    let rows = s.app.registry.tickets(10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].account, None);

    // Signed by a Plus account: attached, answered first.
    let (account, c) = device(&s).await;
    let today = songbird_entitlements::today();
    s.app
        .registry
        .set_entitlement(
            &account,
            1,
            today + 30,
            songbird_accounts::entitlements::Source::Grant,
        )
        .await
        .unwrap();
    s.app
        .plans
        .live
        .store(songbird_entitlements::load(&s.app.registry).await.unwrap());
    let appeal = seal_with(Domain::Support, &support, b"please review").unwrap();
    let (st, b) = c
        .send(
            "POST",
            "/v1/support/tickets",
            Some(&body("appeal", &appeal)),
        )
        .await
        .unwrap();
    assert_eq!(st, 201);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["priority"], true);
    let rows = s.app.registry.tickets(10).await.unwrap();
    assert_eq!(rows[0].account, Some(account));
    assert!(rows[0].priority);

    // The bearer reads; staff reply sealed to the reply key; a wrong bearer is a 404.
    let path = format!("/v1/support/tickets/{ticket}");
    let with = |b: &str| vec![("x-songbird-ticket-bearer", b.to_owned())];
    let id: [u8; 16] = hex::decode(&ticket).unwrap().try_into().unwrap();
    let answer = seal_with(Domain::SupportReply, &reply.public(), b"try this").unwrap();
    s.app
        .registry
        .ticket_post(&id, None, &answer, today)
        .await
        .unwrap();
    let (st, v) = raw(s.addr, "GET", &path, &with(&bearer), None).await;
    assert_eq!(st, 200);
    assert_eq!(v["kind"], "help");
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1]["from"], "staff");
    let theirs = hex::decode(msgs[1]["sealed"].as_str().unwrap()).unwrap();
    assert_eq!(
        open_with(Domain::SupportReply, &reply, &theirs).unwrap(),
        b"try this"
    );
    assert!(open_with(Domain::Support, &reply, &theirs).is_err());
    let wrong = hex::encode([0u8; 32]);
    assert_eq!(raw(s.addr, "GET", &path, &with(&wrong), None).await.0, 404);
    assert_eq!(raw(s.addr, "GET", &path, &[], None).await.0, 404);
    assert_eq!(
        raw(s.addr, "DELETE", &path, &with(&wrong), None).await.0,
        404
    );
    let more = serde_json::json!({ "sealed": hex::encode(&sealed) })
        .to_string()
        .into_bytes();
    let (st, _) = raw(
        s.addr,
        "POST",
        &format!("{path}/messages"),
        &with(&bearer),
        Some(more),
    )
    .await;
    assert_eq!(st, 204);

    // Closed: no row, no message, and the same 404 as a ticket that never was.
    assert_eq!(
        raw(s.addr, "DELETE", &path, &with(&bearer), None).await.0,
        204
    );
    assert_eq!(raw(s.addr, "GET", &path, &with(&bearer), None).await.0, 404);
    assert!(matches!(
        s.app.registry.ticket_read(&id, None).await,
        Err(songbird_accounts::RegistryError::NotFound)
    ));
    assert_eq!(s.app.registry.tickets(10).await.unwrap().len(), 1);

    // A crash report (D-0170): unsigned only, nobody attached; signed, refused.
    let crash = seal_with(Domain::Support, &support, b"{\"panic\":\"<scrubbed>\"}").unwrap();
    let (st, _) = c
        .send("POST", "/v1/support/tickets", Some(&body("crash", &crash)))
        .await
        .unwrap();
    assert_eq!(st, 400);
    let (st, _) = raw(
        s.addr,
        "POST",
        "/v1/support/tickets",
        &[],
        Some(body("crash", &crash)),
    )
    .await;
    assert_eq!(st, 201);
    let rows = s.app.registry.tickets(10).await.unwrap();
    assert!(
        rows.iter()
            .any(|r| r.account.is_none() && r.kind == "crash")
    );

    // Opening has a budget per IP range.
    let mut throttled = false;
    for _ in 0..12 {
        let (st, _) = raw(
            s.addr,
            "POST",
            "/v1/support/tickets",
            &[],
            Some(body("help", &sealed)),
        )
        .await;
        if st == 429 {
            throttled = true;
            break;
        }
        assert_eq!(st, 201);
    }
    assert!(throttled);
}

#[tokio::test]
async fn an_inviter_sees_only_which_of_its_codes_are_live() {
    let Some(s) = stack(1_000, 100).await else {
        return;
    };
    let (_, c) = device(&s).await;
    let codes: Vec<[u8; 15]> = (0..3u8).map(|i| [i + 1; 15]).collect();
    let hashes: Vec<[u8; 32]> = codes.iter().map(songbird_accounts::invite_hash).collect();
    s.app
        .registry
        .create_invites(&hashes[..2], 1, Duration::from_secs(3600))
        .await
        .unwrap();
    s.app.registry.revoke_invite(&hashes[1]).await.unwrap();
    let ask = |h: Vec<String>| {
        let c = &c;
        async move {
            let (st, b) = c
                .send(
                    "POST",
                    "/v1/invites/status",
                    Some(&serde_json::to_vec(&serde_json::json!({ "codes": h })).unwrap()),
                )
                .await
                .unwrap();
            (
                st,
                serde_json::from_slice::<serde_json::Value>(&b).unwrap_or_default(),
            )
        }
    };
    let (st, v) = ask(hashes.iter().map(hex::encode).collect()).await;
    assert_eq!(st, 200);
    assert_eq!(v, serde_json::json!({ "live": [true, false, false] }));
    assert_eq!(ask(vec![]).await.0, 400);
    assert_eq!(ask(vec![hex::encode(hashes[0]); 51]).await.0, 400);
    assert_eq!(ask(vec!["zz".into()]).await.0, 400);
    // Unsigned: refused.
    let (st, _) = raw(
        s.addr,
        "POST",
        "/v1/invites/status",
        &[],
        Some(b"{\"codes\":[]}".to_vec()),
    )
    .await;
    assert_eq!(st, 401);
}
