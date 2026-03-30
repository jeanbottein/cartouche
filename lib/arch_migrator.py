"""
Migrate target architecture from x86_64 to x64.

Old: "arch": "x86_64"
New: "arch": "x64"
"""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path

from lib.models import CARTOUCHE_DIR, GAME_JSON

logger = logging.getLogger(__name__)


def migrate_all_games(games_dir: str) -> int:
    """
    Migrate all games' target architectures from x86_64 to x64.
    Returns count of games that were migrated.
    """
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = 0
    for item in os.listdir(games_dir):
        game_path = Path(games_dir) / item
        if not game_path.is_dir() or item.startswith("."):
            continue

        if _migrate_single_game(game_path):
            count += 1

    return count


def _migrate_single_game(game_dir: Path) -> bool:
    """Migrate a single game. Return True if migrated, False if skipped or error."""
    game_json_path = game_dir / CARTOUCHE_DIR / GAME_JSON
    if not game_json_path.exists():
        return False

    try:
        with open(game_json_path, "r") as fh:
            data = json.load(fh)
    except Exception as exc:
        logger.error(f"Failed to read {game_json_path}: {exc}")
        return False

    targets = data.get("targets", [])
    if not targets:
        return False

    # Check if any target has x86_64
    has_x86_64 = any(t.get("arch") == "x86_64" for t in targets)
    if not has_x86_64:
        return False

    # Migrate x86_64 → x64
    for target in targets:
        if target.get("arch") == "x86_64":
            target["arch"] = "x64"

    # Write back
    try:
        with open(game_json_path, "w") as fh:
            json.dump(data, fh, indent=4)
        logger.info(f"Migrated arch in {game_json_path}")
        return True
    except Exception as exc:
        logger.error(f"Failed to write {game_json_path}: {exc}")
        return False
