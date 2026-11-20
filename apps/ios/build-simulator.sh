#!/usr/bin/env bash
# Builds Songbird for the iOS simulator with its Notification Service Extension (Prompt 8,
# D-0099) and its screen-sharing broadcast extension (D-0180), without an Apple account:
# `nsh package --target ios-simulator`, then the extensions compiled for the simulator, placed
# in the app's PlugIns, all signed ad hoc.
#   apps/ios/build-simulator.sh [out-dir]   →   <out-dir>/Songbird.app
# Real APNs tokens and PushKit need the registered bundle IDs and a provisioning profile (D-0094 §2).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/build}
mkdir -p "$out"
(cd "$here/../ui" && SONGBIRD_PREVIEW_BUILD=1 nsh package --target ios-simulator -o "$out")
app="$out/Songbird.app"
# The alternate app icons (D-0179), before signing.
"$here/alternate-icons.sh" "$app" iphonesimulator
appex="$app/PlugIns/SongbirdNotify.appex"
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path)
mkdir -p "$appex"
xcrun --sdk iphonesimulator swiftc \
  -target arm64-apple-ios17.0-simulator -sdk "$sdk" \
  -module-name SongbirdNotify -parse-as-library -application-extension -O \
  -Xlinker -e -Xlinker _NSExtensionMain -Xlinker -rpath -Xlinker @executable_path/../../Frameworks \
  "$here/NotifyRules/Sources/NotifyRules/Rules.swift" \
  "$here/NotificationService/NotificationService.swift" \
  -o "$appex/SongbirdNotify"
cp "$here/NotificationService/Info.plist" "$appex/Info.plist"
codesign --force --sign - --timestamp=none "$appex"
# The screen-sharing extension (D-0180), the same way.
screen="$app/PlugIns/SongbirdScreen.appex"
mkdir -p "$screen"
xcrun --sdk iphonesimulator swiftc \
  -target arm64-apple-ios17.0-simulator -sdk "$sdk" \
  -module-name SongbirdScreen -parse-as-library -application-extension -O \
  -Xlinker -e -Xlinker _NSExtensionMain -Xlinker -rpath -Xlinker @executable_path/../../Frameworks \
  -framework ReplayKit -framework Accelerate \
  "$here/ScreenShare/SampleHandler.swift" \
  -o "$screen/SongbirdScreen"
cp "$here/ScreenShare/Info.plist" "$screen/Info.plist"
codesign --force --sign - --timestamp=none "$screen"
codesign --force --sign - --timestamp=none --preserve-metadata=entitlements "$app"
echo "$app"
