"""Unit tests for lib/saver.py — pure helper functions."""
import pytest

from lib.saver import _sanitize_title, _resolve_base_path


# ── _sanitize_title ───────────────────────────────────────────────────────

class TestSanitizeTitle:
    def test_basic_ascii_unchanged(self):
        assert _sanitize_title("MyGame") == "MyGame"

    def test_removes_forbidden_characters(self):
        for char in '<>:"/\\|?*':
            result = _sanitize_title(f"Game{char}Title")
            assert char not in result

    def test_replaces_whitespace_with_underscore(self):
        assert _sanitize_title("My Awesome Game") == "My_Awesome_Game"

    def test_multiple_spaces_become_single_underscore(self):
        assert _sanitize_title("My   Game") == "My_Game"

    def test_strips_trailing_period(self):
        result = _sanitize_title("Game.")
        assert not result.endswith(".")

    def test_strips_trailing_space(self):
        result = _sanitize_title("Game ")
        assert not result.endswith(" ")

    def test_truncates_at_100_chars(self):
        long_title = "A" * 150
        result = _sanitize_title(long_title)
        assert len(result) <= 100

    def test_truncation_does_not_reintroduce_trailing_period(self):
        """Truncating at 100 chars must not leave a trailing period."""
        # 99 'A's + '.' + 'B' = 101 chars — after [:100] ends in '.'
        title = "A" * 99 + ".B"
        result = _sanitize_title(title)
        assert not result.endswith(".")

    def test_truncation_does_not_reintroduce_trailing_space(self):
        title = "A" * 99 + " B"
        result = _sanitize_title(title)
        assert not result.endswith(" ")

    def test_empty_string_becomes_game(self):
        assert _sanitize_title("") == "game"

    def test_none_becomes_game(self):
        assert _sanitize_title(None) == "game"

    def test_windows_reserved_name_con_gets_suffix(self):
        result = _sanitize_title("CON")
        assert result != "CON"
        assert "CON" in result

    def test_windows_reserved_name_aux_gets_suffix(self):
        result = _sanitize_title("AUX")
        assert result != "AUX"

    def test_windows_reserved_name_nul_gets_suffix(self):
        result = _sanitize_title("NUL")
        assert result != "NUL"

    def test_control_chars_replaced(self):
        result = _sanitize_title("Game\x01Title")
        assert "\x01" not in result

    def test_normal_punctuation_preserved(self):
        """Hyphens, apostrophes, dots in middle are valid."""
        result = _sanitize_title("Baldur's Gate")
        assert "Baldur" in result
        assert "Gate" in result


# ── _resolve_base_path ────────────────────────────────────────────────────

class TestResolveBasePath:
    def test_absolute_path_returned_unchanged(self, tmp_path):
        result = _resolve_base_path(str(tmp_path))
        assert result == str(tmp_path)

    def test_tilde_expanded(self):
        import os
        result = _resolve_base_path("~/saves")
        assert not result.startswith("~")
        assert os.path.expanduser("~/saves") in result or result.startswith("/")

    def test_env_var_expanded(self, monkeypatch, tmp_path):
        monkeypatch.setenv("GAME_SAVES", str(tmp_path))
        result = _resolve_base_path("$GAME_SAVES/backup")
        assert str(tmp_path) in result

    def test_returns_absolute_path(self):
        import os
        result = _resolve_base_path("relative/path")
        assert os.path.isabs(result)
