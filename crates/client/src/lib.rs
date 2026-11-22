//! Songbird delivery client (master prompt §5, `client`).
//!
//! One background task per device runs the connection state machine:
//!
//! ```text
//! Connecting ──Hello/Auth/AuthOk──▶ Connected ──close/error──▶ Unavailable ──backoff──▶ Connecting
//!      │                                  │
//!      └────────── 4000–4099 close ───────┴──▶ Failed (stop; the app must act)
//! ```
//!
//! - **Authentication:** signs the transcript of `songbird_proto::auth` with the device key, for
//!   the server name from *this* configuration (never the one the server announces).
//! - **Backoff:** exponential with full jitter, never shorter than the server's retry hint;
//!   `42xx` closes reconnect after a short jitter only.
//! - **Exactly once to the app:** deliveries with a `seq` at or below the last one handed to the
//!   app are dropped (they arrive again after a reconnect until the app's ACK reaches the server).
//! - **ACKs:** cumulative, sent only for what the app confirmed it stored durably
//!   ([`Client::ack`]), coalesced for `ack_delay`.
//! - **Store generation and resend (D-0040):** every accepted send is kept in a send log until the
//!   app confirms its end-to-end delivery receipt ([`Client::confirm_delivered`]) or 30 days
//!   pass. When the store generation grows (the server was restored), the client reports
//!   [`Event::StoreGenerationChanged`] and resends every logged message with its original
//!   idempotency key: a mailbox that survived replays it, a lost one stores it again; recipients
//!   drop repeats by the message ID inside the envelope (`PROTOCOL.md` §3.6). The log lives in
//!   memory: surviving the app's own restart is the app core's job (Prompt 4).
#![forbid(unsafe_code)]

mod backoff;
pub mod https;
mod key;
mod ws;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use sockudo_ws::client::WebSocketClient;
use sockudo_ws::{Http1, Message, WebSocketStream};
use songbird_proto::close::{self, Reconnect};
use songbird_proto::frame::{Recipient, SendFrame, SendHints, SendPayload, SendStatus};
use songbird_proto::{ClientFrame, CloseCode, Limits, PROTOCOL_VERSION, ServerFrame, control};
use songbird_types::DeviceId;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

pub use backoff::{Backoff, BackoffConfig};
pub use key::{DeviceKey, KeyError};

/// Client settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// `host:port` to connect to.
    pub addr: String,
    /// The endpoint's public name: signed during authentication, sent as `Host`, used for TLS.
    pub server_name: String,
    /// TLS settings; `None` for plain TCP (development and tests only).
    pub tls: Option<Arc<rustls::ClientConfig>>,
    /// Reach the endpoint through a Songbird proxy (`docs/PROTOCOL.md` §1.1; Prompt 9): TLS to
    /// the proxy, and the usual TLS session to the endpoint inside it, which the proxy cannot
    /// read. Needs [`ClientConfig::tls`].
    pub proxy: Option<ProxyRoute>,
    /// Frame limits (must match the server's).
    pub limits: Limits,
    /// Reconnect backoff.
    pub backoff: BackoffConfig,
    /// How long to coalesce ACKs.
    pub ack_delay: Duration,
    /// Limit for TCP connect, TLS, upgrade and each authentication step.
    pub handshake_timeout: Duration,
    /// Events buffered for the app; when full, the client stops reading the socket (the server
    /// then treats it as a slow consumer).
    pub event_capacity: usize,
    /// Reported in `Auth.client_version`.
    pub client_version: String,
    /// Most messages kept in the send log (the oldest go first when full); 0 keeps no log, so
    /// nothing is resent after a restore (tests and drills only: messages can be lost).
    pub send_log_capacity: usize,
    /// How long a message stays in the send log without a delivery receipt.
    pub send_log_max_age: Duration,
    /// For a device registered or linked moments ago: within this time of [`Client::start`], an
    /// authentication failure (close 4000) is retried with backoff instead of stopping. Delivery
    /// nodes learn new devices from the registry within their directory poll (1 s by default),
    /// so a client that connects right after `POST /v1/registration` can arrive first. The server
    /// never looks a device up on behalf of an unauthenticated connection (that would let anyone
    /// turn connection attempts into database queries), so the wait is the client's
    /// (`docs/PROTOCOL.md` §4.1; apps use 10 s after registering, zero otherwise).
    pub new_device_grace: Duration,
}

impl ClientConfig {
    /// Defaults for a plain-TCP endpoint.
    #[must_use]
    pub fn new(addr: impl Into<String>, server_name: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            server_name: server_name.into(),
            tls: None,
            proxy: None,
            limits: Limits::DEFAULT,
            backoff: BackoffConfig::default(),
            ack_delay: Duration::from_millis(10),
            handshake_timeout: Duration::from_secs(10),
            event_capacity: 1024,
            client_version: concat!("songbird-client/", env!("CARGO_PKG_VERSION")).into(),
            send_log_capacity: 100_000,
            send_log_max_age: Duration::from_hours(30 * 24),
            new_device_grace: Duration::ZERO,
        }
    }
}

