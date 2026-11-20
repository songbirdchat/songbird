#!/usr/bin/env bash
# Builds Songbird for Android inside its own Gradle host (D-0113): `nsh package --target android`
# builds the Rust app (libnsh.so with the app and the camera's JNI side) and its assets; Gradle
# compiles NSH's Java sources (the exact nsh-platform-android the app links), Songbird's
# activity and camera (CameraX), and packs them into one APK.
#   apps/android/build.sh [debug|release]   →   apps/android/build/outputs/apk/<variant>/
# Needs ANDROID_HOME (the SDK with the NDK), a JDK 17–21, and Gradle.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
variant=${1:-debug}
ui="$here/../ui"
rust="$here/build/rust"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"

# 1. The Rust app, as nsh packages it.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# arm64 only: LiveKit publishes libwebrtc for arm64-v8a alone (D-0175).
(cd "$ui" && nsh package --target android --abi arm64-v8a -o "$tmp")
apk=$(ls "$tmp"/*.apk)
rm -rf "$rust" && mkdir -p "$rust/jniLibs" "$rust/assets"
(cd "$tmp" && unzip -q "$apk" 'lib/*' 'assets/*')
cp -R "$tmp/lib/." "$rust/jniLibs/"
cp -R "$tmp/assets/." "$rust/assets/"

# 2. NSH's Java sources, from the crate the generated Android build resolved (never a copy).
nsh_java=$(cargo metadata --format-version 1 --manifest-path "$ui/.nsh/rust/rust-android/Cargo.toml" --filter-platform aarch64-linux-android \
  | python3 -c 'import json,sys,os; m=json.load(sys.stdin); print(next(os.path.join(os.path.dirname(p["manifest_path"]),"java") for p in m["packages"] if p["name"]=="nsh-platform-android"))')

# 3. The APK.
task="assemble$(printf %s "${variant:0:1}" | tr "[:lower:]" "[:upper:]")${variant:1}"
(cd "$here" && gradle --quiet "$task" -PnshJava="$nsh_java" ${GRADLE_FLAGS:-})
ls "$here/build/outputs/apk/$variant/"
