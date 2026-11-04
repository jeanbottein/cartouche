#!/usr/bin/env bash
# Wrap a Linux release binary into an AppImage: one file with an icon and a
# desktop entry. Portable mode still works: the app keeps its .cartouche/
# workspace next to the .AppImage file (it reads $APPIMAGE, see core/app.rs).
#
# Usage:
#   ./appimage.sh dist/cartouche-linux-x86_64     # -> dist/cartouche-linux-x86_64.AppImage
#
# Needs curl. Nothing is bundled besides the binary: GL, X11, ALSA and udev
# come from the system, like for the plain binary.
set -euo pipefail

BIN="$1"
ARCH="$(uname -m)"
# Pinned: a release must not change because an upstream "continuous" build did.
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-${ARCH}.AppImage"
# Static runtime: works without libfuse2 (SteamOS, Bazzite).
RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-${ARCH}"

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APPDIR="$WORK/Cartouche.AppDir"

mkdir -p "$APPDIR/usr/bin"
# The inner name must stay "cartouche": the app name (and its .cartouche/
# folder, Steam tags) comes from the binary's filename.
cp "$BIN" "$APPDIR/usr/bin/cartouche"
chmod +x "$APPDIR/usr/bin/cartouche"
ln -s usr/bin/cartouche "$APPDIR/AppRun"
cp "$REPO_ROOT/assets/app-icon.png" "$APPDIR/cartouche.png"
cat > "$APPDIR/cartouche.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Cartouche
Comment=DRM-free game library for Linux and the Steam Deck
Exec=cartouche
Icon=cartouche
Categories=Game;
Terminal=false
EOF

curl -fsSL -o "$WORK/appimagetool" "$TOOL_URL"
curl -fsSL -o "$WORK/runtime" "$RUNTIME_URL"
chmod +x "$WORK/appimagetool"

OUT="${BIN}.AppImage"
# Extract-and-run: no FUSE needed to run the tool itself (CI, containers).
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$WORK/appimagetool" \
    --no-appstream --runtime-file "$WORK/runtime" "$APPDIR" "$OUT"
echo "Done: $OUT ($(du -sh "$OUT" | cut -f1))"
