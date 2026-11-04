#!/usr/bin/env bash
# Assemble a ready-to-use cartridge and archive it:
#
#   Cartridge/
#     <app>           Cartridge.AppImage, Cartridge.app or cartridge.exe
#     README.txt      what this folder is, first launch per system, updating
#     system/         workspace; config.txt is created on first launch, so
#                     extracting a newer release over it keeps the settings
#     games/ mods/ saves-copies/ saves-links/   (each with a README.txt)
#
# Usage: ./package.sh <app file or .app> <app name in the archive> <archive base name>
#   ./package.sh dist/cartridge-linux-x86_64.AppImage Cartridge.AppImage cartridge-linux-x86_64
#     -> dist/cartridge-linux-x86_64.tar.gz   (Linux: tar.gz keeps permissions)
#   ./package.sh dist/Cartridge.app Cartridge.app cartridge-macos
#     -> dist/cartridge-macos.zip             (macOS: ditto keeps the bundle intact)
#   ./package.sh dist/cartridge-windows-x86_64.exe cartridge.exe cartridge-windows-x86_64
#     -> dist/cartridge-windows-x86_64.zip
set -euo pipefail

APP="$1"
APP_NAME="$2"
ARCHIVE="$3"

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/Cartridge"

cp -R "$REPO_ROOT/packaging/cartridge" "$STAGE"
cp -R "$APP" "$STAGE/$APP_NAME"

mkdir -p dist
OUT_DIR="$(cd dist && pwd)"
case "$ARCHIVE" in
    *linux*)
        tar -C "$WORK" -czf "$OUT_DIR/$ARCHIVE.tar.gz" Cartridge
        echo "Done: dist/$ARCHIVE.tar.gz"
        ;;
    *macos*)
        ditto -c -k --keepParent "$STAGE" "$OUT_DIR/$ARCHIVE.zip"
        echo "Done: dist/$ARCHIVE.zip"
        ;;
    *windows*)
        # Git Bash on the Windows runner: 7-Zip is preinstalled, needs Windows paths.
        7z a -tzip "$(cygpath -w "$OUT_DIR/$ARCHIVE.zip")" "$(cygpath -w "$STAGE")" >/dev/null
        echo "Done: dist/$ARCHIVE.zip"
        ;;
    *)
        echo "Unknown platform in archive name: $ARCHIVE" >&2
        exit 1
        ;;
esac
