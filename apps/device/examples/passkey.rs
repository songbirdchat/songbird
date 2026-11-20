//! Makes a passkey with the platform's own dialog and then asks for an assertion with it, to
//! check `passkeys` on a real system (D-0181): `cargo run --example passkey -- <rp id>`.
//! The relying party is a test name by default; nothing is sent anywhere.

use std::sync::mpsc;

use songbird_device::passkeys;

fn main() {
    let rp = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "songbird.test".to_owned());
    println!("available: {}", passkeys::available());
    let (tx, rx) = mpsc::channel();
    let mut challenge = [0u8; 32];
    getrandom::fill(&mut challenge).expect("randomness");
    passkeys::create(
        &rp,
        b"songbird-example-user",
        "Songbird example",
        &challenge,
        Box::new(move |r| drop(tx.send(r))),
    );
    let created = match rx.recv().expect("an answer") {
        Ok(c) => c,
        Err(f) => {
            println!("create: {f:?}");
            return;
        }
    };
    println!(
        "created: credential {} bytes, attestation {} bytes, client data {}",
        created.credential_id.len(),
        created.attestation_object.len(),
        String::from_utf8_lossy(&created.client_data_json)
    );
    let (tx, rx) = mpsc::channel();
    getrandom::fill(&mut challenge).expect("randomness");
    passkeys::assert(
        &rp,
        &challenge,
        std::slice::from_ref(&created.credential_id),
        Box::new(move |r| drop(tx.send(r))),
    );
    match rx.recv().expect("an answer") {
        Ok(a) => println!(
            "asserted: same credential {}, authenticator data {} bytes, signature {} bytes",
            a.credential_id == created.credential_id,
            a.authenticator_data.len(),
            a.signature.len()
        ),
        Err(f) => println!("assert: {f:?}"),
    }
}
