"""
Migration from old launch_manifest.json format to .cartouche/game.json.

Runs once at startup. Converts any existing launch_manifest.json files
to the new .cartouche/ structure, then renames the old files to
launch_manifest.json.migrated.
"""

import json
import logging
import os

from .models import CARTOUCHE_DIR, GAME_JSON
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.migrator")

OLD_MANIFEST    = "launch_manifest.json"
MIGRATED_SUFFIX = ".migrated"


# ── Save-path conversion ─────────────────────────────────────────────────

def _convert_save_path(old_save_path) -> list:
    """
    Convert old savePath format to new flat savePaths format.

    Old format: a string or list of dicts/strings.
    New format: [{"os": "...", "path": "..."}, ...]
    """
    if not old_save_path:
        return []
    if isinstance(old_save_path, str):
        return [{"os": "", "path": old_save_path}] if old_save_path.strip() else []
    if isinstance(old_save_path, list):
        return _convert_save_path_list(old_save_path)
    return []


def _convert_save_path_list(entries: list) -> list:
    paths = []
    for entry in entries:
        if isinstance(entry, dict):
            path = entry.get("path") or entry.get("savePath") or entry.get("value") or ""
            paths.append({"os": entry.get("os", ""), "path": path})
        elif isinstance(entry, str):
            paths.append({"os": "", "path": entry})
    return [p for p in paths if p["path"].strip()]


# ── Single-game migration ─────────────────────────────────────────────────

def _build_new_game_json(old_manifest: dict, game_dir: str) -> dict:
    new_game = {
        "title":     old_manifest.get("title", os.path.basename(game_dir)),
        "targets":   old_manifest.get("targets", []),
        "savePaths": _convert_save_path(old_manifest.get("savePath", "")),
        "images":    {},
    }
    if "steamgriddb_id" in old_manifest:
        new_game["steamgriddb_id"] = old_manifest["steamgriddb_id"]
    if "targets" not in old_manifest and "target" in old_manifest:
        new_game["targets"] = [{
            "os":            old_manifest.get("os", ""),
            "arch":          old_manifest.get("arch", ""),
            "target":        old_manifest.get("target", ""),
            "startIn":       old_manifest.get("startIn", ""),
            "launchOptions": old_manifest.get("launchOptions", ""),
        }]
    return new_game


def _migrate_one(game_dir: str) -> bool:
    """Migrate a single game's launch_manifest.json to .cartouche/game.json. Returns True if migrated."""
    old_path = os.path.join(game_dir, OLD_MANIFEST)
    if not os.path.isfile(old_path):
        return False

    cartouche_dir = os.path.join(game_dir, CARTOUCHE_DIR)
    new_path      = os.path.join(cartouche_dir, GAME_JSON)
    migrated_path = old_path + MIGRATED_SUFFIX

    if os.path.isfile(new_path):
        if not os.path.exists(migrated_path):
            os.rename(old_path, migrated_path)
        return False

    try:
        with open(old_path, "r") as f:
            old_manifest = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        logger.error(f"Failed to read {old_path}: {e}")
        return False

    os.makedirs(cartouche_dir, exist_ok=True)
    try:
        with open(new_path, "w") as f:
            json.dump(_build_new_game_json(old_manifest, game_dir), f, indent=4)
    except OSError as e:
        logger.error(f"Failed to write {new_path}: {e}")
        return False

    try:
        os.rename(old_path, migrated_path)
    except OSError as e:
        logger.warning(f"Could not rename {old_path}: {e}")

    logger.info(f"Migrated {os.path.basename(game_dir)}: {OLD_MANIFEST} -> {CARTOUCHE_DIR}/{GAME_JSON}")
    return True


# ── Public entry point ───────────────────────────────────────────────────

def migrate(games_dir: str) -> int:
    """Scan games_dir for old launch_manifest.json files and migrate them. Returns count migrated."""
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = 0
    for item in os.listdir(games_dir):
        if item.startswith("."):
            continue
        item_path = os.path.join(games_dir, item)
        if not os.path.isdir(item_path):
            continue
        if _migrate_one(item_path):
            count += 1
        for root, dirs, files in os.walk(item_path):
            if OLD_MANIFEST in files and root != item_path:
                if _migrate_one(root):
                    count += 1
            dirs[:] = [d for d in dirs if d != CARTOUCHE_DIR]

    if count:
        logger.info(f"Migration complete: {count} game(s) migrated to {CARTOUCHE_DIR}/ format")
    return count
