//! Prompt 5, milestone 1 (D-0066, D-0067), live: MLS groups through `api` and the send path,
//! against the real registry (SurrealDB: groups, epochs, fan-outs) and `valkey-hot` (nonces,
//! budgets). OpenMLS clients (`songbird-groups-harness`) play the apps. The mailbox is Iggy when
//! `SONGBIRD_TEST_IGGY` is set, else in memory. Runs when `SONGBIRD_TEST_SURREALDB` (+ `_USER`,
//! `_PASS`) and `SONGBIRD_TEST_VALKEY` are set; with `SONGBIRD_REQUIRE_STACK=1` a missing one fails
//! instead of skipping.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::large_futures,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    reason = "tests"
)]

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::signature::Ed25519KeyPair;
use bytes::Bytes;
use songbird_accounts::{Db, Registry};
use songbird_api::client::ApiClient;
use songbird_api::groups::Groups;
use songbird_api::messages::{Messages, SendStack};
use songbird_api::{ApiMetrics, App};
use songbird_client::DeviceKey;
use songbird_delivery::{DeliveryMetrics, Directory, Shared};
use songbird_e2e::{settings, test_mailbox};
use songbird_groups::Limits;
use songbird_groups::roles::Role;
use songbird_groups_harness::{Device, GID, LinkSecret};
use songbird_mailbox::{AnyMailbox, Mailbox as _};
use songbird_proto::frame::{RejectReason, SendFrame, SendHints, SendPayload, SendStatus};
use songbird_router::LocalRouter;
use songbird_types::{AccountId, DeviceId, Secret};
use tokio::sync::watch;

const API: &str = "api.test";

fn env(name: &str) -> Option<String> {
    let v = std::env::var(name).ok();
    assert!(
        v.is_some() || std::env::var("SONGBIRD_REQUIRE_STACK").is_err(),
        "SONGBIRD_REQUIRE_STACK is set but {name} is not"
    );
    v
}

fn random32() -> [u8; 32] {
    let mut k = [0u8; 32];
    aws_lc_rs::rand::fill(&mut k).unwrap();
    k
}

/// A person: an account with one device, an HTTP client for it and an MLS client.
struct Person {
    api: ApiClient,
    mls: Device,
}

struct Stack {
    registry: Arc<Registry>,
    shared: Arc<Shared<Arc<AnyMailbox>, Arc<LocalRouter>>>,
    app: Arc<App>,
    people: Vec<Person>,
    _stop: watch::Sender<bool>,
}

impl Stack {
    fn messages(&self) -> &Messages {
        self.app.messages.as_ref().unwrap()
    }

    fn groups(&self) -> &Groups {
        self.messages().groups.as_ref().unwrap()
    }

    /// Envelopes waiting for person `i`'s device.
    async fn pending(&self, i: usize) -> Vec<Bytes> {
        self.shared
            .mailbox
            .fetch_after(&self.people[i].mls.id, 0, 1_000)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.envelope)
            .collect()
    }
}

