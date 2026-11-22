//! The server runs OpenMLS's core only, behind Songbird's verify-only aws-lc-rs provider
//! (D-0065 §1, D-0066). OpenMLS's own crypto providers (`RustCrypto`, libcrux) and the client
//! harness built on them are for tests and apps: no service, and no crate a service links, may
//! depend on them. Checked from the manifests, so a new dependency fails here before review.
//! The app's client core (`crates/app-core`, D-0145) keeps OpenMLS's state on the device and is
//! exempt by name; in exchange, no service or other crate may depend on it.
#![allow(clippy::unwrap_used, clippy::panic, reason = "tests")]

const FORBIDDEN: &[&str] = &[
    "openmls_rust_crypto",
    "openmls_libcrux_crypto",
    "openmls_basic_credential",
    "openmls_memory_storage",
    "openmls_sqlite_storage",
    "songbird-groups-harness",
];

/// Client crates: they run on devices, never in a service.
const CLIENT: &[&str] = &["app-core"];

#[test]
fn no_service_or_server_crate_links_client_side_mls_crypto() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut checked = 0;
    for dir in ["services", "crates"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap() {
            let dir_path = e.unwrap().path();
            let path = dir_path.join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let name = dir_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if dir == "crates" && CLIENT.contains(&name) {
                continue;
            }
            assert!(
                !text.contains("songbird-app-core"),
                "{} depends on the app's client core",
                path.display()
            );
            for name in FORBIDDEN {
                assert!(!text.contains(name), "{} depends on {name}", path.display());
            }
            checked += 1;
        }
    }
    assert!(checked >= 20, "found the manifests ({checked})");
}

#[test]
fn openmls_has_no_default_features_here() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .unwrap();
    let line = text.lines().find(|l| l.starts_with("openmls =")).unwrap();
    assert!(line.contains("default-features = false"), "{line}");
}
