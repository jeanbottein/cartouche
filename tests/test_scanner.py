"""Unit tests for lib/scanner.py — pure helpers and scan() with tmp_path."""
import json
import os

import pytest

from lib.scanner import _pick_target_entry, _resolve_save_path, scan
from lib.models import CARTOUCHE_DIR, GAME_JSON


# ── _resolve_save_path ────────────────────────────────────────────────────

class TestResolveSavePath:
    def test_absolute_path_returned_as_is(self):
        result = _resolve_save_path("/absolute/path", "/game/dir")
        assert result == "/absolute/path"

    def test_relative_path_joined_to_game_dir(self):
        result = _resolve_save_path("saves", "/game/dir")
        assert result == "/game/dir/saves"

    def test_empty_path_returns_empty_string(self):
        assert _resolve_save_path("", "/game/dir") == ""

    def test_tilde_expanded(self):
        result = _resolve_save_path("~/saves", "/game/dir")
        assert not result.startswith("~")

    def test_env_var_expanded(self, monkeypatch, tmp_path):
        monkeypatch.setenv("GAME_HOME", str(tmp_path))
        result = _resolve_save_path("$GAME_HOME/saves", "/game/dir")
        assert str(tmp_path) in result

    def test_normalizes_path(self):
        result = _resolve_save_path("/some//double/../path", "/game/dir")
        assert "//" not in result
        assert ".." not in result


# ── _pick_target_entry ────────────────────────────────────────────────────

class TestPickTargetEntry:
    def test_returns_none_for_empty_list(self):
        assert _pick_target_entry([]) is None

    def test_returns_single_entry(self, monkeypatch):
        monkeypatch.setattr("lib.scanner.os_tag", lambda: "linux")
        monkeypatch.setattr("lib.scanner.arch_tag", lambda: "x64")
        target = {"os": "linux", "arch": "x64", "target": "game", "startIn": "."}
        assert _pick_target_entry([target]) is target

    def test_prefers_matching_os(self, monkeypatch):
        monkeypatch.setattr("lib.scanner.os_tag", lambda: "linux")
        monkeypatch.setattr("lib.scanner.arch_tag", lambda: "x64")
        linux_target = {"os": "linux", "arch": "x64", "target": "linux_game"}
        windows_target = {"os": "windows", "arch": "x64", "target": "win_game.exe"}
        result = _pick_target_entry([windows_target, linux_target])
        assert result["target"] == "linux_game"

    def test_any_os_matches_current_os(self, monkeypatch):
        monkeypatch.setattr("lib.scanner.os_tag", lambda: "linux")
        monkeypatch.setattr("lib.scanner.arch_tag", lambda: "x64")
        target = {"os": "any", "arch": "x64", "target": "cross_game"}
        assert _pick_target_entry([target]) is target

    def test_empty_os_acts_as_any(self, monkeypatch):
        monkeypatch.setattr("lib.scanner.os_tag", lambda: "linux")
        monkeypatch.setattr("lib.scanner.arch_tag", lambda: "x64")
        target = {"os": "", "arch": "", "target": "any_game"}
        assert _pick_target_entry([target]) is target

    def test_prefers_matching_arch(self, monkeypatch):
        monkeypatch.setattr("lib.scanner.os_tag", lambda: "linux")
        monkeypatch.setattr("lib.scanner.arch_tag", lambda: "x64")
        x64_target = {"os": "linux", "arch": "x64", "target": "x64_game"}
        arm_target = {"os": "linux", "arch": "arm64", "target": "arm_game"}
        result = _pick_target_entry([arm_target, x64_target])
        assert result["target"] == "x64_game"


# ── scan() ────────────────────────────────────────────────────────────────

class TestScan:
    def test_returns_empty_db_for_invalid_path(self):
        db = scan("/nonexistent/path/that/does/not/exist")
        assert len(db) == 0

    def test_returns_empty_db_for_none(self):
        db = scan(None)
        assert len(db) == 0

    def test_discovers_game_without_cartouche_as_skeleton(self, tmp_path):
        game_dir = tmp_path / "CoolGame"
        game_dir.mkdir()
        db = scan(str(tmp_path))
        assert len(db) == 1
        game = db.get_by_folder("CoolGame")
        assert game is not None
        assert game.folder_name == "CoolGame"
        assert game.has_cartouche is False

    def test_loads_game_with_valid_game_json(self, tmp_path):
        game_dir = tmp_path / "MyGame"
        cartouche_dir = game_dir / CARTOUCHE_DIR
        cartouche_dir.mkdir(parents=True)
        game_json = {
            "schema_version": 2,
            "title": "My Game",
            "targets": [],
            "savePaths": [],
            "images": {},
        }
        (cartouche_dir / GAME_JSON).write_text(json.dumps(game_json))

        db = scan(str(tmp_path))
        game = db.get_by_folder("MyGame")
        assert game is not None
        assert game.title == "My Game"
        assert game.has_cartouche is True

    def test_skips_hidden_directories(self, tmp_path):
        hidden = tmp_path / ".hidden"
        hidden.mkdir()
        visible = tmp_path / "VisibleGame"
        visible.mkdir()
        db = scan(str(tmp_path))
        assert db.get_by_folder(".hidden") is None
        assert db.get_by_folder("VisibleGame") is not None

    def test_skips_files_in_root(self, tmp_path):
        (tmp_path / "somefile.txt").write_text("not a game")
        game_dir = tmp_path / "RealGame"
        game_dir.mkdir()
        db = scan(str(tmp_path))
        assert len(db) == 1

    def test_handles_invalid_json_gracefully(self, tmp_path):
        game_dir = tmp_path / "BrokenGame"
        cartouche_dir = game_dir / CARTOUCHE_DIR
        cartouche_dir.mkdir(parents=True)
        (cartouche_dir / GAME_JSON).write_text("NOT VALID JSON {{{")
        db = scan(str(tmp_path))
        # Should still discover the game as a skeleton
        game = db.get_by_folder("BrokenGame")
        assert game is not None

    def test_multiple_games_all_discovered(self, tmp_path):
        for name in ["GameA", "GameB", "GameC"]:
            (tmp_path / name).mkdir()
        db = scan(str(tmp_path))
        assert len(db) == 3
