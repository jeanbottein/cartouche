"""Unit tests for lib/steam_exporter.py — pure helper functions."""
import pytest

from lib.steam_exporter import generate_appid, _signed32, _next_index, _make_shortcut_entry, _get_grid_dir


# ── _signed32 ─────────────────────────────────────────────────────────────

class TestSigned32:
    def test_small_positive_passthrough(self):
        assert _signed32(0) == 0
        assert _signed32(100) == 100
        assert _signed32(0x7FFFFFFF) == 0x7FFFFFFF

    def test_large_value_wraps_to_negative(self):
        assert _signed32(0x80000000) == -2147483648
        assert _signed32(0xFFFFFFFF) == -1

    def test_just_below_boundary_unchanged(self):
        assert _signed32(0x7FFFFFFF) > 0


# ── generate_appid ────────────────────────────────────────────────────────

class TestGenerateAppid:
    def test_deterministic_same_inputs(self):
        appid1 = generate_appid("My Game", "/path/to/game.x86_64")
        appid2 = generate_appid("My Game", "/path/to/game.x86_64")
        assert appid1 == appid2

    def test_different_names_produce_different_ids(self):
        appid1 = generate_appid("Game A", "/path/game")
        appid2 = generate_appid("Game B", "/path/game")
        assert appid1 != appid2

    def test_different_paths_produce_different_ids(self):
        appid1 = generate_appid("Game", "/path/a/game")
        appid2 = generate_appid("Game", "/path/b/game")
        assert appid1 != appid2

    def test_result_is_unsigned_32bit(self):
        appid = generate_appid("Game", "/path/game")
        assert 0 <= appid <= 0xFFFFFFFF

    def test_high_bit_is_set(self):
        """Non-Steam shortcut appids always have bit 31 set."""
        appid = generate_appid("Game", "/path/game")
        assert appid & 0x80000000

    def test_none_inputs_do_not_crash(self):
        appid = generate_appid(None, None)
        assert isinstance(appid, int)


# ── _next_index ───────────────────────────────────────────────────────────

class TestNextIndex:
    def test_empty_dict_returns_zero(self):
        assert _next_index({}) == "0"

    def test_existing_entries_returns_next(self):
        shortcuts = {"0": {}, "1": {}, "2": {}}
        assert _next_index(shortcuts) == "3"

    def test_non_contiguous_entries_returns_max_plus_one(self):
        shortcuts = {"0": {}, "5": {}}
        assert _next_index(shortcuts) == "6"

    def test_single_entry_returns_one(self):
        assert _next_index({"0": {}}) == "1"


# ── _make_shortcut_entry ──────────────────────────────────────────────────

class TestMakeShortcutEntry:
    def test_contains_required_fields(self):
        entry = _make_shortcut_entry("My Game", "/path/game.x86_64", "/path")
        assert "appid" in entry
        assert "AppName" in entry
        assert "Exe" in entry
        assert "StartDir" in entry
        assert "tags" in entry

    def test_exe_is_quoted(self):
        entry = _make_shortcut_entry("Game", "/path/game", "/path")
        assert entry["Exe"] == '"/path/game"'

    def test_start_dir_is_quoted(self):
        entry = _make_shortcut_entry("Game", "/path/game", "/start")
        assert entry["StartDir"] == '"/start"'

    def test_app_name_matches_input(self):
        entry = _make_shortcut_entry("Awesome Game", "/path/game", "/path")
        assert entry["AppName"] == "Awesome Game"

    def test_ownership_tag_is_set(self):
        from lib.steam_exporter import OWNERSHIP_TAG
        entry = _make_shortcut_entry("Game", "/path/game", "/path")
        tags = entry["tags"]
        assert isinstance(tags, dict)
        assert any(OWNERSHIP_TAG in str(v) for v in tags.values())

    def test_launch_options_included(self):
        entry = _make_shortcut_entry("Game", "/path/game", "/path", launch_options="--fullscreen")
        assert entry["LaunchOptions"] == "--fullscreen"

    def test_icon_path_included(self):
        entry = _make_shortcut_entry("Game", "/path/game", "/path", icon_path="/grid/icon.png")
        assert entry["icon"] == "/grid/icon.png"

    def test_appid_is_signed_32bit(self):
        entry = _make_shortcut_entry("Game", "/path/game", "/path")
        assert isinstance(entry["appid"], int)
        assert -2147483648 <= entry["appid"] <= 2147483647


# ── _get_grid_dir ─────────────────────────────────────────────────────────

class TestGetGridDir:
    def test_returns_sibling_config_grid(self):
        config_dir = "/home/user/.steam/userdata/12345/config"
        grid = _get_grid_dir(config_dir)
        assert grid == "/home/user/.steam/userdata/12345/config/grid"
