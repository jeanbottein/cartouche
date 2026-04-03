"""Unit tests for lib/models.py — pure dataclass logic, no I/O."""
from pathlib import Path

import pytest

from lib.models import Game, GameTarget, GameImages, GameDatabase


# ── GameTarget ────────────────────────────────────────────────────────────

class TestGameTarget:
    def test_to_dict_roundtrip(self):
        gt = GameTarget(os="linux", arch="x64", target="game.x86_64", start_in=".", launch_options="-v")
        d = gt.to_dict()
        restored = GameTarget.from_dict(d)
        assert restored.os == gt.os
        assert restored.arch == gt.arch
        assert restored.target == gt.target
        assert restored.start_in == gt.start_in
        assert restored.launch_options == gt.launch_options

    def test_from_dict_missing_optional_fields(self):
        gt = GameTarget.from_dict({"os": "linux", "arch": "x64", "target": "foo", "startIn": "."})
        assert gt.launch_options == ""

    def test_from_dict_all_fields_missing_defaults_to_empty(self):
        gt = GameTarget.from_dict({})
        assert gt.os == ""
        assert gt.arch == ""
        assert gt.target == ""
        assert gt.start_in == ""
        assert gt.launch_options == ""

    def test_to_dict_key_names(self):
        gt = GameTarget(os="windows", arch="x64", target="game.exe", start_in=".", launch_options="")
        d = gt.to_dict()
        assert "startIn" in d
        assert "launchOptions" in d
        assert "start_in" not in d  # snake_case should not leak


# ── GameImages ────────────────────────────────────────────────────────────

class TestGameImages:
    def test_to_dict_only_includes_set_fields(self):
        gi = GameImages(cover="cover.png")
        d = gi.to_dict()
        assert d == {"cover": "cover.png"}
        assert "icon" not in d
        assert "hero" not in d

    def test_to_dict_empty_produces_empty_dict(self):
        assert GameImages().to_dict() == {}

    def test_from_dict_roundtrip(self):
        gi = GameImages(cover="cover.jpg", icon="icon.png", hero="hero.jpg", logo="logo.png", header="header.png")
        restored = GameImages.from_dict(gi.to_dict())
        assert restored.cover == "cover.jpg"
        assert restored.icon == "icon.png"
        assert restored.hero == "hero.jpg"
        assert restored.logo == "logo.png"
        assert restored.header == "header.png"

    def test_from_dict_missing_fields_are_none(self):
        gi = GameImages.from_dict({})
        assert gi.cover is None
        assert gi.icon is None


# ── Game ──────────────────────────────────────────────────────────────────

class TestGame:
    def test_post_init_converts_string_game_dir_to_path(self, tmp_path):
        game = Game(folder_name="foo", game_dir=str(tmp_path))
        assert isinstance(game.game_dir, Path)

    def test_post_init_none_title_becomes_empty_string(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path, title=None)
        assert game.title == ""

    def test_cartouche_dir_property(self, tmp_path):
        from lib.models import CARTOUCHE_DIR
        game = Game(folder_name="foo", game_dir=tmp_path)
        assert game.cartouche_dir == tmp_path / CARTOUCHE_DIR

    def test_game_json_path_property(self, tmp_path):
        from lib.models import CARTOUCHE_DIR, GAME_JSON
        game = Game(folder_name="foo", game_dir=tmp_path)
        assert game.game_json_path == tmp_path / CARTOUCHE_DIR / GAME_JSON

    def test_to_dict_includes_required_fields(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path, title="Foo Game")
        d = game.to_dict()
        assert d["title"] == "Foo Game"
        assert d["schema_version"] == 2
        assert "targets" in d
        assert "savePaths" in d
        assert "images" in d

    def test_to_dict_excludes_none_steamgriddb_id(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path)
        d = game.to_dict()
        assert "steamgriddb_id" not in d

    def test_to_dict_includes_steamgriddb_id_when_set(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path, steamgriddb_id=9999)
        d = game.to_dict()
        assert d["steamgriddb_id"] == 9999

    def test_to_dict_excludes_empty_notes(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path, notes="")
        d = game.to_dict()
        assert "notes" not in d

    def test_to_dict_includes_notes_when_set(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path, notes="Some note")
        d = game.to_dict()
        assert d["notes"] == "Some note"

    def test_to_dict_roundtrip_targets(self, tmp_path):
        gt = GameTarget(os="linux", arch="x64", target="game", start_in=".")
        game = Game(folder_name="foo", game_dir=tmp_path, title="Foo", targets=[gt])
        d = game.to_dict()
        assert len(d["targets"]) == 1
        assert d["targets"][0]["os"] == "linux"

    def test_hash_uses_folder_name(self, tmp_path):
        game = Game(folder_name="foo", game_dir=tmp_path)
        assert hash(game) == hash("foo")

    def test_equality_by_folder_name(self, tmp_path):
        g1 = Game(folder_name="foo", game_dir=tmp_path)
        g2 = Game(folder_name="foo", game_dir=tmp_path / "other")
        assert g1 == g2

    def test_inequality_different_folder_name(self, tmp_path):
        g1 = Game(folder_name="foo", game_dir=tmp_path)
        g2 = Game(folder_name="bar", game_dir=tmp_path)
        assert g1 != g2


