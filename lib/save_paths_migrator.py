"""
Migrate save paths from nested format to flat format.

Old format:
  "savePaths": [
    {
      "name": "saves",
      "paths": [
        {"os": "linux", "path": "~/.local/share/Game/saves"}
      ]
    }
  ]

New format:
  "savePaths": [
    {"os": "linux", "path": "~/.local/share/Game/saves"}
  ]
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path

from lib.models import CARTOUCHE_DIR, GAME_JSON

logger = logging.getLogger(__name__)


def migrate_game_json(game_dir: Path) -> bool:
    """
    Migrate a single game.json from old to new save paths format.
    Returns True if migration was performed, False if no migration needed or error.
    """
    game_json_path = game_dir / CARTOUCHE_DIR / GAME_JSON
    if not game_json_path.exists():
        return False

    try:
        with open(game_json_path, "r") as fh:
            data = json.load(fh)
    except Exception as exc:
        logger.error(f"Failed to read {game_json_path}: {exc}")
        return False

    save_paths = data.get("savePaths", [])
    if not save_paths:
        return False

    # Check if already in new format (list of dicts with "os" and "path")
    if _is_new_format(save_paths):
        return False

    # Migrate old format to new
    migrated = _flatten_save_paths(save_paths)
    if migrated is None:
        return False

    data["savePaths"] = migrated

    try:
        with open(game_json_path, "w") as fh:
            json.dump(data, fh, indent=4)
        logger.info(f"Migrated savePaths in {game_json_path}")
        return True
    except Exception as exc:
        logger.error(f"Failed to write {game_json_path}: {exc}")
        return False


def migrate_all_games(games_dir: str) -> int:
    """
    Scan all games and migrate save paths format. Returns count of migrated games.
    """
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = 0
    for item in os.listdir(games_dir):
        game_path = Path(games_dir) / item
        if not game_path.is_dir() or item.startswith("."):
            continue
        if migrate_game_json(game_path):
            count += 1

    if count > 0:
        logger.info(f"Migrated {count} game(s)")
    return count


# -- Helpers ------------------------------------------------------------------

def _is_new_format(save_paths: list) -> bool:
    """Check if save paths are already in new format."""
    if not save_paths:
        return False
    first = save_paths[0]
    if not isinstance(first, dict):
        return False
    # New format: each entry has "os" and "path"
    return "os" in first and "path" in first


def _flatten_save_paths(old_format: list) -> list | None:
    """
    Convert old nested format to new flat format.
    Returns migrated list, or None if format is invalid.
    """
    flattened = []

    for entry in old_format:
        if not isinstance(entry, dict):
            logger.warning(f"Invalid save path entry (not dict): {entry}")
            continue

        # Old format has "name" and "paths"
        # But we only care about the paths
        paths = entry.get("paths", [])
        if not paths:
            # Maybe it's already flat? Check for "os" and "path"
            if "os" in entry and "path" in entry:
                flattened.append(entry)
            continue

        # Flatten each path in the nested "paths" array
        for path_entry in paths:
            if isinstance(path_entry, dict) and "os" in path_entry and "path" in path_entry:
                flattened.append({
                    "os": path_entry["os"],
                    "path": path_entry["path"],
                })

    return flattened if flattened else None
