#!/bin/sh
# Wrap the built `cian` binary in a macOS application bundle.
#
# The binary runs perfectly well on its own — this is not a build step, it is a
# way of telling the desktop what the program is called and what it looks like.
# Two things need it and cannot be done from inside a running process:
#
#   * the name in the menu bar, which comes from CFBundleName;
#   * the icon Finder draws on the file, which comes from the bundle resources.
#
# Everything else (the Dock icon, the window) the program already sets itself.
#
# Usage:  packaging/macos/bundle.sh [output-directory]
# Output: cian.app, ready to double-click or drag to /Applications.

set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:-$root/target}
app=$out/cian.app
bin=$root/target/release/cian
icon=$root/cian.ico

[ -x "$bin" ] || {
    echo "no binary at $bin — run: cargo build --release -p cian-gui" >&2
    exit 1
}

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/cian"

# .ico is not something macOS reads; build a proper .icns from it. Every size
# comes from the one 256px source, so the small ones are downscaled rather than
# drawn — good enough for a program whose icon is a flat mark.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Every icon in the Dock sits on the same invisible grid, with the artwork
# filling about four fifths of it and transparent margin around the rest. The
# .ico fills its square edge to edge — right for Windows, and a size too large
# next to everything else on a Mac. So each size is drawn at 80% and padded.
inset_png() {  # inset_png <side> <out>
    side=$1; out=$2
    art=$(( side * 80 / 100 ))
    pad=$(( (side - art) / 2 ))
    sips -s format png -z "$art" "$art" "$icon" --out "$work/art.png" >/dev/null
    sips -p "$side" "$side" "$work/art.png" --out "$out" >/dev/null
    rm -f "$work/art.png"
    unset side art pad
}

set -- 16 32 128 256 512
for size in "$@"; do
    inset_png "$size" "$work/icon_${size}x${size}.png"
    inset_png $((size * 2)) "$work/icon_${size}x${size}@2x.png"
done
mv "$work" "$work.iconset" 2>/dev/null || cp -R "$work" "$work.iconset"
iconutil -c icns "$work.iconset" -o "$app/Contents/Resources/cian.icns"
rm -rf "$work.iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>              <string>cian</string>
    <key>CFBundleDisplayName</key>       <string>cian</string>
    <key>CFBundleExecutable</key>        <string>cian</string>
    <key>CFBundleIdentifier</key>        <string>com.taketan.cian</string>
    <key>CFBundleIconFile</key>          <string>cian</string>
    <key>CFBundlePackageType</key>       <string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key>           <string>$version</string>
    <key>LSMinimumSystemVersion</key>    <string>11.0</string>
    <!-- Without this the window is drawn at 1x and scaled up, which on a
         Retina display looks exactly like a blurry font. -->
    <key>NSHighResolutionCapable</key>   <true/>
</dict>
</plist>
PLIST

# Ad-hoc signing. Not a distribution signature — it is what stops macOS
# treating a freshly built bundle as damaged on the machine that built it.
#
# It is *not* notarisation, and cannot be: that needs an Apple Developer
# certificate and a round trip to Apple. So a bundle that reaches another Mac
# through a browser arrives quarantined, and since Sequoia the right-click →
# Open way round that is gone. `xattr -dr com.apple.quarantine cian.app`, or
# System Settings → Privacy & Security → Open Anyway. A bundle built on the
# machine that runs it is never quarantined in the first place.
# After the copy, never before: signing a binary and then replacing it leaves a
# signature that describes something else, which macOS reports as damage.
codesign --force --deep --sign - "$app" 2>/dev/null || true

# What is actually in there. A bundle built for one architecture opens on that
# architecture and nowhere else, and the message macOS gives for the other case
# — "not supported on this Mac" — does not say which Mac it *was* built for.
echo "built $app"
lipo -info "$app/Contents/MacOS/cian" 2>/dev/null || true
