"""
Games view -- browse and inspect the game library.

Left panel: scrollable game list.  Right panel: selected game details
with inline editing, target/save-path management, and artwork previews.
"""

from __future__ import annotations

import json
import logging
import os
import webbrowser
from typing import Callable

import dearpygui.dearpygui as dpg
from PIL import Image as PILImage

from lib import scanner, detector
from lib import enricher as _enricher
from lib import persister as _persister
from lib.models import Game, GameDatabase, GameTarget, CARTOUCHE_DIR, GAME_JSON
from lib.api_keys import get_steamgriddb_key
from .theme import TEXT_SECONDARY, TEXT_MUTED, ACCENT, SUCCESS, WARNING, ERROR

logger = logging.getLogger(__name__)

# -- Window / panel tags --------------------------------------------------
TAG_WINDOW          = "games_view_window"
TAG_GAME_LIST       = "games_list_child"
TAG_DETAIL_PANEL    = "games_detail_child"
TAG_DETAIL_TITLE    = "games_detail_title"
TAG_DETAIL_DIR      = "games_detail_dir"
TAG_IMG_GROUP       = "games_img_group"

# -- Edit field tags -------------------------------------------------------
TAG_EDIT_TITLE      = "games_edit_title_input"
TAG_EDIT_SGDB       = "games_edit_sgdb_input"
TAG_EDIT_STATUS     = "games_edit_inline_status"
TAG_TARGETS_SECTION = "games_targets_section"
TAG_SAVES_SECTION   = "games_saves_section"

# -- File dialog tags ------------------------------------------------------
TAG_FILE_DLG        = "games_file_dlg"
TAG_DIR_DLG         = "games_dir_dlg"
TAG_FETCH_STATUS    = "games_fetch_status"
TAG_IMG_DELETE_POPUP = "games_img_delete_popup"

# -- Themes -------------------------------------------------------------------
TAG_DELETE_BTN_THEME  = "games_delete_btn_theme"
TAG_ADD_BTN_THEME     = "games_add_btn_theme"
TAG_TIGHT_THEME       = "games_tight_theme"
TAG_AUTO_WINDOW_THEME = "games_auto_window_theme"
TAG_IMG_SLOT_THEME    = "games_img_slot_theme"

# -- Artwork thumbnails (fixed order: 5 slots always shown) ---------------
_ARTWORK_ORDER = ["icon", "cover", "hero", "logo", "header"]
_ARTWORK_SIZES: dict[str, tuple[int, int]] = {
    "icon":   (60,  60),
    "cover":  (120, 180),
    "hero":   (200, 75),
    "logo":   (150, 60),
    "header": (200, 93),
}

# -- Dropdown options ------------------------------------------------------
_OS_OPTIONS   = ["linux", "windows", "macos", "android", "web"]
_ARCH_OPTIONS = ["x64", "arm64"]

# -- Module state ----------------------------------------------------------
_db: GameDatabase | None = None
_selected_game: Game | None = None
_cfg: dict = {}
_texture_registry_tag = "games_tex_registry"
_loaded_textures: dict[str, int | str] = {}
_texture_sizes: dict[str, tuple[int, int]] = {}  # tex_tag -> (width, height)

# Dynamic rows: each entry is a dict of widget tags for one row
_target_row_tags: list[dict[str, str]] = []
_save_row_tags:   list[dict[str, str]] = []
_row_counter: int = 0          # ever-increasing, never reused
_pending_field_tag: str | None = None  # which input the open dialog fills
_pending_delete_field: str | None = None  # which image field the delete popup targets


# =========================================================================
# Public API
# =========================================================================

