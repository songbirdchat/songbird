//! A2's gate, first part (D-0138): two app-cores exchange end-to-end encrypted 1:1 messages and
//! receipts through the real `api` and `delivery`, against the real registry (SurrealDB) and
//! `valkey-hot`. Each side has its own `SQLCipher` store; keys, the signed device list and the
//! account identity key go through the endpoints the app uses. Runs when the compose stack is
//! configured, like `send_api`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::large_futures,
    reason = "tests"
)]

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use bytes::Bytes;
use songbird_accounts::{Db, NewAccount, Registry};
use songbird_api::client::ApiClient;
use songbird_api::messages::{Messages, SendStack};
use songbird_api::{ApiMetrics, App};
use songbird_app_core::crypto::X25519Pair;
use songbird_app_core::messenger::{DeviceIdBytes, Messenger, Net, Received};
use songbird_app_core::session::Local;
use songbird_app_core::store::{Status, Store};
use songbird_client::{Client, ClientConfig, DeviceKey, Event, ResumeState, State};
use songbird_delivery::{DeliveryMetrics, Directory, Shared, server};
use songbird_e2e::{SERVER_NAME, settings, test_mailbox};
use songbird_proto::frame::{Recipient, SendHints, SendStatus};
use songbird_router::LocalRouter;
use songbird_types::{AccountId, DeviceId, Secret};
use tokio::sync::{mpsc, watch};

const API: &str = "api.test";
const SYSTEM_SEED: [u8; 32] = [0x51; 32];
const KT_LOG_SEED: [u8; 32] = [0x61; 32];
const KT_VRF_SEED: [u8; 32] = [0x62; 32];

/// Each stack's key-transparency store, by its api address (tests run side by side).
static KT_STORES: std::sync::Mutex<Vec<(std::net::SocketAddr, songbird_akd::SurrealStore)>> =
    std::sync::Mutex::new(Vec::new());

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

/// The network for one app-core: signed `api` requests and the delivery client.
struct TestNet {
    api: ApiClient,
    addr: std::net::SocketAddr,
    ws: Arc<Client>,
    ws_addr: std::net::SocketAddr,
}

