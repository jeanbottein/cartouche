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


# ── Format detection ─────────────────────────────────────────────────────

def _is_new_format(save_paths: list) -> bool:
    """Return True if save paths are already in the flat {"os", "path"} format."""
    if not save_paths:
        return False
    first = save_paths[0]
    return isinstance(first, dict) and "os" in first and "path" in first


# ── Format conversion ────────────────────────────────────────────────────

def _extract_flat_entries(entry: dict) -> list[dict]:
    """Extract flat {"os", "path"} entries from one old-format block."""
    nested_paths = entry.get("paths", [])
    if nested_paths:
        return [
            {"os": p["os"], "path": p["path"]}
            for p in nested_paths
            if isinstance(p, dict) and "os" in p and "path" in p
        ]
    if "os" in entry and "path" in entry:
        return [entry]
    return []


def _flatten_save_paths(old_format: list) -> list | None:
    """Convert old nested format to flat format. Returns migrated list or None if invalid."""
    flattened = []
    for entry in old_format:
        if not isinstance(entry, dict):
            logger.warning(f"Invalid save path entry (not dict): {entry}")
            continue
        flattened.extend(_extract_flat_entries(entry))
    return flattened if flattened else None


# ── Per-game migration ───────────────────────────────────────────────────

def migrate_game_json(game_dir: Path) -> bool:
    """
    Migrate a single game.json from old to new save paths format.
    Returns True if migration was performed, False otherwise.
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
    if not save_paths or _is_new_format(save_paths):
        return False

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


# ── Public entry point ───────────────────────────────────────────────────

def migrate_all_games(games_dir: str) -> int:
    """Scan all games and migrate save paths format. Returns count of migrated games."""
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = sum(
        1 for item in os.listdir(games_dir)
        if not item.startswith(".")
        and (game_path := Path(games_dir) / item).is_dir()
        and migrate_game_json(game_path)
    )

    if count > 0:
        logger.info(f"Migrated {count} game(s)")
    return count
