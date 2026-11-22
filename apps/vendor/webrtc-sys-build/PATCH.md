# webrtc-sys-build 0.3.21, patched for Songbird (D-0175)

LiveKit's `webrtc-sys-build` 0.3.21 (Apache-2.0, https://github.com/livekit/rust-sdks),
unchanged except `custom_dir()` in `src/lib.rs`: it reads `LK_CUSTOM_WEBRTC_<target>` (the Rust
target triple with `-` as `_`) before `LK_CUSTOM_WEBRTC`. NSH packages a phone app and a host
tool in one build; each must link its own pinned, checksum-verified libwebrtc
(`scripts/fetch-libwebrtc.sh`), and without this one variable named one archive for both.

Patched in by `apps/ui/.cargo/config.toml` (`[patch.crates-io]`). Drop it when upstream reads a
per-target variable.
