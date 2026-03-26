#!/usr/bin/env bash
# Build a self-contained cartouche binary for the current platform.
#
# Usage:
#   ./release.sh                  # build for current arch
#   ./release.sh --no-venv        # skip venv creation, use active env
#
# Output: dist/cartouche-<os>-<arch>
#
# Cross-arch builds: run this script natively on each target machine
# (e.g. x86_64 and aarch64), or inside QEMU / a Docker cross-build container.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
DIST_DIR="$REPO_ROOT/dist"
BUILD_DIR="$REPO_ROOT/.build"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
BINARY_NAME="cartouche-${OS}-${ARCH}"

USE_VENV=true
for arg in "$@"; do
  [[ "$arg" == "--no-venv" ]] && USE_VENV=false
done

mkdir -p "$DIST_DIR" "$BUILD_DIR"

# ── Virtualenv ────────────────────────────────────────────────────────────────
VENV_DIR="$BUILD_DIR/venv"
if $USE_VENV; then
  if [[ ! -d "$VENV_DIR" ]]; then
    echo "Creating build virtualenv..."
    python3 -m venv "$VENV_DIR"
  fi
  # shellcheck disable=SC1091
  source "$VENV_DIR/bin/activate"
fi

# ── Dependencies ──────────────────────────────────────────────────────────────
echo "Installing build dependencies..."
pip install --quiet --upgrade pip
pip install --quiet pyinstaller
if [[ -f "$REPO_ROOT/requirements.txt" ]]; then
  pip install --quiet -r "$REPO_ROOT/requirements.txt"
fi

# ── PyInstaller ───────────────────────────────────────────────────────────────
echo "Building $BINARY_NAME..."

pyinstaller \
  --onefile \
  --name "$BINARY_NAME" \
  --distpath "$DIST_DIR" \
  --workpath "$BUILD_DIR/pyinstaller-work" \
  --specpath "$BUILD_DIR" \
  --add-data "$REPO_ROOT/config-default.txt:." \
  --add-data "$REPO_ROOT/lib/configurer.json:lib" \
  --add-data "$REPO_ROOT/lib/games_locations.json:lib" \
  --collect-all vdf \
  --clean \
  --noconfirm \
  "$REPO_ROOT/cartouche.py"

# ── Done ──────────────────────────────────────────────────────────────────────
SIZE=$(du -sh "$DIST_DIR/$BINARY_NAME" | cut -f1)
echo ""
echo "Done: dist/$BINARY_NAME  ($SIZE)"
echo ""
echo "Note: copy config-default.txt alongside the binary — on first run it"
echo "      will be used as a template to create config.txt in the same dir."
