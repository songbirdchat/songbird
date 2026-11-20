//! Songbird's client core (A2, D-0136).
//!
//! Everything security-critical on the device lives here, so the UI only displays state and sends
//! intents (A0, architecture rules):
//!
//! - [`crypto`]: X25519, ML-KEM, HKDF and AES-256-GCM on aws-lc-rs (the constitution's library);
//!   PQXDH ([`crypto::pqxdh`]) and the post-quantum Double Ratchet ([`crypto::ratchet`]),
//!   implemented from Signal's published specifications (D-0047: implement the protocols, do not
//!   link libsignal); the sealed per-device envelope ([`crypto::envelope`]) that carries who sent a
//!   message, since the server never says (PROTOCOL §1.2, `Deliver` has no sender); safety numbers
//!   ([`crypto::safety`]).
//! - [`session`]: one session per remote device, the prekey store, and encrypting and decrypting
//!   whole envelopes.
//! - [`store`]: the `SQLCipher` store (D-0137).
//!
//! No service links this crate (the server never decrypts).
#![forbid(unsafe_code)]

pub mod attachments;
pub mod backup;
pub mod blob;
pub mod call_links;
pub mod calls;
pub mod crypto;
pub mod kt;
pub mod maps;
pub mod messenger;
pub mod mls;
pub mod packs;
pub mod session;
pub mod store;
pub mod username_link;

/// Errors the core reports. Decryption failures are deliberately coarse: which check failed is
/// never told to a peer, and the UI shows one plain message.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// A message or key did not parse.
    #[error("malformed input")]
    Malformed,
    /// A signature, tag or key check failed.
    #[error("verification failed")]
    Verify,
    /// The system's random source failed (nothing is safe to do without it).
    #[error("no randomness")]
    Random,
    /// A message from too far ahead in a chain, or a chain skipped too often.
    #[error("too many skipped messages")]
    TooFarAhead,
    /// A message already decrypted, or whose key was already used.
    #[error("duplicate message")]
    Duplicate,
    /// No session with the device, and the message does not start one.
    #[error("no session")]
    NoSession,
    /// A one-time or signed prekey the message names is not held (used or replaced).
    #[error("unknown prekey")]
    UnknownPrekey,
    /// The on-device store failed (no detail: it could carry content).
    #[error("storage")]
    Storage,
    /// The store's key is wrong (or the file is not a Songbird store).
    #[error("locked")]
    Locked,
    /// The server answered an unexpected status.
    #[error("server status {0}")]
    Server(u16),
    /// An account's identity key differs from the one pinned: the user must accept the change.
    #[error("identity changed")]
    IdentityChanged,
    /// The network failed (retry later).
    #[error("network")]
    Network,
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;
