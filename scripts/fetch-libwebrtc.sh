#!/usr/bin/env bash
# Fetches LiveKit's prebuilt libwebrtc for one platform, checks it against the pinned SHA-256
# (supply-chain/libwebrtc.sha256, D-0151) and unpacks it; prints the directory to give the build
# as LK_CUSTOM_WEBRTC, so `webrtc-sys` never downloads anything unverified itself.
#
#   eval "$(scripts/fetch-libwebrtc.sh mac-arm64-release)"   # exports LK_CUSTOM_WEBRTC
set -euo pipefail
cd "$(dirname "$0")/.."
triple="${1:?usage: fetch-libwebrtc.sh <mac-arm64-release|ios-device-arm64-release|...>}"
tag="webrtc-a545de4"
zip="webrtc-${triple}.zip"
want=$(awk -v z="$zip" '$2 == z { print $1 }' supply-chain/libwebrtc.sha256)
[ -n "$want" ] || { echo "no pinned hash for $zip" >&2; exit 1; }
dir="target/libwebrtc"
mkdir -p "$dir"
if [ ! -f "$dir/$zip" ]; then
  curl -fsSL -o "$dir/$zip.part" "https://github.com/livekit/rust-sdks/releases/download/${tag}/${zip}"
  mv "$dir/$zip.part" "$dir/$zip"
fi
got=$(shasum -a 256 "$dir/$zip" | awk '{ print $1 }')
if [ "$got" != "$want" ]; then
  echo "SHA-256 mismatch for $zip: got $got, want $want" >&2
  rm -f "$dir/$zip"
  exit 1
fi
out="$dir/$triple"
if [ ! -d "$out" ]; then
  tmp="$dir/.extract-$triple"
  rm -rf "$tmp"
  mkdir -p "$tmp"
  # -o: the Android archive has headers that differ only in case (Linux sysroot), which
  # collide on a case-insensitive disk; none is used to build Songbird.
  unzip -o -q "$dir/$zip" -d "$tmp"
  mv "$tmp/$triple" "$out"
  rm -rf "$tmp"
fi
case "$triple" in
  mac-arm64-*) rust=aarch64_apple_darwin ;;
  ios-device-arm64-*) rust=aarch64_apple_ios ;;
  ios-simulator-arm64-*) rust=aarch64_apple_ios_sim ;;
  android-arm64-*) rust=aarch64_linux_android ;;
  linux-x64-*) rust=x86_64_unknown_linux_gnu ;;
  win-x64-*) rust=x86_64_pc_windows_msvc ;;
  *) rust= ;;
esac
# Per target first (apps/vendor/webrtc-sys-build, D-0175): a build of several targets (a phone
# app and NSH's host tool) links each its own. The plain variable stays for one-target builds.
[ -n "$rust" ] && echo "export LK_CUSTOM_WEBRTC_${rust}=\"$PWD/$out\""
echo "export LK_CUSTOM_WEBRTC=\"$PWD/$out\""
