//! Refuses to build without a verified libwebrtc (D-0151): `webrtc-sys` would otherwise download
//! one unchecked. `scripts/fetch-libwebrtc.sh <platform>` checks the pinned SHA-256 and prints
//! the `LK_CUSTOM_WEBRTC` to export.
fn main() {
    println!("cargo:rerun-if-env-changed=LK_CUSTOM_WEBRTC");
    if std::env::var_os("LK_CUSTOM_WEBRTC").is_none() {
        println!(
            "cargo:warning=LK_CUSTOM_WEBRTC is not set: run `eval \"$(scripts/fetch-libwebrtc.sh <platform>)\"` first"
        );
        std::process::exit(1);
    }
}
