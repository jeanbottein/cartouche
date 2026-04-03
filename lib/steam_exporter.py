"""
Step 6: Export games to Steam as non-Steam shortcuts.

Creates or updates Steam shortcuts for all games in the database,
copies artwork from .cartouche/ to Steam's grid directory.
"""

import logging
import os
import shutil
import zlib

from .models import GameDatabase
from . import steam_compat
from .steam_cleaner import (
    find_steam_userdata_dirs, load_shortcuts, save_shortcuts,
    _has_ownership_tag, _get_appname,
)

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.steam_exporter")

OWNERSHIP_TAG = APP_NAME


# ── AppID generation ─────────────────────────────────────────────────────

def generate_appid(app_name: str, exe_path: str) -> int:
    """Generate a stable non-Steam shortcut appid (unsigned 32-bit)."""
    unique = (str(exe_path or "") + str(app_name or "")).encode('utf-8')
    crc = zlib.crc32(unique) & 0xFFFFFFFF
    return (crc | 0x80000000) & 0xFFFFFFFF


def _signed32(val: int) -> int:
    """Convert an unsigned 32-bit int to a signed 32-bit int."""
    return val - 0x100000000 if val >= 0x80000000 else val


# ── Shortcut helpers ─────────────────────────────────────────────────────

def _next_index(shortcuts_dict: dict) -> str:
    indices = [int(k) for k in shortcuts_dict if k.isdigit()]
    return str(max(indices) + 1) if indices else "0"


def _make_shortcut_entry(app_name: str, exe_path: str, start_dir: str,
                          launch_options: str = "", icon_path: str = "") -> dict:
    """Build a shortcut dict entry in the format Steam expects."""
    appid = generate_appid(app_name, exe_path)
    return {
        "appid":              _signed32(appid),
        "AppName":            app_name,
        "Exe":                f'"{exe_path}"',
        "StartDir":           f'"{start_dir}"',
        "icon":               icon_path,
        "ShortcutPath":       "",
        "LaunchOptions":      launch_options,
        "IsHidden":           0,
        "AllowDesktopConfig": 1,
        "AllowOverlay":       1,
        "OpenVR":             0,
        "Devkit":             0,
        "DevkitGameID":       "",
        "DevkitOverrideAppID": 0,
        "LastPlayTime":       0,
        "tags":               {"0": OWNERSHIP_TAG},
    }


def _get_grid_dir(config_dir: str) -> str:
    """Return the grid folder path for storing artwork images."""
    return os.path.join(os.path.dirname(config_dir), "config", "grid")


# ── Artwork copying ──────────────────────────────────────────────────────

def _copy_artwork_item(src: str, dest: str) -> None:
    if os.path.isfile(dest):
        return
    try:
        shutil.copy2(src, dest)
    except OSError as e:
        logger.warning(f"    Failed to copy artwork {src} -> {dest}: {e}")


def _copy_artwork_to_grid(game, grid_dir: str, appid: int) -> None:
    """Copy artwork from .cartouche/ to Steam's grid directory."""
    os.makedirs(grid_dir, exist_ok=True)
    cartouche_dir = str(game.cartouche_dir)

    field_prefixes = {
        "cover": f"{appid}p",
        "hero":  f"{appid}_hero",
        "logo":  f"{appid}_logo",
        "icon":  f"{appid}_icon",
    }
    for field_name, prefix in field_prefixes.items():
        filename = getattr(game.images, field_name)
        if not filename:
            continue
        src = os.path.join(cartouche_dir, filename)
        if not os.path.isfile(src):
            continue
        _, ext = os.path.splitext(filename)
        _copy_artwork_item(src, os.path.join(grid_dir, f"{prefix}{ext}"))

    # Also copy cover as horizontal grid image (no suffix)
    cover = game.images.cover
    if cover:
        src = os.path.join(cartouche_dir, cover)
        if os.path.isfile(src):
            _, ext = os.path.splitext(cover)
            _copy_artwork_item(src, os.path.join(grid_dir, f"{appid}{ext}"))


# ── Per-user export ──────────────────────────────────────────────────────

def _build_owned_exe_map(shortcuts: dict) -> dict:
    return {
        shortcut.get("Exe", shortcut.get("exe", "")).strip('"'): key
        for key, shortcut in shortcuts.items()
        if _has_ownership_tag(shortcut)
    }


def _resolve_icon_path(game, grid_dir: str, appid: int) -> str:
    if not game.images.icon:
        return ""
    _, ext = os.path.splitext(game.images.icon)
    return os.path.join(grid_dir, f"{appid}_icon{ext}")


def _export_to_config_dir(config_dir: str, games: list, game_appids: dict) -> tuple[int, int]:
    """Export shortcuts to a single Steam user directory. Returns (added, updated)."""
    shortcuts_path = os.path.join(config_dir, "shortcuts.vdf")
    grid_dir       = _get_grid_dir(config_dir)
    shortcuts      = load_shortcuts(shortcuts_path)
    owned_exes     = _build_owned_exe_map(shortcuts)

    added = updated = 0

    for game in games:
        target      = game.resolved_target
        start_in    = game.resolved_start_in or os.path.dirname(target)
        launch_opts = game.resolved_launch_options
        name        = game.title
        appid       = game_appids[game]
        icon_path   = _resolve_icon_path(game, grid_dir, appid)

        if target in owned_exes:
            key = owned_exes[target]
            existing = shortcuts[key]
            if _get_appname(existing) != name or existing.get("icon", "") != icon_path:
                shortcuts[key] = _make_shortcut_entry(name, target, start_in, launch_opts, icon_path)
                updated += 1
        else:
            idx = _next_index(shortcuts)
            shortcuts[idx] = _make_shortcut_entry(name, target, start_in, launch_opts, icon_path)
            owned_exes[target] = idx
            added += 1

        _copy_artwork_to_grid(game, grid_dir, appid)

    if added or updated:
        save_shortcuts(shortcuts_path, shortcuts)
        uid = os.path.basename(os.path.dirname(config_dir))
        logger.info(f"  Steam user {uid}: +{added} added, ~{updated} updated")

    return added, updated


# ── Main entry point ─────────────────────────────────────────────────────

def export(db: GameDatabase, cfg: dict) -> None:
    """Create or update Steam shortcuts for all games with resolved targets."""
    if cfg.get("STEAM_EXPOSE", "False").lower() != "true":
        return

    config_dirs = find_steam_userdata_dirs()
    if not config_dirs:
        logger.warning("No Steam userdata directories found")
        return

    steam_userid = cfg.get("STEAM_USERID", "").strip()
    if steam_userid:
        config_dirs = [d for d in config_dirs if f"/{steam_userid}/" in d]
        if not config_dirs:
            logger.warning(f"STEAM_USERID={steam_userid} not found")
            return

    games = db.games_with_targets()
    if not games:
        return

    game_appids    = {g: generate_appid(g.title, g.resolved_target) for g in games}
    total_added    = total_updated = 0

    for config_dir in config_dirs:
        added, updated = _export_to_config_dir(config_dir, games, game_appids)
        total_added   += added
        total_updated += updated

    if total_added or total_updated:
        logger.info(f"Steam export complete: {total_added} added, {total_updated} updated")
    else:
        logger.info("Steam shortcuts already up to date")

    compat_tool    = cfg.get("PROTON_VERSION", "proton_experimental").strip()
    windows_appids = [game_appids[g] for g in games if g.resolved_target_os == "windows"]
    if windows_appids:
        for config_dir in config_dirs:
            steam_compat.set_compat_tools(windows_appids, compat_tool, config_dir)
