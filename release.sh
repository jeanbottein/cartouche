#!/usr/bin/env bash
# Build a self-contained release binary for the current platform.
#
# The app name comes from the binary filename, so the output is named after
# the Cargo package: dist/cartouche-<os>-<arch>. Rename it to rebrand.
#
# Usage:
#   ./release.sh                        # build for this machine
#   STEAMGRIDDB_API_KEY=xxx ./release.sh # embed a default SteamGridDB key
#   CARGO=./container.sh ./release.sh   # Linux: build in the Debian 12 container
#                                       # (old glibc: runs on SteamOS & older distros)
#
# Linux build deps: libudev-dev libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev pkg-config
# Linux AppImage: ./appimage.sh dist/cartouche-linux-<arch> (done in CI on release)
# (or build keyboard/mouse-only with: CARGO_FLAGS=--no-default-features).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
EXE=""
case "$OS" in mingw*|msys*|cygwin*) OS="windows"; EXE=".exe" ;; esac

cd "$REPO_ROOT"
${CARGO:-cargo} build --release ${CARGO_FLAGS:-}

mkdir -p dist
OUT="dist/cartouche-${OS}-${ARCH}${EXE}"
cp "target/release/cartouche${EXE}" "$OUT"
echo "Done: $OUT ($(du -sh "$OUT" | cut -f1))"
