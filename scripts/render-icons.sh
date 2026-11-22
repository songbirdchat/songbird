#!/usr/bin/env bash
# Renders Songbird's app icons (D-0179) from their SVG masters in apps/ui/assets/icon with
# macOS's own tools (Quick Look renders, Core Graphics drops the alpha channel the App Store
# refuses). Run after changing a master; the PNGs are committed.
#   scripts/render-icons.sh
#   → icon.png (the main icon, 1024, for nsh.json), <name>-1024.png (iOS's asset catalog),
#     <name>-512.png (the desktop's Dock and window icons, embedded in the app: Apple's macOS
#     shape, a rounded square on 824 of 1024 with a transparent margin, as the Dock doesn't mask),
#     icon-macos.png (the same shape at 1024, the macOS bundle's icon).
set -euo pipefail
dir=$(cd "$(dirname "$0")/../apps/ui/assets/icon" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/opaque.swift" <<'SWIFT'
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
// opaque <in.png> <out.png> <size> [mac]: redraws the image without an alpha channel, at size;
// with `mac`, as macOS's rounded square on a transparent margin instead.
let a = CommandLine.arguments
let size = Int(a[3])!
let mac = a.count > 4
guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: a[1]) as CFURL, nil),
      let img = CGImageSourceCreateImageAtIndex(src, 0, nil),
      let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                          space: CGColorSpace(name: CGColorSpace.sRGB)!,
                          bitmapInfo: mac ? CGImageAlphaInfo.premultipliedLast.rawValue
                                          : CGImageAlphaInfo.noneSkipLast.rawValue)
else { exit(1) }
ctx.interpolationQuality = .high
var rect = CGRect(x: 0, y: 0, width: size, height: size)
if mac {
  let k = CGFloat(size) / 1024
  rect = CGRect(x: 100 * k, y: 100 * k, width: 824 * k, height: 824 * k)
  ctx.addPath(CGPath(roundedRect: rect, cornerWidth: 185.4 * k, cornerHeight: 185.4 * k, transform: nil))
  ctx.clip()
}
ctx.draw(img, in: rect)
guard let out = ctx.makeImage(),
      let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: a[2]) as CFURL,
                                                 UTType.png.identifier as CFString, 1, nil)
else { exit(1) }
CGImageDestinationAddImage(dest, out, nil)
exit(CGImageDestinationFinalize(dest) ? 0 : 1)
SWIFT
xcrun swiftc -O "$tmp/opaque.swift" -o "$tmp/opaque" 2>/dev/null
for name in songbird night paper lavender; do
  qlmanage -t -s 1024 -o "$tmp" "$dir/$name.svg" > /dev/null 2>&1
  "$tmp/opaque" "$tmp/$name.svg.png" "$dir/$name-1024.png" 1024
  "$tmp/opaque" "$tmp/$name.svg.png" "$dir/$name-512.png" 512 mac
done
cp "$dir/songbird-1024.png" "$dir/icon.png"
qlmanage -t -s 1024 -o "$tmp" "$dir/songbird.svg" > /dev/null 2>&1
"$tmp/opaque" "$tmp/songbird.svg.png" "$dir/icon-macos.png" 1024 mac
ls "$dir"
