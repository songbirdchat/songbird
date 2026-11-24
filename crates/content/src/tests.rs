#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

use proptest::prelude::*;

use super::*;
use crate::schema::{
    DataMessage, Message, MessageRef, PollVote, Span, Text, content::Body, data_message::Kind, span,
};

fn text_content(body: &str) -> Content {
    Content {
        version: VERSION,
        message_id: vec![1; ID_LEN],
        sent_at_ms: 1_800_000_000_000,
        body: Some(Body::Data(DataMessage {
            kind: Some(Kind::Message(Message {
                text: Some(Text {
                    body: body.into(),
                    spans: vec![],
                }),
                ..Message::default()
            })),
            ..DataMessage::default()
        })),
        ..Content::default()
    }
}

#[test]
fn franking_commitment_known_answer() {
    // Recomputed independently: Python's hmac over the same context and bytes.
    assert_eq!(
        hex::encode(commitment(&[0x42; 32], b"hello")),
        "13997e62c03ebcbe38fec3dc88e8b002d8f90378f5e174db2563cbef853680f5"
    );
}

#[test]
fn padding_is_canonical() {
    for n in [0usize, 1, 158, 159, 160, 161, 319, 320] {
        let bytes = vec![7u8; n];
        let p = pad(&bytes);
        assert!(p.len().is_multiple_of(PAD_BLOCK) && p.len() > n && p.len() - n <= PAD_BLOCK);
        assert_eq!(unpad(&p).unwrap(), bytes.as_slice());
    }
    // Missing marker, all zeros, a length off the block, a second block of padding, too large.
    assert_eq!(unpad(&[0u8; 160]), Err(Error::Padding));
    assert_eq!(unpad(&[1u8; 160]), Err(Error::Padding));
    assert_eq!(unpad(&[0x80u8; 161]), Err(Error::Padding));
    let mut two = vec![1u8, 0x80];
    two.resize(320, 0);
    assert_eq!(unpad(&two), Err(Error::Padding));
    assert_eq!(unpad(&[]), Err(Error::Padding));
    assert_eq!(
        unpad(&vec![0x80; MAX_PADDED + PAD_BLOCK]),
        Err(Error::TooLarge)
    );
}

#[test]
fn sealed_plaintexts_open_and_franking_verifies() {
    let c = text_content("hi");
    let (padded, commit) = seal(&c, Some([9; 32])).unwrap();
    assert!(padded.len().is_multiple_of(PAD_BLOCK));
    let o = open(&padded).unwrap();
    assert_eq!(o.content, c);
    assert_eq!(o.franking_key, Some([9; 32]));
    assert!(o.franking_matches(&commit.unwrap()));
    assert!(!o.franking_matches(&[0; 32]));
    assert!(!o.needs_update());

    let (padded, commit) = seal(&c, None).unwrap();
    assert!(commit.is_none());
    let o = open(&padded).unwrap();
    assert_eq!(o.franking_key, None);
    assert!(!o.franking_matches(&commitment(&[9; 32], &o.content_bytes)));
}

#[test]
fn a_newer_writer_asks_for_an_update() {
    let mut c = text_content("hi");
    c.required_version = VERSION + 1;
    let (padded, _) = seal(&c, None).unwrap();
    assert!(open(&padded).unwrap().needs_update());
}

#[test]
fn bounds_are_refused_with_their_field() {
    let mut c = text_content("hi");
    c.message_id = vec![1; 15];
    assert_eq!(
        seal(&c, None).unwrap_err(),
        Error::Invalid("content.message_id")
    );

    let mut c = text_content("é");
    if let Some(Body::Data(d)) = &mut c.body
        && let Some(Kind::Message(m)) = &mut d.kind
    {
        // A span ending inside the two-byte character.
        m.text.as_mut().unwrap().spans.push(Span {
            start: 0,
            length: 1,
            kind: Some(span::Kind::Style(span::Style::Bold.into())),
        });
    }
    assert_eq!(seal(&c, None).unwrap_err(), Error::Invalid("span.range"));

    let big = "x".repeat(validate::MAX_TEXT + 1);
    assert_eq!(
        seal(&text_content(&big), None).unwrap_err(),
        Error::Invalid("text.body")
    );

    let vote = |options: Vec<u32>| Content {
        body: Some(Body::Data(DataMessage {
            kind: Some(Kind::PollVote(PollVote {
                poll: Some(MessageRef {
                    author: vec![2; 16],
                    message_id: vec![3; 16],
                }),
                options,
                counter: 1,
            })),
            ..DataMessage::default()
        })),
        ..text_content("")
    };
    assert!(seal(&vote(vec![0, 11]), None).is_ok());
    assert_eq!(
        seal(&vote(vec![1, 1]), None).unwrap_err(),
        Error::Invalid("vote.options")
    );
    assert_eq!(
        seal(&vote(vec![12]), None).unwrap_err(),
        Error::Invalid("vote.options")
    );
    assert_eq!(
        seal(&vote(vec![40]), None).unwrap_err(),
        Error::Invalid("vote.options")
    );
}

