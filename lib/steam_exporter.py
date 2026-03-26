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
from . import steam_vdf
from . import steam_compat
from .steam_cleaner import (
    find_steam_userdata_dirs, load_shortcuts, save_shortcuts,
    _has_ownership_tag, _get_appname,
)

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.steam_exporter")

OWNERSHIP_TAG = APP_NAME


# ── AppID generation ─────────────────────────────────────────────────────

def generate_appid(app_name, exe_path):
    """Generate a stable non-Steam shortcut appid (unsigned 32-bit)."""
    unique = (exe_path + app_name).encode('utf-8')
    crc = zlib.crc32(unique) & 0xFFFFFFFF
    return (crc | 0x80000000) & 0xFFFFFFFF


def _signed32(val):
    """Convert an unsigned 32-bit int to a signed 32-bit int."""
    if val >= 0x80000000:
        return val - 0x100000000
    return val


# ── Shortcut helpers ─────────────────────────────────────────────────────

def _next_index(shortcuts_dict):
    if not shortcuts_dict:
        return "0"
    indices = [int(k) for k in shortcuts_dict if k.isdigit()]
    return str(max(indices) + 1) if indices else "0"


def _make_shortcut_entry(app_name, exe_path, start_dir, launch_options="", icon_path=""):
    """Build a shortcut dict entry in the format Steam expects."""
    appid = generate_appid(app_name, exe_path)
    tags = {"0": OWNERSHIP_TAG}
    return {
        "appid": _signed32(appid),
        "AppName": app_name,
        "Exe": f'"{exe_path}"',
        "StartDir": f'"{start_dir}"',
        "icon": icon_path,
        "ShortcutPath": "",
        "LaunchOptions": launch_options,
        "IsHidden": 0,
        "AllowDesktopConfig": 1,
        "AllowOverlay": 1,
        "OpenVR": 0,
        "Devkit": 0,
        "DevkitGameID": "",
        "DevkitOverrideAppID": 0,
        "LastPlayTime": 0,
        "tags": tags,
    }


def _get_grid_dir(config_dir):
    """Return the grid folder path for storing artwork images."""
    return os.path.join(os.path.dirname(config_dir), "config", "grid")


def _copy_artwork_to_grid(game, grid_dir, appid):
    """Copy artwork from .cartouche/ to Steam's grid directory."""
    os.makedirs(grid_dir, exist_ok=True)
    cartouche_dir = str(game.cartouche_dir)

    name_map = {
        "cover": f"{appid}p",       # poster format
        "hero": f"{appid}_hero",
        "logo": f"{appid}_logo",
        "icon": f"{appid}_icon",
    }

    for field_name, prefix in name_map.items():
        filename = getattr(game.images, field_name)
        if not filename:
            continue
        src = os.path.join(cartouche_dir, filename)
        if not os.path.isfile(src):
            continue
        _, ext = os.path.splitext(filename)
        dest = os.path.join(grid_dir, f"{prefix}{ext}")
        # Skip if already exists
        if os.path.isfile(dest):
            continue
        try:
            shutil.copy2(src, dest)
        except OSError as e:
            logger.warning(f"    Failed to copy artwork {src} -> {dest}: {e}")

    # Also copy cover as the grid image (non-poster format)
    cover = game.images.cover
    if cover:
        src = os.path.join(cartouche_dir, cover)
        if os.path.isfile(src):
            _, ext = os.path.splitext(cover)
            dest = os.path.join(grid_dir, f"{appid}{ext}")
            if not os.path.isfile(dest):
                try:
                    shutil.copy2(src, dest)
                except OSError:
                    pass


# ── Main entry point ─────────────────────────────────────────────────────

def export(db: GameDatabase, cfg: dict):
    """
    Create or update Steam shortcuts for all games with resolved targets.
    Copies artwork from .cartouche/ to Steam's grid directory.
    """
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

    total_added = 0
    total_updated = 0

    for config_dir in config_dirs:
        shortcuts_path = os.path.join(config_dir, "shortcuts.vdf")
        grid_dir = _get_grid_dir(config_dir)
        shortcuts = load_shortcuts(shortcuts_path)

        # Build map of existing owned shortcuts by exe path
        owned_exes = {}
        for key, shortcut in shortcuts.items():
            if _has_ownership_tag(shortcut):
                exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
                owned_exes[exe] = key

        added = 0
        updated = 0

        for game in games:
            target = game.resolved_target
            start_in = game.resolved_start_in or os.path.dirname(target)
            launch_opts = game.resolved_launch_options
            name = game.title

            # Build icon path
            icon_path = ""
            if game.images.icon:
                appid = generate_appid(name, target)
                _, ext = os.path.splitext(game.images.icon)
                icon_path = os.path.join(grid_dir, f"{appid}_icon{ext}")

            if target in owned_exes:
                # Update existing shortcut if name/icon changed
                key = owned_exes[target]
                existing = shortcuts[key]
                name_changed = _get_appname(existing) != name
                icon_changed = existing.get("icon", "") != icon_path

                if name_changed or icon_changed:
                    shortcuts[key] = _make_shortcut_entry(name, target, start_in, launch_opts, icon_path)
                    updated += 1
            else:
                # Add new shortcut
                idx = _next_index(shortcuts)
                shortcuts[idx] = _make_shortcut_entry(name, target, start_in, launch_opts, icon_path)
                owned_exes[target] = idx
                added += 1

            # Copy artwork
            appid = generate_appid(name, target)
            _copy_artwork_to_grid(game, grid_dir, appid)

        if added or updated:
            save_shortcuts(shortcuts_path, shortcuts)
            uid = os.path.basename(os.path.dirname(config_dir))
            logger.info(f"  Steam user {uid}: +{added} added, ~{updated} updated")

        total_added += added
        total_updated += updated

    if total_added or total_updated:
        logger.info(f"Steam export complete: {total_added} added, {total_updated} updated")
    else:
        logger.info("Steam shortcuts already up to date")

    # Set Proton compat tools for Windows games
    compat_tool = cfg.get("PROTON_VERSION", "proton_experimental").strip()
    windows_appids = [
        generate_appid(g.title, g.resolved_target)
        for g in games if g.resolved_target_os == "windows"
    ]
    if windows_appids:
        for config_dir in config_dirs:
            steam_compat.set_compat_tools(windows_appids, compat_tool, config_dir)
