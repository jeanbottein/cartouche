"""Unit tests for lib/steam_cleaner.py — pure helpers and mocked vdf tests."""
import pytest

from lib.steam_cleaner import _has_ownership_tag, _reindex, _get_appname, _get_shortcuts_path


# ── _has_ownership_tag ────────────────────────────────────────────────────

class TestHasOwnershipTag:
    def test_matches_cartouche_tag(self):
        shortcut = {"tags": {"0": "cartouche"}}
        assert _has_ownership_tag(shortcut) is True

    def test_matches_legacy_gamer_sidekick_tag(self):
        shortcut = {"tags": {"0": "gamer-sidekick"}}
        assert _has_ownership_tag(shortcut) is True

    def test_returns_false_for_other_tag(self):
        shortcut = {"tags": {"0": "steam"}}
        assert _has_ownership_tag(shortcut) is False

    def test_returns_false_when_no_tags_key(self):
        shortcut = {}
        assert _has_ownership_tag(shortcut) is False

    def test_returns_false_when_tags_is_empty_dict(self):
        shortcut = {"tags": {}}
        assert _has_ownership_tag(shortcut) is False

    def test_returns_false_when_tags_is_not_dict(self):
        """Non-dict tags field is gracefully handled."""
        shortcut = {"tags": "cartouche"}
        assert _has_ownership_tag(shortcut) is False

    def test_multiple_tags_one_matches(self):
        shortcut = {"tags": {"0": "some-other-tag", "1": "cartouche"}}
        assert _has_ownership_tag(shortcut) is True

    def test_case_sensitive_match(self):
        """Tags are matched case-sensitively — 'Cartouche' should NOT match."""
        shortcut = {"tags": {"0": "Cartouche"}}
        assert _has_ownership_tag(shortcut) is False


# ── _reindex ──────────────────────────────────────────────────────────────

class TestReindex:
    def test_empty_dict_returns_empty_dict(self):
        assert _reindex({}) == {}

    def test_renumbers_sequentially_from_zero(self):
        shortcuts = {"0": "a", "1": "b", "2": "c"}
        result = _reindex(shortcuts)
        assert result == {"0": "a", "1": "b", "2": "c"}

    def test_fills_gaps_in_numbering(self):
        shortcuts = {"0": "a", "5": "b", "10": "c"}
        result = _reindex(shortcuts)
        assert set(result.keys()) == {"0", "1", "2"}
        assert list(result.values()) == ["a", "b", "c"]

    def test_sorts_numerically_not_lexicographically(self):
        """{"9": "nine", "10": "ten"} should be 9 before 10, not "10" before "9"."""
        shortcuts = {"9": "nine", "10": "ten"}
        result = _reindex(shortcuts)
        assert result["0"] == "nine"
        assert result["1"] == "ten"

    def test_single_entry_reindexed_to_zero(self):
        shortcuts = {"5": "only"}
        result = _reindex(shortcuts)
        assert result == {"0": "only"}

    def test_non_numeric_keys_sorted_to_position_zero(self):
        """
        Known issue: non-numeric key uses fallback sort key of 0.
        This test documents the current (potentially surprising) behavior
        rather than asserting it raises an error.
        """
        shortcuts = {"abc": "value", "0": "first"}
        # Should not raise — non-numeric key uses fallback
        result = _reindex(shortcuts)
        assert len(result) == 2


# ── _get_appname ──────────────────────────────────────────────────────────

class TestGetAppname:
    def test_returns_AppName_field(self):
        assert _get_appname({"AppName": "My Game"}) == "My Game"

    def test_falls_back_to_lowercase_appname(self):
        assert _get_appname({"appname": "my game"}) == "my game"

    def test_prefers_AppName_over_appname(self):
        assert _get_appname({"AppName": "A", "appname": "b"}) == "A"

    def test_empty_shortcut_returns_empty_string(self):
        assert _get_appname({}) == ""


# ── _get_shortcuts_path ───────────────────────────────────────────────────

class TestGetShortcutsPath:
    def test_returns_shortcuts_vdf_path(self):
        config_dir = "/home/user/.steam/userdata/12345/config"
        result = _get_shortcuts_path(config_dir)
        assert result == "/home/user/.steam/userdata/12345/config/shortcuts.vdf"


# ── clean() — mocked vdf ─────────────────────────────────────────────────

class TestCleanMocked:
    def test_clean_skips_when_steam_expose_not_true(self, mocker):
        mocker.patch("lib.steam_cleaner.find_steam_userdata_dirs", return_value=["/fake/config"])
        mock_load = mocker.patch("lib.steam_cleaner.load_shortcuts")
        mock_save = mocker.patch("lib.steam_cleaner.save_shortcuts")

        from lib.steam_cleaner import clean
        from lib.models import GameDatabase
        db = GameDatabase()
        clean(db, {"STEAM_EXPOSE": "False"})

        mock_load.assert_not_called()
        mock_save.assert_not_called()

    def test_clean_removes_stale_shortcut(self, tmp_path, mocker):
        config_dir = str(tmp_path / "config")
        mocker.patch("lib.steam_cleaner.find_steam_userdata_dirs", return_value=[config_dir])
        mocker.patch("lib.steam_cleaner.load_shortcuts", return_value={
            "0": {"Exe": '"/stale/game.x86_64"', "tags": {"0": "cartouche"}},
        })
        mock_save = mocker.patch("lib.steam_cleaner.save_shortcuts")
        mocker.patch("os.makedirs")

        from lib.steam_cleaner import clean
        from lib.models import GameDatabase
        db = GameDatabase()
        # No games with resolved targets → all shortcuts are stale
        clean(db, {"STEAM_EXPOSE": "true"})

        mock_save.assert_called_once()
        saved_shortcuts = mock_save.call_args[0][1]
        assert len(saved_shortcuts) == 0

    def test_clean_preserves_non_cartouche_shortcut(self, tmp_path, mocker):
        config_dir = str(tmp_path / "config")
        mocker.patch("lib.steam_cleaner.find_steam_userdata_dirs", return_value=[config_dir])
        mocker.patch("lib.steam_cleaner.load_shortcuts", return_value={
            "0": {"Exe": '"/some/game.exe"', "tags": {"0": "steam"}},
        })
        mock_save = mocker.patch("lib.steam_cleaner.save_shortcuts")

        from lib.steam_cleaner import clean
        from lib.models import GameDatabase
        db = GameDatabase()
        clean(db, {"STEAM_EXPOSE": "true"})

        # No stale owned shortcuts → save should not be called
        mock_save.assert_not_called()