def create(cfg: dict) -> None:
    """Build the games-browser view."""
    global _cfg
    _cfg = cfg
    games_dir: str = cfg.get("FREEGAMES_PATH", "")

    # Create red delete button theme
    with dpg.theme(tag=TAG_DELETE_BTN_THEME):
        with dpg.theme_component(dpg.mvButton):
            dpg.add_theme_color(dpg.mvThemeCol_Button, ERROR)
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, (255, 120, 120, 255))
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, (180, 50, 50, 255))

    # Create green add button theme
    with dpg.theme(tag=TAG_ADD_BTN_THEME):
        with dpg.theme_component(dpg.mvButton):
            dpg.add_theme_color(dpg.mvThemeCol_Button, SUCCESS)
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, (120, 210, 150, 255))
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, (50, 150, 90, 255))

    # Zero horizontal spacing — applied to input+button sub-groups
    with dpg.theme(tag=TAG_TIGHT_THEME):
        with dpg.theme_component(dpg.mvAll):
            dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, 0, 4)
            dpg.add_theme_style(dpg.mvStyleVar_FramePadding, 5, 6)

    # Auto-window theme: group with border
    with dpg.theme(tag=TAG_AUTO_WINDOW_THEME):
        with dpg.theme_component(dpg.mvAll):
            dpg.add_theme_color(dpg.mvThemeCol_Border, (60, 65, 78, 255))
            dpg.add_theme_style(dpg.mvStyleVar_ChildBorderSize, 1)
            dpg.add_theme_style(dpg.mvStyleVar_ChildRounding, 4)

    # Image slot theme: zero padding, border
    with dpg.theme(tag=TAG_IMG_SLOT_THEME):
        with dpg.theme_component(dpg.mvAll):
            dpg.add_theme_color(dpg.mvThemeCol_Border, (60, 65, 78, 255))
            dpg.add_theme_style(dpg.mvStyleVar_ChildBorderSize, 1)
            dpg.add_theme_style(dpg.mvStyleVar_ChildRounding, 4)
            dpg.add_theme_style(dpg.mvStyleVar_WindowPadding, 0, 0)
            dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, 0, 2)

    with dpg.texture_registry(tag=_texture_registry_tag):
        pass

    with dpg.window(
        label="Games", tag=TAG_WINDOW,
        no_title_bar=True, no_move=True, no_resize=True,
        no_close=True, no_collapse=True, show=False,
    ):

        with dpg.group(horizontal=True):
            # -- Left panel: game list ------------------------------------
            with dpg.child_window(tag=TAG_GAME_LIST, width=280, height=-1, border=True):
                dpg.add_text("Loading...", color=TEXT_MUTED)

            # -- Right panel: detail + editor -----------------------------
            with dpg.child_window(tag=TAG_DETAIL_PANEL, width=-1, height=-1, border=True):
                with dpg.group(horizontal=True):
                    dpg.add_text("Select a game", tag=TAG_DETAIL_TITLE, color=ACCENT)
                    dpg.add_text("", tag=TAG_DETAIL_DIR, color=TEXT_MUTED)

                # Title + SGDB ID on one line
                with dpg.group(horizontal=True):
                    dpg.add_text("Title:", color=TEXT_MUTED)
                    dpg.add_input_text(tag=TAG_EDIT_TITLE, default_value="", width=280)
                    dpg.add_text("SteamGridDB ID:", color=TEXT_MUTED)
                    dpg.add_input_text(tag=TAG_EDIT_SGDB, default_value="",
                                       width=70, decimal=True)
                    dpg.add_button(label="Go to SteamGridDB game page",
                                   callback=_on_open_sgdb_page)

                # -- Targets ----------------------------------------------
                dpg.add_separator()
                dpg.add_text("Targets", color=TEXT_SECONDARY)
                with dpg.group(horizontal=True):
                    dpg.add_text("OS",             color=TEXT_MUTED); dpg.add_dummy(width=70)
                    dpg.add_text("Arch",           color=TEXT_MUTED); dpg.add_dummy(width=40)
                    dpg.add_text("Target exe",     color=TEXT_MUTED); dpg.add_dummy(width=180)
                    dpg.add_text("Start In",       color=TEXT_MUTED); dpg.add_dummy(width=160)
                    dpg.add_text("Opts",           color=TEXT_MUTED)
                with dpg.group(tag=TAG_TARGETS_SECTION, horizontal=False) as grp:
                    dpg.bind_item_theme(grp, TAG_AUTO_WINDOW_THEME)
                    pass
                with dpg.group(horizontal=True):
                    add_target_btn = dpg.add_button(label="+ Add Target",
                                                     callback=_on_add_target)
                    dpg.bind_item_theme(add_target_btn, TAG_ADD_BTN_THEME)
                    auto_detect_btn = dpg.add_button(label="Auto-Detect",
                                                      callback=_on_auto_detect_targets)
                    dpg.bind_item_theme(auto_detect_btn, TAG_ADD_BTN_THEME)
                    dpg.add_text("", tag=TAG_EDIT_STATUS, color=SUCCESS)

                dpg.add_separator()

                # -- Save Paths -------------------------------------------
                dpg.add_text("Save Paths", color=TEXT_SECONDARY)
                with dpg.group(horizontal=True):
                    dpg.add_text("OS",   color=TEXT_MUTED); dpg.add_dummy(width=60)
                    dpg.add_text("Path", color=TEXT_MUTED)
                with dpg.group(tag=TAG_SAVES_SECTION, horizontal=False) as grp:
                    dpg.bind_item_theme(grp, TAG_AUTO_WINDOW_THEME)
                    pass
                add_save_btn = dpg.add_button(label="+ Add Save Path",
                                              callback=_on_add_save_path)
                dpg.bind_item_theme(add_save_btn, TAG_ADD_BTN_THEME)

                dpg.add_separator()
                dpg.add_text("Images", color=TEXT_SECONDARY)
                dpg.add_group(tag=TAG_IMG_GROUP)
                with dpg.group(horizontal=True):
                    dpg.add_button(label="Fetch Images",
                                   callback=_on_fetch_images)
                    dpg.add_text("", tag=TAG_FETCH_STATUS, color=TEXT_MUTED)

                dpg.add_separator()
                # Save button
                dpg.add_separator()
                dpg.add_button(label="Save", width=80,
                               callback=_save_game_from_detail)


    # File dialog (for target exe)
    with dpg.file_dialog(
        directory_selector=False, show=False,
        callback=_on_file_selected, tag=TAG_FILE_DLG,
        width=700, height=450, file_count=1,
    ):
        dpg.add_file_extension(".*")
        dpg.add_file_extension(".exe", color=(100, 200, 100, 255))
        dpg.add_file_extension(".sh",  color=(100, 200, 100, 255))

    # Directory dialog (for start_in and save paths)
    with dpg.file_dialog(
        directory_selector=True, show=False,
        callback=_on_dir_selected, tag=TAG_DIR_DLG,
        width=700, height=450,
    ):
        pass

    # Image delete confirmation popup
    with dpg.window(
        label="Delete Image", tag=TAG_IMG_DELETE_POPUP,
        modal=True, show=False, no_resize=True, no_move=True,
        width=300, height=180, no_collapse=True,
    ):
        dpg.add_text("Remove this image?", tag=f"{TAG_IMG_DELETE_POPUP}_text")
        dpg.add_separator()
        dpg.add_spacer(height=4)
        dpg.add_button(label="Delete entry only", width=-1,
                       callback=_on_image_delete_entry_only)
        dpg.add_spacer(height=2)
        dpg.add_button(label="Delete entry + file", width=-1,
                       callback=_on_image_delete_entry_and_file)
        dpg.add_spacer(height=2)
        dpg.add_button(label="Cancel", width=-1,
                       callback=lambda: dpg.configure_item(TAG_IMG_DELETE_POPUP, show=False))

    _refresh_list(games_dir)


