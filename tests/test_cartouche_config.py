"""Unit tests for cartouche.py — load_config_map() and parse_args()."""
from pathlib import Path

import pytest

from cartouche import load_config_map, parse_args


# ── load_config_map ───────────────────────────────────────────────────────

class TestLoadConfigMap:
    def test_parses_key_value_pairs(self, tmp_path):
        cfg = tmp_path / "config.txt"
        cfg.write_text("FOO=bar\nBAZ=qux\n")
        result = load_config_map(cfg)
        assert result == {"FOO": "bar", "BAZ": "qux"}

    def test_strips_inline_comments(self, tmp_path):
        cfg = tmp_path / "config.txt"
        cfg.write_text("FOO=bar # this is a comment\n")
        result = load_config_map(cfg)
        assert result["FOO"] == "bar"

    def test_skips_full_line_comments(self, tmp_path):
        cfg = tmp_path / "config.txt"
        cfg.write_text("# This is a comment\nFOO=bar\n")
        result = load_config_map(cfg)
        assert "# This is a comment" not in result
        assert result["FOO"] == "bar"

    def test_skips_blank_lines(self, tmp_path):
        cfg = tmp_path / "config.txt"
        cfg.write_text("\n\nFOO=bar\n\n")
        result = load_config_map(cfg)
        assert result == {"FOO": "bar"}

    def test_handles_empty_value_excluded(self, tmp_path):
        """Keys with empty values are excluded from the map."""
        cfg = tmp_path / "config.txt"
        cfg.write_text("EMPTY=\n")
        result = load_config_map(cfg)
        assert "EMPTY" not in result

    def test_handles_value_with_equals_sign(self, tmp_path):
        """Only the first '=' is used as separator."""
        cfg = tmp_path / "config.txt"
        cfg.write_text("URL=http://example.com?foo=bar\n")
        result = load_config_map(cfg)
        assert result["URL"] == "http://example.com?foo=bar"

    def test_missing_file_returns_empty_dict(self, tmp_path):
        result = load_config_map(tmp_path / "nonexistent.txt")
        assert result == {}

    def test_strips_whitespace_from_keys_and_values(self, tmp_path):
        cfg = tmp_path / "config.txt"
        cfg.write_text("  FOO  =  bar  \n")
        result = load_config_map(cfg)
        assert result.get("FOO") == "bar"


# ── parse_args ────────────────────────────────────────────────────────────

class TestParseArgs:
    def test_empty_argv_returns_no_dir_no_flags(self):
        cli_dir, dry_run, batch_mode = parse_args([])
        assert cli_dir is None
        assert dry_run is False
        assert batch_mode is False

    def test_batch_mode_flag(self):
        cli_dir, dry_run, batch_mode = parse_args(["--", "batch"])
        assert batch_mode is True
        assert dry_run is False

    def test_test_steam_flag(self):
        cli_dir, dry_run, batch_mode = parse_args(["--", "test", "steam"])
        assert dry_run is True
        assert batch_mode is False

    def test_batch_mode_not_set_for_test_steam(self):
        _, _, batch_mode = parse_args(["--", "test", "steam"])
        assert batch_mode is False

    def test_dry_run_not_set_for_batch(self):
        _, dry_run, _ = parse_args(["--", "batch"])
        assert dry_run is False

    def test_nonexistent_dir_arg_before_separator_is_ignored(self, tmp_path):
        """A non-directory path before '--' should not set cli_dir."""
        cli_dir, _, _ = parse_args(["/nonexistent/path/that/does/not/exist"])
        assert cli_dir is None

    def test_real_directory_before_separator_sets_cli_dir(self, tmp_path):
        cli_dir, _, _ = parse_args([str(tmp_path)])
        assert cli_dir == tmp_path.resolve()

    def test_real_directory_with_batch_mode(self, tmp_path):
        cli_dir, dry_run, batch_mode = parse_args([str(tmp_path), "--", "batch"])
        assert cli_dir == tmp_path.resolve()
        assert batch_mode is True
        assert dry_run is False
