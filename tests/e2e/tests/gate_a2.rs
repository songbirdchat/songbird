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
    let db = Db::connect(songbird_surreal::Config {
        addr: surreal,
        host: "localhost".into(),
        tls: None,
        user: std::env::var("SONGBIRD_TEST_SURREALDB_USER").unwrap_or_else(|_| "root".into()),
        pass: std::env::var("SONGBIRD_TEST_SURREALDB_PASS").unwrap(),
        namespace: "test".into(),
        database: format!("gatea2_{}", DeviceId::random().unwrap().to_hex_unredacted()),
        timeout: Duration::from_secs(30),
        max_message_bytes: 16 << 20,
    })
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
        kt: None,
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
    assert_eq!(pump_app(&mut bob).await, Received::Joined { group: gid2 });
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
    let Received::Message(m) = pump_app(&mut bob).await else {
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
