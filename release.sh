#!/usr/bin/env bash
# Build a self-contained binary for the current platform.
#
# The binary name is derived from the main Python entry-point script.
# Rename cartouche.py → potato.py and the output binary will be "potato".
#
# Usage:
#   ./release.sh                  # build for current arch
#   ./release.sh --no-venv        # skip venv creation, use active env
#
# Output: dist/<os>-<arch>/<appname>
#
# Cross-arch builds: run this script natively on each target machine
# (e.g. x86_64 and aarch64), or inside QEMU / a Docker cross-build container.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
DIST_DIR="$REPO_ROOT/dist"
BUILD_DIR="$REPO_ROOT/.build"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

# Derive app name from the entry-point script (rename the .py file to rebrand)
ENTRY_SCRIPT="$(ls "$REPO_ROOT"/*.py 2>/dev/null | head -1)"
APP_NAME="$(basename "$ENTRY_SCRIPT" .py)"

OUTPUT_DIR="$DIST_DIR"

USE_VENV=true
for arg in "$@"; do
  [[ "$arg" == "--no-venv" ]] && USE_VENV=false
done

mkdir -p "$OUTPUT_DIR" "$BUILD_DIR"

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

# ── API key injection (if available) ──────────────────────────────────────────
if [[ -n "${STEAMGRIDDB_API_KEY:-}" ]]; then
  echo "Injecting SteamGridDB API key..."
  python3 "$REPO_ROOT/scripts/inject_api_key.py"
fi

# ── PyInstaller ───────────────────────────────────────────────────────────────
echo "Building ${APP_NAME}-${OS}-${ARCH}..."

pyinstaller \
  --onefile \
  --name "$APP_NAME-${OS}-${ARCH}" \
  --distpath "$OUTPUT_DIR" \
  --workpath "$BUILD_DIR/pyinstaller-work" \
  --specpath "$BUILD_DIR" \
  --add-data "$REPO_ROOT/lib/config-default.txt:lib" \
  --add-data "$REPO_ROOT/lib/configurer.json:lib" \
  --add-data "$REPO_ROOT/lib/games_locations.json:lib" \
  --add-data "$REPO_ROOT/lib/app-icon.png:lib" \
  --icon "$REPO_ROOT/lib/app-icon.png" \
  --collect-all vdf \
  --clean \
  --noconfirm \
  "$ENTRY_SCRIPT"

# ── Done ──────────────────────────────────────────────────────────────────────
SIZE=$(du -sh "$OUTPUT_DIR/$APP_NAME-${OS}-${ARCH}" | cut -f1)
echo ""
echo "Done: dist/${APP_NAME}-${OS}-${ARCH}  ($SIZE)"