# ── GameDatabase ──────────────────────────────────────────────────────────

class TestGameDatabase:
    def test_add_and_get_by_folder(self, tmp_path):
        db = GameDatabase()
        game = Game(folder_name="foo", game_dir=tmp_path)
        db.add(game)
        assert db.get_by_folder("foo") is game

    def test_get_by_folder_returns_none_for_unknown(self):
        db = GameDatabase()
        assert db.get_by_folder("nonexistent") is None

    def test_len(self, tmp_path):
        db = GameDatabase()
        db.add(Game(folder_name="a", game_dir=tmp_path))
        db.add(Game(folder_name="b", game_dir=tmp_path))
        assert len(db) == 2

    def test_iter(self, tmp_path):
        db = GameDatabase()
        g1 = Game(folder_name="a", game_dir=tmp_path)
        g2 = Game(folder_name="b", game_dir=tmp_path)
        db.add(g1)
        db.add(g2)
        assert list(db) == [g1, g2]

    def test_incomplete_games_returns_games_without_targets(self, tmp_path):
        db = GameDatabase()
        g_no_target = Game(folder_name="a", game_dir=tmp_path)
        g_with_target = Game(folder_name="b", game_dir=tmp_path,
                             targets=[GameTarget("linux", "x64", "game", ".")])
        db.add(g_no_target)
        db.add(g_with_target)
        assert db.incomplete_games() == [g_no_target]

    def test_games_needing_enrichment_returns_games_without_sgdb_id(self, tmp_path):
        db = GameDatabase()
        g_missing = Game(folder_name="a", game_dir=tmp_path)
        g_enriched = Game(folder_name="b", game_dir=tmp_path, steamgriddb_id=1)
        db.add(g_missing)
        db.add(g_enriched)
        assert db.games_needing_enrichment() == [g_missing]

    def test_dirty_games_returns_games_with_needs_persist(self, tmp_path):
        db = GameDatabase()
        g_clean = Game(folder_name="a", game_dir=tmp_path)
        g_dirty = Game(folder_name="b", game_dir=tmp_path)
        g_dirty.needs_persist = True
        db.add(g_clean)
        db.add(g_dirty)
        assert db.dirty_games() == [g_dirty]

    def test_games_with_targets_returns_games_with_resolved_target(self, tmp_path):
        db = GameDatabase()
        g_no_resolved = Game(folder_name="a", game_dir=tmp_path)
        g_with_resolved = Game(folder_name="b", game_dir=tmp_path)
        g_with_resolved.resolved_target = "/path/to/game.x86_64"
        db.add(g_no_resolved)
        db.add(g_with_resolved)
        assert db.games_with_targets() == [g_with_resolved]
