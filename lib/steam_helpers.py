"""
Shared Steam helper utilities.

Centralizes Steam config validation (STEAM_EXPOSE check, userdata directory
discovery, STEAM_USERID filtering) and Proton path constants used across
steam_cleaner, steam_exporter, scanner, and detector modules.
"""

import logging
import os

from .app import APP_NAME
from .steam_cleaner import find_steam_userdata_dirs

logger = logging.getLogger(f"{APP_NAME}.steam_helpers")

PROTON_PREFIX_TEMPLATE = "~/.local/share/Steam/steamapps/compatdata/{appid}/pfx/drive_c"


def resolve_steam_config_dirs(cfg: dict) -> list[str]:
    """Resolve Steam userdata config directories from config, applying STEAM_EXPOSE and STEAM_USERID filters.

    Returns an empty list if Steam export is disabled or no matching directories are found.
    """
    if cfg.get("STEAM_EXPOSE", "False").lower() != "true":
        return []

    config_dirs = find_steam_userdata_dirs()
    if not config_dirs:
        logger.warning("No Steam userdata directories found")
        return []

    steam_userid = cfg.get("STEAM_USERID", "").strip()
    if not steam_userid:
        return config_dirs

    filtered = [d for d in config_dirs if f"/{steam_userid}/" in d]
    if not filtered:
        logger.warning(f"STEAM_USERID={steam_userid} not found")
    return filtered