def refresh(cfg: dict) -> None:
    """Re-scan and rebuild the list after a pipeline run."""
    global _cfg
    _cfg = cfg
    _refresh_list(cfg.get("FREEGAMES_PATH", ""))


# =========================================================================
# List management
# =========================================================================

def _refresh_list(games_dir: str) -> None:
    global _db
    _db = (scanner.scan(games_dir)
           if games_dir and os.path.isdir(games_dir)
           else GameDatabase())

    if not dpg.does_item_exist(TAG_GAME_LIST):
        return

    for child in dpg.get_item_children(TAG_GAME_LIST, 1) or []:
        dpg.delete_item(child)

    if len(_db) == 0:
        dpg.add_text("No games found.", parent=TAG_GAME_LIST, color=WARNING)
        return

    for game in sorted(_db.games, key=lambda g: g.title.lower()):
        dpg.add_button(
            label=game.title, width=-1,
            parent=TAG_GAME_LIST,
            callback=_on_game_selected,
            user_data=game.folder_name,
        )


def _on_game_selected(sender: int | str, app_data: object, user_data: str) -> None:
    global _selected_game
    if _db is None:
        return
    game = _db.get_by_folder(user_data)
    if game is None:
        return
    _selected_game = game
    _show_detail(game)


# =========================================================================
# Detail panel
# =========================================================================