impl Net for TestNet {
    async fn api(
        &self,
        method: &str,
        path: &str,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<(u16, Vec<u8>)> {
        let has_body = !body.is_empty();
        let (authz, hash) = self
            .api
            .sign(method, path, has_body.then_some(body.as_slice()));
        let mut req = hyper::Request::builder()
            .method(method)
            .uri(path)
            .header("host", API)
            .header(songbird_api::auth::AUTHORIZATION, authz);
        if let Some(h) = hash {
            req = req.header(songbird_api::auth::CONTENT_SHA256, h);
        }
        let (status, b) = self
            .api
            .raw(req, Bytes::from(body))
            .await
            .map_err(|_| songbird_app_core::Error::Network)?;
        Ok((status, b.to_vec()))
    }

    async fn upload(
        &self,
        method: &str,
        path: &str,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<(u16, Vec<u8>)> {
        let len = body.len().to_string();
        let (status, _, b) = self
            .api
            .upload_with(method, path, &[("content-length", len)], Bytes::from(body))
            .await
            .map_err(|_| songbird_app_core::Error::Network)?;
        Ok((status, b.to_vec()))
    }

    async fn fetch(&self, path: &str) -> songbird_app_core::Result<(u16, Vec<u8>, Option<String>)> {
        let (status, headers, body) =
            songbird_api::client::unsigned_full(self.addr, API, "GET", path, &[])
                .await
                .map_err(|_| songbird_app_core::Error::Network)?;
        let location = headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok((status, body.to_vec(), location))
    }

    async fn fetch_post(&self, path: &str) -> songbird_app_core::Result<u16> {
        songbird_api::client::unsigned_body(self.addr, API, "POST", path, &[], Bytes::new())
            .await
            .map(|(s, _)| s)
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn send_group(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        body: Vec<u8>,
    ) -> songbird_app_core::Result<SendStatus> {
        self.ws
            .send_with(
                idempotency_key,
                hints,
                songbird_proto::frame::SendPayload::Shared {
                    devices: Vec::new(),
                    body: Bytes::from(body),
                },
            )
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn ephemeral(
        &self,
        device: DeviceIdBytes,
        payload: Vec<u8>,
    ) -> songbird_app_core::Result<()> {
        self.ws
            .ephemeral(DeviceId::from_bytes(device), Bytes::from(payload))
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn attention(
        &self,
        device: DeviceIdBytes,
        payload: Vec<u8>,
    ) -> songbird_app_core::Result<()> {
        self.ws
            .attention(DeviceId::from_bytes(device), Bytes::from(payload))
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }

    async fn send(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        recipients: Vec<(DeviceIdBytes, Vec<u8>)>,
    ) -> songbird_app_core::Result<SendStatus> {
        let r = recipients
            .into_iter()
            .map(|(d, e)| Recipient {
                device: DeviceId::from_bytes(d),
                envelope: Bytes::from(e),
            })
            .collect();
        self.ws
            .send_hinted(idempotency_key, hints, r)
            .await
            .map_err(|_| songbird_app_core::Error::Network)
    }
}

/// One person: an account with one device, its app-core and its delivery events.
struct Person {
    account: [u8; 16],
    account_key: Ed25519KeyPair,
    core: Messenger<TestNet>,
    events: mpsc::Receiver<Event>,
}

struct Registered {
    n: NewAccount,
    account_key: Ed25519KeyPair,
    auth_seed: [u8; 32],
}

async fn register(registry: &Registry) -> Registered {
    let account_key = Ed25519KeyPair::generate().unwrap();
    let device = DeviceId::random().unwrap();
    let (_, auth_seed) = DeviceKey::generate(device).unwrap();
    let auth = DeviceKey::from_seed(device, &auth_seed).unwrap();
    let n = NewAccount {
        account: AccountId::random().unwrap(),
        identity: account_key.public_key().as_ref().try_into().unwrap(),
        device,
        auth_key: auth.public_key(),
        allowance: 3,
    };
    registry.register(&n, None).await.unwrap();
    Registered {
        n,
        account_key,
        auth_seed,
    }
}

async fn person(r: Registered, api: std::net::SocketAddr, ws: std::net::SocketAddr) -> Person {
    let mut c = ClientConfig::new(ws.to_string(), SERVER_NAME);
    c.backoff.initial = Duration::from_millis(20);
    let (mut client, events) = Client::start(
        c,
        DeviceKey::from_seed(r.n.device, &r.auth_seed).unwrap(),
        ResumeState::default(),
    );
    client.wait_for(State::Connected).await;
    let net = TestNet {
        addr: api,
        api: ApiClient::new(
            api,
            API,
            r.n.device,
            Ed25519KeyPair::from_seed_unchecked(&r.auth_seed).unwrap(),
        ),
        ws: Arc::new(client),
        ws_addr: ws,
    };
    let local = Local {
        account: *r.n.account.as_bytes(),
        device: *r.n.device.as_bytes(),
        identity: X25519Pair::generate().unwrap(),
    };
    let store = Store::memory(&random32()).unwrap();
    let mut core =
        Messenger::create(store, net, local, Some(songbird_send_system_public())).unwrap();
    core.publish_keys(true).await.unwrap();
    core.publish_key_packages(5, true).await.unwrap();
    core.publish_device_list(&r.account_key, |_, _| false)
        .await
        .unwrap();
    // Every app publishes a profile with its access key at registration (PROTOCOL §5.10).
    core.publish_profile(&songbird_content::profile::ProfilePlain::default(), false)
        .await
        .unwrap();
    Person {
        account: *r.n.account.as_bytes(),
        account_key: r.account_key,
        core,
        events,
    }
}

fn songbird_send_system_public() -> [u8; 32] {
    Ed25519KeyPair::from_seed_unchecked(&SYSTEM_SEED)
        .unwrap()
        .public_key()
        .as_ref()
        .try_into()
        .unwrap()
}

/// Delivers the next envelope to `p`'s app-core, then acknowledges it.
async fn pump(p: &mut Person) -> Received {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(10), p.events.recv())
            .await
            .expect("an event within 10 s")
            .expect("events open");
        if let Event::Deliver { seq, envelope } = ev {
            let r = p.core.receive(&envelope).await.unwrap();
            p.core.net().ws.ack(seq).await.unwrap();
            return r;
        }
    }
}

async fn stack() -> Option<(Arc<App>, Person, Person, watch::Sender<bool>)> {
    let surreal = env("SONGBIRD_TEST_SURREALDB")?;
    let hot = env("SONGBIRD_TEST_VALKEY")?;
    let s3_addr = env("SONGBIRD_TEST_RUSTFS")?;
    let s3 = songbird_s3::Client::new(songbird_s3::Config::new(
        s3_addr,
        format!("gatea2-{}", &hex::encode(random32())[..12]),
        Secret::new(env("SONGBIRD_TEST_RUSTFS_ACCESS_KEY")?),
        Secret::new(env("SONGBIRD_TEST_RUSTFS_SECRET_KEY")?),
    ))
    .unwrap();
    s3.create_bucket().await.unwrap();
    let cfg = songbird_surreal::Config {
        addr: surreal,
        host: "localhost".into(),
        tls: None,
        user: std::env::var("SONGBIRD_TEST_SURREALDB_USER").unwrap_or_else(|_| "root".into()),
        pass: std::env::var("SONGBIRD_TEST_SURREALDB_PASS").unwrap(),
        namespace: "test".into(),
        database: format!("gatea2_{}", DeviceId::random().unwrap().to_hex_unredacted()),
        timeout: Duration::from_secs(30),
        max_message_bytes: 16 << 20,
    };
    let db = Db::connect(cfg.clone()).await.unwrap();
    let akd_store = songbird_akd::SurrealStore::connect(Arc::new(Db::connect(cfg).await.unwrap()))
        .await
        .unwrap();
    let registry = Arc::new(Registry::connect(db).await.unwrap());
    let (alice, bob) = (register(&registry).await, register(&registry).await);
    // The production send policy: franking stamps and the contact gate (requests, access keys).
    let secret = random32();
    let limits = songbird_send::RequestLimits::default();
    // One gate key for the send gate, blocks and reports, as a deployment has.
    let keys = songbird_send::GateKeys::from_cluster_secret(&random32()).unwrap();
    let moderation = songbird_keys::provisioning::ProvisioningKey::generate().unwrap();
    let gate: Arc<dyn songbird_send::Gate> = Arc::new(songbird_send::RegistryGate::new(
        Arc::clone(&registry),
        songbird_valkey::ValkeyConn::connect(redis::Client::open(hot.as_str()).unwrap())
            .await
            .unwrap(),
        keys.clone(),
        limits,
    ));
    let policy = || songbird_send::Policy {
        franking: Some(songbird_send::Franker::from_secret(&secret).unwrap()),
        gate: Some(Arc::clone(&gate)),
        max_broadcast_accounts: limits.max_broadcast_accounts,
        rosters: Some(Arc::new(songbird_send::RegistryRosters::new(
            Arc::clone(&registry),
            songbird_send::groups::ROSTER_TTL,
        ))),
        channels: Arc::default(),
        standings: None,
    };

    let mailbox = Arc::new(test_mailbox().await);
    let router = Arc::new(LocalRouter::new());
    let mut cfg = settings();
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let listener = tokio::net::TcpListener::bind(cfg.listen).await.unwrap();
    let ws_addr = listener.local_addr().unwrap();
    let shared = Arc::new(
        Shared::new(
            cfg,
            Directory::from_registry(&registry).await.unwrap(),
            Arc::clone(&mailbox),
            Arc::clone(&router),
            DeliveryMetrics::default(),
        )
        .with_policy(policy()),
    );
    let (stop, stop_rx) = watch::channel(false);
    tokio::spawn(server::serve(
        listener,
        None,
        Arc::clone(&shared),
        stop_rx.clone(),
    ));
    // Profiles (access keys) reach the directory snapshot as a deployment's do.
    tokio::spawn(songbird_delivery::directory::follow_registry(
        Arc::clone(&registry),
        Arc::clone(&shared.directory),
        Arc::clone(&shared.generation),
        registry.directory_version().await.unwrap(),
        Duration::from_millis(50),
        stop_rx.clone(),
    ));

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
            policy: policy(),
        }),
        limits: songbird_proto::Limits::DEFAULT,
        sends_per_sec: 50,
        send_burst: 200,
        group_sends_per_sec: 50,
        group_send_burst: 200,
        rate_key: songbird_api::messages::rate_key(&random32()).unwrap(),
        metrics: metrics.send.clone(),
        groups: Some({
            let mut g =
                songbird_api::groups::Groups::new(&random32(), songbird_groups::Limits::default())
                    .unwrap();
            // A test does in a second what people do over minutes; the budgets themselves are
            // `gate_p05`'s.
            g.commits_per_sec = 1_000;
            g.commit_burst = 1_000;
            g.links_per_sec = 1_000;
            g.link_burst = 1_000;
            g
        }),
    };
    let app = Arc::new(App {
        plans: songbird_api::plans::Plans::default(),
        calls: {
            let lk = env("SONGBIRD_TEST_LIVEKIT")?;
            let calls = songbird_calls::Calls::new(
                &random32(),
                &env("SONGBIRD_TEST_LIVEKIT_KEY")?,
                zeroize::Zeroizing::new(env("SONGBIRD_TEST_LIVEKIT_SECRET")?),
                &lk.replacen("http", "ws", 1),
                &lk,
                None,
                songbird_calls::CallLimits::default(),
            )
            .unwrap();
            Some(songbird_api::calls::CallsApi {
                calls: Arc::new(calls),
                metrics: ApiMetrics::default().calls.clone(),
            })
        },
        requests: Some(songbird_api::requests::Requests::new(
            keys.clone(),
            songbird_api::requests::RequestsMetrics::default(),
        )),
        reports: Some(songbird_api::reports::Reports::new(
            songbird_send::Frankers {
                current: songbird_send::Franker::from_secret(&secret).unwrap(),
                previous: Vec::new(),
            },
            keys.clone(),
            moderation.public(),
            songbird_api::config::ReportSettings {
                moderation_key: hex::encode(moderation.public()),
                retention_days: 30,
                window_days: 30,
                per_hour: 100,
                burst: 20,
            },
            songbird_api::reports::ReportMetrics::default(),
        )),
        support: None,
        server_name: API.into(),
        push_store: Arc::new(
            songbird_accounts::PushStore::beside(&registry)
                .await
                .unwrap(),
        ),
        registry: Arc::clone(&registry),
        s3,
        queue: Arc::new(
            songbird_jobs::Queue::connect(
                hot_client.clone(),
                &[3; 32],
                songbird_jobs::QueueConfig::new(format!(
                    "gate-a2-{}",
                    &hex::encode(random32())[..8]
                )),
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
        media: Some(songbird_api::media::Media::new(
            songbird_api::config::MediaBudgets {
                grants_per_hour: 10_000,
                grant_burst: 1_000,
                bytes_per_day: 1 << 34,
            },
            songbird_api::config::MediaDownloads {
                per_hour: 100_000,
                burst: 10_000,
                ..songbird_api::config::MediaDownloads::default()
            },
            &random32(),
        )),
        metrics,
        messages: Some(messages),
        registration: None,
        usernames: None,
        keys: Some(
            songbird_api::keys::KeyDirectory::new(
                songbird_api::config::KeySettings {
                    secret: songbird_types::SecretRef::Env("UNUSED".into()),
                    system_key: None,
                    max_stored: 200,
                    refill_below: 10,
                    throttle: songbird_api::config::KeyThrottle::default(),
                    devices: songbird_api::config::DeviceSettings::default(),
                },
                &random32(),
                Some(&SYSTEM_SEED),
                songbird_api::keys::KeyMetrics::default(),
            )
            .unwrap(),
        ),
        storage: None,
        vault: None,
        kt: Some(
            songbird_api::kt::Transparency::new(
                songbird_api::config::KtSettings {
                    secret: songbird_types::SecretRef::Env("UNUSED".into()),
                    log_key: songbird_types::SecretRef::Env("UNUSED".into()),
                    vrf_key: songbird_types::SecretRef::Env("UNUSED".into()),
                    fetches_per_hour: 100_000,
                    fetch_burst: 10_000,
                },
                &random32(),
                &KT_LOG_SEED,
                &KT_VRF_SEED,
                akd_store.clone(),
            )
            .unwrap(),
        ),
        profiles: Some(
            songbird_api::profiles::Profiles::new(
                songbird_api::config::ProfileSettings {
                    secret: songbird_types::SecretRef::Env("UNUSED".into()),
                    certificate_key: None,
                    sealed_sender: false,
                    certificate_hours: 24,
                    fetches_per_hour: 600,
                    fetch_burst: 120,
                    sealed_per_hour: 3_600,
                    sealed_per_account_hour: 3_600,
                    sealed_burst: 200,
                    presence_per_hour: 1_000_000,
                    presence_burst: 20_000,
                    policy_updates_per_hour: 120,
                    policy_update_burst: 20,
                    ipv4_prefix: 24,
                    ipv6_prefix: 48,
                },
                &random32(),
                None,
                songbird_api::profiles::ProfileMetrics::default(),
            )
            .unwrap(),
        ),
        devices: Some(
            songbird_api::devices::Devices::new(
                songbird_api::config::DeviceSettings::default(),
                &songbird_api::config::KeyThrottle::default(),
                &random32(),
                Some(&SYSTEM_SEED),
                songbird_api::devices::DeviceMetrics::default(),
            )
            .unwrap(),
        ),
    });
    let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_addr = api_listener.local_addr().unwrap();
    tokio::spawn(songbird_api::server::serve(
        api_listener,
        None,
        Arc::clone(&app),
        64,
        stop_rx,
    ));
    KT_STORES.lock().unwrap().push((api_addr, akd_store));
    let alice = person(alice, api_addr, ws_addr).await;
    let bob = person(bob, api_addr, ws_addr).await;
    // Let the directory follower see both profiles.
    tokio::time::sleep(Duration::from_millis(300)).await;
    Some((app, alice, bob, stop))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_app_cores_exchange_messages_and_receipts_end_to_end() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let _ = &alice.account_key;
    // Alice writes first, as a message request (she has no access key of Bob's): she resolves
    // Bob's identity and signed list, fetches his bundle, starts a session (PQXDH) and sends,
    // franked; the server stamps and accepts it (one tick).
    let id = alice
        .core
        .send_text(&bob.account, "Hello Bob, this is end-to-end")
        .await
        .unwrap();
    let mine = alice
        .core
        .store
        .message(&alice.account, &id)
        .unwrap()
        .unwrap();
    assert_eq!(mine.status, Status::Sent);

    // Bob receives, decrypts, stores; his app-core sends a delivery receipt back.
    let Received::Message(m) = pump(&mut bob).await else {
        panic!("a message")
    };
    assert_eq!(m.text, "Hello Bob, this is end-to-end");
    assert_eq!((m.chat, m.author, m.id), (alice.account, alice.account, id));
    assert_eq!(bob.core.store.chats().unwrap()[0].unread, 1);
    // A stranger's first message is a request on Bob's side; Alice wrote, so not on hers (D-0184).
    assert!(bob.core.is_request(&alice.account).unwrap());
    assert!(!alice.core.is_request(&bob.account).unwrap());

    // Alice gets the receipt: two ticks.
    assert_eq!(
        pump(&mut alice).await,
        Received::Status {
            id,
            status: Status::Delivered
        }
    );

    // Bob reads the chat: a read receipt.
    bob.core.mark_read(&alice.account).await.unwrap();
    assert_eq!(
        pump(&mut alice).await,
        Received::Status {
            id,
            status: Status::Read
        }
    );
    assert_eq!(
        alice
            .core
            .store
            .message(&alice.account, &id)
            .unwrap()
            .unwrap()
            .status,
        Status::Read
    );

    // Bob answers on the session Alice started (replying shares his profile key, accepting the
    // request); from then on both send as contacts, with each other's access key.
    for i in 0..3 {
        let b = bob
            .core
            .send_text(&alice.account, &format!("reply {i}"))
            .await
            .unwrap();
        let Received::Message(m) = pump(&mut alice).await else {
            panic!("a reply")
        };
        assert_eq!((m.id, m.text.as_str()), (b, format!("reply {i}").as_str()));
        assert!(matches!(
            pump(&mut bob).await,
            Received::Status {
                status: Status::Delivered,
                ..
            }
        ));
        let a = alice
            .core
            .send_text(&bob.account, &format!("again {i}"))
            .await
            .unwrap();
        let Received::Message(m) = pump(&mut bob).await else {
            panic!("a message")
        };
        assert_eq!(m.id, a);
        assert!(matches!(
            pump(&mut alice).await,
            Received::Status {
                status: Status::Delivered,
                ..
            }
        ));
    }
    // Replying accepted Alice's request.
    assert!(!bob.core.is_request(&alice.account).unwrap());
    // Search runs over the encrypted store.
    assert_eq!(bob.core.store.search("end-to-end", 10).unwrap().len(), 1);
    assert_eq!(
        alice
            .core
            .store
            .messages(&bob.account, None, 50)
            .unwrap()
            .len(),
        7
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_is_accepted_or_deleted_without_a_reply() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    alice
        .core
        .send_text(&bob.account, "Hi, we met at the conference")
        .await
        .unwrap();
    assert!(matches!(pump(&mut bob).await, Received::Message(_)));
    assert!(matches!(
        pump(&mut alice).await,
        Received::Status {
            status: Status::Delivered,
            ..
        }
    ));
    assert!(bob.core.is_request(&alice.account).unwrap());
    // Accepting tells Alice (her app gets Bob's profile key) and ends the request at once.
    bob.core.accept_request(&alice.account).await.unwrap();
    assert!(!bob.core.is_request(&alice.account).unwrap());
    assert_eq!(pump(&mut alice).await, Received::Nothing);
    assert!(alice.core.store.profile_key(&bob.account).unwrap().is_some());
    // Deleting a chat removes it and its messages from Bob's store; Alice is not told.
    bob.core.delete_chat(&alice.account).unwrap();
    assert_eq!(bob.core.store.chats().unwrap(), Vec::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_photo_goes_encrypted_and_arrives_verified() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // A file larger than one 64 KiB chunk, so the stream crosses chunks.
    let photo: Vec<u8> = (0..300_000u32)
        .map(|i| u8::try_from(i % 241).unwrap())
        .collect();
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: "image/jpeg".into(),
        file_name: "IMG_0001.jpg".into(),
        width: 1200,
        height: 900,
        ..Default::default()
    };
    let id = alice
        .core
        .send_files(&bob.account, vec![(photo.clone(), meta)], "")
        .await
        .unwrap();
    let Received::Message(m) = pump(&mut bob).await else {
        panic!("a message")
    };
    assert_eq!((m.id, m.text.as_str()), (id, "Photo"));
    // Bob's app-core reads the pointer from the stored content and downloads the object.
    let content =
        <songbird_content::schema::Content as prost::Message>::decode(m.content.as_slice())
            .unwrap();
    let Some(songbird_content::schema::content::Body::Data(d)) = content.body else {
        panic!("data")
    };
    let Some(songbird_content::schema::data_message::Kind::Message(msg)) = d.kind else {
        panic!("message")
    };
    let pointer = &msg.attachments[0];
    assert_eq!(pointer.size, 300_000);
    // A changed digest is refused before any byte is trusted (and nothing is confirmed).
    let mut forged = pointer.clone();
    forged.digest[0] ^= 1;
    assert!(
        songbird_app_core::attachments::download(bob.core.net(), &forged)
            .await
            .is_err()
    );
    // The app's path: `file` downloads, verifies, confirms and keeps it (encrypted in the store);
    // the next read comes from the store, and the sender has its own copy already.
    let author = alice.account;
    assert_eq!(&*bob.core.file(&author, &id, 0).await.unwrap(), &photo);
    assert!(bob.core.store.file(&author, &id, 0).unwrap().is_some());
    assert_eq!(&*bob.core.file(&author, &id, 0).await.unwrap(), &photo);
    assert_eq!(&*alice.core.file(&author, &id, 0).await.unwrap(), &photo);
    // Its one recipient device confirmed it: the server deleted the object.
    assert!(
        songbird_app_core::attachments::download(bob.core.net(), pointer)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn map_tiles_come_from_our_storage_and_decode_on_the_device() {
    use prost::Message as _;
    use songbird_app_core::maps::{Class, Feature, Layer, Tile, Value, shapes};
    let Some((app, alice, _bob, _stop)) = stack().await else {
        return;
    };
    // What `tools/maps-import` stores: a raw vector tile at maps/v1/z/x/y.mvt.
    let tile = Tile {
        layers: vec![Layer {
            name: "water".into(),
            features: vec![Feature {
                tags: vec![0, 0],
                r#type: Some(3),
                geometry: vec![9, 0, 0, 18, 4096, 0, 0, 4096, 15],
            }],
            keys: vec!["class".into()],
            values: vec![Value {
                string_value: Some("lake".into()),
                int_value: None,
                uint_value: None,
            }],
            extent: Some(4096),
        }],
    }
    .encode_to_vec();
    app.s3
        .put_object_bytes(
            &songbird_api::maps::object_name(12, 2316, 1440),
            Bytes::from(tile),
        )
        .await
        .unwrap();
    let net = alice.core.net();
    let (status, body, _) = net.fetch("/v1/maps/v1/12/2316/1440").await.unwrap();
    assert_eq!(status, 200);
    let s = shapes(&body, 12).unwrap();
    assert_eq!(s[0].class, Class::Water);
    // An empty tile is 204; outside the grid is 404. No account is named in any of it.
    assert_eq!(net.fetch("/v1/maps/v1/12/2316/1441").await.unwrap().0, 204);
    assert_eq!(net.fetch("/v1/maps/v1/12/9999/1440").await.unwrap().0, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_picked_place_arrives_as_that_place() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let id = alice
        .core
        .send_location(&bob.account, 46.769_7, 23.589_6, "Piața Unirii")
        .await
        .unwrap();
    let Received::Message(m) = pump(&mut bob).await else {
        panic!("a message")
    };
    assert_eq!((m.id, m.text.as_str()), (id, "Location"));
    let (lat, lon, name) = songbird_app_core::messenger::location_of(&m.content).unwrap();
    assert!((lat - 46.769_7).abs() < 1e-7 && (lon - 23.589_6).abs() < 1e-7);
    assert_eq!(name, "Piața Unirii");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reactions_edits_and_deletes_reach_the_other_side() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let id = alice
        .core
        .send_text(&bob.account, "first draft")
        .await
        .unwrap();
    assert!(matches!(pump(&mut bob).await, Received::Message(_)));
    assert!(matches!(pump(&mut alice).await, Received::Status { .. }));
    // Bob reacts; Alice sees it, and it is stored on both sides.
    bob.core
        .react(&alice.account, &alice.account, &id, "👍")
        .await
        .unwrap();
    let r = pump(&mut alice).await;
    assert_eq!(
        r,
        Received::Reaction {
            author: alice.account,
            id,
            reactor: bob.account,
            emoji: "👍".into()
        }
    );
    assert_eq!(
        alice.core.store.reactions(&alice.account, &id).unwrap(),
        vec![(bob.account, "👍".to_owned())]
    );
    // Alice edits: Bob's copy changes.
    alice
        .core
        .edit(&bob.account, &id, "final text")
        .await
        .unwrap();
    assert_eq!(
        pump(&mut bob).await,
        Received::Edited {
            author: alice.account,
            id,
            text: "final text".into()
        }
    );
    assert_eq!(
        bob.core
            .store
            .message(&alice.account, &id)
            .unwrap()
            .unwrap()
            .text,
        "final text"
    );
    // Bob cannot edit or delete Alice's message: only its author can.
    bob.core
        .delete_for_everyone(&alice.account, &id)
        .await
        .unwrap_err();
    // Alice deletes it for everyone.
    alice
        .core
        .delete_for_everyone(&bob.account, &id)
        .await
        .unwrap();
    assert_eq!(
        pump(&mut bob).await,
        Received::Deleted {
            author: alice.account,
            id
        }
    );
    let gone = bob
        .core
        .store
        .message(&alice.account, &id)
        .unwrap()
        .unwrap();
    assert!(gone.text.is_empty() && gone.content.is_empty());
    assert_eq!(
        bob.core.store.reactions(&alice.account, &id).unwrap().len(),
        0
    );
}

/// The next ephemeral event for `p`, through its app-core.
async fn next_ephemeral(p: &mut Person) -> Received {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(10), p.events.recv())
            .await
            .expect("an event within 10 s")
            .expect("events open");
        if let Event::Ephemeral { payload } = ev {
            return p.core.receive(&payload).await.unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn typing_reports_and_blocks_work_for_real() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // A conversation first (sessions both ways, profile keys exchanged).
    alice.core.send_text(&bob.account, "hello").await.unwrap();
    assert!(matches!(pump(&mut bob).await, Received::Message(_)));
    assert!(matches!(pump(&mut alice).await, Received::Status { .. }));
    bob.core.send_text(&alice.account, "hi").await.unwrap();
    assert!(matches!(pump(&mut alice).await, Received::Message(_)));
    assert!(matches!(pump(&mut bob).await, Received::Status { .. }));

    // Typing goes on the ephemeral lane, end-to-end encrypted.
    alice.core.typing(&bob.account, true).await.unwrap();
    assert_eq!(
        next_ephemeral(&mut bob).await,
        Received::Typing {
            account: alice.account,
            started: true
        }
    );

    // Bob reports Alice's message: the server verifies the franking proof and accepts it once.
    assert_eq!(bob.core.report(&alice.account, 2, 5).await.unwrap(), 1);
    assert_eq!(
        bob.core.report(&alice.account, 2, 5).await.unwrap(),
        1,
        "a repeat is 409, counted"
    );

    // Bob blocks Alice: the server records it; Alice's later messages never reach Bob's chat.
    let new_key = [0x42u8; 32];
    bob.core.block(&alice.account, &new_key).await.unwrap();
    assert!(bob.core.store.is_blocked(&alice.account).unwrap());
    let _ = alice.core.send_text(&bob.account, "are you there?").await;
    let before = bob
        .core
        .store
        .messages(&alice.account, None, 50)
        .unwrap()
        .len();
    // Whatever reaches Bob's device from Alice is dropped there.
    if let Ok(Some(Event::Deliver { seq, envelope })) =
        tokio::time::timeout(Duration::from_secs(2), bob.events.recv()).await
    {
        assert_eq!(
            bob.core.receive(&envelope).await.unwrap(),
            Received::Nothing
        );
        bob.core.net().ws.ack(seq).await.unwrap();
    }
    assert_eq!(
        bob.core
            .store
            .messages(&alice.account, None, 50)
            .unwrap()
            .len(),
        before
    );
    bob.core.unblock(&alice.account).await.unwrap();
    assert!(!bob.core.store.is_blocked(&alice.account).unwrap());
}

/// The next envelope that is not a system notice (prekeys or KeyPackages low).
/// The next event after the change-log key a member hands out when someone joins by link
/// (D-0156), which arrives as an untitled group update.
async fn pump_past_key(p: &mut Person) -> Received {
    loop {
        let r = pump_app(p).await;
        if !matches!(&r, Received::GroupUpdated { title, .. } if title.is_empty()) {
            return r;
        }
    }
}

async fn pump_app(p: &mut Person) -> Received {
    loop {
        let r = pump(p).await;
        if r != Received::System {
            return r;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_is_created_joined_and_talked_in() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // Alice creates "Hiking" with Bob: the server validates the group and the commit, and fans
    // the Welcome out to Bob's device.
    let gid = alice
        .core
        .create_group("Hiking", &[bob.account])
        .await
        .unwrap();
    assert_eq!(alice.core.group_devices(&gid).unwrap().len(), 2);
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    assert_eq!(bob.core.group_devices(&gid).unwrap().len(), 2);
    // The title arrives as an encrypted group update.
    assert_eq!(
        pump_app(&mut bob).await,
        Received::GroupUpdated {
            group: gid,
            title: "Hiking".into()
        }
    );
    assert_eq!(bob.core.group_title(&gid).unwrap(), "Hiking");
    // Messages both ways, one shared body each, stored under the group.
    let id = alice
        .core
        .send_group_text(&gid, "Saturday at 8?")
        .await
        .unwrap();
    let Received::Message(m) = pump_app(&mut bob).await else {
        panic!("a group message")
    };
    assert_eq!(
        (m.chat, m.author, m.id, m.text.as_str()),
        (gid, alice.account, id, "Saturday at 8?")
    );
    let id = bob.core.send_group_text(&gid, "Count me in").await.unwrap();
    let Received::Message(m) = pump_app(&mut alice).await else {
        panic!("a group message")
    };
    assert_eq!(
        (m.chat, m.author, m.id, m.text.as_str()),
        (gid, bob.account, id, "Count me in")
    );
    assert!(
        alice
            .core
            .store
            .chats()
            .unwrap()
            .iter()
            .any(|c| c.id == gid && c.group)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn group_photos_removals_and_leaving() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let gid = alice
        .core
        .create_group("Trip", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));

    // A photo in the group: uploaded once for Bob's device, verified by Bob's app-core.
    let photo: Vec<u8> = (0..100_000u32)
        .map(|i| u8::try_from(i % 239).unwrap())
        .collect();
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: "image/jpeg".into(),
        file_name: "view.jpg".into(),
        ..Default::default()
    };
    let id = alice
        .core
        .send_group_files(&gid, vec![(photo.clone(), meta)], "")
        .await
        .unwrap();
    let Received::Message(m) = pump_app(&mut bob).await else {
        panic!("a photo")
    };
    assert_eq!((m.chat, m.text.as_str()), (gid, "Photo"));
    assert_eq!(
        &*bob.core.file(&alice.account, &id, 0).await.unwrap(),
        &photo
    );

    // Alice (the owner) removes Bob: Bob's device learns it is out.
    alice
        .core
        .remove_members(&gid, &[bob.account])
        .await
        .unwrap();
    assert_eq!(alice.core.group_devices(&gid).unwrap().len(), 1);
    assert_eq!(
        pump_app(&mut bob).await,
        Received::RemovedFromGroup { group: gid }
    );

    // A second group; this time Bob leaves by himself and Alice's device commits his removal.
    let gid2 = alice
        .core
        .create_group("Book club", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid2 });
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));
    bob.core.leave_group(&gid2).await.unwrap();
    assert!(
        matches!(pump_app(&mut alice).await, Received::GroupUpdated { group, .. } if group == gid2)
    );
    assert_eq!(alice.core.group_devices(&gid2).unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invite_links_and_join_requests() {
    use songbird_app_core::mls::links::Opened;
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // A link anyone may use: its proof key goes into the roles first, then Bob joins with it
    // by external commit, carrying the proof; Alice's device checks the proof and follows.
    let gid = alice.core.create_group("Open club", &[]).await.unwrap();
    let url = alice.core.create_link(&gid, false).await.unwrap();
    assert!(url.starts_with("https://songbird.chat/g#"));
    assert_eq!(bob.core.open_link(&url).await.unwrap(), Opened::Joined(gid));
    assert!(
        matches!(pump_app(&mut alice).await, Received::GroupUpdated { group, .. } if group == gid)
    );
    assert_eq!(alice.core.group_devices(&gid).unwrap().len(), 2);
    let id = bob
        .core
        .send_group_text(&gid, "Thanks for the link")
        .await
        .unwrap();
    let Received::Message(m) = pump_app(&mut alice).await else {
        panic!("a message")
    };
    assert_eq!((m.id, m.chat), (id, gid));
    // A revoked link opens nothing.
    alice.core.revoke_link(&gid, &url).await.unwrap();
    assert!(bob.core.open_link(&url).await.is_err());

    // A link that needs approval: Bob asks; Alice sees the request and approves (adds him).
    let gid2 = alice.core.create_group("Private club", &[]).await.unwrap();
    let url2 = alice.core.create_link(&gid2, true).await.unwrap();
    assert_eq!(bob.core.open_link(&url2).await.unwrap(), Opened::Requested);
    let waiting = alice.core.join_requests(&gid2).await.unwrap();
    assert_eq!(
        waiting.iter().map(|(a, _)| *a).collect::<Vec<_>>(),
        vec![bob.account]
    );
    alice
        .core
        .approve_request(&gid2, &bob.account)
        .await
        .unwrap();
    assert_eq!(
        pump_past_key(&mut bob).await,
        Received::Joined { group: gid2 }
    );
    assert_eq!(alice.core.join_requests(&gid2).await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn channels_and_communities() {
    use songbird_app_core::mls::links::Opened;
    use songbird_groups::roles::Role;
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // A private channel: Alice posts; Bob subscribes by link, read-only.
    let ch = alice
        .core
        .create_channel([0x0c; 16], "Town news")
        .await
        .unwrap();
    assert_eq!(
        alice.core.group_kind(&ch).unwrap(),
        (true, false, Role::Owner)
    );
    let url = alice.core.create_link(&ch, false).await.unwrap();
    assert_eq!(bob.core.open_link(&url).await.unwrap(), Opened::Joined(ch));
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::GroupUpdated { .. }
    ));
    assert_eq!(
        bob.core.group_kind(&ch).unwrap(),
        (true, false, Role::ReadOnly)
    );
    let post = alice
        .core
        .send_group_text(&ch, "Road closed on Monday")
        .await
        .unwrap();
    let Received::Message(m) = pump_past_key(&mut bob).await else {
        panic!("a post")
    };
    assert_eq!((m.id, m.chat), (post, ch));
    // A subscriber's post is refused by the server (reason 14: the roles don't let it send).
    assert!(bob.core.send_group_text(&ch, "me too").await.is_err());

    // A community: its announcement group (only admins post) carries the community's ID.
    let cid = alice
        .core
        .create_community([0xc0; 16], "Neighbours", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: cid });
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));
    assert_eq!(
        bob.core.group_kind(&cid).unwrap(),
        (false, true, Role::Member)
    );
    assert_eq!(
        bob.core.community_groups(&cid).await.unwrap(),
        Some(Vec::new())
    );
    // Alice adds a group of hers; members of the announcement group see it listed.
    let garden = alice
        .core
        .create_group("Garden", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: garden });
    alice.core.community_add_group(&cid, &garden).await.unwrap();
    assert_eq!(
        bob.core.community_groups(&cid).await.unwrap(),
        Some(vec![garden])
    );
    // A plain group is no community.
    assert_eq!(bob.core.community_groups(&garden).await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_ring_answer_and_agree_on_keys() {
    use songbird_app_core::calls::{Signal, one_to_one_code, one_to_one_key};
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // Contacts first (profile keys both ways), so the call may ring.
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    bob.core.send_text(&alice.account, "hi").await.unwrap();
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::Status { .. }
    ));
    assert!(matches!(pump_app(&mut alice).await, Received::Message(_)));
    assert!(matches!(pump_app(&mut bob).await, Received::Status { .. }));

    // Alice calls: a ticket for a LiveKit room, and an encrypted offer that rings Bob.
    let (ticket, secret) = alice.core.start_call(&bob.account, true).await.unwrap();
    assert!(ticket.url.starts_with("ws"));
    let Received::Call {
        account,
        call,
        signal:
            Signal::Offer {
                video,
                secret: got,
                group,
                ..
            },
        ..
    } = pump_app(&mut bob).await
    else {
        panic!("an offer")
    };
    assert_eq!(
        (account, call, video, group),
        (alice.account, ticket.call, true, None)
    );
    assert_eq!(got, secret.to_vec());
    // Bob answers: his own ticket for the same room; Alice hears the answer.
    let bt = bob
        .core
        .answer_call(&alice.account, &call, None)
        .await
        .unwrap();
    assert_eq!(bt.call, ticket.call);
    assert_ne!(bt.identity, ticket.identity, "one identity per device");
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::Call {
            signal: Signal::Answer,
            ..
        }
    ));
    // Both derive the same media key for each sender, and different keys for different senders.
    let a_on_a = one_to_one_key(&call, &secret[..], &ticket.identity).unwrap();
    let a_on_b = one_to_one_key(&call, &got, &ticket.identity).unwrap();
    assert_eq!((a_on_a.0, &*a_on_a.1), (a_on_b.0, &*a_on_b.1));
    assert_ne!(
        &*a_on_a.1,
        &*one_to_one_key(&call, &got, &bt.identity).unwrap().1
    );
    // The verification code agrees only on the same secret.
    assert_eq!(
        one_to_one_code(&call, &secret[..]).unwrap(),
        one_to_one_code(&call, &got).unwrap()
    );
    // Hellos: each device says who it is in the room, signed with its device key; the other
    // checks it against the signed device list. A hello for another identity, or tampered, fails.
    let hello = alice.core.call_hello(&call, &ticket.identity).unwrap();
    assert_eq!(
        bob.core
            .verify_hello(&call, &ticket.identity, &hello)
            .await
            .unwrap(),
        alice.account
    );
    assert!(
        bob.core
            .verify_hello(&call, &bt.identity, &hello)
            .await
            .is_err()
    );
    let mut forged = hello.clone();
    forged[..16].copy_from_slice(&bob.account);
    assert!(
        bob.core
            .verify_hello(&call, &ticket.identity, &forged)
            .await
            .is_err()
    );
    // Hanging up reaches Bob.
    alice.core.hang_up(&bob.account, &call).await.unwrap();
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::Call {
            signal: Signal::Hangup,
            ..
        }
    ));

    // A group call: keys from the MLS exporter; both members compute the same.
    let gid = alice
        .core
        .create_group("Call me", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));
    let gt = alice.core.start_group_call(&gid, false).await.unwrap();
    let Received::Call {
        group: Some(g),
        call,
        signal: Signal::Offer { .. },
        ..
    } = pump_app(&mut bob).await
    else {
        panic!("a group offer")
    };
    assert_eq!((g, call), (gid, gt.call));
    let bt = bob
        .core
        .answer_call(&alice.account, &call, Some(&gid))
        .await
        .unwrap();
    assert_eq!(bt.call, gt.call);
    let a = alice
        .core
        .group_call_key(&gid, &call, &gt.identity)
        .unwrap();
    let b = bob.core.group_call_key(&gid, &call, &gt.identity).unwrap();
    assert_eq!((a.0, &*a.1), (b.0, &*b.1));
    // The secret the media layer derives from gives the same keys and code on both members.
    let (sa, ea) = alice.core.group_call_secret(&gid, &call).unwrap();
    let (sb, eb) = bob.core.group_call_secret(&gid, &call).unwrap();
    assert_eq!((&*sa, ea), (&*sb, eb));
    let e = songbird_content::call::EpochSecret::group(*sa, ea);
    assert_eq!(&*e.sender_key(gt.identity.as_bytes()).unwrap(), &*a.1);
    assert_eq!(
        songbird_app_core::calls::group_code(&sa, ea),
        songbird_app_core::calls::group_code(&sb, eb)
    );

    // Busy: Bob, in a call, answers a new 1:1 offer with "busy".
    let (t2, _) = alice.core.start_call(&bob.account, false).await.unwrap();
    let Received::Call {
        call: c2,
        signal: Signal::Offer { .. },
        ..
    } = pump_app(&mut bob).await
    else {
        panic!("a second offer")
    };
    assert_eq!(c2, t2.call);
    bob.core.busy_call(&alice.account, &c2).await.unwrap();
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::Call {
            signal: Signal::Busy,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn call_links_lobby_keys_and_revoke() {
    use songbird_app_core::call_links::{LinkJoin, open_link_card};
    use songbird_app_core::calls::Signal;
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // Alice makes a link with a lobby; the link is its root, which never reaches the server.
    let link = alice
        .core
        .create_call_link("Book club", true, true)
        .await
        .unwrap();
    assert_eq!(alice.core.call_links().unwrap().len(), 1);
    assert!(alice.core.is_link_admin(&link.root));
    let url = songbird_proto::call_link::CallLink { root: *link.root }.to_link();
    let root = songbird_proto::call_link::CallLink::parse(&url)
        .unwrap()
        .root;

    // Bob (no contact of hers) asks to join: he waits in the lobby with his card.
    let LinkJoin::Waiting(place) = bob.core.join_call_link(&root, "Bob").await.unwrap() else {
        panic!("a lobby place")
    };
    assert_eq!(bob.core.poll_lobby(&root, &place).await.unwrap(), None);
    // Alice sees his card, opened with the link key, and lets him in.
    let waiting = alice.core.lobby_waiting(&root).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0].account, waiting[0].name.as_str()),
        (bob.account, "Bob")
    );
    alice
        .core
        .admit(&root, &waiting[0].handle, true)
        .await
        .unwrap();
    let bt = bob.core.poll_lobby(&root, &place).await.unwrap().unwrap();
    // Alice, the admin, skips the lobby into the same room.
    let LinkJoin::Ticket(at) = alice.core.join_call_link(&root, "Alice").await.unwrap() else {
        panic!("the admin's ticket")
    };
    assert_eq!(at.call, bt.call);
    assert_ne!(at.identity, bt.identity);

    // Cards in the room carry the real identity; only the link's key opens them.
    let card = alice.core.link_card(&root, &at.identity, "Alice").unwrap();
    let opened = open_link_card(&root, &card).unwrap();
    assert_eq!(
        (opened.account, opened.identity.as_str()),
        (alice.account, at.identity.as_str())
    );
    assert!(open_link_card(&[0; 32], &card).is_err());

    // Alice's sender key reaches Bob's device in a Signal session (carried on the room's data
    // channel, never the server's lanes: they are strangers), naming her device.
    let key = [9u8; 32];
    let (alice_device, bob_device) = (alice.core.local().device, bob.core.local().device);
    let envelope = alice
        .core
        .link_key_envelope(&bob.account, &bob_device, &at.call, &at.identity, 3, &key)
        .await
        .unwrap();
    let Received::Call {
        account,
        call,
        signal:
            Signal::SenderKey {
                device,
                identity,
                index,
                key: got,
            },
        ..
    } = bob.core.receive(&envelope).await.unwrap()
    else {
        panic!("a sender key")
    };
    assert_eq!(
        (account, call, device, identity.as_str(), index, *got),
        (
            alice.account,
            at.call,
            alice_device,
            at.identity.as_str(),
            3,
            key
        )
    );

    // Someone turned away gets 404 on their next poll.
    let carol_root = alice
        .core
        .create_call_link("Closed", false, true)
        .await
        .unwrap()
        .root;
    let LinkJoin::Waiting(p2) = bob.core.join_call_link(&carol_root, "Bob").await.unwrap() else {
        panic!("a lobby place")
    };
    let w = alice.core.lobby_waiting(&carol_root).await.unwrap();
    alice
        .core
        .admit(&carol_root, &w[0].handle, false)
        .await
        .unwrap();
    assert_eq!(
        bob.core.poll_lobby(&carol_root, &p2).await.unwrap_err(),
        songbird_app_core::Error::Server(404)
    );

    // Revoking deletes the link: nobody joins it any more, and Alice forgets it.
    alice.core.revoke_call_link(&root).await.unwrap();
    assert_eq!(
        bob.core.join_call_link(&root, "Bob").await.unwrap_err(),
        songbird_app_core::Error::Server(404)
    );
    assert_eq!(
        alice.core.call_links().unwrap().len(),
        1,
        "the other link stays"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timers_pins_polls_and_group_events() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // Contacts first.
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    bob.core.send_text(&alice.account, "hi").await.unwrap();
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::Status { .. }
    ));
    assert!(matches!(pump_app(&mut alice).await, Received::Message(_)));
    assert!(matches!(pump_app(&mut bob).await, Received::Status { .. }));

    // A poll: Bob votes, Alice sees the tally; a vote for an option that does not exist is
    // refused; Alice closes it and a later vote no longer counts.
    let options = vec!["Tea".to_string(), "Coffee".to_string()];
    let poll = alice
        .core
        .send_poll(&bob.account, "Drink?", &options, false)
        .await
        .unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::Status { .. }
    ));
    bob.core
        .vote(&alice.account, &alice.account, &poll, &[1])
        .await
        .unwrap();
    assert_eq!(
        pump_app(&mut alice).await,
        Received::PollChanged {
            author: alice.account,
            id: poll
        }
    );
    assert_eq!(
        alice.core.poll_tally(&alice.account, &poll).unwrap(),
        (vec![0, 1], vec![], false)
    );
    assert!(
        bob.core
            .vote(&alice.account, &alice.account, &poll, &[0, 1])
            .await
            .is_err(),
        "one choice only"
    );
    alice.core.close_poll(&bob.account, &poll).await.unwrap();
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::PollChanged { .. }
    ));
    assert_eq!(
        bob.core.poll_tally(&alice.account, &poll).unwrap(),
        (vec![0, 1], vec![1], true)
    );
    assert!(
        bob.core
            .vote(&alice.account, &alice.account, &poll, &[0])
            .await
            .is_err(),
        "closed"
    );

    // A pin, for everyone in the chat.
    alice
        .core
        .pin(&bob.account, &alice.account, &poll, 86_400, false)
        .await
        .unwrap();
    let Received::Pinned {
        author,
        id,
        until_ms,
        ..
    } = pump_app(&mut bob).await
    else {
        panic!("a pin")
    };
    assert_eq!((author, id), (alice.account, poll));
    assert!(until_ms > 0);
    assert_eq!(bob.core.pins(&alice.account).unwrap().len(), 1);

    // Disappearing messages: Alice turns on a 5-second timer; Bob learns it, and the next
    // message goes on both devices once its time is up.
    alice.core.set_timer(&bob.account, 5).await.unwrap();
    assert_eq!(
        pump_app(&mut bob).await,
        Received::TimerChanged {
            chat: alice.account,
            by: alice.account,
            secs: 5
        }
    );
    assert_eq!(bob.core.timer(&alice.account).unwrap(), 5);
    let gone = alice
        .core
        .send_text(&bob.account, "soon gone")
        .await
        .unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    assert_eq!(bob.core.expire_messages().unwrap(), Vec::new());
    tokio::time::sleep(Duration::from_secs(6)).await;
    let expired = bob.core.expire_messages().unwrap();
    assert!(
        expired
            .iter()
            .any(|(_, a, i)| *a == alice.account && *i == gone)
    );
    assert!(
        bob.core
            .store
            .message(&alice.account, &gone)
            .unwrap()
            .is_none()
    );
    assert!(
        alice
            .core
            .expire_messages()
            .unwrap()
            .iter()
            .any(|(_, _, i)| *i == gone)
    );

    // In a group: a reaction and a poll vote travel as group events (not as empty messages).
    let gid = alice
        .core
        .create_group("Club", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    // Joining, Bob learned (and checked against Alice's signed device list) whose device is
    // whose: what he checks every later commit's roles with (D-0155).
    let known = bob
        .core
        .store
        .secret(&format!("group_accounts.{}", hex::encode(gid)))
        .unwrap()
        .unwrap();
    assert!(
        known
            .as_chunks::<32>()
            .0
            .iter()
            .any(|c| c[..16] == alice.core.local().device && c[16..] == alice.account)
    );
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));
    let gpoll = alice
        .core
        .send_poll(&gid, "When?", &["Mon".into(), "Tue".into()], true)
        .await
        .unwrap();
    let Received::Message(row) = pump_app(&mut bob).await else {
        panic!("the group poll")
    };
    assert_eq!(row.chat, gid);
    bob.core
        .react(&gid, &alice.account, &gpoll, "👍")
        .await
        .unwrap();
    // Bob's delivery receipt for the message that already disappeared arrives first, as
    // nothing to show.
    let mut got = pump_app(&mut alice).await;
    while matches!(got, Received::Nothing | Received::Status { .. }) {
        got = pump_app(&mut alice).await;
    }
    assert!(
        matches!(&got, Received::Reaction { emoji, .. } if emoji == "👍"),
        "{got:?}"
    );
    bob.core
        .vote(&gid, &alice.account, &gpoll, &[0, 1])
        .await
        .unwrap();
    assert!(matches!(
        pump_app(&mut alice).await,
        Received::PollChanged { .. }
    ));
    assert_eq!(
        alice.core.poll_tally(&alice.account, &gpoll).unwrap().0,
        vec![1, 1]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn safety_numbers_match_and_verification_is_kept() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let (a, a_verified) = alice.core.safety(&bob.account).await.unwrap();
    let (b, _) = bob.core.safety(&alice.account).await.unwrap();
    // Both sides show the same 60 digits; each side's code, scanned by the other, matches.
    assert_eq!(a.digits, b.digits);
    assert_eq!(a.digits.len(), 60);
    assert!(songbird_app_core::crypto::safety::scanned_matches(
        &a,
        &b.scannable
    ));
    assert!(!a_verified);
    alice.core.set_verified(&bob.account, true).unwrap();
    assert!(alice.core.safety(&bob.account).await.unwrap().1);
    assert!(!alice.core.identity_changed(&bob.account).await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_change_log_is_written_encrypted_and_read_by_members() {
    use songbird_content::schema::group_update::Kind;
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let gid = alice
        .core
        .create_group("Choir", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    // The "created" update hands Bob the change log's key.
    assert!(matches!(
        pump_app(&mut bob).await,
        Received::GroupUpdated { .. }
    ));
    assert_eq!(
        bob.core.log_key(&gid).unwrap().as_deref(),
        alice.core.log_key(&gid).unwrap().as_deref()
    );
    // The add is in the log, written by Alice, readable by Bob (the server cannot read it).
    let log = bob.core.change_log(&gid).await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].kind, log[0].actor, log[0].subjects.clone()),
        (Kind::MembersAdded, alice.account, vec![bob.account])
    );
    // A removal is logged too.
    alice
        .core
        .remove_members(&gid, &[bob.account])
        .await
        .unwrap();
    let log = alice.core.change_log(&gid).await.unwrap();
    assert_eq!(
        log.iter().map(|c| c.kind).collect::<Vec<_>>(),
        vec![Kind::MembersAdded, Kind::MembersRemoved]
    );
    assert_eq!(log[1].subjects, vec![bob.account]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_view_once_photo_is_shown_once_then_gone() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    let photo = vec![7u8; 40_000];
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: "image/jpeg".into(),
        file_name: "once.jpg".into(),
        ..Default::default()
    };
    let id = alice
        .core
        .send_view_once(&bob.account, &photo, &meta)
        .await
        .unwrap();
    // The sender keeps nothing to view again.
    assert!(alice.core.view_once_opened(&alice.account, &id).unwrap());
    let Received::Message(row) = pump_app(&mut bob).await else {
        panic!("the view-once photo")
    };
    assert!(songbird_app_core::messenger::view_once_in(&row.content));
    assert!(!bob.core.view_once_opened(&alice.account, &id).unwrap());
    // Shown once…
    assert_eq!(
        &*bob.core.file(&alice.account, &id, 0).await.unwrap(),
        &photo[..]
    );
    bob.core.consume_view_once(&alice.account, &id).unwrap();
    // …then gone for good.
    assert!(bob.core.view_once_opened(&alice.account, &id).unwrap());
    assert!(bob.core.file(&alice.account, &id, 0).await.is_err());
}

