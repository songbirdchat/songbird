//! The call key schedule and signalling (Prompt 7, PROTOCOL §8, D-0086): published test vectors
//! (`vectors/call/v1.txt`, rebuilt independently by `tools/call-vectors/check.mjs`), properties
//! of the derivations, sealed cards and the signalling bounds.
//!
//! `SONGBIRD_WRITE_VECTORS=1 cargo test -p songbird-content --test call` rewrites the vectors.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests"
)]

use proptest::prelude::*;
use prost::Message as _;
use songbird_content::call::{
    self, EpochSecret, KEY_RING, LinkSecrets, MAX_RING_MS, link_id_hash, open_card, seal_card,
    seal_card_with_nonce,
};
use songbird_content::schema::{
    CallAnswer, CallHangup, CallLinkCard, CallMessage, CallOffer, CallSenderKey, Content,
    call_event, call_message::Kind, content::Body,
};
use songbird_content::{Error, validate};

fn seq(start: u8, n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| start.wrapping_add(u8::try_from(i % 256).unwrap()))
        .collect()
}

fn arr<const N: usize>(start: u8) -> [u8; N] {
    seq(start, N).try_into().unwrap()
}

fn card(identity: &str, name: &str) -> CallLinkCard {
    CallLinkCard {
        account: vec![0xA1; 16],
        device: vec![0xD1; 16],
        identity: identity.to_owned(),
        name: name.to_owned(),
    }
}

/// Each line: a kind, then its inputs and outputs in hex.
///
/// - `1to1 call_id call_secret identity sender_key`
/// - `sender epoch_secret identity sender_key`
/// - `link root id key id_hash`
/// - `card key nonce card sealed`
fn vector_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for (call_id, secret) in [(arr::<16>(0x00), arr::<32>(0x40)), (arr(0x10), arr(0x80))] {
        // The API never hands out an epoch secret, so a 1:1 line pins it through one sender's key.
        let identity = b"0123456789abcdef0123456789abcdef";
        let s = EpochSecret::one_to_one(&call_id, &secret).unwrap();
        let key = s.sender_key(identity).unwrap();
        lines.push(format!(
            "1to1 {} {} {} {}",
            hex::encode(call_id),
            hex::encode(secret),
            hex::encode(identity),
            hex::encode(*key)
        ));
    }
    for (epoch_secret, identity) in [
        (
            arr::<32>(0x20),
            "0123456789abcdef0123456789abcdef".to_owned(),
        ),
        (arr(0x20), "0123456789abcdef0123456789abcdee".to_owned()),
        (arr(0x21), "a".to_owned()),
        (arr(0x22), "z".repeat(64)),
    ] {
        let s = EpochSecret::group(epoch_secret, 7);
        let key = s.sender_key(identity.as_bytes()).unwrap();
        lines.push(format!(
            "sender {} {} {}",
            hex::encode(epoch_secret),
            hex::encode(identity.as_bytes()),
            hex::encode(*key)
        ));
    }
    for root in [arr::<32>(0x00), arr(0xC0)] {
        let l = LinkSecrets::from_root(&root).unwrap();
        lines.push(format!(
            "link {} {} {} {}",
            hex::encode(root),
            hex::encode(*l.id),
            hex::encode(*l.key),
            hex::encode(link_id_hash(&l.id))
        ));
    }
    for (key, nonce, c) in [
        (
            arr::<32>(0x30),
            arr::<12>(0x01),
            card("0123456789abcdef0123456789abcdef", ""),
        ),
        (arr(0x31), arr(0x02), card("p", "Ada Lovelace")),
    ] {
        let sealed = seal_card_with_nonce(&key, &c, nonce).unwrap();
        lines.push(format!(
            "card {} {} {} {}",
            hex::encode(key),
            hex::encode(nonce),
            hex::encode(c.encode_to_vec()),
            hex::encode(sealed)
        ));
    }
    lines
}

#[test]
fn the_published_vectors_hold() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors/call/v1.txt");
    let got = vector_lines().join("\n") + "\n";
    if std::env::var_os("SONGBIRD_WRITE_VECTORS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .expect("vectors/call/v1.txt (SONGBIRD_WRITE_VECTORS=1 writes it)")
        .replace("\r\n", "\n");
    assert_eq!(got, want);
}

#[test]
fn the_vectors_open() {
    for line in vector_lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if f[0] == "card" {
            let key: [u8; 32] = hex::decode(f[1]).unwrap().try_into().unwrap();
            let sealed = hex::decode(f[4]).unwrap();
            let c = open_card(&key, &sealed).unwrap();
            assert_eq!(hex::encode(c.encode_to_vec()), f[3]);
        }
    }
}

