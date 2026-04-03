"""Unit tests for lib/enricher.py — pure helper functions."""
import pytest

from lib.enricher import _pick_best, _build_content_filters, _get_extension


# ── _pick_best ────────────────────────────────────────────────────────────

class TestPickBest:
    def test_returns_url_of_highest_score(self):
        data = [
            {"score": 10, "url": "https://example.com/low.png"},
            {"score": 99, "url": "https://example.com/high.png"},
            {"score": 50, "url": "https://example.com/mid.png"},
        ]
        assert _pick_best(data) == "https://example.com/high.png"

    def test_returns_none_for_empty_list(self):
        assert _pick_best([]) is None

    def test_returns_none_for_none_input(self):
        assert _pick_best(None) is None

    def test_single_entry_returns_its_url(self):
        data = [{"score": 5, "url": "https://example.com/only.png"}]
        assert _pick_best(data) == "https://example.com/only.png"

    def test_missing_score_treated_as_zero(self):
        data = [
            {"url": "https://example.com/no-score.png"},
            {"score": 1, "url": "https://example.com/has-score.png"},
        ]
        assert _pick_best(data) == "https://example.com/has-score.png"

    def test_entry_without_url_returns_none(self):
        """Entry with highest score but no url key → returns None."""
        data = [{"score": 100}]
        assert _pick_best(data) is None


# ── _build_content_filters ────────────────────────────────────────────────

class TestBuildContentFilters:
    def test_all_false_by_default(self):
        result = _build_content_filters({})
        assert "nsfw=false" in result
        assert "humor=false" in result
        assert "epilepsy=false" in result

    def test_nsfw_true_sets_any(self):
        result = _build_content_filters({"STEAMGRIDDB_NSFW": "True"})
        assert "nsfw=any" in result
        assert "humor=false" in result
        assert "epilepsy=false" in result

    def test_humor_true_sets_any(self):
        result = _build_content_filters({"STEAMGRIDDB_HUMOR": "true"})
        assert "humor=any" in result
        assert "nsfw=false" in result

    def test_epilepsy_true_sets_any(self):
        result = _build_content_filters({"STEAMGRIDDB_EPILEPSY": "TRUE"})
        assert "epilepsy=any" in result

    def test_all_enabled(self):
        cfg = {
            "STEAMGRIDDB_NSFW": "true",
            "STEAMGRIDDB_HUMOR": "true",
            "STEAMGRIDDB_EPILEPSY": "true",
        }
        result = _build_content_filters(cfg)
        assert "nsfw=any" in result
        assert "humor=any" in result
        assert "epilepsy=any" in result

    def test_false_string_values_stay_false(self):
        cfg = {
            "STEAMGRIDDB_NSFW": "false",
            "STEAMGRIDDB_HUMOR": "False",
        }
        result = _build_content_filters(cfg)
        assert "nsfw=false" in result
        assert "humor=false" in result


# ── _get_extension ────────────────────────────────────────────────────────

class TestGetExtension:
    def test_extracts_png_extension(self):
        assert _get_extension("https://example.com/image.png") == ".png"

    def test_extracts_jpg_extension(self):
        assert _get_extension("https://example.com/image.jpg") == ".jpg"

    def test_strips_query_string_before_extracting(self):
        assert _get_extension("https://example.com/image.png?v=1&size=large") == ".png"

    def test_falls_back_to_png_when_no_extension(self):
        assert _get_extension("https://example.com/image") == ".png"

    def test_webp_extension(self):
        assert _get_extension("https://cdn.example.com/art.webp") == ".webp"
