"""
Set Proton compatibility tools for Windows games in Steam's config.vdf.

When a non-Steam shortcut points to a Windows executable, Steam needs to
know which compatibility tool (Proton) to use. This module writes those
mappings into Steam's text-format config.vdf file.
"""

import logging
import os

try:
    import vdf
except ImportError:
    vdf = None

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.steam_compat")

_COMPAT_KEY_PATH = ("InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping")
_DEFAULT_PRIORITY = "250"


def _find_config_vdf(steam_root: str) -> str | None:
    path = os.path.join(steam_root, "config", "config.vdf")
    return path if os.path.isfile(path) else None


def _steam_root_from_userdata_config(config_dir: str) -> str | None:
    """Derive Steam root from a userdata/<id>/config path."""
    steam_root = os.path.dirname(os.path.dirname(os.path.dirname(config_dir)))
    if os.path.isdir(os.path.join(steam_root, "config")):
        return steam_root
    return None


def set_compat_tools(windows_appids: list, compat_tool: str, config_dir: str) -> int:
    """
    Add Proton compat tool mappings for the given appids.

    Args:
        windows_appids: List of unsigned 32-bit appids for Windows games.
        compat_tool: Proton tool name (e.g. "proton_experimental").
        config_dir: A Steam userdata/<id>/config directory path.

    Returns:
        Number of new mappings added.
    """
    if not vdf:
        logger.warning("vdf package not installed, skipping Proton compat setup (pip install vdf)")
        return 0

    if not windows_appids:
        return 0

    steam_root = _steam_root_from_userdata_config(config_dir)
    if not steam_root:
        logger.warning(f"Could not determine Steam root from {config_dir}")
        return 0

    config_vdf_path = _find_config_vdf(steam_root)
    if not config_vdf_path:
        logger.warning(f"config.vdf not found at {steam_root}/config/")
        return 0

    try:
        with open(config_vdf_path, "r", encoding="utf-8") as f:
            data = vdf.load(f)
    except Exception as e:
        logger.error(f"Failed to read {config_vdf_path}: {e}")
        return 0

    root = data
    for key in _COMPAT_KEY_PATH:
        root = root.setdefault(key, {})

    added = 0
    for appid in windows_appids:
        appid_str = str(appid)
        if appid_str not in root:
            root[appid_str] = {"name": compat_tool, "config": "", "priority": _DEFAULT_PRIORITY}
            added += 1

    if added:
        try:
            with open(config_vdf_path, "w", encoding="utf-8") as f:
                vdf.dump(data, f, pretty=True)
            logger.info(f"Set {compat_tool} for {added} Windows game(s) in config.vdf")
        except Exception as e:
            logger.error(f"Failed to write {config_vdf_path}: {e}")
            return 0

    return added