#[test]
fn identities_are_bounded() {
    let s = EpochSecret::group([1; 32], 0);
    assert_eq!(
        s.sender_key(b"").unwrap_err(),
        Error::Invalid("call.identity")
    );
    assert!(s.sender_key(&[b'a'; 64]).is_ok());
    assert_eq!(
        s.sender_key(&[b'a'; 65]).unwrap_err(),
        Error::Invalid("call.identity")
    );
}

#[test]
fn key_indexes_wrap_at_the_ring() {
    assert_eq!(call::key_index(0), 0);
    assert_eq!(call::key_index(15), 15);
    assert_eq!(call::key_index(16), 0);
    assert_eq!(call::key_index(u64::MAX), 15);
}

#[test]
fn cards_are_sealed_under_the_link_key_only() {
    let l = LinkSecrets::from_root(&[9; 32]).unwrap();
    let other = LinkSecrets::from_root(&[8; 32]).unwrap();
    let c = card("p", "Grace");
    let sealed = seal_card(&l.key, &c).unwrap();
    assert_eq!(open_card(&l.key, &sealed).unwrap(), c);
    assert_eq!(open_card(&other.key, &sealed).unwrap_err(), Error::Decode);
    // The ID the server sees opens nothing.
    assert_eq!(open_card(&l.id, &sealed).unwrap_err(), Error::Decode);
    // Random nonces: two seals differ.
    assert_ne!(sealed, seal_card(&l.key, &c).unwrap());
    // Bounds before sealing and before opening.
    assert_eq!(
        seal_card(&l.key, &card("p", &"n".repeat(129))).unwrap_err(),
        Error::Invalid("card.name")
    );
    assert_eq!(
        seal_card(&l.key, &card("", "")).unwrap_err(),
        Error::Invalid("card.identity")
    );
    assert_eq!(open_card(&l.key, &[1; 513]).unwrap_err(), Error::TooLarge);
    assert_eq!(open_card(&l.key, &[]).unwrap_err(), Error::Decode);
    assert_eq!(open_card(&l.key, &[2; 40]).unwrap_err(), Error::Decode);
    assert_eq!(open_card(&l.key, &[1; 5]).unwrap_err(), Error::Decode);
}

fn msg(kind: Kind) -> Content {
    Content {
        version: 1,
        message_id: vec![1; 16],
        sent_at_ms: 1_800_000_000_000,
        body: Some(Body::Call(CallMessage {
            call_id: vec![2; 16],
            kind: Some(kind),
        })),
        ..Content::default()
    }
}

fn offer(secret: usize, group: usize) -> Kind {
    Kind::Offer(CallOffer {
        media: call_event::Kind::Video.into(),
        call_secret: vec![3; secret],
        group_id: vec![4; group],
        ring_until_ms: 1_800_000_060_000,
    })
}

#[test]
fn signalling_is_bounded() {
    assert!(validate::content(&msg(offer(32, 0))).is_ok());
    assert!(validate::content(&msg(offer(0, 16))).is_ok());
    for (s, g) in [(0, 0), (32, 16), (31, 0), (0, 15)] {
        assert_eq!(
            validate::content(&msg(offer(s, g))).unwrap_err(),
            Error::Invalid("call.offer.secret"),
            "{s} {g}"
        );
    }
    let Kind::Offer(mut o) = offer(32, 0) else {
        unreachable!()
    };
    o.media = 0;
    assert_eq!(
        validate::content(&msg(Kind::Offer(o))).unwrap_err(),
        Error::Invalid("call.offer.media")
    );
    assert!(validate::content(&msg(Kind::Answer(CallAnswer {}))).is_ok());
    assert_eq!(
        validate::content(&msg(Kind::Hangup(CallHangup { reason: 0 }))).unwrap_err(),
        Error::Invalid("call.hangup.reason")
    );
    let sk = |identity: &str, key_index: u32, key: usize| {
        Kind::SenderKey(CallSenderKey {
            identity: identity.to_owned(),
            key_index,
            key: vec![5; key],
        })
    };
    assert!(validate::content(&msg(sk("p", 15, 32))).is_ok());
    assert_eq!(
        validate::content(&msg(sk("p", KEY_RING, 32))).unwrap_err(),
        Error::Invalid("call.sender_key.key_index")
    );
    assert_eq!(
        validate::content(&msg(sk("p", 0, 31))).unwrap_err(),
        Error::Invalid("call.sender_key.key")
    );
    assert_eq!(
        validate::content(&msg(sk("", 0, 32))).unwrap_err(),
        Error::Invalid("call.sender_key.identity")
    );
    let mut c = msg(Kind::Answer(CallAnswer {}));
    if let Some(Body::Call(m)) = &mut c.body {
        m.call_id = vec![2; 15];
    }
    assert_eq!(
        validate::content(&c).unwrap_err(),
        Error::Invalid("call.call_id")
    );
}