/// TLS 1.3 settings trusting Mozilla's roots (`webpki-roots`): for public endpoints and the
/// proxies people run. `None` only if the TLS library refuses TLS 1.3 (it does not).
#[must_use]
pub fn web_tls() -> Option<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    // A development build's own root, set when it is built (`SONGBIRD_DEV_CA`, a PEM
    // certificate), for a test server on a local network. No release build sets it, so they
    // trust Mozilla's roots alone.
    if let Some(pem) = option_env!("SONGBIRD_DEV_CA") {
        use rustls_pki_types::{CertificateDer, pem::PemObject as _};
        if let Ok(ca) = CertificateDer::from_pem_slice(pem.as_bytes()) {
            let _ = roots.add(ca);
        }
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .ok()
        .map(|b| Arc::new(b.with_root_certificates(roots).with_no_client_auth()))
}

/// A Songbird proxy to dial (from a proxy link, `songbird_types::proxy_link`).
#[derive(Clone)]
pub struct ProxyRoute {
    /// `host:port` of the proxy.
    pub addr: String,
    /// The proxy's host name: its TLS certificate's name and the outer session's SNI.
    pub host: String,
    /// TLS settings for the outer session (the system's roots, normally).
    pub tls: Arc<rustls::ClientConfig>,
}

impl std::fmt::Debug for ProxyRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyRoute").finish_non_exhaustive()
    }
}

impl ProxyRoute {
    /// The route a proxy link names, with `tls` for the outer session.
    #[must_use]
    pub fn from_link(
        link: &songbird_types::proxy_link::ProxyLink,
        tls: Arc<rustls::ClientConfig>,
    ) -> Self {
        Self {
            addr: format!("{}:{}", link.host, link.port),
            host: link.host.clone(),
            tls,
        }
    }
}

/// What the app persists between runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumeState {
    /// Highest `seq` the app stored durably and acknowledged.
    pub acked_seq: u64,
    /// Last store generation seen (0: never connected).
    pub store_generation: u64,
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Connecting or authenticating.
    Connecting,
    /// Authenticated; deliveries flow.
    Connected,
    /// Waiting to reconnect.
    Unavailable,
    /// Stopped after a do-not-reconnect close; the app must act.
    Failed,
}

/// Events for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Authenticated.
    Connected {
        /// Current store generation.
        store_generation: u64,
        /// Delivery resumes after this `seq`.
        resume_after_seq: u64,
    },
    /// A new envelope, exactly once and in `seq` order. Store it, then call [`Client::ack`].
    Deliver {
        /// Its sequence number.
        seq: u64,
        /// Opaque ciphertext.
        envelope: Bytes,
    },
    /// An ephemeral event (never stored).
    Ephemeral {
        /// Opaque payload.
        payload: Bytes,
    },
    /// An attention event (a nudge) that came through the server's attention class, within its
    /// budgets and the recipient's blocks (D-0108). The app honours `Content.attention` only
    /// from this event, never from [`Event::Ephemeral`].
    Attention {
        /// Opaque payload.
        payload: Bytes,
    },
    /// The server's data was restored from a backup: the client resends every message in its
    /// send log.
    StoreGenerationChanged {
        /// Generation before.
        previous: u64,
        /// Generation now.
        current: u64,
    },
    /// A resent message was refused (a device list changed, an MLS epoch moved on, a reused
    /// key): it left the send log, and the app re-encrypts and sends it anew.
    ResendRefused {
        /// Its idempotency key.
        idempotency_key: [u8; 16],
        /// The server's answer.
        status: SendStatus,
    },
    /// The connection ended; a reconnect follows after `retry_in`.
    Disconnected {
        /// Close code, if the server sent one.
        code: Option<u16>,
        /// Wait before the next attempt.
        retry_in: Duration,
    },
    /// The server closed with a do-not-reconnect code; the client stopped.
    Failed {
        /// The close code.
        code: u16,
    },
}

/// Errors returned to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// No authenticated connection right now; retry with the same idempotency key.
    #[error("not connected")]
    NotConnected,
    /// The connection dropped before the result arrived; retry with the same idempotency key.
    #[error("disconnected before the result")]
    Disconnected,
    /// The client task has stopped.
    #[error("client stopped")]
    Stopped,
}

enum Command {
    Send {
        idempotency_key: [u8; 16],
        hints: SendHints,
        payload: SendPayload,
        reply: oneshot::Sender<Result<SendStatus, ClientError>>,
    },
    Ephemeral {
        to: DeviceId,
        payload: Bytes,
    },
    /// Send an attention event (a nudge).
    Attention {
        to: DeviceId,
        payload: Bytes,
    },
    Ack(u64),
    /// The message's end-to-end delivery receipt arrived.
    Confirm([u8; 16]),
    /// Drop the socket without a close frame (tests and chaos runs).
    Abort,
    Close,
}

/// Handle to a running client. Dropping it stops the client.
#[derive(Debug)]
pub struct Client {
    cmds: mpsc::Sender<Command>,
    state: watch::Receiver<State>,
}

