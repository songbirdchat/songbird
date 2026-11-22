//! The server never decodes content (constitution, privacy model): no service, and no crate a
//! service links, may depend on `songbird-content`. Checked from the manifests, so a new
//! dependency fails here before review.
//!
//! Client-only crates (the apps' core, A2) do depend on it; none of them may be linked by a
//! service or by any other crate here, which is checked too.
#![allow(clippy::unwrap_used, reason = "tests")]

/// Crates only the apps link.
const CLIENT_ONLY: &[&str] = &["songbird-app-core"];

#[test]
fn no_service_or_server_crate_depends_on_content() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut checked = 0;
    for dir in ["services", "crates"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = e.unwrap().path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.contains("name = \"songbird-content\"") {
                continue;
            }
            let client = CLIENT_ONLY
                .iter()
                .any(|c| text.contains(&format!("name = \"{c}\"")));
            if client {
                continue;
            }
            assert!(
                !text.contains("songbird-content"),
                "{} depends on songbird-content",
                path.display()
            );
            for c in CLIENT_ONLY {
                assert!(
                    !text.contains(c),
                    "{} depends on the client-only {c}",
                    path.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 20, "found the manifests ({checked})");
}
