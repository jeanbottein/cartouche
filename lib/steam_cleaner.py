"""
Step 5: Remove stale Steam shortcuts.

Loads Steam's shortcuts.vdf and removes shortcuts tagged with
"cartouche" or "gamer-sidekick" (backward compat) whose executable
is no longer in the game database.
"""

import logging
import os

import vdf

from .models import GameDatabase

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.steam_cleaner")

OWNERSHIP_TAGS = {APP_NAME, "cartouche", "gamer-sidekick"}


def _has_ownership_tag(shortcut):
    """Return True if this shortcut was created by cartouche (or gamer-sidekick)."""
    tags = shortcut.get("tags", {})
    if isinstance(tags, dict):
        return bool(OWNERSHIP_TAGS & set(tags.values()))
    return False


def _get_appname(shortcut):
    return shortcut.get("AppName") or shortcut.get("appname") or ""


def _get_shortcuts_path(config_dir):
    return os.path.join(config_dir, "shortcuts.vdf")


def _config_dirs_from_base(base: str, seen: set) -> list[str]:
    if not os.path.isdir(base):
        return []
    results = []
    for uid in os.listdir(base):
        config_dir = os.path.join(base, uid, "config")
        if not os.path.isdir(config_dir):
            continue
        real_dir = os.path.realpath(config_dir)
        if real_dir not in seen:
            seen.add(real_dir)
            results.append(config_dir)
    return results


def find_steam_userdata_dirs() -> list[str]:
    """Return a list of all Steam userdata/<id>/config directories found."""
    bases = [
        os.path.expanduser("~/.steam/steam/userdata"),
        os.path.expanduser("~/.local/share/Steam/userdata"),
    ]
    seen = set()
    return [d for base in bases for d in _config_dirs_from_base(base, seen)]


def load_shortcuts(shortcuts_path):
    """Load the shortcuts.vdf file, returning the inner dict."""
    if not os.path.exists(shortcuts_path):
        return {}
    with open(shortcuts_path, 'rb') as f:
        data = vdf.binary_load(f)
    return data.get("shortcuts", {})


def save_shortcuts(shortcuts_path, shortcuts_dict):
    """Write the shortcuts dict back to shortcuts.vdf."""
    os.makedirs(os.path.dirname(shortcuts_path), exist_ok=True)
    with open(shortcuts_path, 'wb') as f:
        vdf.binary_dump({"shortcuts": shortcuts_dict}, f)


def _reindex(shortcuts_dict):
    """Reindex shortcuts to keep keys contiguous ("0", "1", "2", ...)."""
    sorted_by_index = sorted(
        shortcuts_dict.items(),
        key=lambda item: int(item[0]) if item[0].isdigit() else 0,
    )
    return {str(i): value for i, (_, value) in enumerate(sorted_by_index)}


def _find_stale_keys(shortcuts: dict, valid_targets: set) -> list[str]:
    """Identify shortcut keys whose executables are no longer in the game database."""
    stale = []
    for key, shortcut in shortcuts.items():
        if not _has_ownership_tag(shortcut):
            continue
        exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
        if exe not in valid_targets:
            stale.append(key)
    return stale


def _remove_stale_shortcuts(config_dir: str, valid_targets: set) -> None:
    """Remove stale shortcuts from a single Steam user directory."""
    shortcuts_path = _get_shortcuts_path(config_dir)
    shortcuts = load_shortcuts(shortcuts_path)
    if not shortcuts:
        return

    stale_keys = _find_stale_keys(shortcuts, valid_targets)
    if not stale_keys:
        return

    for key in stale_keys:
        logger.info(f"  Removing stale shortcut: {_get_appname(shortcuts[key])}")
        del shortcuts[key]

    shortcuts = _reindex(shortcuts)
    save_shortcuts(shortcuts_path, shortcuts)
    uid = os.path.basename(os.path.dirname(config_dir))
    logger.info(f"  Steam user {uid}: removed {len(stale_keys)} stale shortcut(s)")


def clean(db: GameDatabase, cfg: dict):
    """
    Remove stale shortcuts (tagged cartouche/gamer-sidekick)
    whose EXE path is no longer in the game database.
    """
    from .steam_helpers import resolve_steam_config_dirs

    config_dirs = resolve_steam_config_dirs(cfg)
    if not config_dirs:
        return

    valid_targets = {g.resolved_target for g in db.games_with_targets()}
    for config_dir in config_dirs:
        _remove_stale_shortcuts(config_dir, valid_targets)
