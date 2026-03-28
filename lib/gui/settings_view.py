"""
Settings view for Cartouche.
Categorized editor for all configuration values.
"""

from __future__ import annotations

import os
import logging
from typing import Any, Callable

import dearpygui.dearpygui as dpg

from .theme import TEXT_SECONDARY, TEXT_MUTED, ACCENT, SUCCESS, ERROR

logger = logging.getLogger(__name__)

TAG_WINDOW = "settings_view_window"
TAG_STATUS = "settings_status_text"
TAG_FILE_DIALOG = "settings_file_dialog"

# Schema defines how each setting is rendered and handled.
SETTINGS_SCHEMA = [
    {
        "category": "General",
        "settings": [
            {"key": "MACHINE_NAME", "label": "Machine Name", "type": "text", "help": "Used to prefix backups."},
            {"key": "PERSIST_DATA", "label": "Persist game data", "type": "bool", "help": "Write .cartouche/ folders in game dirs."},
        ]
    },
    {
        "category": "Paths & Directories",
        "settings": [
            {"key": "FREEGAMES_PATH", "label": "Games Path", "type": "path", "help": "Where your DRM-free games are stored."},
            {"key": "PATCHES_PATH", "label": "Patches Path", "type": "path", "help": "Where mods/patches are stored."},
        ]
    },
    {
        "category": "Save Backup & Sync",
        "settings": [
            {"key": "SAVESCOPY_PATH", "label": "Backup Path", "type": "path", "help": "Where save backups are stored."},
            {"key": "SAVESCOPY_STRATEGY", "label": "Strategy", "type": "choice", "options": ["backup", "sync", "restore"], "help": "How to handle save files."},
            {"key": "SAVESLINK_PATH", "label": "Sync Symlinks Path", "type": "path", "help": "Folder for symlinks (Syncthing)."},
        ]
    },
    {
        "category": "Steam & Integration",
        "settings": [
            {"key": "STEAM_EXPOSE", "label": "Add to Steam", "type": "bool", "help": "Automatically add games as non-Steam shortcuts."},
            {"key": "STEAM_USERID", "label": "Steam User ID", "type": "text", "help": "Optional: target specific user ID."},
            {"key": "PROTON_VERSION", "label": "Proton Version", "type": "text", "help": "Tool for Windows games (e.g. proton_experimental)."},
            {"key": "STEAMGRIDDB_API_KEY", "label": "SteamGridDB API Key", "type": "password", "help": "For automatic artwork (steamgriddb.com)."},
            {"key": "MANIFEST_EXPORT", "label": "Export Manifests", "type": "bool", "help": "Generate manifests.json."},
            {"key": "MANIFEST_PATH", "label": "Manifest File Path", "type": "path", "help": "Custom path for manifests.json."},
        ]
    },
    {
        "category": "Emulators",
        "settings": [
            {"key": "DOLPHIN_GFX_BACKEND", "label": "Dolphin GFX", "type": "choice", "options": ["OGL", "Vulkan"], "help": "Backend for Dolphin."},
            {"key": "DOLPHIN_FULLSCREEN", "label": "Dolphin Fullscreen", "type": "bool"},
            {"key": "RYUJINX_SYSTEM_LANGUAGE", "label": "RyuJinx Language", "type": "text"},
            {"key": "RETROARCH_NETPLAY_NICKNAME", "label": "RetroArch Nickname", "type": "text"},
        ]
    }
]

_config_path: str = ""
_current_cfg: dict = {}
_on_saved_cb: Callable[[], None] | None = None
_pending_path_tag: str | None = None


def create(cfg: dict, on_saved: Callable[[], None] | None = None) -> None:
    """Build the settings view."""
    global _config_path, _current_cfg, _on_saved_cb
    _current_cfg = cfg
    _on_saved_cb = on_saved
    _config_path = cfg.get("_CONFIG_PATH", "")

    if dpg.does_item_exist(TAG_WINDOW):
        dpg.delete_item(TAG_WINDOW)

    with dpg.window(
        label="Settings",
        tag=TAG_WINDOW,
        no_title_bar=True,
        no_move=True,
        no_resize=True,
        no_close=True,
        no_collapse=True,
        show=False,
    ):
        with dpg.child_window(border=False):
            dpg.add_text("Settings", color=TEXT_SECONDARY)
            dpg.add_separator()
            dpg.add_spacer(height=8)

            if not _config_path:
                dpg.add_text("Error: Config path not found.", color=ERROR)
                return

            dpg.add_text(f"Editing: {_config_path}", color=TEXT_MUTED)
            dpg.add_spacer(height=12)

            # 1. Main Categories
            for cat in SETTINGS_SCHEMA:
                _build_category(cat["category"], cat["settings"])
                dpg.add_spacer(height=16)

            # 2. Custom/Other Settings (BACKUP_*, RUN_AFTER_*, etc.)
            other_keys = [k for k in sorted(_current_cfg.keys()) 
                         if not k.startswith("_") and not _is_schema_key(k)]
            if other_keys:
                with dpg.tree_node(label="Custom Assignments", default_open=True):
                    for key in other_keys:
                        _build_generic_setting(key, _current_cfg[key])
                dpg.add_spacer(height=16)

            dpg.add_separator()
            dpg.add_spacer(height=8)
            
            with dpg.group(horizontal=True):
                dpg.add_button(label="Save Settings", width=150, callback=_on_save_clicked)
                dpg.add_text("", tag=TAG_STATUS, color=SUCCESS)

    # Setup File Dialog (same window context)
    if not dpg.does_item_exist(TAG_FILE_DIALOG):
        with dpg.file_dialog(
            directory_selector=True,
            show=False,
            callback=_on_file_selected,
            tag=TAG_FILE_DIALOG,
            width=700,
            height=400,
        ):
            pass


