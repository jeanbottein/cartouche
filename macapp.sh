#!/usr/bin/env bash
# Wrap macOS binaries into Cartridge.app (macOS only: lipo, sips, iconutil,
# codesign). Several binaries (x86_64 + arm64) make a universal app.
# Portable mode still works: the app keeps its system/ workspace next to
# Cartridge.app (see core/app.rs).
#
# Usage:
#   ./macapp.sh target/release/cartridge                    # -> dist/Cartridge.app
#   ./macapp.sh target/x86_64-apple-darwin/release/cartridge \
#               target/aarch64-apple-darwin/release/cartridge   # universal
#   OUT_DIR=somewhere ./macapp.sh ...
#
# Ad-hoc signed only (no paid Apple account): a downloaded copy must be moved
# with Finder or un-quarantined before its first launch, see README.txt.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="${OUT_DIR:-dist}"
APP="$OUT_DIR/Cartridge.app"
VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | cut -d'"' -f2)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create "$@" -output "$APP/Contents/MacOS/cartridge"

ICONSET="$WORK/cartridge.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$REPO_ROOT/assets/app-icon.png" \
        --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$REPO_ROOT/assets/app-icon.png" \
        --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/cartridge.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>•cartridge</string>
    <key>CFBundleDisplayName</key><string>•cartridge</string>
    <key>CFBundleIdentifier</key><string>io.github.jeanbottein.cartridge</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>cartridge</string>
    <key>CFBundleIconFile</key><string>cartridge</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Apple Silicon refuses unsigned code: an ad-hoc signature is enough to run.
codesign --force --deep --sign - "$APP"
echo "Done: $APP ($(lipo -archs "$APP/Contents/MacOS/cartridge"))"
