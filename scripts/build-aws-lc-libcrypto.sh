#!/usr/bin/env bash
# Builds AWS-LC's libcrypto for one target, for SQLCipher's crypto on Android, Windows and Linux
# (D-0137, D-0175): from the AWS-LC source `aws-lc-sys` itself ships (the version Cargo.lock
# pins), so the app holds one crypto library family and never OpenSSL. Prints the per-target
# variables `libsqlite3-sys` reads; other targets' builds are untouched.
#
#   eval "$(scripts/build-aws-lc-libcrypto.sh aarch64-linux-android)"
#
# Android needs ANDROID_NDK_HOME (or ANDROID_HOME with an NDK); all need CMake.
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: build-aws-lc-libcrypto.sh <rust target>}"
version=$(awk '/^name = "aws-lc-sys"$/ { getline; gsub(/"/, "", $3); print $3; exit }' apps/ui/Cargo.lock)
src=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/aws-lc-sys-"$version"/aws-lc 2>/dev/null | head -1)
[ -d "$src" ] || { echo "aws-lc-sys $version's source isn't in Cargo's registry: run a build first" >&2; exit 1; }
out="$PWD/target/aws-lc-libcrypto/$target-$version"
if [ ! -f "$out/lib/libcrypto.a" ]; then
  args=(-DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF -DBUILD_TOOL=OFF -DBUILD_LIBSSL=OFF
        -DDISABLE_GO=ON -DDISABLE_PERL=ON -DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON)
  case "$target" in
    aarch64-linux-android)
      ndk=${ANDROID_NDK_HOME:-$(ls -d "${ANDROID_HOME:?}"/ndk/* | sort -V | tail -1)}
      args+=(-DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake"
             -DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=26) ;;
    *) echo "no recipe for $target yet" >&2; exit 1 ;;
  esac
  build="$PWD/target/aws-lc-libcrypto/.build-$target"
  rm -rf "$build"
  cmake -S "$src" -B "$build" "${args[@]}" >&2
  cmake --build "$build" --target crypto -j "$(sysctl -n hw.ncpu 2>/dev/null || nproc)" >&2
  mkdir -p "$out/lib"
  cp "$(find "$build" -name libcrypto.a | head -1)" "$out/lib/"
  cp -R "$src/include" "$out/include"
  rm -rf "$build"
fi
var=$(printf %s "$target" | tr '[:lower:]-' '[:upper:]_')
echo "export ${var}_OPENSSL_LIB_DIR=\"$out/lib\""
echo "export ${var}_OPENSSL_INCLUDE_DIR=\"$out/include\""
