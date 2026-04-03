"""Unit tests for lib/configurer.py — pure transformation functions."""
import pytest

from lib.configurer import resolve_variables, apply_text_replacements, apply_hex_replacements


# ── resolve_variables ─────────────────────────────────────────────────────

class TestResolveVariables:
    def test_substitutes_known_variable(self):
        result, unresolved = resolve_variables("${HOME}/config", {"HOME": "/home/user"})
        assert result == "/home/user/config"
        assert unresolved == []

    def test_returns_none_for_missing_variable(self):
        result, unresolved = resolve_variables("${MISSING_VAR}/path", {})
        assert result is None
        assert "MISSING_VAR" in unresolved

    def test_substitutes_multiple_variables(self):
        result, unresolved = resolve_variables(
            "${A}/${B}",
            {"A": "foo", "B": "bar"},
        )
        assert result == "foo/bar"
        assert unresolved == []

    def test_returns_none_if_any_variable_is_missing(self):
        result, unresolved = resolve_variables(
            "${A}/${B}",
            {"A": "foo"},
        )
        assert result is None
        assert "B" in unresolved

    def test_non_string_input_passthrough(self):
        """Non-string input is returned as-is with empty unresolved list."""
        result, unresolved = resolve_variables(42, {})
        assert result == 42
        assert unresolved == []

    def test_text_without_variables_unchanged(self):
        result, unresolved = resolve_variables("plain text", {})
        assert result == "plain text"
        assert unresolved == []

    def test_falls_back_to_env_variable(self, monkeypatch):
        monkeypatch.setenv("MY_ENV_VAR", "/env/path")
        result, unresolved = resolve_variables("${MY_ENV_VAR}/file", {})
        assert result == "/env/path/file"
        assert unresolved == []


# ── apply_text_replacements ───────────────────────────────────────────────

class TestApplyTextReplacements:
    def test_basic_pattern_replacement(self):
        content = 'language_code = "en"\n'
        reps = [{"name": "lang", "pattern": r'"en"', "value": '"fr"', "insert": False}]
        result, modified = apply_text_replacements(content, reps)
        assert '"fr"' in result
        assert modified is True

    def test_no_match_leaves_content_unchanged(self):
        content = "nothing to replace\n"
        reps = [{"name": "x", "pattern": "NOTFOUND", "value": "y", "insert": False}]
        result, modified = apply_text_replacements(content, reps)
        assert result == content
        assert modified is False

    def test_insert_mode_appends_at_end_when_no_after(self):
        content = "existing line\n"
        reps = [{"name": "new", "pattern": "NOTFOUND", "value": "new_line", "insert": True, "after": None}]
        result, modified = apply_text_replacements(content, reps)
        assert "new_line" in result
        assert modified is True

    def test_insert_mode_inserts_after_marker(self):
        content = "[Settings]\nother_key=1\n"
        reps = [{"name": "ins", "pattern": "NOTFOUND", "value": "key=value", "insert": True, "after": "[Settings]"}]
        result, modified = apply_text_replacements(content, reps)
        lines = result.splitlines()
        settings_idx = lines.index("[Settings]")
        assert lines[settings_idx + 1] == "key=value"
        assert modified is True

    def test_insert_after_marker_not_found_skips(self):
        content = "no marker here\n"
        reps = [{"name": "ins", "pattern": "NOTFOUND", "value": "val", "insert": True, "after": "[Missing]"}]
        result, modified = apply_text_replacements(content, reps)
        assert modified is False

    def test_multiple_replacements_applied_in_order(self):
        content = "a=1\nb=2\n"
        reps = [
            {"name": "r1", "pattern": "a=1", "value": "a=10", "insert": False},
            {"name": "r2", "pattern": "b=2", "value": "b=20", "insert": False},
        ]
        result, modified = apply_text_replacements(content, reps)
        assert "a=10" in result
        assert "b=20" in result
        assert modified is True


# ── apply_hex_replacements ────────────────────────────────────────────────

class TestApplyHexReplacements:
    def test_simple_ascii_byte_replacement(self):
        content = b"hello world"
        reps = [{"name": "r", "pattern": "world", "value": "earth"}]
        result, modified = apply_hex_replacements(content, reps)
        assert result == b"hello earth"
        assert modified is True

    def test_no_match_leaves_content_unchanged(self):
        content = b"hello world"
        reps = [{"name": "r", "pattern": "xyz", "value": "abc"}]
        result, modified = apply_hex_replacements(content, reps)
        assert result == content
        assert modified is False

    def test_empty_replacements_list(self):
        content = b"unchanged"
        result, modified = apply_hex_replacements(content, [])
        assert result == content
        assert modified is False
