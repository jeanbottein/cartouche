"""Unit tests for lib/steam_self.py — self shortcut registration on a fake Steam userdata dir."""
import os

import pytest

from lib import steam_self
from lib.steam_cleaner import load_shortcuts, save_shortcuts, _has_ownership_tag
from lib.steam_exporter import generate_appid, _signed32


@pytest.fixture
def steam(tmp_path):
    """A fake userdata/<uid>/config dir plus an executable to point at."""
    config_dir = tmp_path / "userdata" / "123" / "config"
    config_dir.mkdir(parents=True)
    exe = tmp_path / "cartouche-ui"
    exe.write_text("#!/bin/sh\n")
    return config_dir, str(exe), str(tmp_path / "workspace")


def _shortcuts(config_dir):
    return load_shortcuts(str(config_dir / "shortcuts.vdf"))


class TestRegisterInConfigDir:
    def test_adds_tagged_shortcut_with_art(self, steam):
        config_dir, exe, start = steam
        assert steam_self._register_in_config_dir(str(config_dir), exe, start)

        (entry,) = _shortcuts(config_dir).values()
        appid = generate_appid(steam_self.SELF_NAME, exe)
        assert entry["AppName"] == steam_self.SELF_NAME
        assert entry["Exe"] == f'"{exe}"'
        assert entry["StartDir"] == f'"{start}"'
        assert entry["appid"] == _signed32(appid)
        assert entry["tags"] == {"0": steam_self.SELF_TAG}

        grid = config_dir / "grid"
        for name in (f"{appid}p.png", f"{appid}.png", f"{appid}_hero.png", f"{appid}_icon.png"):
            assert (grid / name).is_file(), name

    def test_not_treated_as_owned_game(self, steam):
        config_dir, exe, start = steam
        steam_self._register_in_config_dir(str(config_dir), exe, start)
        (entry,) = _shortcuts(config_dir).values()
        assert not _has_ownership_tag(entry)  # steam_cleaner must never remove it

    def test_second_run_is_noop(self, steam):
        config_dir, exe, start = steam
        steam_self._register_in_config_dir(str(config_dir), exe, start)
        assert not steam_self._register_in_config_dir(str(config_dir), exe, start)

    def test_existing_untagged_entry_keeps_appid(self, steam):
        config_dir, exe, start = steam
        save_shortcuts(str(config_dir / "shortcuts.vdf"), {
            "0": {"appname": "Other", "exe": '"/usr/bin/other"', "appid": -5, "tags": {}},
            "1": {"AppName": "Cartouche", "Exe": f'"{exe}"', "appid": _signed32(2575556104),
                  "LastPlayTime": 42, "tags": {}},
        })
        assert steam_self._register_in_config_dir(str(config_dir), exe, start)

        shortcuts = _shortcuts(config_dir)
        assert len(shortcuts) == 2
        assert shortcuts["0"]["appname"] == "Other"
        assert shortcuts["1"]["AppName"] == steam_self.SELF_NAME
        assert shortcuts["1"]["appid"] == _signed32(2575556104)
        assert shortcuts["1"]["LastPlayTime"] == 42
        assert (config_dir / "grid" / "2575556104p.png").is_file()


class TestRegister:
    def test_disabled_by_default(self, steam, monkeypatch):
        called = []
        monkeypatch.setattr(steam_self, "_register_in_config_dir", lambda *a: called.append(a))
        steam_self.register({})
        assert called == []

    def test_uses_override_target_and_workspace_root(self, steam, monkeypatch):
        config_dir, exe, _ = steam
        monkeypatch.setattr("lib.steam_helpers.find_steam_userdata_dirs", lambda: [str(config_dir)])
        cfg = {
            "STEAM_ADD_SELF": "True",
            "STEAM_SELF_TARGET": exe,
            "_CONFIG_PATH": "/ws/.cartouche/config.txt",
        }
        steam_self.register(cfg)
        (entry,) = _shortcuts(config_dir).values()
        assert entry["Exe"] == f'"{exe}"'
        assert entry["StartDir"] == '"/ws"'

    def test_missing_target_skips(self, steam, monkeypatch):
        config_dir, _, _ = steam
        monkeypatch.setattr("lib.steam_helpers.find_steam_userdata_dirs", lambda: [str(config_dir)])
        steam_self.register({"STEAM_ADD_SELF": "True", "STEAM_SELF_TARGET": "/nope/missing"})
        assert not os.path.exists(config_dir / "shortcuts.vdf")
