"""Unit tests for lib/detector.py — _get_real_first_path() depth limit."""
import os
import pytest

from lib.detector import _get_real_first_path


class TestGetRealFirstPath:
    def test_returns_game_dir_when_multiple_entries(self, tmp_path):
        """Directory with multiple entries is not descended into."""
        (tmp_path / "file1.txt").write_text("x")
        (tmp_path / "subdir").mkdir()
        assert _get_real_first_path(str(tmp_path)) == str(tmp_path)

    def test_descends_single_empty_subdir(self, tmp_path):
        """Single subdirectory with no files → descend."""
        inner = tmp_path / "GameFiles"
        inner.mkdir()
        (inner / "game.x86_64").write_text("x")
        result = _get_real_first_path(str(tmp_path))
        assert result == str(inner)

    def test_stops_at_depth_limit(self, tmp_path):
        """Does not recurse more than 10 levels — no RecursionError on deep nesting."""
        current = tmp_path
        for i in range(15):
            sub = current / f"level{i}"
            sub.mkdir()
            current = sub

        # Should not raise RecursionError; must return some valid path
        result = _get_real_first_path(str(tmp_path))
        assert isinstance(result, str)

    def test_returns_game_dir_when_files_present(self, tmp_path):
        """Directory containing files is not descended even if only one subdir."""
        (tmp_path / "readme.txt").write_text("x")
        sub = tmp_path / "sub"
        sub.mkdir()
        assert _get_real_first_path(str(tmp_path)) == str(tmp_path)

    def test_skips_hidden_entries(self, tmp_path):
        """Hidden directories (.cartouche, .hidden) are excluded from consideration."""
        from lib.models import CARTOUCHE_DIR
        (tmp_path / CARTOUCHE_DIR).mkdir()
        inner = tmp_path / "GameFiles"
        inner.mkdir()
        (inner / "game.x86_64").write_text("x")
        # Only one visible subdirectory → should descend
        result = _get_real_first_path(str(tmp_path))
        assert result == str(inner)