def _show_detail(game: Game) -> None:
    if dpg.does_item_exist(TAG_DETAIL_TITLE):
        dpg.set_value(TAG_DETAIL_TITLE, game.title)
        dpg.configure_item(TAG_DETAIL_TITLE, color=ACCENT)
    if dpg.does_item_exist(TAG_DETAIL_DIR):
        dpg.set_value(TAG_DETAIL_DIR, str(game.game_dir))

    if dpg.does_item_exist(TAG_EDIT_TITLE):
        dpg.set_value(TAG_EDIT_TITLE, game.title)
    if dpg.does_item_exist(TAG_EDIT_SGDB):
        dpg.set_value(TAG_EDIT_SGDB, str(game.steamgriddb_id or ""))
    if dpg.does_item_exist(TAG_EDIT_STATUS):
        dpg.set_value(TAG_EDIT_STATUS, "")

    _build_targets_section(game)
    _build_saves_section(game)

    _clear_image_group()
    _try_load_all_artwork(game)


# =========================================================================
# Targets
# =========================================================================

def _build_targets_section(game: Game) -> None:
    global _target_row_tags
    _target_row_tags = []
    for child in dpg.get_item_children(TAG_TARGETS_SECTION, 1) or []:
        dpg.delete_item(child)
    for t in game.targets:
        _add_target_row(t)


def _add_target_row(target: GameTarget | None = None) -> None:
    global _row_counter
    _row_counter += 1
    rid = _row_counter

    tags = {
        "os":       f"tgt_{rid}_os",
        "arch":     f"tgt_{rid}_arch",
        "target":   f"tgt_{rid}_target",
        "start_in": f"tgt_{rid}_start",
        "opts":     f"tgt_{rid}_opts",
        "delete":   f"tgt_{rid}_delete",
        "row":      f"tgt_{rid}_row",
    }
    _target_row_tags.append(tags)

    with dpg.group(horizontal=True, tag=tags["row"], parent=TAG_TARGETS_SECTION):
        dpg.add_combo(
            tag=tags["os"], items=_OS_OPTIONS,
            default_value=target.os if target else "linux",
            width=90,
        )
        dpg.add_combo(
            tag=tags["arch"], items=_ARCH_OPTIONS,
            default_value=target.arch if target else "x64",
            width=75,
        )
        with dpg.group(horizontal=True) as grp_target:
            dpg.add_input_text(
                tag=tags["target"], width=230,
                default_value=target.target if target else "",
                hint="Target executable",
            )
            dpg.add_button(
                label="...", width=30,
                callback=lambda s, a, u: _show_file_dialog(u),
                user_data=tags["target"],
            )
        dpg.bind_item_theme(grp_target, TAG_TIGHT_THEME)

        with dpg.group(horizontal=True) as grp_start:
            dpg.add_input_text(
                tag=tags["start_in"], width=200,
                default_value=target.start_in if target else "",
                hint="Working directory",
            )
            dpg.add_button(
                label="...", width=30,
                callback=lambda s, a, u: _show_dir_dialog(u),
                user_data=tags["start_in"],
            )
        dpg.bind_item_theme(grp_start, TAG_TIGHT_THEME)
        dpg.add_input_text(
            tag=tags["opts"], width=150,
            default_value=target.launch_options if target else "",
            hint="Launch options",
        )
        del_btn = dpg.add_button(
            tag=tags["delete"], label="X", width=30,
            callback=lambda s, a, u: _delete_target_row(u),
            user_data=tags,
        )
        dpg.bind_item_theme(del_btn, TAG_DELETE_BTN_THEME)


def _delete_target_row(tags: dict) -> None:
    if dpg.does_item_exist(tags["row"]):
        dpg.delete_item(tags["row"])
    if tags in _target_row_tags:
        _target_row_tags.remove(tags)


def _on_add_target(sender=None, app_data=None, user_data=None) -> None:
    if _selected_game is not None:
        _add_target_row()


_ARCH_NORMALIZE = {"x86_64": "x64", "amd64": "x64"}


def _on_auto_detect_targets(sender=None, app_data=None, user_data=None) -> None:
    if _selected_game is None:
        return

    detected = detector.collect_targets(str(_selected_game.game_dir))
    if not detected:
        _set_edit_status("No executables detected.", WARNING)
        return

    # Build set of existing (os, arch, target) from live widgets
    existing = set()
    for tags in _target_row_tags:
        if dpg.does_item_exist(tags["target"]):
            existing.add((
                dpg.get_value(tags["os"]),
                dpg.get_value(tags["arch"]),
                dpg.get_value(tags["target"]),
            ))

    added = 0
    for t in detected:
        arch = _ARCH_NORMALIZE.get(t.arch.lower(), t.arch)
        key = (t.os, arch, t.target)
        if key not in existing:
            _add_target_row(GameTarget(
                os=t.os, arch=arch,
                target=t.target, start_in=t.start_in,
                launch_options=t.launch_options,
            ))
            existing.add(key)
            added += 1

    if added:
        _set_edit_status(f"Detected {added} new target(s).", SUCCESS)
    else:
        _set_edit_status("No new targets found.", WARNING)


