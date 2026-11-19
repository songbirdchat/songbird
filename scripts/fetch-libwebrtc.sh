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
  unzip -q "$dir/$zip" -d "$tmp"
  mv "$tmp/$triple" "$out"
  rm -rf "$tmp"
fi
echo "export LK_CUSTOM_WEBRTC=\"$PWD/$out\""
