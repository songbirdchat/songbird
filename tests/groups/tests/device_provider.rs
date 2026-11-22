//! Songbird's device MLS provider (aws-lc-rs, D-0145) against `OpenMLS`'s `RustCrypto` provider
//! (libcrux X-Wing, `hpke-rs`): the same keys from the same path secrets, and HPKE ciphertexts
//! and exports each side opens from the other.
#![allow(clippy::unwrap_used, reason = "tests")]

use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider as _;
use openmls_traits::crypto::OpenMlsCrypto;
use openmls_traits::types::{HpkeAeadType, HpkeConfig, HpkeKdfType, HpkeKemType};
use songbird_app_core::mls::crypto::DeviceCrypto;

const CFG: HpkeConfig = HpkeConfig(
    HpkeKemType::XWingKemDraft6,
    HpkeKdfType::HkdfSha256,
    HpkeAeadType::AesGcm128,
);

#[test]
fn both_providers_derive_the_same_x_wing_keys() {
    let theirs = OpenMlsRustCrypto::default();
    for ikm in [&b"path secret"[..], &[0u8; 32], &[0xff; 64]] {
        let a = DeviceCrypto.derive_hpke_keypair(CFG, ikm).unwrap();
        let b = theirs.crypto().derive_hpke_keypair(CFG, ikm).unwrap();
        assert_eq!(a.public, b.public, "public keys");
        assert_eq!(&a.private[..], &b.private[..], "private seeds");
    }
}

#[test]
fn ciphertexts_and_exports_cross_both_ways() {
    let theirs = OpenMlsRustCrypto::default();
    let kp = DeviceCrypto.derive_hpke_keypair(CFG, b"node").unwrap();
    // Ours seals, theirs opens.
    let ct = DeviceCrypto
        .hpke_seal(CFG, &kp.public, b"info", b"aad", b"welcome")
        .unwrap();
    assert_eq!(
        theirs
            .crypto()
            .hpke_open(CFG, &ct, &kp.private, b"info", b"aad")
            .unwrap(),
        b"welcome"
    );
    // Theirs seals, ours opens.
    let ct = theirs
        .crypto()
        .hpke_seal(CFG, &kp.public, b"info", b"aad", b"commit")
        .unwrap();
    assert_eq!(
        DeviceCrypto
            .hpke_open(CFG, &ct, &kp.private, b"info", b"aad")
            .unwrap(),
        b"commit"
    );
    // Exporters agree.
    let (enc, a) = DeviceCrypto
        .hpke_setup_sender_and_export(CFG, &kp.public, b"i", b"ctx", 32)
        .unwrap();
    let b = theirs
        .crypto()
        .hpke_setup_receiver_and_export(CFG, &enc, &kp.private, b"i", b"ctx", 32)
        .unwrap();
    assert_eq!(&*a, &*b);
}

/// A whole group across both providers: a Songbird device (aws-lc-rs) creates it and adds a
/// member on `OpenMLS`'s `RustCrypto` provider; both read each other's messages; the group
/// survives the Songbird device's state going through its store.
#[test]
fn a_group_works_across_both_providers() {
    use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
    use openmls::prelude::*;
    use openmls_basic_credential::SignatureKeyPair;
    use songbird_app_core::mls::DeviceProvider;
    use songbird_app_core::mls::crypto::{DeviceSigner, GROUP_CIPHERSUITE};

    let suite = GROUP_CIPHERSUITE;
    // Alice: Songbird's provider.
    let alice = DeviceProvider::default();
    let alice_sig = DeviceSigner::generate().unwrap();
    let alice_cred = CredentialWithKey {
        credential: BasicCredential::new(b"alice".to_vec()).into(),
        signature_key: alice_sig.public().into(),
    };
    // Bob: RustCrypto.
    let bob = OpenMlsRustCrypto::default();
    let bob_sig = SignatureKeyPair::new(suite.signature_algorithm()).unwrap();
    bob_sig.store(bob.storage()).unwrap();
    let bob_cred = CredentialWithKey {
        credential: BasicCredential::new(b"bob".to_vec()).into(),
        signature_key: bob_sig.public().into(),
    };
    let bob_kp = KeyPackage::builder()
        .build(suite, &bob, &bob_sig, bob_cred)
        .unwrap();

    let config = MlsGroupCreateConfig::builder()
        .ciphersuite(suite)
        .use_ratchet_tree_extension(true)
        .build();
    let mut group = MlsGroup::new(&alice, &alice_sig, &config, alice_cred).unwrap();
    let (_commit, welcome, _) = group
        .add_members(&alice, &alice_sig, &[bob_kp.key_package().clone()])
        .unwrap();
    group.merge_pending_commit(&alice).unwrap();

    // The Welcome crosses the wire as bytes.
    let welcome =
        MlsMessageIn::tls_deserialize_exact(welcome.tls_serialize_detached().unwrap()).unwrap();
    let MlsMessageBodyIn::Welcome(welcome) = welcome.extract() else {
        panic!("a welcome")
    };
    let mut bob_group =
        StagedWelcome::new_from_welcome(&bob, &MlsGroupJoinConfig::default(), welcome, None)
            .unwrap()
            .into_group(&bob)
            .unwrap();

    // Alice → Bob.
    let out = group
        .create_message(&alice, &alice_sig, b"hello from aws-lc-rs")
        .unwrap();
    let inp = MlsMessageIn::tls_deserialize_exact(out.tls_serialize_detached().unwrap()).unwrap();
    let pm = bob_group
        .process_message(&bob, inp.try_into_protocol_message().unwrap())
        .unwrap();
    let ProcessedMessageContent::ApplicationMessage(m) = pm.into_content() else {
        panic!("app")
    };
    assert_eq!(m.into_bytes(), b"hello from aws-lc-rs");

    // Alice's state through the store, then Bob → Alice.
    let saved = alice.snapshot().unwrap();
    let alice = DeviceProvider::restore(&saved).unwrap();
    let mut group = MlsGroup::load(alice.storage(), group.group_id())
        .unwrap()
        .unwrap();
    let out = bob_group
        .create_message(&bob, &bob_sig, b"hello from RustCrypto")
        .unwrap();
    let inp = MlsMessageIn::tls_deserialize_exact(out.tls_serialize_detached().unwrap()).unwrap();
    let pm = group
        .process_message(&alice, inp.try_into_protocol_message().unwrap())
        .unwrap();
    let ProcessedMessageContent::ApplicationMessage(m) = pm.into_content() else {
        panic!("app")
    };
    assert_eq!(m.into_bytes(), b"hello from RustCrypto");
}