# =========================================================================
# Save paths
# =========================================================================

def _build_saves_section(game: Game) -> None:
    global _save_row_tags
    _save_row_tags = []
    for child in dpg.get_item_children(TAG_SAVES_SECTION, 1) or []:
        dpg.delete_item(child)
    for sp in game.save_paths:
        if isinstance(sp, dict):
            _add_save_row(sp)


def _add_save_row(sp: dict | None = None) -> None:
    global _row_counter
    _row_counter += 1
    rid = _row_counter

    tags = {
        "os":     f"sp_{rid}_os",
        "path":   f"sp_{rid}_path",
        "delete": f"sp_{rid}_delete",
        "row":    f"sp_{rid}_row",
    }
    _save_row_tags.append(tags)

    with dpg.group(horizontal=True, tag=tags["row"], parent=TAG_SAVES_SECTION):
        dpg.add_combo(
            tag=tags["os"], items=_OS_OPTIONS,
            default_value=sp.get("os", "linux") if sp else "linux",
            width=78,
        )
        with dpg.group(horizontal=True) as grp_path:
            dpg.add_input_text(
                tag=tags["path"], width=430,
                default_value=sp.get("path", "") if sp else "",
                hint="Save file or folder path",
            )
            dpg.add_button(
                label="...", width=26,
                callback=lambda s, a, u: _show_dir_dialog(u),
                user_data=tags["path"],
            )
        dpg.bind_item_theme(grp_path, TAG_TIGHT_THEME)
        del_btn = dpg.add_button(
            tag=tags["delete"], label="X", width=28,
            callback=lambda s, a, u: _delete_save_row(u),
            user_data=tags,
        )
        dpg.bind_item_theme(del_btn, TAG_DELETE_BTN_THEME)


def _delete_save_row(tags: dict) -> None:
    if dpg.does_item_exist(tags["row"]):
        dpg.delete_item(tags["row"])
    if tags in _save_row_tags:
        _save_row_tags.remove(tags)


def _on_add_save_path(sender=None, app_data=None, user_data=None) -> None:
    if _selected_game is not None:
        _add_save_row()


# =========================================================================
# File / directory dialogs
# =========================================================================

def _show_file_dialog(field_tag: str) -> None:
    global _pending_field_tag
    _pending_field_tag = field_tag
    if dpg.does_item_exist(TAG_FILE_DLG):
        if _selected_game is not None:
            dpg.configure_item(TAG_FILE_DLG, default_path=str(_selected_game.game_dir))
        dpg.show_item(TAG_FILE_DLG)


def _show_dir_dialog(field_tag: str) -> None:
    global _pending_field_tag
    _pending_field_tag = field_tag
    if dpg.does_item_exist(TAG_DIR_DLG):
        if _selected_game is not None:
            dpg.configure_item(TAG_DIR_DLG, default_path=str(_selected_game.game_dir))
        dpg.show_item(TAG_DIR_DLG)


def _on_file_selected(sender: object, app_data: dict) -> None:
    global _pending_field_tag
    # "selections" maps display names to actual full paths — use it when available
    # as file_path_name mangles multi-part extensions (e.g. .bin.x86_64 → .bin.*)
    selections = app_data.get("selections", {})
    if selections:
        path = next(iter(selections.values()), "")
    else:
        path = app_data.get("file_path_name", "")
    if _pending_field_tag and path and dpg.does_item_exist(_pending_field_tag):
        if _selected_game is not None:
            path = os.path.relpath(path, _selected_game.game_dir)
        dpg.set_value(_pending_field_tag, path)
    _pending_field_tag = None


def _on_dir_selected(sender: object, app_data: dict) -> None:
    global _pending_field_tag
    path = app_data.get("file_path_name", "")
    if _pending_field_tag and path and dpg.does_item_exist(_pending_field_tag):
        if _selected_game is not None:
            # Convert to relative path from game directory
            path = os.path.relpath(path, _selected_game.game_dir)
        dpg.set_value(_pending_field_tag, path)
    _pending_field_tag = None