impl Client {
    /// Starts the client task on the current Tokio runtime. Returns the handle and the event
    /// stream.
    #[must_use]
    pub fn start(
        cfg: ClientConfig,
        key: DeviceKey,
        resume: ResumeState,
    ) -> (Self, mpsc::Receiver<Event>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let (ev_tx, ev_rx) = mpsc::channel(cfg.event_capacity.max(1));
        let (state_tx, state_rx) = watch::channel(State::Connecting);
        let task = Task {
            started: std::time::Instant::now(),
            backoff: Backoff::new(cfg.backoff),
            cfg,
            key,
            cmds: cmd_rx,
            events: ev_tx,
            state: state_tx,
            acked: resume.acked_seq,
            sent_ack: resume.acked_seq,
            delivered: resume.acked_seq,
            store_generation: resume.store_generation,
            next_req: 1,
            pending: HashMap::new(),
            log: SendLog::default(),
            resend_due: false,
            buf: BytesMut::with_capacity(1024),
            channel_binding: songbird_proto::auth::NO_CHANNEL_BINDING,
        };
        tokio::spawn(task.run());
        (
            Self {
                cmds: cmd_tx,
                state: state_rx,
            },
            ev_rx,
        )
    }

    /// Current state.
    #[must_use]
    pub fn state(&self) -> State {
        *self.state.borrow()
    }

    /// Waits until the state is `want` (or the client stopped).
    pub async fn wait_for(&mut self, want: State) -> State {
        match self
            .state
            .wait_for(|s| *s == want || *s == State::Failed)
            .await
        {
            Ok(s) => *s,
            Err(_) => State::Failed,
        }
    }

    /// Sends envelopes and waits for the server's result.
    ///
    /// # Errors
    /// [`ClientError`]: not connected, disconnected before the result, or stopped. Retry with
    /// the same `idempotency_key`: the server stores a retried send once.
    pub async fn send(
        &self,
        idempotency_key: [u8; 16],
        recipients: Vec<Recipient>,
    ) -> Result<SendStatus, ClientError> {
        self.send_with(
            idempotency_key,
            SendHints::default(),
            SendPayload::PerDevice(recipients),
        )
        .await
    }

    /// Sends per-device envelopes with hints (Prompt 4): a contact's access key, a message
    /// request, a franking commitment, the disappearing hint.
    ///
    /// # Errors
    /// As [`Client::send`].
    pub async fn send_hinted(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        recipients: Vec<Recipient>,
    ) -> Result<SendStatus, ClientError> {
        self.command(idempotency_key, hints, SendPayload::PerDevice(recipients))
            .await
    }

    /// Sends any payload with any hints (a shared body; a group message with
    /// [`SendHints::group`], Prompt 5) and waits for the result.
    ///
    /// # Errors
    /// As [`Client::send`].
    pub async fn send_with(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        payload: SendPayload,
    ) -> Result<SendStatus, ClientError> {
        self.command(idempotency_key, hints, payload).await
    }

    async fn command(
        &self,
        idempotency_key: [u8; 16],
        hints: SendHints,
        payload: SendPayload,
    ) -> Result<SendStatus, ClientError> {
        let (reply, rx) = oneshot::channel();
        self.cmds
            .send(Command::Send {
                idempotency_key,
                hints,
                payload,
                reply,
            })
            .await
            .map_err(|_| ClientError::Stopped)?;
        rx.await.map_err(|_| ClientError::Stopped)?
    }

    /// Sends an ephemeral event (best effort: dropped if not connected).
    ///
    /// # Errors
    /// [`ClientError::Stopped`].
    pub async fn ephemeral(&self, to: DeviceId, payload: Bytes) -> Result<(), ClientError> {
        self.cmds
            .send(Command::Ephemeral { to, payload })
            .await
            .map_err(|_| ClientError::Stopped)
    }

    /// Sends an attention event, a nudge (Prompt 9): best effort, within the server's budgets
    /// per pair of accounts; dropped if not connected.
    ///
    /// # Errors
    /// [`ClientError::Stopped`].
    pub async fn attention(&self, to: DeviceId, payload: Bytes) -> Result<(), ClientError> {
        self.cmds
            .send(Command::Attention { to, payload })
            .await
            .map_err(|_| ClientError::Stopped)
    }

    /// Confirms that everything up to `seq` is stored durably on the device.
    ///
    /// # Errors
    /// [`ClientError::Stopped`].
    pub async fn ack(&self, seq: u64) -> Result<(), ClientError> {
        self.cmds
            .send(Command::Ack(seq))
            .await
            .map_err(|_| ClientError::Stopped)
    }

    /// The end-to-end delivery receipt of the message sent with `idempotency_key` arrived: it
    /// leaves the send log and will not be resent after a restore.
    ///
    /// # Errors
    /// [`ClientError::Stopped`].
    pub async fn confirm_delivered(&self, idempotency_key: [u8; 16]) -> Result<(), ClientError> {
        self.cmds
            .send(Command::Confirm(idempotency_key))
            .await
            .map_err(|_| ClientError::Stopped)
    }

