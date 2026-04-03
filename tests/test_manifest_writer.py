"""Unit tests for lib/manifest_writer.py — write() with tmp_path."""
import json

import pytest

from lib.models import Game, GameDatabase, GameTarget
from lib.manifest_writer import write


class TestManifestWriter:
    def _make_game_with_target(self, tmp_path, folder="MyGame", title="My Game"):
        game_dir = tmp_path / folder
        game_dir.mkdir(exist_ok=True)
        gt = GameTarget(os="linux", arch="x64", target="game.x86_64", start_in=".")
        game = Game(
            folder_name=folder,
            game_dir=game_dir,
            title=title,
            targets=[gt],
            steamgriddb_id=12345,
        )
        game.resolved_target = str(game_dir / "game.x86_64")
        game.resolved_start_in = str(game_dir)
        game.resolved_launch_options = ""
        game.resolved_target_os = "linux"
        return game

    def test_creates_valid_json_file(self, tmp_path):
        db = GameDatabase()
        db.add(self._make_game_with_target(tmp_path))
        output = str(tmp_path / "manifests.json")
        write(db, output)
        data = json.loads((tmp_path / "manifests.json").read_text())
        assert isinstance(data, list)
        assert len(data) == 1

    def test_manifest_contains_required_fields(self, tmp_path):
        db = GameDatabase()
        db.add(self._make_game_with_target(tmp_path))
        output = str(tmp_path / "manifests.json")
        write(db, output)
        entry = json.loads((tmp_path / "manifests.json").read_text())[0]
        assert "title" in entry
        assert "target" in entry
        assert "startIn" in entry
        assert "launchOptions" in entry
        assert "savePath" in entry

    def test_manifest_title_matches_game_title(self, tmp_path):
        db = GameDatabase()
        db.add(self._make_game_with_target(tmp_path, title="Awesome RPG"))
        output = str(tmp_path / "out.json")
        write(db, output)
        entry = json.loads((tmp_path / "out.json").read_text())[0]
        assert entry["title"] == "Awesome RPG"

    def test_manifest_includes_steamgriddb_id_when_set(self, tmp_path):
        db = GameDatabase()
        db.add(self._make_game_with_target(tmp_path))
        output = str(tmp_path / "out.json")
        write(db, output)
        entry = json.loads((tmp_path / "out.json").read_text())[0]
        assert entry["steamgriddb_id"] == 12345

    def test_manifest_omits_steamgriddb_id_when_none(self, tmp_path):
        db = GameDatabase()
        game = self._make_game_with_target(tmp_path)
        game.steamgriddb_id = None
        db.add(game)
        output = str(tmp_path / "out.json")
        write(db, output)
        entry = json.loads((tmp_path / "out.json").read_text())[0]
        assert "steamgriddb_id" not in entry

    def test_skips_games_without_resolved_target(self, tmp_path):
        db = GameDatabase()
        game_dir = tmp_path / "NoTarget"
        game_dir.mkdir()
        game = Game(folder_name="NoTarget", game_dir=game_dir, title="No Target Game")
        # resolved_target is None by default
        db.add(game)
        output = str(tmp_path / "out.json")
        write(db, output)
        # File should not be created (or be empty) — write() returns early
        import os
        assert not os.path.exists(output)

    def test_uses_first_save_path_only(self, tmp_path):
        """Known limitation: only the first resolved_save_path is written."""
        db = GameDatabase()
        game = self._make_game_with_target(tmp_path)
        game.resolved_save_paths = ["/path/to/saves1", "/path/to/saves2"]
        db.add(game)
        output = str(tmp_path / "out.json")
        write(db, output)
        entry = json.loads((tmp_path / "out.json").read_text())[0]
        assert entry["savePath"] == "/path/to/saves1"

    def test_multiple_games_all_in_manifest(self, tmp_path):
        db = GameDatabase()
        db.add(self._make_game_with_target(tmp_path, folder="GameA", title="Game A"))
        db.add(self._make_game_with_target(tmp_path, folder="GameB", title="Game B"))
        output = str(tmp_path / "out.json")
        write(db, output)
        data = json.loads((tmp_path / "out.json").read_text())
        assert len(data) == 2
        titles = {e["title"] for e in data}
        assert "Game A" in titles
        assert "Game B" in titles
