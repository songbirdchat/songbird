//! The compatibility suite (PROTOCOL §5.12): this reader decodes every published version's
//! vectors (`vectors/v*/`), and a newer writer's unknown fields, kinds and enum values are ignored
//! rather than refused.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::manual_assert,
    clippy::const_is_empty,
    reason = "tests"
)]

use prost::Message as _;
use songbird_content::schema::{Content, content::Body, data_message::Kind};

#[test]
fn this_reader_decodes_every_versions_vectors() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors");
    let mut versions: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with('v'))
        .map(|e| e.path())
        .collect();
    versions.sort();
    assert_ne!(versions.len(), 0);
    let mut checked = 0;
    for dir in versions {
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("vectors.json")).unwrap())
                .unwrap();
        let version = doc["version"].as_u64().unwrap();
        assert!(version <= u64::from(songbird_content::VERSION), "{dir:?}");
        for v in doc["vectors"].as_array().unwrap() {
            let name = v["name"].as_str().unwrap();
            let bytes = hex::decode(v["hex"].as_str().unwrap()).unwrap();
            match v["type"].as_str().unwrap() {
                "Content" => {
                    let c = songbird_content::decode_content(&bytes)
                        .unwrap_or_else(|e| panic!("v{version} {name}: {e}"));
                    assert!(c.version >= 1);
                }
                "PaddedPlaintext" => {
                    songbird_content::open(&bytes)
                        .unwrap_or_else(|e| panic!("v{version} {name}: {e}"));
                }
                other => panic!("unknown vector type {other}"),
            }
            checked += 1;
        }
    }
    assert!(checked >= 32);
}

/// A simulated version 2: new fields at three depths, a new data kind and a new content body.
mod v2 {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub(crate) struct Text {
        #[prost(string, tag = "1")]
        pub(crate) body: String,
        #[prost(string, tag = "3")]
        pub(crate) language: String,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub(crate) struct Message {
        #[prost(message, optional, tag = "1")]
        pub(crate) text: Option<Text>,
        // A tag this version doesn't use (12 is `album`, D-0186).
        #[prost(bool, tag = "40")]
        pub(crate) reply_privately: bool,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub(crate) struct DataMessage {
        #[prost(message, optional, tag = "10")]
        pub(crate) message: Option<Message>,
        #[prost(bytes = "vec", tag = "23")]
        pub(crate) new_kind: Vec<u8>,
    }

    #[derive(Clone, PartialEq, ::prost::Message)]
    pub(crate) struct Content {
        #[prost(uint32, tag = "1")]
        pub(crate) version: u32,
        #[prost(uint32, tag = "2")]
        pub(crate) required_version: u32,
        #[prost(bytes = "vec", tag = "3")]
        pub(crate) message_id: Vec<u8>,
        #[prost(message, optional, tag = "10")]
        pub(crate) data: Option<DataMessage>,
        #[prost(bytes = "vec", tag = "31")]
        pub(crate) new_body: Vec<u8>,
        #[prost(string, tag = "99")]
        pub(crate) new_top: String,
    }
}

#[test]
fn a_newer_writers_unknown_fields_are_ignored() {
    let newer = v2::Content {
        version: 2,
        required_version: 1,
        message_id: vec![1; 16],
        data: Some(v2::DataMessage {
            message: Some(v2::Message {
                text: Some(v2::Text {
                    body: "hello".into(),
                    language: "ro".into(),
                }),
                reply_privately: true,
            }),
            new_kind: Vec::new(),
        }),
        new_body: Vec::new(),
        new_top: "future".into(),
    };
    let c = songbird_content::decode_content(&newer.encode_to_vec()).unwrap();
    let Some(Body::Data(d)) = c.body else {
        panic!("data body")
    };
    let Some(Kind::Message(m)) = d.kind else {
        panic!("message kind")
    };
    assert_eq!(m.text.unwrap().body, "hello");
}

#[test]
fn a_newer_writers_new_kind_asks_this_reader_to_update() {
    // A body this version does not know, marked as needing version 2.
    let newer = v2::Content {
        version: 2,
        required_version: 2,
        message_id: vec![1; 16],
        data: None,
        new_body: b"something new".to_vec(),
        new_top: String::new(),
    };
    // Framed by hand: this reader cannot build the newer content itself.
    let plain = songbird_content::Plaintext {
        content: newer.encode_to_vec(),
        franking_key: Vec::new(),
        profile_key: Vec::new(),
    };
    let padded = songbird_content::pad(&plain.encode_to_vec());
    let o = songbird_content::open(&padded).unwrap();
    assert!(o.content.body.is_none());
    assert!(o.needs_update());

    // A new data kind without a required version: decoded as a data message with no kind.
    let newer = v2::Content {
        version: 2,
        required_version: 0,
        message_id: vec![1; 16],
        data: Some(v2::DataMessage {
            message: None,
            new_kind: vec![1, 2, 3],
        }),
        new_body: Vec::new(),
        new_top: String::new(),
    };
    let c: Content = songbird_content::decode_content(&newer.encode_to_vec()).unwrap();
    assert!(matches!(c.body, Some(Body::Data(ref d)) if d.kind.is_none()));
}

#[test]
fn an_unknown_enum_value_is_kept_not_refused() {
    use songbird_content::schema::ReceiptMessage;
    // A receipt kind from a newer writer.
    let r = Content {
        version: 1,
        message_id: vec![1; 16],
        body: Some(Body::Receipt(ReceiptMessage {
            kind: 9,
            up_to: vec![2; 16],
            ..ReceiptMessage::default()
        })),
        ..Content::default()
    };
    assert!(songbird_content::decode_content(&r.encode_to_vec()).is_ok());
}
