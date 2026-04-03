"""Shared fixtures for cartouche unit tests."""
import json
import sys
from pathlib import Path
from unittest.mock import MagicMock

import pytest

# Stub optional heavy dependencies that may not be installed in CI.
# Must be done before any lib import triggers lib/__init__.py.
for _mod in ("bps", "bps.apply"):
    if _mod not in sys.modules:
        sys.modules[_mod] = MagicMock()

from lib.models import Game, GameTarget, GameImages, GameDatabase, CARTOUCHE_DIR, GAME_JSON


@pytest.fixture
def sample_game_target():
    return GameTarget(
        os="linux",
        arch="x64",
        target="game.x86_64",
        start_in=".",
        launch_options="",
    )


@pytest.fixture
def sample_game(tmp_path):
    game_dir = tmp_path / "MyGame"
    game_dir.mkdir()
    return Game(
        folder_name="MyGame",
        game_dir=game_dir,
        title="My Game",
    )


@pytest.fixture
def sample_game_with_target(tmp_path, sample_game_target):
    game_dir = tmp_path / "MyGame"
    game_dir.mkdir(exist_ok=True)
    game = Game(
        folder_name="MyGame",
        game_dir=game_dir,
        title="My Game",
        targets=[sample_game_target],
    )
    game.resolved_target = str(game_dir / "game.x86_64")
    game.resolved_start_in = str(game_dir)
    game.resolved_target_os = "linux"
    return game


@pytest.fixture
def sample_game_database(sample_game):
    db = GameDatabase()
    db.add(sample_game)
    return db


@pytest.fixture
def sample_game_json():
    """A dict matching the game.json schema."""
    return {
        "schema_version": 2,
        "title": "My Game",
        "targets": [
            {
                "os": "linux",
                "arch": "x64",
                "target": "game.x86_64",
                "startIn": ".",
                "launchOptions": "",
            }
        ],
        "savePaths": [
            {"os": "linux", "path": "~/.local/share/MyGame/saves"},
        ],
        "images": {
            "cover": "cover.png",
        },
        "steamgriddb_id": 12345,
        "notes": "",
    }


@pytest.fixture
def game_dir_with_json(tmp_path, sample_game_json):
    """Creates a game directory with a valid .cartouche/game.json."""
    game_dir = tmp_path / "MyGame"
    cartouche_dir = game_dir / CARTOUCHE_DIR
    cartouche_dir.mkdir(parents=True)
    game_json = cartouche_dir / GAME_JSON
    game_json.write_text(json.dumps(sample_game_json))
    return game_dir