    /// Drops the current socket without a close frame, as a network failure would.
    ///
    /// # Errors
    /// [`ClientError::Stopped`].
    pub async fn abort_connection(&self) -> Result<(), ClientError> {
        self.cmds
            .send(Command::Abort)
            .await
            .map_err(|_| ClientError::Stopped)
    }

    /// Closes the connection cleanly and stops the client.
    pub async fn close(self) {
        let _ = self.cmds.send(Command::Close).await;
        let mut state = self.state;
        let _ = state.wait_for(|s| *s == State::Failed).await;
    }
}

enum Outcome {
    /// The server closed with this code and reason.
    Closed(u16, String),
    /// Transport error, timeout or protocol failure.
    Error,
    /// The app asked to stop.
    Shutdown,
}

struct Task {
    cfg: ClientConfig,
    key: DeviceKey,
    backoff: Backoff,
    cmds: mpsc::Receiver<Command>,
    events: mpsc::Sender<Event>,
    state: watch::Sender<State>,
    /// Highest `seq` the app confirmed durable.
    acked: u64,
    /// Highest ACK sent on the current connection.
    sent_ack: u64,
    /// Highest `seq` handed to the app.
    delivered: u64,
    store_generation: u64,
    next_req: u32,
    pending: HashMap<u32, Pending>,
    /// Accepted messages without a delivery receipt (D-0040).
    log: SendLog,
    /// The server was restored: resend the log once authenticated.
    resend_due: bool,
    buf: BytesMut,
    /// TLS exporter of the current connection (zeros without TLS).
    channel_binding: [u8; songbird_proto::auth::CHANNEL_BINDING_LEN],
    /// When [`Client::start`] ran (for [`ClientConfig::new_device_grace`]).
    started: std::time::Instant,
}

type Tls<S> = tokio_rustls::client::TlsStream<S>;

enum Conn {
    Plain(Box<WebSocketStream<TcpStream>>),
    Tls(Box<WebSocketStream<Tls<TcpStream>>>),
    /// Through a proxy: the endpoint's TLS inside the proxy's.
    Proxied(Box<WebSocketStream<Tls<Tls<TcpStream>>>>),
}

/// A send waiting for its result.
struct Pending {
    key: [u8; 16],
    hints: SendHints,
    payload: SendPayload,
    /// The app's waiter; `None` for a resend after a restore.
    reply: Option<oneshot::Sender<Result<SendStatus, ClientError>>>,
}

/// One logged message.
struct Logged {
    hints: SendHints,
    payload: SendPayload,
    at: Instant,
}

/// Accepted messages without a delivery receipt, bounded in number and age (D-0040).
#[derive(Default)]
struct SendLog {
    entries: HashMap<[u8; 16], Logged>,
    /// Insertion order, for dropping the oldest; stale keys are skipped.
    order: VecDeque<([u8; 16], Instant)>,
    /// Receipts that arrived before their send was logged: the recipient can confirm before
    /// this client has read its own `SendResult`, or between a disconnect and the app's retry.
    /// Bounded; the oldest are forgotten first.
    early: HashSet<[u8; 16]>,
    early_order: VecDeque<[u8; 16]>,
}

/// How many early receipts are remembered (a few seconds of sends; more only costs a resend
/// the recipient drops by message ID).
const EARLY_RECEIPTS: usize = 4096;

impl SendLog {
    fn insert(
        &mut self,
        key: [u8; 16],
        hints: SendHints,
        payload: SendPayload,
        cfg: &ClientConfig,
    ) {
        let now = Instant::now();
        self.prune(now, cfg);
        // A retried send replayed by the server keeps its first entry (and evicts nothing); a
        // send already confirmed by its recipient needs no entry.
        if cfg.send_log_capacity == 0 || self.entries.contains_key(&key) || self.early.remove(&key)
        {
            return;
        }
        while self.entries.len() >= cfg.send_log_capacity {
            match self.order.pop_front() {
                Some((k, at)) => {
                    if self.entries.get(&k).is_some_and(|e| e.at == at) {
                        self.entries.remove(&k);
                    }
                }
                None => break,
            }
        }
        self.entries.insert(
            key,
            Logged {
                hints,
                payload,
                at: now,
            },
        );
        self.order.push_back((key, now));
    }

    /// Drops a refused resend.
    fn remove(&mut self, key: &[u8; 16]) {
        self.entries.remove(key);
    }

    /// A delivery receipt: the entry goes; a receipt for a send not logged yet is remembered.
    fn confirm(&mut self, key: [u8; 16]) {
        if self.entries.remove(&key).is_some() || !self.early.insert(key) {
            return;
        }
        self.early_order.push_back(key);
        while self.early_order.len() > EARLY_RECEIPTS {
            if let Some(old) = self.early_order.pop_front() {
                self.early.remove(&old);
            }
        }
    }

