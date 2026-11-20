//! StoreKit 2 (D-0178): `swift/Store.swift`, compiled for the target into a static library and
//! linked, on macOS and iOS only.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=swift/Store.swift");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "ios" && os != "macos" {
        return;
    }
    let target = env::var("TARGET").unwrap_or_default();
    let (sdk, triple) = match target.as_str() {
        "aarch64-apple-ios" => ("iphoneos", "arm64-apple-ios16.0"),
        "aarch64-apple-ios-sim" => ("iphonesimulator", "arm64-apple-ios16.0-simulator"),
        "x86_64-apple-ios" => ("iphonesimulator", "x86_64-apple-ios16.0-simulator"),
        "aarch64-apple-darwin" => ("macosx", "arm64-apple-macos13.0"),
        "x86_64-apple-darwin" => ("macosx", "x86_64-apple-macos13.0"),
        _ => return,
    };
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());
    let lib = out.join("libsbstore.a");
    let status = Command::new("xcrun")
        .args(["--sdk", sdk, "swiftc", "-parse-as-library", "-emit-library", "-static", "-O"])
        .args(["-module-name", "SongbirdStore", "-target", triple])
        .arg("swift/Store.swift")
        .arg("-o")
        .arg(&lib)
        .status()
        .expect("xcrun swiftc (Xcode) is needed to build StoreKit for Apple targets");
    assert!(status.success(), "swiftc failed for swift/Store.swift");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=sbstore");
    println!("cargo:rustc-link-lib=framework=StoreKit");
    // The Swift runtime the system ships, and the toolchain's compatibility libraries.
    let sdk_path = String::from_utf8(
        Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output().map(|o| o.stdout).unwrap_or_default(),
    )
    .unwrap_or_default();
    println!("cargo:rustc-link-search=native={}/usr/lib/swift", sdk_path.trim());
    let toolchain = String::from_utf8(
        Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]).output().map(|o| o.stdout).unwrap_or_default(),
    )
    .unwrap_or_default();
    if let Some(bin) = PathBuf::from(toolchain.trim()).parent() {
        let platform = match sdk {
            "iphoneos" => "iphoneos",
            "iphonesimulator" => "iphonesimulator",
            _ => "macosx",
        };
        println!(
            "cargo:rustc-link-search=native={}",
            bin.join("../lib/swift").join(platform).display()
        );
    }
}
