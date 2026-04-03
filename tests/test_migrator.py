"""Unit tests for lib/migrator.py — _convert_save_path() and file I/O migration."""
import json

import pytest

from lib.migrator import _convert_save_path
from lib.models import CARTOUCHE_DIR, GAME_JSON


# ── _convert_save_path ────────────────────────────────────────────────────

class TestConvertSavePath:
    def test_none_returns_empty_list(self):
        assert _convert_save_path(None) == []

    def test_empty_string_returns_empty_list(self):
        assert _convert_save_path("") == []

    def test_whitespace_string_returns_empty_list(self):
        assert _convert_save_path("   ") == []

    def test_string_path_wrapped_in_list(self):
        result = _convert_save_path("~/.local/share/Game/saves")
        assert result == [{"os": "", "path": "~/.local/share/Game/saves"}]

    def test_list_of_dicts_preserved(self):
        old = [{"os": "linux", "path": "/home/user/.saves"}]
        result = _convert_save_path(old)
        assert result == [{"os": "linux", "path": "/home/user/.saves"}]

    def test_list_of_strings_wrapped(self):
        result = _convert_save_path(["/path/a", "/path/b"])
        assert result == [
            {"os": "", "path": "/path/a"},
            {"os": "", "path": "/path/b"},
        ]

    def test_list_with_empty_path_entries_excluded(self):
        old = [
            {"os": "linux", "path": ""},
            {"os": "linux", "path": "/valid/path"},
        ]
        result = _convert_save_path(old)
        assert len(result) == 1
        assert result[0]["path"] == "/valid/path"

    def test_list_dict_uses_savePath_key_alias(self):
        old = [{"os": "windows", "savePath": "C:\\Saves"}]
        result = _convert_save_path(old)
        assert result == [{"os": "windows", "path": "C:\\Saves"}]

    def test_list_dict_uses_value_key_alias(self):
        old = [{"os": "", "value": "/path/to/saves"}]
        result = _convert_save_path(old)
        assert result == [{"os": "", "path": "/path/to/saves"}]

    def test_unsupported_type_returns_empty_list(self):
        assert _convert_save_path(42) == []
        assert _convert_save_path({"os": "linux"}) == []

    def test_mixed_list_strings_and_dicts(self):
        old = [
            "/string/path",
            {"os": "linux", "path": "/dict/path"},
        ]
        result = _convert_save_path(old)
        assert len(result) == 2
        paths = {r["path"] for r in result}
        assert "/string/path" in paths
        assert "/dict/path" in paths