    /// Drops entries older than the maximum age.
    fn prune(&mut self, now: Instant, cfg: &ClientConfig) {
        while let Some((k, at)) = self.order.front().copied() {
            if now.saturating_duration_since(at) < cfg.send_log_max_age {
                break;
            }
            self.order.pop_front();
            if self.entries.get(&k).is_some_and(|e| e.at == at) {
                self.entries.remove(&k);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Task {
    async fn run(mut self) {
        loop {
            self.state.send_replace(State::Connecting);
            let outcome = match self.connect().await {
                Ok(Conn::Plain(ws)) => self.session(*ws).await,
                Ok(Conn::Tls(ws)) => self.session(*ws).await,
                Ok(Conn::Proxied(ws)) => self.session(*ws).await,
                Err(()) => Outcome::Error,
            };
            for (_, p) in self.pending.drain() {
                match p.reply {
                    Some(reply) => {
                        let _ = reply.send(Err(ClientError::Disconnected));
                    }
                    // A resend cut off by the disconnect: resend again on the next connection.
                    None => self.resend_due = true,
                }
            }
            let (code, wait) = match outcome {
                Outcome::Shutdown => break,
                Outcome::Error => (None, self.backoff.next(None)),
                Outcome::Closed(code, reason) => match CloseCode::reconnect(code) {
                    // A new device the server's directory may not hold yet.
                    Reconnect::Never
                        if code == CloseCode::AuthFailed.code()
                            && self.started.elapsed() < self.cfg.new_device_grace =>
                    {
                        (Some(code), self.backoff.next(None))
                    }
                    Reconnect::Never => {
                        let _ = self.events.send(Event::Failed { code }).await;
                        break;
                    }
                    Reconnect::Now => (Some(code), Backoff::now_jitter()),
                    Reconnect::Backoff => {
                        let hint = close::parse_retry(&reason)
                            .map(|ms| Duration::from_millis(u64::from(ms)));
                        (Some(code), self.backoff.next(hint))
                    }
                },
            };
            self.state.send_replace(State::Unavailable);
            if self
                .events
                .send(Event::Disconnected {
                    code,
                    retry_in: wait,
                })
                .await
                .is_err()
                || !self.idle(wait).await
            {
                break;
            }
        }
        self.state.send_replace(State::Failed);
    }

    /// Waits `wait` while answering commands; `false` if the app asked to stop.
    async fn idle(&mut self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => return true,
                cmd = self.cmds.recv() => match cmd {
                    None | Some(Command::Close) => return false,
                    Some(Command::Send { reply, .. }) => {
                        let _ = reply.send(Err(ClientError::NotConnected));
                    }
                    Some(Command::Ack(seq)) => self.record_ack(seq),
                    Some(Command::Confirm(key)) => self.log.confirm(key),
                    Some(
                        Command::Ephemeral { .. } | Command::Attention { .. } | Command::Abort,
                    ) => {}
                },
            }
        }
    }

    fn record_ack(&mut self, seq: u64) {
        // Never acknowledge beyond what was handed to the app.
        self.acked = self.acked.max(seq.min(self.delivered));
    }

    fn ws_config(&self) -> sockudo_ws::Config {
        let max = self.cfg.limits.max_message_bytes();
        sockudo_ws::Config::builder()
            .max_message_size(max)
            .max_frame_size(max)
            // The server pings every 25 s; the client only answers (saves mobile radio wake-ups)
            // and gives up after 90 s of silence.
            .auto_ping(false)
            .idle_timeout(90)
            .build()
    }

    /// Through the proxy: TCP and TLS to the proxy (its own name), then the endpoint's TLS inside
    /// it. The channel binding is the inner session's, so the proxy (or anyone holding the outer
    /// session) cannot relay the authentication.
    async fn connect_proxied(&mut self, proxy: ProxyRoute) -> Result<Conn, ()> {
        let t = self.cfg.handshake_timeout;
        let Some(inner_tls) = self.cfg.tls.clone() else {
            return Err(());
        };
        let tcp = tokio::time::timeout(t, TcpStream::connect(&proxy.addr))
            .await
            .map_err(drop)?
            .map_err(drop)?;
        tcp.set_nodelay(true).map_err(drop)?;
        let outer_name =
            rustls_pki_types::ServerName::try_from(proxy.host.clone()).map_err(drop)?;
        let outer = tokio::time::timeout(
            t,
            tokio_rustls::TlsConnector::from(proxy.tls).connect(outer_name, tcp),
        )
        .await
        .map_err(drop)?
        .map_err(drop)?;
        let host = self.cfg.server_name.clone();
        let name = rustls_pki_types::ServerName::try_from(host.clone()).map_err(drop)?;
        let stream = tokio::time::timeout(
            t,
            tokio_rustls::TlsConnector::from(inner_tls).connect(name, outer),
        )
        .await
        .map_err(drop)?
        .map_err(drop)?;
        self.channel_binding = stream
            .get_ref()
            .1
            .export_keying_material(
                [0u8; songbird_proto::auth::CHANNEL_BINDING_LEN],
                songbird_proto::auth::EXPORTER_LABEL,
                None,
            )
            .map_err(drop)?;
        let client = WebSocketClient::<Http1>::new(self.ws_config());
        let (ws, hs) = tokio::time::timeout(
            t,
            client.connect_raw(
                stream,
                &host,
                songbird_proto::PATH,
                Some(songbird_proto::SUBPROTOCOL),
            ),
        )
        .await
        .map_err(drop)?
        .map_err(drop)?;
        check_protocol(hs.protocol.as_deref())?;
        Ok(Conn::Proxied(Box::new(ws)))
    }

    async fn connect(&mut self) -> Result<Conn, ()> {
        if let Some(proxy) = self.cfg.proxy.clone() {
            self.channel_binding = songbird_proto::auth::NO_CHANNEL_BINDING;
            return self.connect_proxied(proxy).await;
        }
        let t = self.cfg.handshake_timeout;
        let tcp = tokio::time::timeout(t, TcpStream::connect(&self.cfg.addr))
            .await
            .map_err(drop)?
            .map_err(drop)?;
        tcp.set_nodelay(true).map_err(drop)?;
        let client = WebSocketClient::<Http1>::new(self.ws_config());
        let host = self.cfg.server_name.clone();
        self.channel_binding = songbird_proto::auth::NO_CHANNEL_BINDING;
        let proto = Some(songbird_proto::SUBPROTOCOL);
        if let Some(tls) = &self.cfg.tls {
            let name = rustls_pki_types::ServerName::try_from(host.clone()).map_err(drop)?;
            let connector = tokio_rustls::TlsConnector::from(Arc::clone(tls));
            let stream = tokio::time::timeout(t, connector.connect(name, tcp))
                .await
                .map_err(drop)?
                .map_err(drop)?;
            // RFC 9266 tls-exporter, signed during authentication (D-0018).
            self.channel_binding = stream
                .get_ref()
                .1
                .export_keying_material(
                    [0u8; songbird_proto::auth::CHANNEL_BINDING_LEN],
                    songbird_proto::auth::EXPORTER_LABEL,
                    None,
                )
                .map_err(drop)?;
            let (ws, hs) = tokio::time::timeout(
                t,
                client.connect_raw(stream, &host, songbird_proto::PATH, proto),
            )
            .await
            .map_err(drop)?
            .map_err(drop)?;
            check_protocol(hs.protocol.as_deref())?;
            Ok(Conn::Tls(Box::new(ws)))
        } else {
            let (ws, hs) = tokio::time::timeout(
                t,
                client.connect_raw(tcp, &host, songbird_proto::PATH, proto),
            )
            .await
            .map_err(drop)?
            .map_err(drop)?;
            check_protocol(hs.protocol.as_deref())?;
            Ok(Conn::Plain(Box::new(ws)))
        }
    }

    fn encode(&mut self, f: &ClientFrame) -> Bytes {
        f.encode(&mut self.buf);
        self.buf.split().freeze()
    }

    /// Writes a `Send` and records it as pending.
    async fn send_frame<S>(
        &mut self,
        ws: &mut WebSocketStream<S>,
        idempotency_key: [u8; 16],
        hints: SendHints,
        payload: SendPayload,
        reply: Option<oneshot::Sender<Result<SendStatus, ClientError>>>,
    ) -> Result<(), ()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let req_id = self.next_req;
        self.next_req = self.next_req.wrapping_add(1);
        let f = ClientFrame::Send(SendFrame {
            hints,
            req_id,
            idempotency_key,
            payload: payload.clone(),
        });
        self.pending.insert(
            req_id,
            Pending {
                key: idempotency_key,
                hints,
                payload,
                reply,
            },
        );
        self.write(ws, &f).await
    }

    /// Resends every logged message after a restore, oldest first, with its original key.
    async fn resend<S>(&mut self, ws: &mut WebSocketStream<S>) -> Result<(), ()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        self.log.prune(Instant::now(), &self.cfg);
        let mut todo: Vec<([u8; 16], Instant)> =
            self.log.entries.iter().map(|(k, e)| (*k, e.at)).collect();
        todo.sort_by_key(|(_, at)| *at);
        for (key, _) in todo {
            let Some(e) = self.log.entries.get(&key) else {
                continue;
            };
            let (hints, payload) = (e.hints, e.payload.clone());
            self.send_frame(ws, key, hints, payload, None).await?;
        }
        Ok(())
    }

