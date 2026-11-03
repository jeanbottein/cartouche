"""
Step 6c: Add Cartouche itself to Steam as a non-Steam shortcut.

Enabled with STEAM_ADD_SELF=True (independent of STEAM_EXPOSE). The shortcut is
named ".{APP_NAME}", gets the retro artwork from lib/steam-art/, and starts in
the workspace root so the app finds its .{APP_NAME}/ config.

Launch target, first match wins:
  1. STEAM_SELF_TARGET from config
  2. the frozen binary (release builds)
  3. the retro UI build: cartouche-ui/target/release/cartouche-ui
  4. the shell launcher: cartouche.sh (cartouche.bat on Windows)

The shortcut carries its own tag ("{APP_NAME}-self"), not the ownership tag,
so steam_cleaner never treats it as a stale game. An existing entry (same tag
or same exe) keeps its appid, so artwork and playtime stay attached.
"""

import filecmp
import logging
import os
import shutil
import sys
from pathlib import Path

from .app import APP_NAME, get_script_dir
from .steam_cleaner import load_shortcuts, save_shortcuts, _get_appname
from .steam_exporter import generate_appid, _signed32, _next_index, _make_shortcut_entry, _get_grid_dir

logger = logging.getLogger(f"{APP_NAME}.steam_self")

SELF_TAG = f"{APP_NAME}-self"
SELF_NAME = f".{APP_NAME}"

# lib/steam-art/<file> -> Steam grid suffix
_ART_FILES = {
    "cover.png": "p.png",
    "wide.png":  ".png",
    "hero.png":  "_hero.png",
}


def _bundled(rel: str) -> Path:
    """Path of a file shipped in lib/ (PyInstaller bundle first, then source tree)."""
    if getattr(sys, 'frozen', False):
        bundled = Path(sys._MEIPASS) / rel
        if bundled.exists():
            return bundled
    return get_script_dir() / rel


def resolve_self_target(cfg: dict) -> str:
    """Pick the executable the shortcut launches (see module docstring)."""
    override = cfg.get("STEAM_SELF_TARGET", "").strip()
    if override:
        return str(Path(override).expanduser().resolve())
    if getattr(sys, 'frozen', False):
        return str(Path(sys.executable).resolve())

    script_dir = get_script_dir()
    exe_suffix = ".exe" if sys.platform.startswith("win") else ""
    retro_ui = script_dir / "cartouche-ui" / "target" / "release" / f"cartouche-ui{exe_suffix}"
    if retro_ui.is_file():
        return str(retro_ui)
    launcher = "cartouche.bat" if sys.platform.startswith("win") else "cartouche.sh"
    return str(script_dir / launcher)


def _find_existing(shortcuts: dict, exe: str) -> str | None:
    for key, shortcut in shortcuts.items():
        tags = shortcut.get("tags", {})
        if isinstance(tags, dict) and SELF_TAG in tags.values():
            return key
    for key, shortcut in shortcuts.items():
        if shortcut.get("Exe", shortcut.get("exe", "")).strip('"') == exe:
            return key
    return None


def _install_art(grid_dir: str, appid: int) -> str:
    """Copy artwork into the grid folder (refreshing changed files). Returns the icon path."""
    os.makedirs(grid_dir, exist_ok=True)
    pairs = [(_bundled(f"lib/steam-art/{src}"), f"{appid}{suffix}") for src, suffix in _ART_FILES.items()]
    pairs.append((_bundled("lib/app-icon.png"), f"{appid}_icon.png"))

    for src, name in pairs:
        dest = os.path.join(grid_dir, name)
        if not src.is_file():
            logger.warning(f"    Missing artwork {src}")
            continue
        if os.path.isfile(dest) and filecmp.cmp(src, dest, shallow=False):
            continue
        try:
            shutil.copyfile(src, dest)
        except OSError as e:
            logger.warning(f"    Failed to copy artwork {src} -> {dest}: {e}")
    return os.path.join(grid_dir, f"{appid}_icon.png")


def _register_in_config_dir(config_dir: str, exe: str, start_dir: str) -> bool:
    """Add or refresh the self shortcut for one Steam user. Returns True if shortcuts.vdf changed."""
    shortcuts_path = os.path.join(config_dir, "shortcuts.vdf")
    shortcuts = load_shortcuts(shortcuts_path)
    key = _find_existing(shortcuts, exe)

    if key is not None:
        existing = shortcuts[key]
        appid = existing.get("appid", 0) & 0xFFFFFFFF or generate_appid(SELF_NAME, exe)
    else:
        existing = None
        appid = generate_appid(SELF_NAME, exe)
        key = _next_index(shortcuts)

    icon_path = _install_art(_get_grid_dir(config_dir), appid)

    entry = _make_shortcut_entry(SELF_NAME, exe, start_dir, icon_path=icon_path)
    entry["appid"] = _signed32(appid)
    entry["tags"] = {"0": SELF_TAG}
    if existing is not None:
        entry["LastPlayTime"] = existing.get("LastPlayTime", 0)
        unchanged = (
            _get_appname(existing) == SELF_NAME
            and existing.get("Exe", existing.get("exe", "")) == entry["Exe"]
            and existing.get("StartDir", "") == entry["StartDir"]
            and existing.get("icon", "") == icon_path
            and existing.get("tags", {}) == entry["tags"]
        )
        if unchanged:
            return False

    shortcuts[key] = entry
    save_shortcuts(shortcuts_path, shortcuts)
    uid = os.path.basename(os.path.dirname(config_dir))
    logger.info(f"  Steam user {uid}: {'updated' if existing is not None else 'added'} {SELF_NAME} -> {exe}")
    return True


def register(cfg: dict) -> None:
    """Add/refresh the Cartouche shortcut in Steam when STEAM_ADD_SELF=True."""
    if cfg.get("STEAM_ADD_SELF", "False").lower() != "true":
        return

    from .steam_helpers import select_steam_config_dirs

    config_dirs = select_steam_config_dirs(cfg)
    if not config_dirs:
        return

    exe = resolve_self_target(cfg)
    if not os.path.isfile(exe):
        logger.warning(f"{SELF_NAME} shortcut skipped: target not found ({exe})")
        return

    config_path = cfg.get("_CONFIG_PATH")
    start_dir = str(Path(config_path).parent.parent) if config_path else os.path.dirname(exe)

    changed = [d for d in config_dirs if _register_in_config_dir(d, exe, start_dir)]
    if not changed:
        logger.info(f"{SELF_NAME} shortcut already up to date")