/// Makes a sticker pack on the server as a pack creator's app would (P06's construction):
/// answers its link.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "test sizes and 6-bit indices"
)]
async fn make_pack(p: &Person, assets: &[Vec<u8>]) -> String {
    use songbird_app_core::messenger::Net as _;
    use songbird_content::attachment::{self, Suite};
    use songbird_content::pack::{Item, Kind, Manifest, object_key};
    let pack_key = [0x5au8; 32];
    let mut objects = Vec::new();
    let mut items = Vec::new();
    for (i, a) in assets.iter().enumerate() {
        let n = u16::try_from(i + 1).unwrap();
        let (_, salt) = attachment::fresh().unwrap();
        let mut ct = Vec::new();
        let digest = attachment::encrypt(
            Suite::DEFAULT,
            &object_key(&pack_key, n).unwrap(),
            &salt,
            a.len() as u64,
            &mut &a[..],
            &mut ct,
        )
        .unwrap();
        objects.push(ct);
        items.push(Item {
            object: u32::from(n),
            content_type: "image/webp".into(),
            size: a.len() as u64,
            digest: digest.to_vec(),
            emoji: vec!["🐦".into()],
            ..Item::default()
        });
    }
    let manifest = Manifest {
        version: 1,
        kind: Kind::Stickers.into(),
        title: "Songbirds".into(),
        author: "Gate".into(),
        cover: 1,
        items,
    }
    .seal()
    .unwrap();
    let (_, salt) = attachment::fresh().unwrap();
    let mut mct = Vec::new();
    attachment::encrypt(
        Suite::DEFAULT,
        &object_key(&pack_key, 0).unwrap(),
        &salt,
        manifest.len() as u64,
        &mut &manifest[..],
        &mut mct,
    )
    .unwrap();
    objects.insert(0, mct);
    let hash = hex::encode(aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA256,
        &[0x77u8; 32],
    ));
    let sizes: Vec<String> = objects.iter().map(|o| o.len().to_string()).collect();
    let net = p.core.net();
    let (st, b) = net
        .api(
            "POST",
            "/v1/packs",
            format!(r#"{{"sizes":[{}],"token":"{hash}"}}"#, sizes.join(",")).into_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(st, 201);
    let pack = serde_json::from_slice::<serde_json::Value>(&b).unwrap()["pack"]
        .as_str()
        .unwrap()
        .to_owned();
    for (n, o) in objects.iter().enumerate() {
        let (st, _) = net
            .upload("PUT", &format!("/v1/packs/{pack}/{n}"), o.clone())
            .await
            .unwrap();
        assert_eq!(st, 201);
    }
    assert_eq!(
        net.api("POST", &format!("/v1/packs/{pack}/complete"), Vec::new())
            .await
            .unwrap()
            .0,
        201
    );
    let key: String = {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut s = String::new();
        let (mut acc, mut bits) = (0u32, 0u32);
        for b in pack_key {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                s.push(char::from(A[((acc >> bits) & 63) as usize]));
            }
        }
        if bits > 0 {
            s.push(char::from(A[((acc << (6 - bits)) & 63) as usize]));
        }
        s
    };
    format!("https://songbird.chat/pack#{pack}.{key}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sticker_packs_install_from_a_link_and_stickers_travel() {
    use songbird_app_core::packs::{BUILTIN_PACK_ID, sticker_in};
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    let assets = vec![vec![1u8; 2_000], vec![2u8; 3_000]];
    let link = make_pack(&alice, &assets).await;
    // Bob installs it from the link: every asset fetched, opened and checked.
    let pack = bob.core.install_pack(&link).await.unwrap();
    assert_eq!((pack.title.as_str(), pack.items.len()), ("Songbirds", 2));
    assert_eq!(bob.core.packs().unwrap().len(), 1);
    assert_eq!(
        &*bob.core.pack_item(&pack.id, 2).unwrap().unwrap(),
        &assets[1][..]
    );
    // A sticker from it reaches Alice with its image, though she installed nothing.
    let id = bob
        .core
        .send_sticker(&alice.account, Some(&pack.id), 2, "🐦")
        .await
        .unwrap();
    let mut got = pump_app(&mut alice).await;
    while !matches!(got, Received::Message(_)) {
        got = pump_app(&mut alice).await;
    }
    let Received::Message(row) = got else {
        unreachable!()
    };
    assert_eq!(sticker_in(&row.content), Some((pack.id, "🐦".into(), true)));
    assert_eq!(
        &*alice.core.file(&bob.account, &id, 0).await.unwrap(),
        &assets[1][..]
    );
    // A built-in sticker is just its emoji, drawn by each device.
    alice
        .core
        .send_sticker(&bob.account, None, 0, "🦉")
        .await
        .unwrap();
    let mut got = pump_app(&mut bob).await;
    while !matches!(got, Received::Message(_)) {
        got = pump_app(&mut bob).await;
    }
    let Received::Message(row) = got else {
        unreachable!()
    };
    assert_eq!(
        sticker_in(&row.content),
        Some((BUILTIN_PACK_ID, "🦉".into(), false))
    );
    // Removing it.
    bob.core.remove_pack(&pack.id).unwrap();
    assert!(bob.core.packs().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn presence_and_nudges_reach_contacts_sealed() {
    use songbird_app_core::messenger::Net as _;
    use songbird_content::presence::{Presence, State};
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    // Contacts both ways (each holds the other's profile key).
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    bob.core.send_text(&alice.account, "hi").await.unwrap();
    let mut got = pump_app(&mut alice).await;
    while !matches!(got, Received::Message(_)) {
        got = pump_app(&mut alice).await;
    }
    // Presence reaches exactly the contacts who shared their profile key, with their devices.
    let peers = alice.core.presence_peers().await.unwrap();
    assert_eq!(peers.len(), 1);
    let (account, devices, _) = &peers[0];
    assert_eq!(*account, bob.account);
    let p = Presence {
        state: State::Online.into(),
        last_seen_ms: 0,
        wants_reply: true,
    };
    let content = Messenger::<TestNet>::presence_content(&p).unwrap();
    let sealed = alice
        .core
        .seal_for(&bob.account, &devices[0], &content)
        .await
        .unwrap();
    alice
        .core
        .net()
        .ephemeral(devices[0], sealed)
        .await
        .unwrap();
    assert_eq!(
        next_ephemeral(&mut bob).await,
        Received::Presence {
            account: alice.account,
            presence: p
        }
    );
    // A nudge goes in the attention class and is taken only from there.
    assert_eq!(alice.core.nudge(&bob.account).await.unwrap(), 1);
    let payload = loop {
        let ev = tokio::time::timeout(Duration::from_secs(10), bob.events.recv())
            .await
            .unwrap()
            .unwrap();
        if let Event::Attention { payload } = ev {
            break payload;
        }
    };
    assert_eq!(
        bob.core.receive_attention(&payload).await.unwrap(),
        Received::Nudge {
            account: alice.account
        }
    );
    // The same payload as an ordinary event is no nudge.
    assert_eq!(bob.core.receive(&payload).await.unwrap(), Received::Nothing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_voice_note_arrives_with_its_waveform_and_is_marked_played() {
    let Some((_app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    alice.core.send_text(&bob.account, "hi").await.unwrap();
    assert!(matches!(pump_app(&mut bob).await, Received::Message(_)));
    // 16 kHz mono 16-bit WAV, as the recorder makes it: one second.
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36u32 + 32_000).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes().to_vec(),
        1u16.to_le_bytes().to_vec(),
        1u16.to_le_bytes().to_vec(),
        16_000u32.to_le_bytes().to_vec(),
        32_000u32.to_le_bytes().to_vec(),
        2u16.to_le_bytes().to_vec(),
        16u16.to_le_bytes().to_vec(),
    ] {
        wav.extend_from_slice(&v);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend(std::iter::repeat_n(0x11u8, 32_000));
    let meta = songbird_app_core::attachments::FileMeta {
        content_type: "audio/wav".into(),
        file_name: "voice-note.wav".into(),
        duration_ms: 1_000,
        flags: 1,
        waveform: (0..=255u8).collect(),
        ..Default::default()
    };
    let id = alice
        .core
        .send_files(&bob.account, vec![(wav.clone(), meta)], "")
        .await
        .unwrap();
    let mut got = pump_app(&mut bob).await;
    while !matches!(got, Received::Message(_)) {
        got = pump_app(&mut bob).await;
    }
    let Received::Message(row) = got else {
        unreachable!()
    };
    let p = &songbird_app_core::messenger::attachments_of(&row.content)[0];
    assert_eq!((p.flags, p.duration_ms, p.waveform.len()), (1, 1_000, 256));
    assert_eq!(
        &*bob.core.file(&alice.account, &id, 0).await.unwrap(),
        &wav[..]
    );
    // Played: kept here, and Alice is told.
    bob.core.mark_played(&alice.account, &id).await.unwrap();
    let mut st = pump_app(&mut alice).await;
    while !matches!(
        st,
        Received::Status {
            status: Status::Played,
            ..
        }
    ) {
        st = pump_app(&mut alice).await;
    }
}

/// D-0167, D-0171: a member account's new device joins the account's group by external commit
/// (§6.3 rule 5); the other members follow it only because it is on that account's signed device
/// list. The account's own device hands it the change log's key and the title, and removes the
/// account's leaf of a device no longer listed (a reinstall). Another account's device can't join.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_device_joins_its_accounts_groups() {
    let Some((app, mut alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let gid = alice
        .core
        .create_group("Family", &[bob.account])
        .await
        .unwrap();
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid });
    let _ = pump_app(&mut bob).await; // the title

    // An old device of Alice's joins; Alice's phone hands it the key and the title.
    let (mut old, old_device) = link_device(&app, &mut alice, &[]).await;
    old.core.join_own_group(&gid).await.unwrap();
    assert_eq!(pump(&mut alice).await, Received::System);
    assert_eq!(pump(&mut bob).await, Received::System);
    assert_eq!(
        pump_app(&mut old).await,
        Received::GroupUpdated {
            group: gid,
            title: "Family".into()
        }
    );
    assert!(old.core.log_key(&gid).unwrap().is_some());
    assert_eq!(old.core.group_title(&gid).unwrap(), "Family");
    let _ = pump_app(&mut bob).await; // the same handover, nothing new for Bob
    assert_eq!(bob.core.group_devices(&gid).unwrap().len(), 3);

    // The old device goes from her list (a reinstall); her new device joins. Alice's phone
    // follows, then removes the old leaf.
    let (mut phone, _) = link_device(&app, &mut alice, &[old_device]).await;
    phone.core.join_own_group(&gid).await.unwrap();
    assert!(phone.core.in_group(&gid).unwrap());
    assert_eq!(pump(&mut alice).await, Received::System);
    let devices: Vec<[u8; 16]> = alice.core.group_devices(&gid).unwrap();
    assert_eq!(devices.len(), 3, "the old leaf is gone");
    assert!(!devices.contains(old_device.as_bytes()));
    let mut want = devices.clone();
    want.sort_unstable();
    for p in [&mut bob, &mut phone] {
        // The join, the handover and the removal, in whatever order they come.
        for _ in 0..6 {
            let mut have = p.core.group_devices(&gid).unwrap();
            have.sort_unstable();
            if have == want && p.core.log_key(&gid).unwrap().is_some() {
                break;
            }
            let _ = pump(p).await;
        }
        let mut have = p.core.group_devices(&gid).unwrap();
        have.sort_unstable();
        assert_eq!(have, want);
    }
    assert!(phone.core.log_key(&gid).unwrap().is_some());

    let id = bob.core.send_group_text(&gid, "Hi both").await.unwrap();
    let Received::Message(m) = pump_app(&mut phone).await else {
        panic!("a group message on the new device")
    };
    assert_eq!((m.id, m.text.as_str()), (id, "Hi both"));
    let id = phone
        .core
        .send_group_text(&gid, "From my laptop")
        .await
        .unwrap();
    let Received::Message(m) = pump_app(&mut bob).await else {
        panic!("a group message from the new device")
    };
    assert_eq!((m.author, m.id), (alice.account, id));

    // Another account's device: the server refuses its state, so it can't join.
    let gid2 = alice.core.create_group("Just us", &[]).await.unwrap();
    assert!(matches!(
        bob.core.join_own_group(&gid2).await,
        Err(songbird_app_core::Error::Server(404))
    ));
}

/// Links a new device to `owner`'s account and signs a list naming every enabled device but
/// `without`.
async fn link_device(app: &App, owner: &mut Person, without: &[DeviceId]) -> (Person, DeviceId) {
    let device = DeviceId::random().unwrap();
    let (_, auth_seed) = DeviceKey::generate(device).unwrap();
    let auth = DeviceKey::from_seed(device, &auth_seed).unwrap();
    let identity = X25519Pair::generate().unwrap();
    let token = random32();
    app.registry
        .create_link_token(
            &DeviceId::from_bytes(owner.core.local().device),
            &token,
            &device,
            Duration::from_secs(600),
            6,
        )
        .await
        .unwrap();
    app.registry
        .link_device(
            &token,
            &device,
            &auth.public_key(),
            &songbird_app_core::crypto::serialize_x25519(&identity.public()),
            &random32(),
            6,
        )
        .await
        .unwrap();
    let drop: Vec<[u8; 16]> = without.iter().map(|d| *d.as_bytes()).collect();
    owner
        .core
        .publish_device_list(&owner.account_key, |d, _| !drop.contains(d))
        .await
        .unwrap();
    let r = Registered {
        n: NewAccount {
            account: AccountId::from_bytes(owner.account),
            identity: owner.account_key.public_key().as_ref().try_into().unwrap(),
            device,
            auth_key: auth.public_key(),
            allowance: 3,
        },
        account_key: Ed25519KeyPair::from_seed_unchecked(&random32()).unwrap(),
        auth_seed,
    };
    let (api, ws) = (owner.core.net().addr, owner.core.net().ws_addr);
    (linked(r, identity, api, ws).await, device)
}

/// A device linked to an existing account: its own keys, no new account.
async fn linked(
    r: Registered,
    identity: X25519Pair,
    api: std::net::SocketAddr,
    ws: std::net::SocketAddr,
) -> Person {
    let mut c = ClientConfig::new(ws.to_string(), SERVER_NAME);
    c.backoff.initial = Duration::from_millis(20);
    c.new_device_grace = Duration::from_secs(5);
    let (mut client, events) = Client::start(
        c,
        DeviceKey::from_seed(r.n.device, &r.auth_seed).unwrap(),
        ResumeState::default(),
    );
    client.wait_for(State::Connected).await;
    let net = TestNet {
        addr: api,
        api: ApiClient::new(
            api,
            API,
            r.n.device,
            Ed25519KeyPair::from_seed_unchecked(&r.auth_seed).unwrap(),
        ),
        ws: Arc::new(client),
        ws_addr: ws,
    };
    let local = Local {
        account: *r.n.account.as_bytes(),
        device: *r.n.device.as_bytes(),
        identity,
    };
    let mut core = Messenger::create(
        Store::memory(&random32()).unwrap(),
        net,
        local,
        Some(songbird_send_system_public()),
    )
    .unwrap();
    core.publish_keys(true).await.unwrap();
    core.publish_key_packages(5, true).await.unwrap();
    Person {
        account: *r.n.account.as_bytes(),
        account_key: r.account_key,
        core,
        events,
    }
}

/// The key-transparency store of the stack whose api `p` talks to.
fn kt_store(p: &Person) -> songbird_akd::SurrealStore {
    let addr = p.core.net().addr;
    KT_STORES
        .lock()
        .unwrap()
        .iter()
        .find(|(a, _)| *a == addr)
        .map(|(_, s)| s.clone())
        .unwrap()
}

/// D-0168: a device checks that the log publishes the device list it accepts, notices a list
/// the log doesn't vouch for, and notices new devices on an account it knows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_transparency_vouches_for_device_lists_and_new_devices_are_noticed() {
    use songbird_app_core::kt::Notice;
    let Some((app, alice, mut bob, _stop)) = stack().await else {
        return;
    };
    let publisher =
        songbird_akd::Publisher::open(kt_store(&alice), songbird_akd::SecretVrf::new(&KT_VRF_SEED))
            .await
            .unwrap();
    publisher.publish_pending(&app.registry, 100).await.unwrap();
    // Bob reads Alice's list: the log holds exactly it.
    let list = fetch_list(&bob, alice.account).await;
    assert!(bob.core.kt_check_list(&alice.account, &list).await.unwrap());
    // A list the log doesn't hold: once is "not yet published", twice is noticed.
    let forged = b"not alice's list".to_vec();
    assert!(
        !bob.core
            .kt_check_list(&alice.account, &forged)
            .await
            .unwrap()
    );
    assert!(
        !bob.core
            .kt_check_list(&alice.account, &forged)
            .await
            .unwrap()
    );
    assert!(
        bob.core
            .take_notices()
            .unwrap()
            .contains(&Notice::ListNotInLog {
                account: alice.account
            })
    );

    // Alice links a second device; Bob's next fetch of her list notices it, and once the
    // epoch is published the log vouches for the new list too.
    let _ = bob.core.devices_of(&alice.account, true).await.unwrap();
    let _ = bob.core.take_notices();
    let device = DeviceId::random().unwrap();
    let (_, auth_seed) = DeviceKey::generate(device).unwrap();
    let auth = DeviceKey::from_seed(device, &auth_seed).unwrap();
    let token = random32();
    app.registry
        .create_link_token(
            &DeviceId::from_bytes(alice.core.local().device),
            &token,
            &device,
            Duration::from_secs(600),
            6,
        )
        .await
        .unwrap();
    app.registry
        .link_device(
            &token,
            &device,
            &auth.public_key(),
            &songbird_app_core::crypto::serialize_x25519(&X25519Pair::generate().unwrap().public()),
            &random32(),
            6,
        )
        .await
        .unwrap();
    let mut alice = alice;
    alice
        .core
        .publish_device_list(&alice.account_key, |_, _| true)
        .await
        .unwrap();
    publisher.publish_pending(&app.registry, 100).await.unwrap();
    assert_eq!(
        bob.core
            .devices_of(&alice.account, true)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        bob.core.take_notices().unwrap(),
        [Notice::NewDevices {
            account: alice.account,
            count: 1
        }]
    );
    let list = fetch_list(&bob, alice.account).await;
    assert!(bob.core.kt_check_list(&alice.account, &list).await.unwrap());

    // D-0171: a server whose epochs don't follow from the head Bob kept (its recorded roots
    // rewritten) fails the audit: a fork, noticed and refused.
    alice
        .core
        .publish_device_list(&alice.account_key, |_, _| true)
        .await
        .unwrap();
    publisher.publish_pending(&app.registry, 100).await.unwrap();
    for e in 1..=20 {
        kt_store(&alice).put_root(e, &[7; 32]).await.unwrap();
    }
    let list = fetch_list(&bob, alice.account).await;
    assert_eq!(
        bob.core.kt_check_list(&alice.account, &list).await,
        Err(songbird_app_core::Error::Verify)
    );
    assert!(bob.core.take_notices().unwrap().contains(&Notice::Fork));
}

/// An account's signed device list, as the server serves it.
async fn fetch_list(p: &Person, account: [u8; 16]) -> Vec<u8> {
    let body = format!(r#"{{"account":"{}"}}"#, hex::encode(account)).into_bytes();
    p.core
        .net()
        .api("POST", "/v1/devices/list/fetch", body)
        .await
        .unwrap()
        .1
}
