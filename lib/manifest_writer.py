"""
Step 7: Write manifests.json for ROM manager compatibility.

Generates the aggregated manifests.json file from the in-memory
GameDatabase, using the same format as the old manifester module.
"""

import json
import logging
import os

from .models import GameDatabase
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.manifest_writer")


def write(db: GameDatabase, output_path: str):
    """
    Write manifests.json from the GameDatabase.

    Output format matches the old manifests.json for backward compatibility:
    [
        {
            "title": "Game Name",
            "target": "/absolute/path/to/exe",
            "startIn": "/absolute/path/to/dir",
            "launchOptions": "",
            "savePath": "/absolute/path/to/saves",
            "steamgriddb_id": 12345
        },
        ...
    ]
    """
    games = db.games_with_targets()
    if not games:
        logger.info("No games with targets to write to manifests.json")
        return

    manifests = []
    for game in games:
        entry = {
            "title": game.title,
            "target": game.resolved_target,
            "startIn": game.resolved_start_in,
            "launchOptions": game.resolved_launch_options,
        }

        # Use first resolved save path for backward compat
        if game.resolved_save_paths:
            entry["savePath"] = game.resolved_save_paths[0]
        else:
            entry["savePath"] = ""

        if game.steamgriddb_id is not None:
            entry["steamgriddb_id"] = game.steamgriddb_id

        manifests.append(entry)

    try:
        os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
        with open(output_path, "w") as f:
            json.dump(manifests, f, indent=4)
        logger.info(f"Wrote manifests.json ({len(manifests)} games) to {output_path}")
    except OSError as e:
        logger.error(f"Failed to write manifests.json: {e}")