def _is_schema_key(key: str) -> bool:
    for cat in SETTINGS_SCHEMA:
        for s in cat["settings"]:
            if s["key"] == key:
                return True
    return False


def _build_category(name: str, settings: list[dict]) -> None:
    """Build a category section with collapsible header."""
    with dpg.tree_node(label=name, default_open=True):
        for s in settings:
            key = s["key"]
            label = s["label"]
            stype = s["type"]
            val = _current_cfg.get(key, "")
            help_text = s.get("help", "")

            with dpg.group(horizontal=True):
                dpg.add_text(f"{label}:", color=TEXT_MUTED)
                
                tag = f"conf_{key}"
                
                if stype == "bool":
                    bool_val = str(val).lower() == "true"
                    dpg.add_checkbox(tag=tag, default_value=bool_val)
                elif stype == "choice":
                    dpg.add_combo(tag=tag, items=s["options"], default_value=val, width=200)
                elif stype == "path":
                    with dpg.group(horizontal=True):
                        dpg.add_input_text(tag=tag, default_value=val, width=400)
                        dpg.add_button(label="Browse...", callback=_on_browse_clicked, user_data=tag)
                elif stype == "password":
                    if val:
                        with dpg.group(horizontal=True, tag=f"group_{key}"):
                            dpg.add_text("<provided>", color=ACCENT)
                            dpg.add_button(label="Clear", small=True, callback=lambda: _clear_key(key))
                            # Invisible input to hold the value if not cleared
                            dpg.add_input_text(tag=tag, default_value=val, show=False)
                    else:
                        dpg.add_input_text(tag=tag, default_value="", password=True, width=300)
                else:  # text
                    dpg.add_input_text(tag=tag, default_value=str(val), width=400)

            if help_text:
                dpg.add_text(help_text, color=TEXT_MUTED, indent=188)
                dpg.add_spacer(height=4)


def _build_generic_setting(key: str, val: str) -> None:
    """Build a generic key=value row for custom settings."""
    with dpg.group(horizontal=True):
        dpg.add_text(key, color=TEXT_MUTED)
        dpg.add_input_text(tag=f"conf_{key}", default_value=str(val), width=400)


def _clear_key(key: str):
    """Clear a masked key to allow entering a new value."""
    _current_cfg[key] = ""
    # Rebuild the UI (lazy way)
    create(_current_cfg, _on_saved_cb)
    dpg.configure_item(TAG_WINDOW, show=True)


def _on_browse_clicked(sender, app_data, user_data):
    """Open the file dialog for a specific path field."""
    global _pending_path_tag
    _pending_path_tag = user_data
    dpg.show_item(TAG_FILE_DIALOG)


def _on_file_selected(sender, app_data):
    """Callback when a directory is chosen."""
    if _pending_path_tag and "file_path_name" in app_data:
        path = app_data["file_path_name"]
        dpg.set_value(_pending_path_tag, path)


def _on_save_clicked():
    """Collect all UI values and write to config.txt."""
    new_cfg = {}
    
    # 1. Collect from schema
    for cat in SETTINGS_SCHEMA:
        for s in cat["settings"]:
            key = s["key"]
            tag = f"conf_{key}"
            if dpg.does_item_exist(tag):
                val = dpg.get_value(tag)
                if s["type"] == "bool":
                    val = "True" if val else "False"
                new_cfg[key] = str(val)

    # 2. Collect from other keys
    other_keys = [k for k in sorted(_current_cfg.keys()) 
                 if not k.startswith("_") and not _is_schema_key(k)]
    for key in other_keys:
        tag = f"conf_{key}"
        if dpg.does_item_exist(tag):
            new_cfg[key] = str(dpg.get_value(tag))

    # Write to file
    try:
        _save_to_file(_config_path, new_cfg)
        dpg.set_value(TAG_STATUS, "Saved!")
        _current_cfg.update(new_cfg)
        if _on_saved_cb:
            _on_saved_cb()
    except Exception as e:
        logger.error(f"Failed to save config: {e}")
        dpg.set_value(TAG_STATUS, f"Error: {e}")
        dpg.configure_item(TAG_STATUS, color=ERROR)


def _save_to_file(path: str, updates: dict[str, str]):
    """Update config.txt while trying to preserve comments."""
    if not os.path.exists(path):
        with open(path, "w") as f:
            for k, v in updates.items():
                f.write(f"{k}={v}\n")
        return

    with open(path, "r") as f:
        lines = f.readlines()

    new_lines = []
    seen_keys = set()
    
    for line in lines:
        stripped = line.strip()
        if stripped and not stripped.startswith("#") and "=" in stripped:
            k, _ = stripped.split("=", 1)
            k = k.strip()
            if k in updates:
                new_lines.append(f"{k}={updates[k]}\n")
                seen_keys.add(k)
                continue
        new_lines.append(line)

    # Add any missing keys at the end
    for k, v in updates.items():
        if k not in seen_keys:
            new_lines.append(f"{k}={v}\n")

    with open(path, "w") as f:
        f.writelines(new_lines)