    async fn write<S>(&mut self, ws: &mut WebSocketStream<S>, f: &ClientFrame) -> Result<(), ()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let b = self.encode(f);
        ws::feed(ws, Message::Binary(b)).await.map_err(drop)?;
        ws::flush(ws).await.map_err(drop)
    }

    /// Reads the next server frame within the handshake timeout.
    async fn read_frame<S>(&self, ws: &mut WebSocketStream<S>) -> Result<ServerFrame, Outcome>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let deadline = Instant::now() + self.cfg.handshake_timeout;
        loop {
            let msg = tokio::time::timeout_at(deadline, ws::next(ws))
                .await
                .map_err(|_| Outcome::Error)?;
            match msg {
                Some(Ok(Message::Binary(b))) => {
                    return ServerFrame::decode(&b, &self.cfg.limits).map_err(|_| Outcome::Error);
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(r)) | Err(sockudo_ws::Error::Closed(r))) => {
                    return Err(closed(r));
                }
                _ => return Err(Outcome::Error),
            }
        }
    }

    async fn authenticate<S>(&mut self, ws: &mut WebSocketStream<S>) -> Result<(), Outcome>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let ServerFrame::Hello(hello) = self.read_frame(ws).await? else {
            return Err(Outcome::Error);
        };
        if !hello.protocol_versions.contains(&PROTOCOL_VERSION) {
            return Err(Outcome::Closed(
                CloseCode::UnsupportedVersion.code(),
                String::new(),
            ));
        }
        // A restore from backup: the server's mailboxes and `seq` counters are from the backup,
        // so `seq`s already seen here may be reused for new envelopes. Start over from 0; the app
        // drops repeats by the message ID inside each envelope (PROTOCOL.md §3.6).
        let restored = self.store_generation != 0 && hello.store_generation > self.store_generation;
        if restored {
            self.acked = 0;
            self.sent_ack = 0;
            self.delivered = 0;
        }
        let device = self.key.device();
        let transcript = songbird_proto::auth::transcript(
            &self.cfg.server_name,
            &hello.nonce,
            &device,
            PROTOCOL_VERSION,
            &self.channel_binding,
        )
        .map_err(|_| Outcome::Error)?;
        let auth = ClientFrame::Auth(control::Auth {
            device_id: device.as_bytes().to_vec(),
            signature: self.key.sign(&transcript).to_vec(),
            last_acked_seq: self.acked,
            client_version: self.cfg.client_version.clone(),
            protocol_version: PROTOCOL_VERSION,
        });
        self.write(ws, &auth).await.map_err(|()| Outcome::Error)?;
        self.sent_ack = self.acked;
        let ok = loop {
            match self.read_frame(ws).await? {
                ServerFrame::AuthOk(ok) => break ok,
                // An `Error` frame precedes the close; the close carries the decision.
                ServerFrame::Error(_) => {}
                _ => return Err(Outcome::Error),
            }
        };
        if restored {
            self.resend_due = true;
            let _ = self
                .events
                .send(Event::StoreGenerationChanged {
                    previous: self.store_generation,
                    current: ok.store_generation,
                })
                .await;
        }
        self.store_generation = ok.store_generation;
        Ok(())
    }

    async fn session<S>(&mut self, mut ws: WebSocketStream<S>) -> Outcome
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        if let Err(o) = self.authenticate(&mut ws).await {
            return o;
        }
        self.backoff.reset();
        self.state.send_replace(State::Connected);
        if self
            .events
            .send(Event::Connected {
                store_generation: self.store_generation,
                resume_after_seq: self.acked,
            })
            .await
            .is_err()
        {
            return Outcome::Shutdown;
        }
        if self.resend_due {
            self.resend_due = false;
            if self.resend(&mut ws).await.is_err() {
                return Outcome::Error;
            }
        }
        let mut ack_deadline: Option<Instant> = None;
        loop {
            let ack_timer = async {
                match ack_deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                msg = ws::next(&mut ws) => match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if let Err(o) = self.on_frame(&b).await {
                            return o;
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    Some(Ok(Message::Close(r)) | Err(sockudo_ws::Error::Closed(r))) => {
                        return closed(r);
                    }
                    _ => return Outcome::Error,
                },
                cmd = self.cmds.recv() => {
                    let r = match cmd {
                        None | Some(Command::Close) => {
                            // Flush a coalesced ACK first, so a clean close never leaves
                            // stored envelopes to be redelivered on the next connection.
                            if self.acked > self.sent_ack {
                                let up_to_seq = self.acked;
                                let _ = self.write(&mut ws, &ClientFrame::Ack { up_to_seq }).await;
                            }
                            let _ = ws.close(1000, "").await;
                            return Outcome::Shutdown;
                        }
                        Some(Command::Abort) => return Outcome::Error,
                        Some(Command::Ack(seq)) => {
                            self.record_ack(seq);
                            if self.acked > self.sent_ack && ack_deadline.is_none() {
                                ack_deadline = Some(Instant::now() + self.cfg.ack_delay);
                            }
                            Ok(())
                        }
                        Some(Command::Attention { to, payload }) => {
                            self.write(&mut ws, &ClientFrame::Attention { to, payload }).await
                        }
                        Some(Command::Ephemeral { to, payload }) => {
                            self.write(&mut ws, &ClientFrame::Ephemeral { to, payload }).await
                        }
                        Some(Command::Confirm(key)) => {
                            self.log.confirm(key);
                            Ok(())
                        }
                        Some(Command::Send { idempotency_key, hints, payload, reply }) => {
                            self.send_frame(&mut ws, idempotency_key, hints, payload, Some(reply)).await
                        }
                    };
                    if r.is_err() {
                        return Outcome::Error;
                    }
                }
                () = ack_timer => {
                    ack_deadline = None;
                    let up_to_seq = self.acked;
                    if self.write(&mut ws, &ClientFrame::Ack { up_to_seq }).await.is_err() {
                        return Outcome::Error;
                    }
                    self.sent_ack = up_to_seq;
                }
            }
        }
    }

    async fn on_frame(&mut self, b: &Bytes) -> Result<(), Outcome> {
        match ServerFrame::decode(b, &self.cfg.limits).map_err(|_| Outcome::Error)? {
            ServerFrame::Deliver { seq, envelope } => {
                // Already handed to the app (redelivered after a reconnect before the ACK
                // landed): drop, so the app sees each seq exactly once.
                if seq <= self.delivered {
                    return Ok(());
                }
                self.delivered = seq;
                self.events
                    .send(Event::Deliver { seq, envelope })
                    .await
                    .map_err(|_| Outcome::Shutdown)
            }
            ServerFrame::SendResult { req_id, status } => {
                let Some(p) = self.pending.remove(&req_id) else {
                    return Ok(());
                };
                match (&status, p.reply) {
                    // A first send: logged once accepted, then answered.
                    (SendStatus::Accepted, Some(reply)) => {
                        self.log.insert(p.key, p.hints, p.payload, &self.cfg);
                        let _ = reply.send(Ok(status));
                    }
                    (_, Some(reply)) => {
                        let _ = reply.send(Ok(status));
                    }
                    // A resend: stays in the log when accepted or worth retrying at the next
                    // restore; anything else needs the app (new device list, new epoch).
                    (SendStatus::Accepted, None) => {}
                    (SendStatus::Rejected { reason, .. }, None) if reason.is_retryable() => {}
                    (_, None) => {
                        self.log.remove(&p.key);
                        let _ = self
                            .events
                            .send(Event::ResendRefused {
                                idempotency_key: p.key,
                                status,
                            })
                            .await;
                    }
                }
                Ok(())
            }
            ServerFrame::Ephemeral { payload } => {
                // Ephemeral events never block the connection: dropped if the app is behind.
                let _ = self.events.try_send(Event::Ephemeral { payload });
                Ok(())
            }
            ServerFrame::Attention { payload } => {
                let _ = self.events.try_send(Event::Attention { payload });
                Ok(())
            }
            ServerFrame::Error(_) => Ok(()),
            ServerFrame::Hello(_) | ServerFrame::AuthOk(_) => Err(Outcome::Error),
        }
    }
}

