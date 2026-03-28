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


def find_steam_userdata_dirs():
    """Return a list of all Steam userdata/<id>/config directories found."""
    candidates = [
        os.path.expanduser("~/.steam/steam/userdata"),
        os.path.expanduser("~/.local/share/Steam/userdata"),
    ]
    results = []
    seen = set()
    for base in candidates:
        if not os.path.isdir(base):
            continue
        for uid in os.listdir(base):
            config_dir = os.path.join(base, uid, "config")
            if os.path.isdir(config_dir):
                real_dir = os.path.realpath(config_dir)
                if real_dir not in seen:
                    seen.add(real_dir)
                    results.append(config_dir)
    return results


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
    reindexed = {}
    for i, (_, v) in enumerate(sorted(shortcuts_dict.items(), key=lambda x: int(x[0]) if x[0].isdigit() else 0)):
        reindexed[str(i)] = v
    return reindexed


def clean(db: GameDatabase, cfg: dict):
    """
    Remove stale shortcuts (tagged cartouche/gamer-sidekick)
    whose EXE path is no longer in the game database.
    """
    if cfg.get("STEAM_EXPOSE", "False").lower() != "true":
        return

    config_dirs = find_steam_userdata_dirs()
    if not config_dirs:
        return

    steam_userid = cfg.get("STEAM_USERID", "").strip()
    if steam_userid:
        config_dirs = [d for d in config_dirs if f"/{steam_userid}/" in d]
        if not config_dirs:
            logger.warning(f"STEAM_USERID={steam_userid} not found")
            return

    # Build set of valid exe paths from database
    valid_targets = {g.resolved_target for g in db.games_with_targets()}

    for config_dir in config_dirs:
        shortcuts_path = _get_shortcuts_path(config_dir)
        shortcuts = load_shortcuts(shortcuts_path)
        if not shortcuts:
            continue

        keys_to_remove = []
        for key, shortcut in shortcuts.items():
            if not _has_ownership_tag(shortcut):
                continue
            exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
            if exe not in valid_targets:
                keys_to_remove.append(key)
                logger.info(f"  Removing stale shortcut: {_get_appname(shortcut)}")

        if keys_to_remove:
            for key in keys_to_remove:
                del shortcuts[key]
            shortcuts = _reindex(shortcuts)
            save_shortcuts(shortcuts_path, shortcuts)
            uid = os.path.basename(os.path.dirname(config_dir))
            logger.info(f"  Steam user {uid}: removed {len(keys_to_remove)} stale shortcut(s)")