# =========================================================================
# Save to disk
# =========================================================================

def _save_game_from_detail(
    sender: object = None,
    app_data: object = None,
    user_data: object = None,
) -> None:
    global _selected_game
    if _selected_game is None:
        return

    game = _selected_game

    # Title
    new_title = dpg.get_value(TAG_EDIT_TITLE).strip()
    if new_title:
        game.title = new_title

    # SteamGridDB ID
    sgdb_raw = dpg.get_value(TAG_EDIT_SGDB).strip()
    game.steamgriddb_id = int(sgdb_raw) if sgdb_raw.isdigit() else None

    # Targets — collect from live widgets
    game.targets = []
    for tags in _target_row_tags:
        exe = dpg.get_value(tags["target"]).strip() if dpg.does_item_exist(tags["target"]) else ""
        if not exe:
            continue
        game.targets.append(GameTarget(
            os=dpg.get_value(tags["os"]) if dpg.does_item_exist(tags["os"]) else "linux",
            arch=dpg.get_value(tags["arch"]) if dpg.does_item_exist(tags["arch"]) else "x86_64",
            target=exe,
            start_in=dpg.get_value(tags["start_in"]) if dpg.does_item_exist(tags["start_in"]) else "",
            launch_options=dpg.get_value(tags["opts"]) if dpg.does_item_exist(tags["opts"]) else "",
        ))

    # Save paths — collect from live widgets
    game.save_paths = []
    for tags in _save_row_tags:
        path = dpg.get_value(tags["path"]).strip() if dpg.does_item_exist(tags["path"]) else ""
        if not path:
            continue
        game.save_paths.append({
            "os":   dpg.get_value(tags["os"]) if dpg.does_item_exist(tags["os"]) else "linux",
            "path": path,
        })

    # Update header and list button
    if dpg.does_item_exist(TAG_DETAIL_TITLE):
        dpg.set_value(TAG_DETAIL_TITLE, game.title)
    for btn in dpg.get_item_children(TAG_GAME_LIST, 1) or []:
        if dpg.get_item_user_data(btn) == game.folder_name:
            dpg.configure_item(btn, label=game.title)
            break

    # Write game.json
    cartouche_dir = os.path.join(str(game.game_dir), CARTOUCHE_DIR)
    os.makedirs(cartouche_dir, exist_ok=True)
    json_path = os.path.join(cartouche_dir, GAME_JSON)
    try:
        with open(json_path, "w") as fh:
            json.dump(game.to_dict(), fh, indent=4)
        game.has_cartouche = True
        game.needs_persist = False
        _set_edit_status("Saved.", SUCCESS)
        logger.info("Saved %s", json_path)
    except Exception as exc:
        _set_edit_status(f"Error: {exc}", ERROR)
        logger.error("Failed to save %s: %s", json_path, exc)


def _on_open_sgdb_page(sender=None, app_data=None, user_data=None) -> None:
    sgdb_raw = dpg.get_value(TAG_EDIT_SGDB).strip() if dpg.does_item_exist(TAG_EDIT_SGDB) else ""
    if sgdb_raw.isdigit():
        webbrowser.open(f"https://www.steamgriddb.com/game/{sgdb_raw}")


def _set_edit_status(text: str, color: tuple[int, ...]) -> None:
    if dpg.does_item_exist(TAG_EDIT_STATUS):
        dpg.set_value(TAG_EDIT_STATUS, text)
        dpg.configure_item(TAG_EDIT_STATUS, color=color)


# =========================================================================
# Artwork
# =========================================================================

def _load_image_pil(path: str):
    """Load an image via Pillow (handles .ico and other formats dpg can't).
    Returns (width, height, data) where data is a flat list of floats 0-1 RGBA,
    or None on failure."""
    try:
        img = PILImage.open(path)
        # For .ico, pick the largest resolution
        if hasattr(img, "n_frames") and img.n_frames > 1:
            best, best_size = None, 0
            for i in range(img.n_frames):
                img.seek(i)
                px = img.size[0] * img.size[1]
                if px > best_size:
                    best, best_size = i, px
            if best is not None:
                img.seek(best)
        img = img.convert("RGBA")
        w, h = img.size
        raw = img.tobytes()
        data = [b / 255.0 for b in raw]
        return w, h, data
    except Exception:
        return None

