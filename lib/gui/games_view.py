"""
Games view -- browse and inspect the game library.

Left panel: scrollable game list.  Right panel: selected game details
with inline editing, target/save-path management, and artwork previews.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Callable

import dearpygui.dearpygui as dpg

from lib import scanner
from lib.models import Game, GameDatabase, GameTarget, CARTOUCHE_DIR, GAME_JSON
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

# -- Themes -------------------------------------------------------------------
TAG_DELETE_BTN_THEME  = "games_delete_btn_theme"
TAG_TIGHT_THEME       = "games_tight_theme"
TAG_AUTO_WINDOW_THEME = "games_auto_window_theme"

# -- Artwork thumbnails ----------------------------------------------------
_ARTWORK_SIZES: dict[str, tuple[int, int]] = {
    "cover": (120, 180),
    "hero":  (200, 75),
    "logo":  (150, 60),
    "icon":  (60, 60),
}

# -- Dropdown options ------------------------------------------------------
_OS_OPTIONS   = ["linux", "windows", "mac", "android", "web"]
_ARCH_OPTIONS = ["x64", "arm64"]

# -- Module state ----------------------------------------------------------
_db: GameDatabase | None = None
_selected_game: Game | None = None
_texture_registry_tag = "games_tex_registry"
_loaded_textures: dict[str, int | str] = {}

# Dynamic rows: each entry is a dict of widget tags for one row
_target_row_tags: list[dict[str, str]] = []
_save_row_tags:   list[dict[str, str]] = []
_row_counter: int = 0          # ever-increasing, never reused
_pending_field_tag: str | None = None  # which input the open dialog fills


# =========================================================================
# Public API
# =========================================================================

def create(cfg: dict) -> None:
    """Build the games-browser view."""
    games_dir: str = cfg.get("FREEGAMES_PATH", "")

    # Create red delete button theme
    with dpg.theme(tag=TAG_DELETE_BTN_THEME):
        with dpg.theme_component(dpg.mvButton):
            dpg.add_theme_color(dpg.mvThemeCol_Button, ERROR)
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, (255, 120, 120, 255))
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, (180, 50, 50, 255))

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

    with dpg.texture_registry(tag=_texture_registry_tag):
        pass

    with dpg.window(
        label="Games", tag=TAG_WINDOW,
        no_title_bar=True, no_move=True, no_resize=True,
        no_close=True, no_collapse=True, show=False,
    ):
        dpg.add_text("Game Library", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=4)

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
                    dpg.add_text("ID:", color=TEXT_MUTED)
                    dpg.add_input_text(tag=TAG_EDIT_SGDB, default_value="",
                                       width=70, decimal=True)

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
                dpg.add_button(label="+ Add Target",
                               callback=_on_add_target)

                dpg.add_separator()

                # -- Save Paths -------------------------------------------
                dpg.add_text("Save Paths", color=TEXT_SECONDARY)
                with dpg.group(horizontal=True):
                    dpg.add_text("OS",   color=TEXT_MUTED); dpg.add_dummy(width=60)
                    dpg.add_text("Path", color=TEXT_MUTED)
                with dpg.group(tag=TAG_SAVES_SECTION, horizontal=False) as grp:
                    dpg.bind_item_theme(grp, TAG_AUTO_WINDOW_THEME)
                    pass
                dpg.add_button(label="+ Add Save Path",
                               callback=_on_add_save_path)

                dpg.add_spacer(height=3)

                # Save button + status
                with dpg.group(horizontal=True):
                    dpg.add_button(label="Save", width=80,
                                   callback=_save_game_from_detail)
                    dpg.add_text("", tag=TAG_EDIT_STATUS, color=SUCCESS)

                dpg.add_spacer(height=2)
                dpg.add_group(tag=TAG_IMG_GROUP)

    # File dialog (for target exe)
    with dpg.file_dialog(
        directory_selector=False, show=False,
        callback=_on_file_selected, tag=TAG_FILE_DLG,
        width=700, height=450,
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

    _refresh_list(games_dir)


def refresh(cfg: dict) -> None:
    """Re-scan and rebuild the list after a pipeline run."""
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
    path = app_data.get("file_path_name", "")
    if _pending_field_tag and path and dpg.does_item_exist(_pending_field_tag):
        dpg.set_value(_pending_field_tag, path)
    _pending_field_tag = None


def _on_dir_selected(sender: object, app_data: dict) -> None:
    global _pending_field_tag
    path = app_data.get("file_path_name", "")
    if _pending_field_tag and path and dpg.does_item_exist(_pending_field_tag):
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


def _set_edit_status(text: str, color: tuple[int, ...]) -> None:
    if dpg.does_item_exist(TAG_EDIT_STATUS):
        dpg.set_value(TAG_EDIT_STATUS, text)
        dpg.configure_item(TAG_EDIT_STATUS, color=color)


# =========================================================================
# Artwork
# =========================================================================

def _clear_image_group() -> None:
    if not dpg.does_item_exist(TAG_IMG_GROUP):
        return
    for child in dpg.get_item_children(TAG_IMG_GROUP, 1) or []:
        dpg.delete_item(child)


def _try_load_all_artwork(game: Game) -> None:
    """Load all available artwork images and display them side by side."""
    if not game.images:
        return

    artwork_row = dpg.add_group(horizontal=True, parent=TAG_IMG_GROUP)

    for field, (w, h) in _ARTWORK_SIZES.items():
        filename = getattr(game.images, field, None)
        if not filename:
            continue
        img_path = os.path.join(str(game.game_dir), CARTOUCHE_DIR, filename)
        if not os.path.isfile(img_path):
            continue

        tex_tag = f"tex_{game.folder_name}_{filename}"
        if tex_tag not in _loaded_textures:
            try:
                iw, ih, _ch, data = dpg.load_image(img_path)
                if iw <= 0 or ih <= 0:
                    continue
                texture_id = dpg.add_static_texture(
                    iw, ih, data,
                    parent=_texture_registry_tag,
                    tag=tex_tag,
                )
                _loaded_textures[tex_tag] = texture_id
            except Exception:
                continue

        col = dpg.add_group(parent=artwork_row)
        dpg.add_image(_loaded_textures[tex_tag], parent=col, width=w, height=h)
        dpg.add_text(field, parent=col, color=TEXT_MUTED)
