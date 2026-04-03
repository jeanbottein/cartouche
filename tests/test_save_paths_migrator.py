"""Unit tests for lib/save_paths_migrator.py — pure helpers and file I/O."""
import json

import pytest

from lib.save_paths_migrator import _is_new_format, _flatten_save_paths, migrate_game_json
from lib.models import CARTOUCHE_DIR, GAME_JSON


# ── _is_new_format ────────────────────────────────────────────────────────

class TestIsNewFormat:
    def test_new_format_with_os_and_path_keys(self):
        save_paths = [{"os": "linux", "path": "~/.local/share/saves"}]
        assert _is_new_format(save_paths) is True

    def test_old_format_with_name_and_paths_keys(self):
        save_paths = [{"name": "saves", "paths": [{"os": "linux", "path": "..."}]}]
        assert _is_new_format(save_paths) is False

    def test_empty_list_returns_false(self):
        assert _is_new_format([]) is False

    def test_non_dict_entries_return_false(self):
        assert _is_new_format(["string_entry"]) is False

    def test_dict_without_os_key_returns_false(self):
        assert _is_new_format([{"path": "/somewhere"}]) is False

    def test_dict_without_path_key_returns_false(self):
        assert _is_new_format([{"os": "linux"}]) is False


# ── _flatten_save_paths ───────────────────────────────────────────────────

class TestFlattenSavePaths:
    def test_converts_nested_format_to_flat(self):
        old = [
            {
                "name": "saves",
                "paths": [
                    {"os": "linux", "path": "~/.local/share/Game/saves"},
                    {"os": "windows", "path": "%APPDATA%/Game/saves"},
                ]
            }
        ]
        result = _flatten_save_paths(old)
        assert result is not None
        assert len(result) == 2
        paths = {r["path"] for r in result}
        assert "~/.local/share/Game/saves" in paths
        assert "%APPDATA%/Game/saves" in paths

    def test_returns_none_for_empty_result(self):
        """If no valid paths extracted, returns None."""
        old = [{"name": "saves", "paths": []}]
        result = _flatten_save_paths(old)
        assert result is None

    def test_skips_non_dict_entries(self):
        old = ["not_a_dict"]
        result = _flatten_save_paths(old)
        # No valid entries → None
        assert result is None

    def test_already_flat_entry_preserved_if_has_os_and_path(self):
        old = [{"os": "linux", "path": "/saves", "paths": []}]
        result = _flatten_save_paths(old)
        assert result is not None
        assert any(r["path"] == "/saves" for r in result)

    def test_multiple_named_groups_all_flattened(self):
        old = [
            {"name": "saves", "paths": [{"os": "linux", "path": "/saves"}]},
            {"name": "config", "paths": [{"os": "linux", "path": "/config"}]},
        ]
        result = _flatten_save_paths(old)
        assert result is not None
        assert len(result) == 2


# ── migrate_game_json() (file I/O) ────────────────────────────────────────

class TestMigrateGameJson:
    def _write_game_json(self, game_dir, data):
        cartouche_dir = game_dir / CARTOUCHE_DIR
        cartouche_dir.mkdir(parents=True, exist_ok=True)
        (cartouche_dir / GAME_JSON).write_text(json.dumps(data))

    def test_returns_false_when_no_game_json(self, tmp_path):
        game_dir = tmp_path / "NoJson"
        game_dir.mkdir()
        assert migrate_game_json(game_dir) is False

    def test_returns_false_when_already_new_format(self, tmp_path):
        game_dir = tmp_path / "Game"
        self._write_game_json(game_dir, {
            "savePaths": [{"os": "linux", "path": "/saves"}]
        })
        assert migrate_game_json(game_dir) is False

    def test_returns_false_when_savePaths_empty(self, tmp_path):
        game_dir = tmp_path / "Game"
        self._write_game_json(game_dir, {"savePaths": []})
        assert migrate_game_json(game_dir) is False

    def test_migrates_nested_format_and_returns_true(self, tmp_path):
        game_dir = tmp_path / "Game"
        old_data = {
            "title": "Game",
            "savePaths": [
                {
                    "name": "saves",
                    "paths": [
                        {"os": "linux", "path": "~/.local/share/Game/saves"}
                    ]
                }
            ]
        }
        self._write_game_json(game_dir, old_data)
        result = migrate_game_json(game_dir)
        assert result is True

        # Verify the file was updated
        game_json = (game_dir / CARTOUCHE_DIR / GAME_JSON).read_text()
        updated = json.loads(game_json)
        save_paths = updated["savePaths"]
        assert len(save_paths) >= 1
        assert "os" in save_paths[0]
        assert "path" in save_paths[0]
        # The old "name"/"paths" structure should be gone
        assert "name" not in save_paths[0]

    def test_skips_already_migrated_file(self, tmp_path):
        game_dir = tmp_path / "Game"
        new_data = {
            "title": "Game",
            "savePaths": [{"os": "linux", "path": "~/.local/share/Game/saves"}]
        }
        self._write_game_json(game_dir, new_data)
        # Should return False (already migrated)
        assert migrate_game_json(game_dir) is False
