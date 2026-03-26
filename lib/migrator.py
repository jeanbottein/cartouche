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

logger = logging.getLogger("cartouche.migrator")

OLD_MANIFEST = "launch_manifest.json"
MIGRATED_SUFFIX = ".migrated"


def _convert_save_path(old_save_path) -> list:
    """
    Convert old savePath format to new savePaths format.

    Old format was either:
      - a string: "~/.local/share/Game/Saves"
      - a list: [{"os": "linux", "path": "..."}, ...]

    New format is:
      [{"name": "saves", "paths": [{"os": "linux", "path": "..."}]}]
    """
    if not old_save_path:
        return []

    if isinstance(old_save_path, str):
        if not old_save_path.strip():
            return []
        return [{"name": "saves", "paths": [{"os": "", "path": old_save_path}]}]

    if isinstance(old_save_path, list):
        # Already a list of {os, path} dicts
        paths = []
        for entry in old_save_path:
            if isinstance(entry, dict):
                paths.append({
                    "os": entry.get("os", ""),
                    "path": entry.get("path") or entry.get("savePath") or entry.get("value") or "",
                })
            elif isinstance(entry, str):
                paths.append({"os": "", "path": entry})
        # Filter out empty paths
        paths = [p for p in paths if p["path"].strip()]
        if not paths:
            return []
        return [{"name": "saves", "paths": paths}]

    return []


def _migrate_one(game_dir: str) -> bool:
    """
    Migrate a single game's launch_manifest.json to .cartouche/game.json.
    Returns True if migration happened.
    """
    old_path = os.path.join(game_dir, OLD_MANIFEST)
    if not os.path.isfile(old_path):
        return False

    cartouche_dir = os.path.join(game_dir, CARTOUCHE_DIR)
    new_path = os.path.join(cartouche_dir, GAME_JSON)

    # Don't migrate if new format already exists
    if os.path.isfile(new_path):
        # Rename old file anyway
        migrated_path = old_path + MIGRATED_SUFFIX
        if not os.path.exists(migrated_path):
            os.rename(old_path, migrated_path)
        return False

    try:
        with open(old_path, "r") as f:
            old_manifest = json.load(f)
    except Exception as e:
        logger.error(f"Failed to read {old_path}: {e}")
        return False

    # Build new game.json
    title = old_manifest.get("title", os.path.basename(game_dir))
    new_game = {
        "title": title,
        "original_title": title,
        "targets": old_manifest.get("targets", []),
        "savePaths": _convert_save_path(old_manifest.get("savePath", "")),
        "images": {},
    }

    # Preserve steamgriddb_id if present
    if "steamgriddb_id" in old_manifest:
        new_game["steamgriddb_id"] = old_manifest["steamgriddb_id"]

    # Handle old flat format (no targets array)
    if "targets" not in old_manifest and "target" in old_manifest:
        new_game["targets"] = [{
            "os": old_manifest.get("os", ""),
            "arch": old_manifest.get("arch", ""),
            "target": old_manifest.get("target", ""),
            "startIn": old_manifest.get("startIn", ""),
            "launchOptions": old_manifest.get("launchOptions", ""),
        }]

    # Write new format
    os.makedirs(cartouche_dir, exist_ok=True)
    try:
        with open(new_path, "w") as f:
            json.dump(new_game, f, indent=4)
    except Exception as e:
        logger.error(f"Failed to write {new_path}: {e}")
        return False

    # Rename old file
    migrated_path = old_path + MIGRATED_SUFFIX
    try:
        os.rename(old_path, migrated_path)
    except OSError as e:
        logger.warning(f"Could not rename {old_path}: {e}")

    logger.info(f"Migrated {os.path.basename(game_dir)}: {OLD_MANIFEST} -> {CARTOUCHE_DIR}/{GAME_JSON}")
    return True


def migrate(games_dir: str) -> int:
    """
    Scan games_dir for old launch_manifest.json files and migrate them.
    Returns the number of games migrated.
    """
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = 0
    for item in os.listdir(games_dir):
        item_path = os.path.join(games_dir, item)
        if not os.path.isdir(item_path):
            continue
        if item.startswith("."):
            continue

        # Check in the game folder itself
        if _migrate_one(item_path):
            count += 1

        # Also check nested (get_real_first_path pattern)
        # Walk one level for any launch_manifest.json deeper
        for root, dirs, files in os.walk(item_path):
            if OLD_MANIFEST in files and root != item_path:
                if _migrate_one(root):
                    count += 1
            # Don't descend into .cartouche dirs
            dirs[:] = [d for d in dirs if d != CARTOUCHE_DIR]

    if count:
        logger.info(f"Migration complete: {count} game(s) migrated to {CARTOUCHE_DIR}/ format")

    return count