fn check_protocol(p: Option<&str>) -> Result<(), ()> {
    if p == Some(songbird_proto::SUBPROTOCOL) {
        Ok(())
    } else {
        Err(())
    }
}

fn closed(r: Option<sockudo_ws::error::CloseReason>) -> Outcome {
    r.map_or(Outcome::Error, |r| Outcome::Closed(r.code, r.reason))
}

#[cfg(test)]
mod send_log_tests {
    use super::*;

    fn cfg(capacity: usize, max_age: Duration) -> ClientConfig {
        let mut c = ClientConfig::new("127.0.0.1:1", "localhost");
        c.send_log_capacity = capacity;
        c.send_log_max_age = max_age;
        c
    }

    fn empty() -> SendPayload {
        SendPayload::PerDevice(Vec::new())
    }

    #[test]
    fn the_log_is_bounded_by_count_and_age_and_keeps_first_entries() {
        let c = cfg(3, Duration::from_secs(60));
        let mut log = SendLog::default();
        for k in 1..=3u8 {
            log.insert([k; 16], SendHints::default(), empty(), &c);
        }
        // A replayed send keeps its entry (and its place).
        log.insert([1; 16], SendHints::default(), empty(), &c);
        assert_eq!(log.len(), 3);
        // Full: the oldest goes.
        log.insert([4; 16], SendHints::default(), empty(), &c);
        assert_eq!(log.len(), 3);
        assert!(!log.entries.contains_key(&[1; 16]));
        // A receipt removes one.
        log.confirm([2; 16]);
        assert_eq!(log.len(), 2);
        // Past the maximum age everything goes.
        log.prune(Instant::now() + Duration::from_secs(61), &c);
        assert_eq!(log.len(), 0);
        // A removed key re-sent later is logged again.
        log.insert([2; 16], SendHints::default(), empty(), &c);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn a_receipt_before_the_send_result_keeps_the_send_out_of_the_log() {
        let c = cfg(10, Duration::from_secs(60));
        let mut log = SendLog::default();
        // The recipient confirmed before this client read its `SendResult`.
        log.confirm([7; 16]);
        log.insert([7; 16], SendHints::default(), empty(), &c);
        assert_eq!(log.len(), 0);
        // Remembered once: a later send with that key is logged as usual.
        log.insert([7; 16], SendHints::default(), empty(), &c);
        assert_eq!(log.len(), 1);
        // Early receipts are bounded.
        for i in 0..=EARLY_RECEIPTS {
            let mut k = [0u8; 16];
            k[..8].copy_from_slice(&i.to_be_bytes());
            log.confirm(k);
        }
        assert_eq!(log.early.len(), EARLY_RECEIPTS);
        assert_eq!(log.early_order.len(), EARLY_RECEIPTS);
    }
}
