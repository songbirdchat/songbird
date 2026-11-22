#!/usr/bin/env bash
# Gives a packaged Songbird.app its alternate app icons (D-0179): an asset catalog with the
# main icon and Night, Paper and Lavender, compiled by actool into the app's Assets.car, and
# the icon keys it reports merged into Info.plist. NSH packages the main icon only; run this
# after `nsh package --target ios(-simulator)` and before signing (build-simulator.sh does; a
# release build re-signs after it).
#   apps/ios/alternate-icons.sh <Songbird.app> [iphoneos|iphonesimulator]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
app=${1:?usage: alternate-icons.sh <Songbird.app> [iphoneos|iphonesimulator]}
platform=${2:-iphonesimulator}
icons="$here/../ui/assets/icon"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat_dir="$tmp/Assets.xcassets"
mkdir -p "$cat_dir"
echo '{"info":{"author":"songbird","version":1}}' > "$cat_dir/Contents.json"
set_for() { # <set name> <png>
  mkdir -p "$cat_dir/$1.appiconset"
  cp "$2" "$cat_dir/$1.appiconset/icon.png"
  echo '{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"songbird","version":1}}' \
    > "$cat_dir/$1.appiconset/Contents.json"
}
set_for AppIcon "$icons/songbird-1024.png"
set_for Night "$icons/night-1024.png"
set_for Paper "$icons/paper-1024.png"
set_for Lavender "$icons/lavender-1024.png"
mkdir -p "$tmp/out"
min=$(/usr/libexec/PlistBuddy -c "Print MinimumOSVersion" "$app/Info.plist" 2>/dev/null || echo 16.0)
xcrun actool --compile "$tmp/out" --platform "$platform" --minimum-deployment-target "$min" \
  --target-device iphone --target-device ipad \
  --app-icon AppIcon --include-all-app-icons \
  --output-partial-info-plist "$tmp/partial.plist" --output-format human-readable-text \
  "$cat_dir" > "$tmp/actool.log" 2>&1 || { cat "$tmp/actool.log"; exit 1; }
cp "$tmp"/out/* "$app/"
# The icon keys actool reports (CFBundleIcons with CFBundleAlternateIcons, and ~ipad) replace
# the bundle's.
python3 - "$app/Info.plist" "$tmp/partial.plist" <<'PY'
import plistlib, sys
info_path, partial_path = sys.argv[1], sys.argv[2]
with open(info_path, "rb") as f:
    info = plistlib.load(f)
with open(partial_path, "rb") as f:
    partial = plistlib.load(f)
info.update(partial)
with open(info_path, "wb") as f:
    plistlib.dump(info, f, fmt=plistlib.FMT_BINARY)
PY