def _clear_image_group() -> None:
    if not dpg.does_item_exist(TAG_IMG_GROUP):
        return
    for child in dpg.get_item_children(TAG_IMG_GROUP, 1) or []:
        dpg.delete_item(child)


def _try_load_all_artwork(game: Game) -> None:
    """Display all 5 artwork slots with borders, status text, and delete buttons."""
    artwork_row = dpg.add_group(horizontal=True, parent=TAG_IMG_GROUP)

    for field in _ARTWORK_ORDER:
        w, h = _ARTWORK_SIZES[field]
        col = dpg.add_group(parent=artwork_row)

        dpg.add_text(field, parent=col, color=TEXT_MUTED)

        slot = dpg.add_child_window(
            parent=col, width=w, height=h,
            border=True, no_scrollbar=True,
        )
        dpg.bind_item_theme(slot, TAG_IMG_SLOT_THEME)

        filename = getattr(game.images, field, None)
        loaded = False
        file_missing = False
        if filename:
            img_path = os.path.join(str(game.game_dir), CARTOUCHE_DIR, filename)
            if not os.path.isfile(img_path):
                file_missing = True
                dpg.add_dummy(width=w, height=h // 2 - 8, parent=slot)
                dpg.add_text("file missing", parent=slot, color=WARNING)
            else:
                tex_tag = f"tex_{game.folder_name}_{filename}"
                if tex_tag not in _loaded_textures:
                    # Try dpg native loader first, fall back to Pillow (.ico etc.)
                    tex_data = None
                    try:
                        iw, ih, _ch, data = dpg.load_image(img_path)
                        if iw > 0 and ih > 0:
                            tex_data = (iw, ih, data)
                    except Exception:
                        pass
                    if tex_data is None:
                        pil_result = _load_image_pil(img_path)
                        if pil_result:
                            tex_data = pil_result
                    if tex_data:
                        iw, ih, data = tex_data
                        texture_id = dpg.add_static_texture(
                            iw, ih, data,
                            parent=_texture_registry_tag,
                            tag=tex_tag,
                        )
                        _loaded_textures[tex_tag] = texture_id
                        _texture_sizes[tex_tag] = (iw, ih)
                if tex_tag in _loaded_textures:
                    # Fit image within slot while preserving aspect ratio
                    iw, ih = _texture_sizes.get(tex_tag, (w, h))
                    scale = min(w / iw, h / ih)
                    dw, dh = int(iw * scale), int(ih * scale)
                    dpg.add_image(_loaded_textures[tex_tag], parent=slot, width=dw, height=dh)
                    loaded = True
                if not loaded:
                    ext = os.path.splitext(filename)[1]
                    dpg.add_dummy(width=w, height=h // 2 - 8, parent=slot)
                    dpg.add_text(f"{ext} file", parent=slot, color=WARNING)
        else:
            dpg.add_dummy(width=w, height=h // 2 - 8, parent=slot)
            dpg.add_text("empty", parent=slot, color=TEXT_MUTED)

        # Delete button below the bordered image; hidden when file is missing
        if not file_missing:
            bar = dpg.add_group(horizontal=True, parent=col)
            dpg.add_dummy(width=w - 24, parent=bar)
            del_btn = dpg.add_button(
                label="X", width=24, parent=bar,
                callback=_on_image_delete_click,
                user_data=field,
            )
            dpg.bind_item_theme(del_btn, TAG_DELETE_BTN_THEME)


def _invalidate_game_textures(game: Game) -> None:
    """Remove cached textures for a game so they reload from disk."""
    prefix = f"tex_{game.folder_name}_"
    stale = [k for k in _loaded_textures if k.startswith(prefix)]
    for key in stale:
        try:
            if dpg.does_item_exist(key):
                dpg.delete_item(key)
        except Exception:
            pass
        del _loaded_textures[key]
        _texture_sizes.pop(key, None)


def _on_image_delete_click(sender=None, app_data=None, user_data=None) -> None:
    """Show the delete confirmation popup for an image slot."""
    global _pending_delete_field
    _pending_delete_field = user_data
    if dpg.does_item_exist(TAG_IMG_DELETE_POPUP):
        dpg.set_value(f"{TAG_IMG_DELETE_POPUP}_text",
                      f"Remove '{_pending_delete_field}' image?")
        # Center the popup in the viewport
        vw = dpg.get_viewport_width()
        vh = dpg.get_viewport_height()
        dpg.set_item_pos(TAG_IMG_DELETE_POPUP, [vw // 2 - 150, vh // 2 - 90])
        dpg.configure_item(TAG_IMG_DELETE_POPUP, show=True)


def _on_image_delete_entry_only(sender=None, app_data=None, user_data=None) -> None:
    """Delete the image entry from game.images but leave the file on disk."""
    global _pending_delete_field
    if _selected_game is None or _pending_delete_field is None:
        return
    field = _pending_delete_field
    _pending_delete_field = None
    dpg.configure_item(TAG_IMG_DELETE_POPUP, show=False)

    setattr(_selected_game.images, field, None)
    _selected_game.needs_persist = True
    _save_game_from_detail()

    _clear_image_group()
    _try_load_all_artwork(_selected_game)


def _on_image_delete_entry_and_file(sender=None, app_data=None, user_data=None) -> None:
    """Delete the image entry AND remove the file from .cartouche/."""
    global _pending_delete_field
    if _selected_game is None or _pending_delete_field is None:
        return
    field = _pending_delete_field
    _pending_delete_field = None
    dpg.configure_item(TAG_IMG_DELETE_POPUP, show=False)

    filename = getattr(_selected_game.images, field, None)
    if filename:
        file_path = os.path.join(
            str(_selected_game.game_dir), CARTOUCHE_DIR, filename,
        )
        try:
            if os.path.isfile(file_path):
                os.remove(file_path)
                logger.info("Deleted image file: %s", file_path)
        except OSError as exc:
            logger.error("Failed to delete %s: %s", file_path, exc)

    setattr(_selected_game.images, field, None)
    _selected_game.needs_persist = True
    _save_game_from_detail()

    _clear_image_group()
    _try_load_all_artwork(_selected_game)


def _on_fetch_images(sender=None, app_data=None, user_data=None) -> None:
    """Fetch ALL images from SteamGridDB, overwriting existing ones."""
    if _selected_game is None:
        return

    game = _selected_game
    api_key = get_steamgriddb_key(_cfg)
    if not api_key:
        if dpg.does_item_exist(TAG_FETCH_STATUS):
            dpg.set_value(TAG_FETCH_STATUS, "No API key configured.")
            dpg.configure_item(TAG_FETCH_STATUS, color=WARNING)
        return

    if not game.steamgriddb_id:
        if dpg.does_item_exist(TAG_FETCH_STATUS):
            dpg.set_value(TAG_FETCH_STATUS, "No SteamGridDB ID set.")
            dpg.configure_item(TAG_FETCH_STATUS, color=WARNING)
        return

    if dpg.does_item_exist(TAG_FETCH_STATUS):
        dpg.set_value(TAG_FETCH_STATUS, "Fetching...")
        dpg.configure_item(TAG_FETCH_STATUS, color=TEXT_MUTED)

    try:
        urls = _enricher.fetch_artwork_urls(game.steamgriddb_id, api_key, _cfg)
        new_images = _enricher._urls_to_image_filenames(urls)

        any_found = False
        for field in ("cover", "icon", "hero", "logo", "header"):
            new_val = getattr(new_images, field)
            if new_val:
                setattr(game.images, field, new_val)
                any_found = True

        if urls:
            game._artwork_urls = urls  # type: ignore[attr-defined]

        if any_found:
            game.needs_persist = True
            os.makedirs(str(game.cartouche_dir), exist_ok=True)
            _persister._download_images(game, str(game.cartouche_dir), force=True)
            _save_game_from_detail()
            if dpg.does_item_exist(TAG_FETCH_STATUS):
                dpg.set_value(TAG_FETCH_STATUS, "Done.")
                dpg.configure_item(TAG_FETCH_STATUS, color=SUCCESS)
        else:
            if dpg.does_item_exist(TAG_FETCH_STATUS):
                dpg.set_value(TAG_FETCH_STATUS, "No images found on SteamGridDB.")
                dpg.configure_item(TAG_FETCH_STATUS, color=WARNING)

        _invalidate_game_textures(game)
        _clear_image_group()
        _try_load_all_artwork(game)

    except Exception as exc:
        logger.error("Fetch images failed: %s", exc)
        if dpg.does_item_exist(TAG_FETCH_STATUS):
            dpg.set_value(TAG_FETCH_STATUS, f"Error: {exc}")
            dpg.configure_item(TAG_FETCH_STATUS, color=ERROR)