#[test]
fn a_franking_key_of_the_wrong_length_is_refused() {
    let p = Plaintext {
        content: text_content("hi").encode_to_vec(),
        franking_key: vec![1; 31],
        profile_key: Vec::new(),
    };
    assert_eq!(
        open(&pad(&p.encode_to_vec())).unwrap_err(),
        Error::Invalid("plaintext.franking_key")
    );
}

#[test]
fn the_profile_key_travels_beside_the_committed_content() {
    let c = text_content("accepting your request");
    let (padded, commit) = seal_with_profile_key(&c, Some([5; 32]), Some([0x77; 32])).unwrap();
    let o = open(&padded).unwrap();
    assert_eq!(o.profile_key, Some([0x77; 32]));
    assert!(o.franking_matches(&commit.unwrap()));
    // What a report would reveal (the committed bytes) does not hold it.
    assert!(!o.content_bytes.windows(32).any(|w| w == [0x77; 32]));
}

#[test]
fn debug_prints_no_content() {
    let c = text_content("secret words");
    let d = format!(
        "{c:?} {:?}",
        open(&seal(&c, Some([5; 32])).unwrap().0).unwrap()
    );
    assert!(!d.contains("secret"), "{d}");
    assert!(!d.contains("5, 5"), "{d}");
}

proptest! {
    #[test]
    fn open_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = open(&bytes);
        let _ = open(&pad(&bytes));
    }

    #[test]
    fn any_text_round_trips(body in "\\PC{0,200}") {
        let c = text_content(&body);
        if body.is_empty() {
            prop_assert!(seal(&c, None).is_err());
        } else {
            let (p, _) = seal(&c, None).unwrap();
            prop_assert_eq!(open(&p).unwrap().content, c);
        }
    }
}

#[test]
fn a_storage_sync_carries_no_key_or_a_whole_one() {
    use crate::schema::{StorageSync, SyncMessage, sync_message};
    let with = |key: Vec<u8>| Content {
        body: Some(Body::Sync(SyncMessage {
            kind: Some(sync_message::Kind::Storage(StorageSync {
                key,
                changed: true,
            })),
        })),
        ..text_content("")
    };
    assert!(validate::content(&with(Vec::new())).is_ok());
    assert!(validate::content(&with(vec![7; KEY_LEN])).is_ok());
    assert!(validate::content(&with(vec![7; 31])).is_err());
}

#[test]
fn albums_are_checked() {
    use crate::schema::{Album, AttachmentPointer};
    let photo = || AttachmentPointer {
        object_id: vec![1; 32],
        key: vec![2; 64],
        digest: vec![3; 32],
        size: 10,
        content_type: "image/jpeg".into(),
        ..Default::default()
    };
    let album = |id: usize, index: u32, count: u32| Album {
        id: vec![9; id],
        index,
        count,
    };
    let check = |m: Message| validate::message_for_tests(&m);
    let with = |a: Album, attachments: Vec<AttachmentPointer>, view_once: bool| Message {
        attachments,
        view_once,
        album: Some(a),
        ..Default::default()
    };
    assert!(check(with(album(16, 0, 2), vec![photo()], false)).is_ok());
    assert!(check(with(album(16, 99, 100), vec![photo()], false)).is_ok());
    // A short id, an index past the end, one item or too many, no photo, view once.
    assert!(check(with(album(15, 0, 2), vec![photo()], false)).is_err());
    assert!(check(with(album(16, 2, 2), vec![photo()], false)).is_err());
    assert!(check(with(album(16, 0, 1), vec![photo()], false)).is_err());
    assert!(check(with(album(16, 0, 101), vec![photo()], false)).is_err());
    assert!(check(with(album(16, 0, 2), Vec::new(), false)).is_err());
    assert!(check(with(album(16, 0, 2), vec![photo()], true)).is_err());
}