#[test]
fn ringing_is_capped() {
    let t = 1_000_000;
    assert_eq!(call::ring_left_ms(t, t + 30_000, t), 30_000);
    assert_eq!(call::ring_left_ms(t, t + 10 * MAX_RING_MS, t), MAX_RING_MS);
    assert_eq!(call::ring_left_ms(t, t + 30_000, t + 31_000), 0);
    assert_eq!(call::ring_left_ms(t, u64::MAX, u64::MAX), 0);
    // A sender clock in the future cannot make a phone ring longer.
    assert_eq!(call::ring_left_ms(t + 3_600_000, u64::MAX, t), MAX_RING_MS);
    assert_eq!(call::ring_left_ms(u64::MAX, u64::MAX, t), MAX_RING_MS);
}

#[test]
fn debug_prints_no_key() {
    let s = EpochSecret::group([0x77; 32], 3);
    let d = format!("{s:?} {:?}", LinkSecrets::from_root(&[0x77; 32]).unwrap());
    assert!(!d.contains("77") && !d.contains("119"), "{d}");
}

proptest! {
    #[test]
    fn sender_keys_differ_by_identity_secret_and_call(
        a in proptest::collection::vec(any::<u8>(), 1..=64),
        b in proptest::collection::vec(any::<u8>(), 1..=64),
        s1 in any::<[u8; 32]>(),
        s2 in any::<[u8; 32]>(),
        c1 in any::<[u8; 16]>(),
        c2 in any::<[u8; 16]>(),
    ) {
        let g1 = EpochSecret::group(s1, 0);
        prop_assert_eq!(a == b, *g1.sender_key(&a).unwrap() == *g1.sender_key(&b).unwrap());
        let g2 = EpochSecret::group(s2, 0);
        prop_assert_eq!(s1 == s2, *g1.sender_key(&a).unwrap() == *g2.sender_key(&a).unwrap());
        let o1 = EpochSecret::one_to_one(&c1, &s1).unwrap();
        let o2 = EpochSecret::one_to_one(&c2, &s1).unwrap();
        prop_assert_eq!(c1 == c2, *o1.sender_key(&a).unwrap() == *o2.sender_key(&a).unwrap());
    }

    #[test]
    fn consecutive_epochs_never_share_a_key_index(e in 0u64..u64::MAX - 16) {
        let window: Vec<u32> = (e..e + 16).map(call::key_index).collect();
        let mut sorted = window.clone();
        sorted.sort_unstable();
        sorted.dedup();
        prop_assert_eq!(sorted.len(), 16);
        prop_assert!(window.iter().all(|i| *i < KEY_RING));
    }

    #[test]
    fn a_tampered_card_never_opens(i in 0usize..200, bit in 0u8..8) {
        let l = LinkSecrets::from_root(&[3; 32]).unwrap();
        let mut sealed = seal_card(&l.key, &card("pseudonym", "Name")).unwrap();
        let i = i % sealed.len();
        sealed[i] ^= 1 << bit;
        prop_assert!(open_card(&l.key, &sealed).is_err());
    }

    #[test]
    fn opening_arbitrary_bytes_never_panics(b in proptest::collection::vec(any::<u8>(), 0..600)) {
        let _ = open_card(&[1; 32], &b);
    }
}

#[test]
fn verification_codes_agree_only_on_the_same_secret() {
    let call = [7u8; 16];
    let a = EpochSecret::one_to_one(&call, &[1; 32]).unwrap();
    let b = EpochSecret::one_to_one(&call, &[1; 32]).unwrap();
    let other = EpochSecret::one_to_one(&call, &[2; 32]).unwrap();
    assert_eq!(a.verification_code(), b.verification_code());
    assert_ne!(a.verification_code(), other.verification_code());
    assert!(a.verification_code().iter().all(|s| *s < 64));
    // A new group epoch shows a new code.
    assert_ne!(
        EpochSecret::group([9; 32], 3).verification_code(),
        EpochSecret::group([8; 32], 4).verification_code()
    );
}