async fn stack(people: usize) -> Option<Stack> {
    let surreal = env("SONGBIRD_TEST_SURREALDB")?;
    let hot = env("SONGBIRD_TEST_VALKEY")?;
    let db = Db::connect(songbird_surreal::Config {
        addr: surreal,
        host: "localhost".into(),
        tls: None,
        user: std::env::var("SONGBIRD_TEST_SURREALDB_USER").unwrap_or_else(|_| "root".into()),
        pass: std::env::var("SONGBIRD_TEST_SURREALDB_PASS").unwrap(),
        namespace: "test".into(),
        database: format!("groups_{}", DeviceId::random().unwrap().to_hex_unredacted()),
        timeout: Duration::from_secs(30),
        max_message_bytes: 16 << 20,
    })
    .await
    .unwrap();
    let registry = Arc::new(Registry::connect(db).await.unwrap());
    let mut keys = Vec::new();
    for _ in 0..people {
        let id = DeviceId::random().unwrap();
        let (_, seed) = DeviceKey::generate(id).unwrap();
        let account = AccountId::random().unwrap();
        let public = DeviceKey::from_seed(id, &seed).unwrap().public_key();
        registry
            .create_account(&account, &[(id, public)])
            .await
            .unwrap();
        keys.push((id, seed, account));
    }
    let rows = registry.load_directory().await.unwrap();
    let mailbox = Arc::new(test_mailbox().await);
    let router = Arc::new(LocalRouter::new());
    let rosters = || -> Option<Arc<dyn songbird_send::Rosters>> {
        Some(Arc::new(songbird_send::RegistryRosters::new(
            Arc::clone(&registry),
            songbird_send::groups::ROSTER_TTL,
        )))
    };
    // The gate (the channel rate class in `valkey-hot`, D-0077): one per transport, each on its
    // own connection, with the same keys, as two nodes have.
    let gate_keys = songbird_send::GateKeys::from_cluster_secret(&random32()).unwrap();
    let gate = |conn: songbird_valkey::ValkeyConn| -> Option<Arc<dyn songbird_send::Gate>> {
        Some(Arc::new(songbird_send::RegistryGate::new(
            Arc::clone(&registry),
            conn,
            gate_keys.clone(),
            songbird_send::RequestLimits::default(),
        )))
    };
    let hot_conn = || async {
        songbird_valkey::ValkeyConn::connect(redis::Client::open(hot.as_str()).unwrap())
            .await
            .unwrap()
    };
    let mut cfg = settings();
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let shared = Arc::new(
        Shared::new(
            cfg,
            Directory::from_accounts(rows.into_iter().map(|(d, a, k)| (d, k, a))),
            Arc::clone(&mailbox),
            Arc::clone(&router),
            DeliveryMetrics::default(),
        )
        .with_policy(songbird_delivery::Policy {
            rosters: rosters(),
            gate: gate(hot_conn().await),
            ..Default::default()
        }),
    );
    let (stop, stop_rx) = watch::channel(false);

    let metrics = ApiMetrics::default();
    let (push, _pushes) = songbird_send::Pusher::new(metrics.push.clone());
    let hot_client = redis::Client::open(hot.as_str()).unwrap();
    let messages = Messages {
        path: Box::new(SendStack {
            directory: Arc::clone(&shared.directory),
            mailbox: Arc::clone(&mailbox),
            router: Arc::clone(&router),
            push,
            metrics: metrics.send.clone(),
            policy: songbird_send::Policy {
                rosters: rosters(),
                gate: gate(hot_conn().await),
                ..Default::default()
            },
        }),
        limits: songbird_proto::Limits::DEFAULT,
        sends_per_sec: 1_000,
        send_burst: 1_000,
        group_sends_per_sec: 1_000,
        group_send_burst: 1_000,
        rate_key: songbird_api::messages::rate_key(&random32()).unwrap(),
        metrics: metrics.send.clone(),
        groups: Some({
            let mut g = Groups::new(&random32(), Limits::default()).unwrap();
            // Races send many commits at once; the budget itself is tested below.
            g.commits_per_sec = 1_000;
            g.commit_burst = 1_000;
            // Room for the link tests; the throttling test spends the burst on purpose.
            g.links_per_sec = 1;
            g.link_burst = 20;
            g.reads_per_sec = 1_000;
            g.read_burst = 1_000;
            g
        }),
    };
    let app = Arc::new(App {
        plans: songbird_api::plans::Plans::default(),
        requests: None,
        reports: None,
        support: None,
        calls: None,
        server_name: API.into(),
        push_store: Arc::new(
            songbird_accounts::PushStore::beside(&registry)
                .await
                .unwrap(),
        ),
        registry: Arc::clone(&registry),
        s3: songbird_s3::Client::new(songbird_s3::Config::new(
            "127.0.0.1:9".into(),
            "unused".into(),
            Secret::new("unused".into()),
            Secret::new("unused-secret".into()),
        ))
        .unwrap(),
        queue: Arc::new(
            songbird_jobs::Queue::connect(
                hot_client.clone(),
                &[3; 32],
                songbird_jobs::QueueConfig::new("groups-test".into()),
            )
            .await
            .unwrap(),
        ),
        hot: songbird_valkey::ValkeyConn::connect(hot_client)
            .await
            .unwrap(),
        push_public: [0; 32],
        push_index_secret: [9; 32],
        limits: songbird_api::config::AttachmentLimits::default(),
        media: None,
        metrics,
        messages: Some(messages),
        registration: None,
        usernames: None,
        keys: None,
        devices: None,
        profiles: None,
        storage: None,
        vault: None,
        kt: None,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_addr = listener.local_addr().unwrap();
    tokio::spawn(songbird_api::server::serve(
        listener,
        None,
        Arc::clone(&app),
        64,
        stop_rx,
    ));
    let ds_key = app
        .messages
        .as_ref()
        .unwrap()
        .groups
        .as_ref()
        .unwrap()
        .external_sender();
    let people = keys
        .into_iter()
        .map(|(id, seed, account)| Person {
            api: ApiClient::new(
                api_addr,
                API,
                id,
                Ed25519KeyPair::from_seed_unchecked(&seed).unwrap(),
            ),
            mls: {
                // Every group names the delivery service as its external sender (D-0075).
                let mut d = Device::unregistered(id, account);
                d.ds_key = Some(ds_key);
                d
            },
        })
        .collect();
    Some(Stack {
        registry,
        shared,
        app,
        people,
        _stop: stop,
    })
}

fn json(b: &[u8]) -> serde_json::Value {
    serde_json::from_slice(b).unwrap()
}

/// `version | group | rest`.
fn body(rest: &[u8]) -> Vec<u8> {
    let mut b = vec![1];
    b.extend_from_slice(&GID);
    b.extend_from_slice(rest);
    b
}

fn parts(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in [a, b] {
        out.extend_from_slice(&u32::try_from(p.len()).unwrap().to_be_bytes());
        out.extend_from_slice(p);
    }
    out
}

async fn create(s: &mut Stack, creator: usize) {
    let (info, tree) = s.people[creator].mls.create();
    let (st, reply) = s.people[creator]
        .api
        .send("POST", "/v1/groups", Some(&body(&parts(&info, &tree))))
        .await
        .unwrap();
    assert_eq!(st, 204, "{}", String::from_utf8_lossy(&reply));
}

async fn commit(s: &Stack, who: usize, bundle: &[u8]) -> (u16, serde_json::Value) {
    let (st, reply) = s.people[who]
        .api
        .send("POST", "/v1/groups/commit", Some(&body(bundle)))
        .await
        .unwrap();
    (
        st,
        if reply.is_empty() {
            serde_json::Value::Null
        } else {
            json(&reply)
        },
    )
}

async fn state(s: &Stack, who: usize) -> (u16, Bytes) {
    s.people[who]
        .api
        .send("POST", "/v1/groups/state", Some(&body(&[])))
        .await
        .unwrap()
}

/// The ratchet tree from a `/v1/groups/state` answer.
fn tree_of(b: &[u8]) -> Vec<u8> {
    let info_len = usize::try_from(u32::from_be_bytes(b[8..12].try_into().unwrap())).unwrap();
    let at = 12 + info_len;
    let tree_len = usize::try_from(u32::from_be_bytes(b[at..at + 4].try_into().unwrap())).unwrap();
    b[at + 4..at + 4 + tree_len].to_vec()
}

fn group_frame(key: u8, msg: Vec<u8>) -> SendFrame {
    SendFrame {
        req_id: u32::from(key),
        idempotency_key: [key; 16],
        hints: SendHints {
            group: Some(GID),
            ..SendHints::default()
        },
        payload: SendPayload::Shared {
            devices: Vec::new(),
            body: Bytes::from(msg),
        },
    }
}

/// A KeyPackage of person `who`, uploaded to the key directory and claimed by person `by`, as
/// an app adding someone does: a commit may add only published packages (D-0074).
async fn published(s: &Stack, who: usize, by: usize) -> songbird_groups_harness::KeyPackage {
    let (kp, bytes) = s.people[who].mls.key_package_with_bytes();
    let device = s.people[who].mls.id;
    let stored = songbird_accounts::StoredKeyPackage {
        reference: songbird_keys::mls::reference(&bytes),
        package: bytes,
        not_after: 4_000_000_000,
    };
    s.registry
        .put_key_packages(&device, &[stored], None, 100)
        .await
        .unwrap();
    let claimed = s
        .registry
        .claim_key_packages(
            &s.people[who].mls.account,
            Some(&device),
            &s.people[by].mls.id,
            true,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    kp
}

/// Adds people `who` (by Alice, person 0) and has them join from their Welcome and the tree.
async fn add(s: &mut Stack, who: &[usize]) {
    let mut kps = Vec::new();
    for &i in who {
        kps.push(published(s, i, 0).await);
    }
    let b = s.people[0].mls.add(&kps);
    let (st, reply) = commit(s, 0, &b).await;
    assert_eq!(st, 200, "{reply}");
    s.people[0].mls.merge();
    for &i in who {
        let welcome = s.pending(i).await.pop().expect("the Welcome");
        let (st, answer) = state(s, i).await;
        assert_eq!(st, 200);
        s.people[i].mls.join(&welcome, &tree_of(&answer));
    }
}

#[tokio::test]
async fn groups_are_created_committed_fanned_out_and_messaged() {
    let Some(mut s) = stack(4).await else { return };
    create(&mut s, 0).await;
    // The same ID again.
    let (info, tree) = (s.people[0].mls.group_info(), s.people[0].mls.tree());
    let (st, _) = s.people[0]
        .api
        .send("POST", "/v1/groups", Some(&body(&parts(&info, &tree))))
        .await
        .unwrap();
    assert_eq!(st, 409);

    // Alice adds Bob and Carol; each finds exactly the Welcome in its mailbox.
    add(&mut s, &[1, 2]).await;
    assert_eq!(s.pending(1).await.len(), 1);
    assert_eq!(s.registry.group(&GID).await.unwrap().unwrap().epoch, 1);
    // Dave (person 3) is outside: no state for him, the same answer as no group.
    assert_eq!(state(&s, 3).await.0, 404);

    // A group message over `PUT /v1/messages`: to Bob and Carol, from the roster.
    let msg = s.people[0].mls.message(b"hello group");
    let (st, result) = s.people[0]
        .api
        .put_message(&group_frame(1, msg))
        .await
        .unwrap();
    assert_eq!((st, result.map(|r| r.1)), (200, Some(SendStatus::Accepted)));
    let got = s.pending(1).await.pop().unwrap();
    assert_eq!(s.people[1].mls.read(&got), b"hello group");
    assert_eq!(s.pending(3).await.len(), 0, "Dave never receives");
    // Dave cannot post into it.
    let fake = s.people[1].mls.message(b"x");
    let (_, result) = s.people[3]
        .api
        .put_message(&group_frame(2, fake))
        .await
        .unwrap();
    assert_eq!(
        result.map(|r| r.1),
        Some(SendStatus::Rejected {
            reason: RejectReason::NotMember,
            retry_after_ms: 0
        })
    );

    // Bob, a member, may not remove Carol (D-0070): 403. Alice, the owner, does: Bob and Carol
    // receive the commit; Carol is out once rosters refresh.
    let carol_leaf = s.people[2].mls.leaf();
    let b = s.people[1].mls.remove(&[carol_leaf]);
    let (st, reply) = commit(&s, 1, &b).await;
    assert_eq!((st, reply["error"].as_str()), (403, Some("role")));
    s.people[1].mls.discard();
    let b = s.people[0].mls.remove(&[carol_leaf]);
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!((st, reply["epoch"].as_u64()), (200, Some(2)));
    s.people[0].mls.merge();
    let to_bob = s.pending(1).await.pop().unwrap();
    s.people[1].mls.receive(
        &songbird_groups::wire::Bundle {
            commit: &to_bob,
            welcome: None,
            group_info: b"-",
            log: None,
        }
        .encode(),
    );
    assert!(
        s.pending(2).await.len() >= 2,
        "Carol is told she was removed"
    );
    tokio::time::sleep(songbird_send::groups::ROSTER_TTL + Duration::from_millis(200)).await;
    let late = s.people[2].mls.message(b"still here?");
    let (_, result) = s.people[2]
        .api
        .put_message(&group_frame(3, late))
        .await
        .unwrap();
    assert_eq!(
        result.map(|r| r.1),
        Some(SendStatus::Rejected {
            reason: RejectReason::NotMember,
            retry_after_ms: 0
        })
    );
    assert_eq!(s.people[0].mls.roster(), s.people[1].mls.roster());
}

#[tokio::test]
async fn concurrent_commits_over_http_have_one_winner_per_epoch() {
    let Some(mut s) = stack(5).await else { return };
    create(&mut s, 0).await;
    add(&mut s, &[1, 2, 3, 4]).await;
    let mut pending: Vec<usize> = (0..5).collect();
    while !pending.is_empty() {
        let epoch = s.registry.group(&GID).await.unwrap().unwrap().epoch;
        let bundles: Vec<Vec<u8>> = pending.iter().map(|&i| s.people[i].mls.update()).collect();
        // All at once, over separate connections.
        let answers = futures_util::future::join_all(
            pending.iter().zip(&bundles).map(|(&i, b)| commit(&s, i, b)),
        )
        .await;
        let winners: Vec<usize> = answers
            .iter()
            .enumerate()
            .filter(|(_, (st, _))| *st == 200)
            .map(|(k, _)| k)
            .collect();
        assert_eq!(winners.len(), 1, "epoch {epoch}: {answers:?}");
        for (k, (st, reply)) in answers.iter().enumerate() {
            if *st != 200 {
                assert_eq!((*st, reply["error"].as_str()), (409, Some("stale")));
                assert_eq!(reply["epoch"].as_u64(), Some(epoch + 1));
                s.people[pending[k]].mls.discard();
            }
        }
        let w = pending[winners[0]];
        s.people[w].mls.merge();
        for i in 0..5 {
            if i != w {
                s.people[i].mls.receive(&bundles[winners[0]]);
            }
        }
        pending.remove(winners[0]);
    }
    let epoch = s.registry.group(&GID).await.unwrap().unwrap().epoch;
    assert_eq!(epoch, 1 + 5);
    for p in &s.people {
        assert_eq!(p.mls.epoch(), epoch, "nobody forked");
    }
}

#[tokio::test]
async fn a_lost_answer_is_recognised_and_an_interrupted_fan_out_is_redriven() {
    let Some(mut s) = stack(3).await else { return };
    create(&mut s, 0).await;
    add(&mut s, &[1]).await;

    // The same commit twice (the first answer was lost): one store, the same epoch.
    let b = s.people[0].mls.update();
    let first = commit(&s, 0, &b).await;
    let again = commit(&s, 0, &b).await;
    assert_eq!(first.0, 200);
    assert_eq!(again, first, "a retried commit is answered as accepted");
    s.people[0].mls.merge();
    assert_eq!(
        s.pending(1).await.len(),
        1 + 1,
        "the Welcome, and the commit once"
    );
    let once = s.pending(1).await.pop().unwrap();
    s.people[1].mls.receive(
        &songbird_groups::wire::Bundle {
            commit: &once,
            welcome: None,
            group_info: b"-",
            log: None,
        }
        .encode(),
    );

    // A crash after the registry recorded a commit, before the mailboxes stored it: the
    // fan-out row stays, and a re-drive pass delivers it.
    let b = s.people[0].mls.update();
    let row = s.registry.group(&GID).await.unwrap().unwrap();
    let rec = songbird_groups::Record::decode(&row.state).unwrap();
    let dir = {
        let mut d = songbird_groups_harness::Dir::default();
        for p in &s.people {
            d.0.insert(p.mls.id, p.mls.account);
        }
        d
    };
    let (g, applied) = songbird_groups::Group::load(rec)
        .unwrap()
        .apply_commit(&s.people[0].mls.id, &b, &dir, &s.groups().limits)
        .unwrap();
    let parts = songbird_groups::wire::Bundle::parse(&b, usize::MAX).unwrap();
    let plan = postcard::to_stdvec(&(
        parts.commit.to_vec(),
        vec![*s.people[1].mls.id.as_bytes()],
        None::<Vec<u8>>,
        Vec::<[u8; 16]>::new(),
    ))
    .unwrap();
    s.registry
        .commit_group(
            &GID,
            applied.from_epoch,
            &songbird_accounts::groups::GroupCommit {
                state: &g.record().encode().unwrap(),
                roster: &g.roster_bytes(&dir).unwrap(),
                last: &applied.commit_hash,
                fanout: &plan,
                log: None,
                link: None,
                joined: &[],
                prune: &[],
            },
        )
        .await
        .unwrap();
    s.people[0].mls.merge();
    let before = s.pending(1).await.len();
    assert_eq!(
        s.groups().redrive_once(&s.registry, s.messages(), 0).await,
        1
    );
    assert_eq!(s.pending(1).await.len(), before + 1);
    assert_eq!(
        s.groups().redrive_once(&s.registry, s.messages(), 0).await,
        0,
        "done once"
    );
    let delivered = s.pending(1).await.pop().unwrap();
    s.people[1].mls.receive(
        &songbird_groups::wire::Bundle {
            commit: &delivered,
            welcome: None,
            group_info: b"-",
            log: None,
        }
        .encode(),
    );
    assert_eq!(s.people[1].mls.epoch(), s.people[0].mls.epoch());
    let _ = &s.people[2];
}

/// Prompt 5, M2 (D-0070), live: roles and policy by commit, "only admins can send" on the send
/// path, the encrypted metadata by role, the change log, and leaving by proposal.
#[tokio::test]
async fn roles_metadata_change_log_and_leaving() {
    let Some(mut s) = stack(3).await else { return };
    create(&mut s, 0).await;
    add(&mut s, &[1, 2]).await;
    // Alice (owner) makes Bob an admin, and the group admins-only for sending and info.
    let mut roles = s.people[0].mls.roles();
    roles.explicit.insert(s.people[1].mls.account, Role::Admin);
    roles.policy.send = Role::Admin;
    roles.policy.edit_info = Role::Admin;
    let b = s.people[0]
        .mls
        .commit_logged(Vec::new(), &[], Some(&roles), b"sealed: settings");
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!((st, reply["epoch"].as_u64()), (200, Some(2)), "{reply}");
    s.people[0].mls.merge();
    for i in [1, 2] {
        let c = s.pending(i).await.pop().unwrap();
        s.people[i].mls.receive(
            &songbird_groups::wire::Bundle {
                commit: &c,
                welcome: None,
                group_info: b"-",
                log: None,
            }
            .encode(),
        );
    }

    // Carol, a member, may no longer send; Bob, an admin, may.
    let m = s.people[2].mls.message(b"hello?");
    let (_, r) = s.people[2]
        .api
        .put_message(&group_frame(10, m))
        .await
        .unwrap();
    assert_eq!(
        r.map(|r| r.1),
        Some(SendStatus::Rejected {
            reason: RejectReason::NotAllowed,
            retry_after_ms: 0
        })
    );
    let m = s.people[1].mls.message(b"announcement");
    let (_, r) = s.people[1]
        .api
        .put_message(&group_frame(11, m))
        .await
        .unwrap();
    assert_eq!(r.map(|r| r.1), Some(SendStatus::Accepted));

    // The encrypted metadata: Carol may not replace it; Bob may; members read it with the
    // state.
    let meta = |expected: u64, blob: &[u8]| {
        let mut b = expected.to_be_bytes().to_vec();
        b.extend_from_slice(blob);
        body(&b)
    };
    let (st, _) = s.people[2]
        .api
        .send("POST", "/v1/groups/meta", Some(&meta(0, b"sealed: name")))
        .await
        .unwrap();
    assert_eq!(st, 403);
    let (st, reply) = s.people[1]
        .api
        .send("POST", "/v1/groups/meta", Some(&meta(0, b"sealed: name")))
        .await
        .unwrap();
    assert_eq!((st, json(&reply)["version"].as_u64()), (200, Some(1)));
    let (st, _) = s.people[1]
        .api
        .send("POST", "/v1/groups/meta", Some(&meta(0, b"sealed: other")))
        .await
        .unwrap();
    assert_eq!(st, 409, "a write from an old version loses");
    let (_, state_bytes) = state(&s, 2).await;
    // The meta blob, then each member device's account (D-0155): `n u16 | n × 32`.
    let at = state_bytes
        .windows(b"sealed: name".len())
        .rposition(|w| w == b"sealed: name")
        .expect("the meta blob");
    let tail = &state_bytes[at + b"sealed: name".len()..];
    let n = usize::from(u16::from_be_bytes([tail[0], tail[1]]));
    assert!(n > 0);
    assert_eq!(tail.len(), 2 + n * 32);

    // The change log: Alice's entry, for any member (a newly linked device reads it too).
    let (st, log) = s.people[2]
        .api
        .send(
            "POST",
            "/v1/groups/log",
            Some(&body(&u64::MAX.to_be_bytes())),
        )
        .await
        .unwrap();
    assert_eq!(st, 200);
    assert_eq!(u16::from_be_bytes([log[0], log[1]]), 1);
    assert!(log.windows(16).any(|w| w == b"sealed: settings"));

    // Carol leaves: her proposal reaches Alice and Bob; Bob commits her departure.
    let p = s.people[2].mls.leave();
    let (st, _) = s.people[2]
        .api
        .send("POST", "/v1/groups/propose", Some(&body(&p)))
        .await
        .unwrap();
    assert_eq!(st, 204);
    let got = s.pending(1).await.pop().unwrap();
    s.people[1].mls.receive_proposal(&got);
    let b = s.people[1].mls.update();
    let (st, reply) = commit(&s, 1, &b).await;
    assert_eq!(st, 200, "{reply}");
    let row = s.registry.group(&GID).await.unwrap().unwrap();
    let roster = songbird_send::Roster::from_bytes(row.epoch, &row.roster).unwrap();
    assert!(!roster.contains(&s.people[2].mls.id), "Carol left");
    assert_eq!(roster.members.len(), 2);
}

/// The `GroupInfo` from a state answer (as [`tree_of`]).
fn info_of(b: &[u8]) -> Vec<u8> {
    let info_len = usize::try_from(u32::from_be_bytes(b[8..12].try_into().unwrap())).unwrap();
    b[12..12 + info_len].to_vec()
}

/// `POST` `path` as person `who` with `version | rest` (no group ID).
async fn post(s: &Stack, who: usize, path: &str, rest: &[u8]) -> (u16, Bytes) {
    let mut b = vec![1];
    b.extend_from_slice(rest);
    s.people[who]
        .api
        .send("POST", path, Some(&b))
        .await
        .unwrap()
}

/// An invite link: a handle and the `auth` the client derives from its secret.
struct Link {
    handle: [u8; 16],
    auth: [u8; 32],
    /// The secret apps derive `auth` and the proof key from (D-0076).
    secret: LinkSecret,
}

impl Link {
    fn new() -> Self {
        let secret = LinkSecret::random();
        Self {
            handle: secret.handle,
            auth: secret.auth(),
            secret,
        }
    }

    /// Person `who` commits the link's proof key into the roles extension (D-0076), and every
    /// other member in the group follows.
    async fn publish(&self, s: &mut Stack, who: usize) {
        let b = s.people[who].mls.publish_link(&self.secret);
        let (st, reply) = commit(s, who, &b).await;
        assert_eq!(st, 200, "{reply}");
        s.people[who].mls.merge();
        for i in 0..s.people.len() {
            if i != who && s.people[i].mls.group.is_some() {
                let c = s.pending(i).await.pop().unwrap();
                s.people[i].mls.receive(&bundle_of(&c));
            }
        }
    }

    /// A join by this link: `handle | auth | bundle`.
    fn join(&self, bundle: &[u8]) -> Vec<u8> {
        let mut req = self.open();
        req.extend_from_slice(bundle);
        req
    }

    /// `handle | auth`.
    fn open(&self) -> Vec<u8> {
        [&self.handle[..], &self.auth[..]].concat()
    }

    /// `POST /v1/groups/links` as person `who`.
    async fn create(&self, s: &Stack, who: usize, ttl: u64, max: u32, approval: bool) -> u16 {
        let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &self.auth);
        let mut rest = self.handle.to_vec();
        rest.extend_from_slice(hash.as_ref());
        rest.extend_from_slice(&ttl.to_be_bytes());
        rest.extend_from_slice(&max.to_be_bytes());
        rest.push(u8::from(approval));
        s.people[who]
            .api
            .send("POST", "/v1/groups/links", Some(&body(&rest)))
            .await
            .unwrap()
            .0
    }
}

#[tokio::test]
async fn invite_links_and_join_requests() {
    let Some(mut s) = stack(6).await else { return };
    let (alice, bob, carol, dave, erin, frank) = (0, 1, 2, 3, 4, 5);
    create(&mut s, alice).await;
    add(&mut s, &[bob]).await;

    // Links are the admins' (the group policy): Bob, a member, may not make one.
    let once = Link::new();
    assert_eq!(once.create(&s, bob, 0, 1, false).await, 403);
    assert_eq!(once.create(&s, alice, 0, 1, false).await, 204);
    assert_eq!(
        once.create(&s, alice, 0, 1, false).await,
        409,
        "the handle is taken"
    );
    // Alice publishes the link's proof key to the members (D-0076): epoch 2.
    once.publish(&mut s, alice).await;

    // Wrong secret and unknown handle: one answer.
    let wrong = Link {
        handle: once.handle,
        auth: random32(),
        secret: LinkSecret::random(),
    };
    assert_eq!(
        post(&s, carol, "/v1/groups/links/open", &wrong.open())
            .await
            .0,
        404
    );
    assert_eq!(
        post(&s, carol, "/v1/groups/links/open", &Link::new().open())
            .await
            .0,
        404
    );

    // Carol opens the link (the group, its state) and joins by external commit.
    let (st, opened) = post(&s, carol, "/v1/groups/links/open", &once.open()).await;
    assert_eq!((st, opened[0]), (200, 0));
    assert_eq!(opened[1..17], GID);
    let opened_state = &opened[17..];
    let (info, tree) = (info_of(opened_state), tree_of(opened_state));
    // The link's `auth` alone is not enough: a join without the proof only the link's secret
    // can make (what a server admitting someone would send), or with a proof under a link the
    // members never published, is refused, and spends nothing (D-0076).
    let bare = s.people[carol].mls.rejoin(&info, &tree);
    let (st, reply) = post(&s, carol, "/v1/groups/join", &once.join(&bare)).await;
    assert_eq!((st, json(&reply)["error"].as_str()), (403, Some("proof")));
    let other = s.people[carol]
        .mls
        .join_by_link(&info, &tree, &LinkSecret::random());
    let (st, reply) = post(&s, carol, "/v1/groups/join", &once.join(&other)).await;
    assert_eq!((st, json(&reply)["error"].as_str()), (403, Some("proof")));
    let join = s.people[carol].mls.join_by_link(&info, &tree, &once.secret);
    // A wrong secret cannot carry a join either.
    assert_eq!(
        post(&s, carol, "/v1/groups/join", &wrong.join(&join))
            .await
            .0,
        404
    );
    let req = once.join(&join);
    let (st, reply) = post(&s, carol, "/v1/groups/join", &req).await;
    assert_eq!((st, json(&reply)["epoch"].as_u64()), (200, Some(3)));
    // The same join again (a lost answer) is recognised, though the link is now spent.
    let (st, reply) = post(&s, carol, "/v1/groups/join", &req).await;
    assert_eq!((st, json(&reply)["epoch"].as_u64()), (200, Some(3)));
    // The members check Carol's proof themselves and follow; Carol reads the group from then
    // on.
    let members = client_dir(&s, &[alice, bob, carol]);
    for i in [alice, bob] {
        let c = s.pending(i).await.pop().unwrap();
        s.people[i]
            .mls
            .receive_join_checked(&bundle_of(&c), &members)
            .unwrap();
    }
    assert_eq!(
        s.pending(bob).await.len(),
        3,
        "the Welcome, the link's key, then Carol's join"
    );
    let msg = s.people[alice].mls.message(b"hi carol");
    assert_eq!(s.people[carol].mls.read(&msg), b"hi carol");
    assert_eq!(state(&s, carol).await.0, 200);

    // Used up: Dave gets the answer for no link.
    assert_eq!(
        post(&s, dave, "/v1/groups/links/open", &once.open())
            .await
            .0,
        404
    );

    // Expiry.
    let brief = Link::new();
    assert_eq!(brief.create(&s, alice, 1, 0, false).await, 204);
    assert_eq!(
        post(&s, dave, "/v1/groups/links/open", &brief.open())
            .await
            .0,
        200
    );
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    assert_eq!(
        post(&s, dave, "/v1/groups/links/open", &brief.open())
            .await
            .0,
        404
    );
    s.registry.purge_group_invites().await.unwrap();
    assert!(
        s.registry
            .group_link(&brief.handle)
            .await
            .unwrap()
            .is_none(),
        "swept"
    );

    // Revocation: Bob may not; Alice does.
    let revoked = Link::new();
    assert_eq!(revoked.create(&s, alice, 0, 0, false).await, 204);
    let revoke = |who: usize| {
        let handle = revoked.handle;
        let s = &s;
        async move {
            s.people[who]
                .api
                .send("POST", "/v1/groups/links/revoke", Some(&body(&handle)))
                .await
                .unwrap()
                .0
        }
    };
    assert_eq!(revoke(bob).await, 403);
    assert_eq!(revoke(alice).await, 204);
    assert_eq!(
        post(&s, dave, "/v1/groups/links/open", &revoked.open())
            .await
            .0,
        404
    );

    // A link that needs approval: Dave sees only the (encrypted) info, cannot join, and asks.
    let ask = Link::new();
    assert_eq!(ask.create(&s, alice, 0, 0, true).await, 204);
    let (st, opened) = post(&s, dave, "/v1/groups/links/open", &ask.open()).await;
    assert_eq!((st, opened.len()), (200, 1 + 8 + 4), "no state, no members");
    assert_eq!(opened[0], 1);
    let mut req = ask.open();
    req.extend_from_slice(&join);
    let (st, reply) = post(&s, dave, "/v1/groups/join", &req).await;
    assert_eq!(
        (st, json(&reply)["error"].as_str()),
        (403, Some("approval"))
    );
    assert_eq!(
        post(&s, dave, "/v1/groups/requests", &ask.open()).await.0,
        204
    );
    assert_eq!(
        post(&s, dave, "/v1/groups/requests", &ask.open()).await.0,
        204
    );
    assert_eq!(
        post(&s, erin, "/v1/groups/requests", &ask.open()).await.0,
        204
    );
    // A member asking: 409. A link that needs no approval: 409 (join instead).
    let (st, reply) = post(&s, bob, "/v1/groups/requests", &ask.open()).await;
    assert_eq!((st, json(&reply)["error"].as_str()), (409, Some("member")));
    let open_link = Link::new();
    assert_eq!(open_link.create(&s, alice, 0, 0, false).await, 204);
    let (st, reply) = post(&s, dave, "/v1/groups/requests", &open_link.open()).await;
    assert_eq!((st, json(&reply)["error"].as_str()), (409, Some("open")));

    // The list is the members' who may add (here everyone); outsiders get 404.
    assert_eq!(list(&s, dave).await.0, 404);
    let (st, l) = list(&s, bob).await;
    assert_eq!((st, u16::from_be_bytes([l[0], l[1]])), (200, 2));
    let (rows, _) = l[2..].as_chunks::<24>();
    let accounts: Vec<[u8; 16]> = rows.iter().map(|c| c[..16].try_into().unwrap()).collect();
    assert_eq!(
        accounts[0],
        *s.people[dave].mls.account.as_bytes(),
        "oldest first"
    );
    assert_eq!(accounts[1], *s.people[erin].mls.account.as_bytes());

    // Approving Dave is adding him: the commit answers his request. Erin is declined.
    add(&mut s, &[dave]).await;
    let decline = body(s.people[erin].mls.account.as_bytes());
    let (st, _) = s.people[bob]
        .api
        .send("POST", "/v1/groups/requests/decline", Some(&decline))
        .await
        .unwrap();
    assert_eq!(st, 204);
    let (st, l) = list(&s, alice).await;
    assert_eq!((st, &l[..]), (200, &[0u8, 0][..]));

    // Opening links at speed is throttled per device.
    let mut throttled = false;
    for _ in 0..30 {
        if post(&s, frank, "/v1/groups/links/open", &Link::new().open())
            .await
            .0
            == 429
        {
            throttled = true;
            break;
        }
    }
    assert!(throttled);
}

/// `POST /v1/groups/requests/list` as person `who`.
async fn list(s: &Stack, who: usize) -> (u16, Bytes) {
    s.people[who]
        .api
        .send("POST", "/v1/groups/requests/list", Some(&body(&[])))
        .await
        .unwrap()
}

#[tokio::test]
async fn parallel_joins_never_exceed_a_links_uses() {
    let Some(mut s) = stack(6).await else { return };
    create(&mut s, 0).await;
    let link = Link::new();
    assert_eq!(link.create(&s, 0, 0, 2, false).await, 204);
    link.publish(&mut s, 0).await;
    let mut pending: Vec<usize> = (1..6).collect();
    let mut joined = Vec::new();
    while !pending.is_empty() {
        // Everyone still trying opens the link and joins from the epoch they saw, at once.
        let mut tries = Vec::new();
        for &i in &pending {
            let (st, opened) = post(&s, i, "/v1/groups/links/open", &link.open()).await;
            if st == 404 {
                continue;
            }
            assert_eq!((st, opened[0]), (200, 0));
            let state = &opened[17..];
            let mut req = link.open();
            req.extend_from_slice(&s.people[i].mls.join_by_link(
                &info_of(state),
                &tree_of(state),
                &link.secret,
            ));
            tries.push((i, req));
        }
        let answers = futures_util::future::join_all(
            tries
                .iter()
                .map(|(i, req)| post(&s, *i, "/v1/groups/join", req)),
        )
        .await;
        let mut next = Vec::new();
        let mut winners = 0;
        for ((i, _), (st, reply)) in tries.iter().zip(&answers) {
            match st {
                200 => {
                    winners += 1;
                    joined.push(*i);
                }
                409 => {
                    assert_eq!(json(reply)["error"].as_str(), Some("stale"));
                    next.push(*i);
                }
                404 => {}
                other => panic!("join answered {other}"),
            }
        }
        assert!(winners <= 1, "one join per epoch");
        pending = next;
    }
    assert_eq!(joined.len(), 2, "exactly the link's two uses: {joined:?}");
    let row = s.registry.group(&GID).await.unwrap().unwrap();
    assert_eq!(row.epoch, 3);
    let link_row = s.registry.group_link(&link.handle).await.unwrap().unwrap();
    assert!(!link_row.usable);
}

/// `POST` `path` as person `who` with `version | ids… | rest`.
async fn post_ids(s: &Stack, who: usize, path: &str, ids: &[&[u8; 16]], rest: &[u8]) -> u16 {
    post_ids_reply(s, who, path, ids, rest).await.0
}

async fn post_ids_reply(
    s: &Stack,
    who: usize,
    path: &str,
    ids: &[&[u8; 16]],
    rest: &[u8],
) -> (u16, Bytes) {
    let mut b = vec![1];
    for id in ids {
        b.extend_from_slice(*id);
    }
    b.extend_from_slice(rest);
    s.people[who]
        .api
        .send("POST", path, Some(&b))
        .await
        .unwrap()
}

/// Person `who` registers `device`'s new group `id` (`device` may be a second handle on the
/// person's device, for a second group).
async fn create_group(
    s: &Stack,
    who: usize,
    device: &mut Device,
    id: [u8; 16],
    policy: songbird_groups::roles::Policy,
) {
    let (info, tree) = device.create_in(id, policy);
    let st = post_ids(s, who, "/v1/groups", &[&id], &parts(&info, &tree)).await;
    assert_eq!(st, 204);
}

#[tokio::test]
async fn communities_are_run_by_the_announcement_groups_admins() {
    use songbird_groups::roles::Policy;
    let Some(s) = stack(3).await else { return };
    let (alice, bob, carol) = (0, 1, 2);
    let (ann, g1, g2, plain) = ([0xA1; 16], [0xA2; 16], [0xA3; 16], [0xA4; 16]);
    let (c, c2) = ([0xC1; 16], [0xC2; 16]);
    let announcement = Policy {
        send: Role::Admin,
        ..Policy::GROUP
    };

    // Alice's announcement group, with Carol as a member; Alice's group G1; Bob's G2 and an
    // ordinary group.
    let mut a = Device::restored(&s.people[alice].mls);
    create_group(&s, alice, &mut a, ann, announcement).await;
    let kp = published(&s, carol, alice).await;
    let b = a.add(&[kp]);
    let mut req = ann.to_vec();
    req.extend_from_slice(&b);
    let (st, _) = post(&s, alice, "/v1/groups/commit", &req).await;
    assert_eq!(st, 200);
    let mut a1 = Device::restored(&s.people[alice].mls);
    create_group(&s, alice, &mut a1, g1, Policy::GROUP).await;
    let mut b2 = Device::restored(&s.people[bob].mls);
    create_group(&s, bob, &mut b2, g2, Policy::GROUP).await;
    let mut bp = Device::restored(&s.people[bob].mls);
    create_group(&s, bob, &mut bp, plain, Policy::GROUP).await;

    // Creating: an announcement-only group, by its admin, once.
    let path = "/v1/communities";
    let (st, reply) = post_ids_reply(&s, bob, path, &[&c, &plain], &[]).await;
    assert_eq!(
        (st, json(&reply)["error"].as_str()),
        (400, Some("announcement"))
    );
    assert_eq!(post_ids(&s, carol, path, &[&c, &ann], &[]).await, 403);
    assert_eq!(post_ids(&s, bob, path, &[&c, &ann], &[]).await, 404);
    assert_eq!(post_ids(&s, alice, path, &[&c, &ann], &[]).await, 204);
    assert_eq!(post_ids(&s, alice, path, &[&c2, &ann], &[]).await, 409);

    // Linking groups: an admin of both.
    let add = "/v1/communities/groups/add";
    assert_eq!(post_ids(&s, carol, add, &[&c, &g1], &[]).await, 403);
    assert_eq!(
        post_ids(&s, alice, add, &[&c, &g2], &[]).await,
        404,
        "not Alice's"
    );
    assert_eq!(
        post_ids(&s, bob, add, &[&c, &g2], &[]).await,
        404,
        "not Bob's community"
    );
    assert_eq!(post_ids(&s, alice, add, &[&c, &g1], &[]).await, 204);
    assert_eq!(post_ids(&s, alice, add, &[&c, &g1], &[]).await, 409);
    assert_eq!(post_ids(&s, alice, add, &[&c, &ann], &[]).await, 409);

    // The state: for the announcement group's members only.
    let state = "/v1/communities/state";
    assert_eq!(post_ids(&s, bob, state, &[&c], &[]).await, 404);
    let (st, view) = post_ids_reply(&s, carol, state, &[&c], &[]).await;
    assert_eq!(st, 200);
    assert_eq!(view[..16], ann);
    assert_eq!(view[16..24], 0u64.to_be_bytes(), "no info yet");
    assert_eq!(view[28..30], 1u16.to_be_bytes());
    assert_eq!(view[30..46], g1);

    // The encrypted info: admins, versioned.
    let meta = "/v1/communities/meta";
    let mut v0 = 0u64.to_be_bytes().to_vec();
    v0.extend_from_slice(b"sealed: name and picture");
    assert_eq!(post_ids(&s, carol, meta, &[&c], &v0).await, 403);
    let (st, reply) = post_ids_reply(&s, alice, meta, &[&c], &v0).await;
    assert_eq!((st, json(&reply)["version"].as_u64()), (200, Some(1)));
    assert_eq!(post_ids(&s, alice, meta, &[&c], &v0).await, 409);
    let (_, view) = post_ids_reply(&s, carol, state, &[&c], &[]).await;
    assert_eq!(view[16..24], 1u64.to_be_bytes());

    // Unlinking: a community admin or the group's own admin; deleting: an owner.
    let remove = "/v1/communities/groups/remove";
    assert_eq!(post_ids(&s, carol, remove, &[&c, &g1], &[]).await, 403);
    assert_eq!(post_ids(&s, alice, remove, &[&c, &g1], &[]).await, 204);
    let (_, view) = post_ids_reply(&s, carol, state, &[&c], &[]).await;
    assert_eq!(view[view.len() - 2..], 0u16.to_be_bytes());
    let delete = "/v1/communities/delete";
    assert_eq!(post_ids(&s, carol, delete, &[&c], &[]).await, 403);
    assert_eq!(post_ids(&s, alice, delete, &[&c], &[]).await, 204);
    assert_eq!(post_ids(&s, carol, state, &[&c], &[]).await, 404);
    // The announcement group is free again.
    assert_eq!(post_ids(&s, alice, path, &[&c2, &ann], &[]).await, 204);
}

#[tokio::test]
async fn groups_are_in_the_export_and_requests_go_with_the_account() {
    let Some(mut s) = stack(3).await else { return };
    let (alice, bob, carol) = (0, 1, 2);
    create(&mut s, alice).await;
    add(&mut s, &[bob]).await;
    let ask = Link::new();
    assert_eq!(ask.create(&s, alice, 0, 0, true).await, 204);
    assert_eq!(
        post(&s, carol, "/v1/groups/requests", &ask.open()).await.0,
        204
    );

    // Alice's export names the group, her device and its role; Bob's his; Carol's her request.
    let gid = hex::encode(GID);
    let export = |who: usize| {
        let account = s.people[who].mls.account;
        let r = Arc::clone(&s.registry);
        async move { r.export(&account, None).await.unwrap() }
    };
    let e = export(alice).await;
    assert_eq!(e["groups"][0]["group"], gid.as_str());
    assert_eq!(e["groups"][0]["devices"][0]["role"], "owner");
    assert_eq!(
        e["groups"][0]["devices"][0]["id"],
        s.people[alice].mls.id.to_hex_unredacted().as_str()
    );
    assert!(e["groups"][0]["community"].is_null());
    let e = export(bob).await;
    assert_eq!(e["groups"][0]["devices"][0]["role"], "member");
    let e = export(carol).await;
    assert_eq!(e["groups"].as_array().unwrap().len(), 0);
    assert_eq!(e["group_requests"][0]["group"], gid.as_str());

    // Deleting Carol's account deletes her request; nothing of hers is left in the registry.
    let account = s.people[carol].mls.account;
    let devices = s.registry.account_devices(&account).await.unwrap();
    s.registry.delete_account_rows(&account).await.unwrap();
    let left = s.registry.remnants(&account, &devices).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
    let (_, l) = list(&s, alice).await;
    assert_eq!(&l[..], &[0u8, 0][..], "the request is gone");
}

#[tokio::test]
async fn a_member_cannot_add_a_leaf_it_made_up_for_another_device() {
    // Review finding (D-0074): a KeyPackage under Bob's device ID that Bob never published (as
    // Alice could make with keys of her own) is refused, and nothing is stored.
    let Some(mut s) = stack(2).await else { return };
    create(&mut s, 0).await;
    let forged = s.people[1].mls.key_package();
    let b = s.people[0].mls.add(&[forged]);
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!((st, reply["error"].as_str()), (403, Some("key_package")));
    assert_eq!(s.registry.group(&GID).await.unwrap().unwrap().epoch, 0);
    assert!(s.pending(1).await.is_empty(), "no Welcome");
    s.people[0].mls.discard();
    // The same device's published package is accepted.
    add(&mut s, &[1]).await;
    assert_eq!(s.registry.group(&GID).await.unwrap().unwrap().epoch, 1);
}

#[tokio::test]
async fn outsiders_get_the_answer_for_no_group() {
    // Review finding (D-0074): a stranger's commit or proposal is refused before any work,
    // with 404, as for a group that does not exist.
    let Some(mut s) = stack(3).await else { return };
    create(&mut s, 0).await;
    add(&mut s, &[1]).await;
    let info = s.people[0].mls.group_info();
    let tree = s.people[0].mls.tree();
    let b = s.people[2].mls.rejoin(&info, &tree);
    let (st, _) = commit(&s, 2, &b).await;
    assert_eq!(st, 404);
    let (st, _) = s.people[2]
        .api
        .send(
            "POST",
            "/v1/groups/propose",
            Some(&body(b"not even a proposal")),
        )
        .await
        .unwrap();
    assert_eq!(st, 404);
}

/// The members' view of the directory: these people's devices and accounts.
fn client_dir(s: &Stack, who: &[usize]) -> songbird_groups_harness::Dir {
    songbird_groups_harness::Dir(
        who.iter()
            .map(|&i| (s.people[i].mls.id, s.people[i].mls.account))
            .collect(),
    )
}

#[tokio::test]
async fn departed_devices_are_removed_through_server_proposals() {
    // D-0075: Carol's account is deleted. The deletion marks the group; `api` proposes her
    // leaf's removal as the group's external sender (confirmed in the registry, although this
    // node's directory snapshot is stale); members check it and the next commit removes her.
    let Some(mut s) = stack(3).await else { return };
    let (alice, bob, carol) = (0, 1, 2);
    let (st, key) = s.people[alice]
        .api
        .send("POST", "/v1/groups/ds", Some(&[1]))
        .await
        .unwrap();
    assert_eq!((st, &key[..]), (200, &s.groups().external_sender()[..]));
    create(&mut s, alice).await;
    add(&mut s, &[bob, carol]).await;
    assert_eq!(s.groups().prune_once(&s.registry, s.messages()).await, 0);

    let account = s.people[carol].mls.account;
    let job = songbird_jobs::Job::new(songbird_jobs::Work::DeleteAccount { account }).unwrap();
    assert!(
        s.registry
            .begin_account_deletion(&account, &job)
            .await
            .unwrap()
    );
    assert_eq!(s.registry.groups_to_prune(10).await.unwrap().len(), 1);
    assert_eq!(s.groups().prune_once(&s.registry, s.messages()).await, 1);
    assert!(s.registry.groups_to_prune(10).await.unwrap().is_empty());

    let members = client_dir(&s, &[alice, bob]);
    for i in [alice, bob] {
        let p = s.pending(i).await.pop().expect("the server's proposal");
        s.people[i]
            .mls
            .receive_server_proposal(&p, &members)
            .unwrap();
    }
    let b = s.people[bob].mls.update();
    // This node's directory snapshot still lists Carol (nothing here follows the registry):
    // the commit is answered "retry", not refused, and stores nothing.
    let (st, _) = commit(&s, bob, &b).await;
    assert_eq!(st, 503);
    assert_eq!(s.registry.group(&GID).await.unwrap().unwrap().epoch, 1);
    // Once the snapshot follows the registry, the same bundle goes through.
    s.shared
        .directory
        .store(Directory::from_registry(&s.registry).await.unwrap());
    let (st, reply) = commit(&s, bob, &b).await;
    assert_eq!(st, 200, "{reply}");
    s.people[bob].mls.merge();
    let c = s.pending(alice).await.pop().unwrap();
    s.people[alice]
        .mls
        .receive_checked(
            &songbird_groups::wire::Bundle {
                commit: &c,
                welcome: None,
                group_info: b"-",
                log: None,
            }
            .encode(),
            &members,
        )
        .unwrap();
    let row = s.registry.group(&GID).await.unwrap().unwrap();
    let roster = songbird_send::Roster::from_bytes(row.epoch, &row.roster).unwrap();
    assert_eq!(roster.members.len(), 2);
    assert!(
        !roster.contains(&s.people[carol].mls.id),
        "Carol's leaf is gone"
    );
    assert_eq!(s.people[alice].mls.roster(), s.people[bob].mls.roster());
    assert!(s.registry.groups_to_prune(10).await.unwrap().is_empty());

    // Her account's rows go; nothing names her devices any more.
    let devices = s.registry.account_devices(&account).await.unwrap();
    s.registry.delete_account_rows(&account).await.unwrap();
    let left = s.registry.remnants(&account, &devices).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
}

/// A fanned-out commit (the `MLSMessage` alone) as a bundle the harness reads.
fn bundle_of(commit: &[u8]) -> Vec<u8> {
    songbird_groups::wire::Bundle {
        commit,
        welcome: None,
        group_info: b"-",
        log: None,
    }
    .encode()
}

#[tokio::test]
async fn a_channels_rate_class_is_shared_by_the_websocket_and_http() {
    // D-0077: Alice posts to her channel through a delivery node's WebSocket path and through
    // `api`'s HTTP path, alternately, at once. Each node's own budget would admit 10 + 10; the
    // class in `valkey-hot` (1 a second, burst 10) admits about 10 in all, and Valkey never
    // sees the group ID.
    let Some(mut s) = stack(2).await else { return };
    let (alice, bob) = (0, 1);
    let (info, tree) = s.people[alice]
        .mls
        .create_with(songbird_groups::roles::Policy::CHANNEL);
    let (st, _) = s.people[alice]
        .api
        .send("POST", "/v1/groups", Some(&body(&parts(&info, &tree))))
        .await
        .unwrap();
    assert_eq!(st, 204);
    add(&mut s, &[bob]).await;

    let mut accepted = 0;
    let mut limited = 0;
    for k in 0..24u8 {
        let frame = group_frame(100 + k, s.people[alice].mls.message(&[k]));
        let st = if k % 2 == 0 {
            s.shared
                .send_path()
                .send(
                    &s.people[alice].mls.id,
                    frame,
                    std::time::Instant::now(),
                    &mut Vec::new(),
                )
                .await
        } else {
            s.people[alice]
                .api
                .put_message(&frame)
                .await
                .unwrap()
                .1
                .unwrap()
                .1
        };
        match st {
            SendStatus::Accepted => accepted += 1,
            SendStatus::Rejected {
                reason: RejectReason::RateLimited,
                retry_after_ms,
            } => {
                assert!((1..=1_000).contains(&retry_after_ms), "{retry_after_ms}");
                limited += 1;
            }
            other => panic!("{other:?}"),
        }
    }
    // A post or two may refill while the loop runs (1 a second).
    assert!((10..=12).contains(&accepted), "accepted {accepted}");
    assert_eq!(accepted + limited, 24);
    assert_eq!(
        s.messages().metrics.channel_local_only.get()
            + s.shared.send_metrics.channel_local_only.get(),
        0
    );
    // The record is a keyed hash: no group ID in `valkey-hot`.
    let mut hot = redis::Client::open(std::env::var("SONGBIRD_TEST_VALKEY").unwrap())
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("q:c:*")
        .query_async(&mut hot)
        .await
        .unwrap();
    assert!(!keys.is_empty());
    let gid = hex::encode(GID);
    assert!(keys.iter().all(|k| !k.contains(&gid)), "{keys:?}");
}

/// A commit with the `Group-Add-Access-Key` header (Prompt 9, D-0103).
async fn commit_keyed(s: &Stack, who: usize, bundle: &[u8], key: [u8; 16]) -> (u16, String) {
    let b = body(bundle);
    let (authz, hash) = s.people[who]
        .api
        .sign("POST", "/v1/groups/commit", Some(&b));
    let req = hyper::Request::builder()
        .method("POST")
        .uri("/v1/groups/commit")
        .header("host", API)
        .header(songbird_api::auth::AUTHORIZATION, authz)
        .header(songbird_api::auth::CONTENT_SHA256, hash.unwrap())
        .header("group-add-access-key", hex::encode(key));
    let (st, reply) = s.people[who].api.raw(req, Bytes::from(b)).await.unwrap();
    (st, String::from_utf8_lossy(&reply).into_owned())
}

/// Prompt 9, M2 (D-0103): an account that takes group adds from contacts only is added only by
/// a commit carrying its access key (or after it asked to join); others are added as before.
#[tokio::test]
async fn group_adds_from_contacts_only_need_the_access_key() {
    let Some(mut s) = stack(4).await else { return };
    let (careful, asked, open) = (1, 2, 3);
    let key = [0x31; 16];
    for (i, k) in [(careful, key), (asked, [0x32; 16])] {
        let a = s.people[i].mls.account;
        s.registry
            .put_profile(&a, 1, b"profile", &k, false)
            .await
            .unwrap();
        s.registry
            .set_groups_contacts(&a, true, false)
            .await
            .unwrap();
    }
    s.shared
        .directory
        .store(Directory::from_registry(&s.registry).await.unwrap());
    create(&mut s, 0).await;

    // No key, and a wrong key: refused alike, nothing stored.
    let epoch = s.registry.group(&GID).await.unwrap().unwrap().epoch;
    let kp = published(&s, careful, 0).await;
    let b = s.people[0].mls.add(&[kp]);
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!((st, reply["error"].as_str()), (403, Some("access")));
    let (st, reply) = commit_keyed(&s, 0, &b, [0x99; 16]).await;
    assert_eq!(st, 403, "{reply}");
    assert_eq!(s.registry.group(&GID).await.unwrap().unwrap().epoch, epoch);
    // With its key (Alice is a contact): added.
    let (st, reply) = commit_keyed(&s, 0, &b, key).await;
    assert_eq!(st, 200, "{reply}");
    s.people[0].mls.merge();

    // An account that asked to join (a pending request) is added without its key.
    let asked_account = s.people[asked].mls.account;
    s.registry
        .db()
        .exec(
            "CREATE grp_req:[$g, $a] SET grp = $g, acct = $a, at = time::now();",
            vec![
                (
                    "g".into(),
                    songbird_surreal::Value::Bytes(Bytes::copy_from_slice(&GID)),
                ),
                (
                    "a".into(),
                    songbird_surreal::Value::Bytes(Bytes::copy_from_slice(
                        asked_account.as_bytes(),
                    )),
                ),
            ],
        )
        .await
        .unwrap();
    let kp = published(&s, asked, 0).await;
    let b = s.people[0].mls.add(&[kp]);
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!(st, 200, "{reply}");
    s.people[0].mls.merge();

    // An account that takes adds from everyone needs nothing.
    let kp = published(&s, open, 0).await;
    let b = s.people[0].mls.add(&[kp]);
    let (st, reply) = commit(&s, 0, &b).await;
    assert_eq!(st, 200, "{reply}");
}

/// Prompt 9, M4 (D-0105): a supervised child whose policy needs the parent's approval for groups
/// is added only with an approval for that group, signed by the parent's policy key and not
/// expired; contact keys alone are not enough.
#[tokio::test]
async fn a_supervised_child_joins_a_group_only_with_the_parents_approval() {
    use songbird_content::supervision::PolicyKey;
    use songbird_proto::supervision::{GROUP_APPROVAL, Policy};
    let Some(mut s) = stack(2).await else { return };
    let child = 1;
    let account = s.people[child].mls.account;
    let parent = PolicyKey::generate().unwrap();
    let mut p = Policy {
        child: account,
        policy_key: [0; 32],
        policy_version: 1,
        flags: GROUP_APPROVAL,
        unlink_at_s: 4_000_000_000,
        client_rules: Vec::new(),
        signature: [0; 64],
    };
    parent.sign(&mut p);
    let access = [0x41; 16];
    s.registry
        .put_profile(&account, 1, b"profile", &access, false)
        .await
        .unwrap();
    s.registry
        .put_supervision(&p, &p.encode(), true)
        .await
        .unwrap();
    s.shared
        .directory
        .store(Directory::from_registry(&s.registry).await.unwrap());
    create(&mut s, 0).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // The contact key alone, an approval for another group, an expired one, one too far ahead,
    // and one signed by another key: all refused.
    let (st, reply) = approval_commit(&mut s, child, access, None).await;
    assert_eq!((st, &reply[..]), (403, &br#"{"error":"approval"}"#[..]));
    for bad in [
        parent.approve(&[9; 16], account.as_bytes(), now + 600),
        parent.approve(&GID, account.as_bytes(), now - 1),
        parent.approve(&GID, account.as_bytes(), now + 30 * 24 * 3600),
        PolicyKey::generate()
            .unwrap()
            .approve(&GID, account.as_bytes(), now + 600),
    ] {
        assert_eq!(
            approval_commit(&mut s, child, access, Some(bad)).await.0,
            403
        );
    }
    // The parent's approval for this group: added.
    let ok = parent.approve(&GID, account.as_bytes(), now + 600);
    let (st, reply) = approval_commit(&mut s, child, access, Some(ok)).await;
    assert_eq!(st, 200, "{}", String::from_utf8_lossy(&reply));
}

/// Alice adds person `who` with the contact key and an optional approval; a refused commit is
/// discarded from Alice's group state.
async fn approval_commit(
    s: &mut Stack,
    who: usize,
    access: [u8; 16],
    approval: Option<Vec<u8>>,
) -> (u16, Bytes) {
    let kp = published(s, who, 0).await;
    let b = body(&s.people[0].mls.add(&[kp]));
    let (authz, hash) = s.people[0].api.sign("POST", "/v1/groups/commit", Some(&b));
    let mut req = hyper::Request::builder()
        .method("POST")
        .uri("/v1/groups/commit")
        .header("host", API)
        .header(songbird_api::auth::AUTHORIZATION, authz)
        .header(songbird_api::auth::CONTENT_SHA256, hash.unwrap())
        .header("group-add-access-key", hex::encode(access));
    if let Some(a) = approval {
        req = req.header("group-add-approval", hex::encode(a));
    }
    let out = s.people[0].api.raw(req, Bytes::from(b)).await.unwrap();
    if out.0 != 200 {
        s.people[0].mls.discard();
    }
    out
}
