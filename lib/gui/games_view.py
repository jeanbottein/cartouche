"""
Games view -- browse and inspect the game library.

Left panel: scrollable game list.  Right panel: selected game details
including artwork preview if an image file exists in .cartouche/.
"""

from __future__ import annotations

import os
from typing import Callable

import dearpygui.dearpygui as dpg

from lib import scanner
from lib.models import Game, GameDatabase, CARTOUCHE_DIR
from .theme import TEXT_SECONDARY, TEXT_MUTED, ACCENT, SUCCESS, WARNING

TAG_WINDOW = "games_view_window"
TAG_GAME_LIST = "games_list_child"
TAG_DETAIL_PANEL = "games_detail_child"
TAG_DETAIL_TITLE = "games_detail_title"
TAG_DETAIL_INFO = "games_detail_info"
TAG_IMG_GROUP = "games_img_group"

_db: GameDatabase | None = None
_selected_game: Game | None = None
_on_edit: Callable[[Game], None] | None = None
_texture_registry_tag = "games_tex_registry"
_loaded_textures: dict[str, int | str] = {}


def create(
    cfg: dict,
    on_edit_game: Callable[[Game], None] | None = None,
) -> None:
    """Build the games-browser view."""
    global _on_edit

    _on_edit = on_edit_game
    games_dir: str = cfg.get("FREEGAMES_PATH", "")

    with dpg.texture_registry(tag=_texture_registry_tag):
        pass  # populated lazily when artwork is loaded

    with dpg.window(
        label="Games",
        tag=TAG_WINDOW,
        no_title_bar=True,
        no_move=True,
        no_resize=True,
        no_close=True,
        no_collapse=True,
        show=False,
    ):
        dpg.add_text("Game Library", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=4)

        with dpg.group(horizontal=True):
            # -- Left panel: game list ------------------------------------
            with dpg.child_window(tag=TAG_GAME_LIST, width=280, height=-1,
                                   border=True):
                dpg.add_text("Loading...", color=TEXT_MUTED)

            # -- Right panel: detail --------------------------------------
            with dpg.child_window(tag=TAG_DETAIL_PANEL, width=-1, height=-1,
                                   border=True):
                dpg.add_text("Select a game", tag=TAG_DETAIL_TITLE,
                              color=TEXT_MUTED)
                dpg.add_separator()
                dpg.add_text("", tag=TAG_DETAIL_INFO, wrap=500,
                              color=TEXT_SECONDARY)
                dpg.add_group(tag=TAG_IMG_GROUP)

    # Populate in a deferred call so the window exists
    _refresh_list(games_dir)


def refresh(cfg: dict) -> None:
    """Re-scan and rebuild the list after pipeline run."""
    _refresh_list(cfg.get("FREEGAMES_PATH", ""))


# -- Internal helpers -----------------------------------------------------

def _refresh_list(games_dir: str) -> None:
    global _db
    _db = scanner.scan(games_dir) if games_dir and os.path.isdir(games_dir) else GameDatabase()

    if not dpg.does_item_exist(TAG_GAME_LIST):
        return

    # Clear the list child
    for child in dpg.get_item_children(TAG_GAME_LIST, 1) or []:
        dpg.delete_item(child)

    if len(_db) == 0:
        dpg.add_text("No games found.", parent=TAG_GAME_LIST, color=WARNING)
        return

    for game in sorted(_db.games, key=lambda g: g.title.lower()):
        color = SUCCESS if game.has_cartouche else TEXT_MUTED
        dpg.add_button(
            label=game.title,
            width=-1,
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


def _show_detail(game: Game) -> None:
    """Populate the right-hand detail panel for *game*."""
    if dpg.does_item_exist(TAG_DETAIL_TITLE):
        dpg.set_value(TAG_DETAIL_TITLE, game.title)
        dpg.configure_item(TAG_DETAIL_TITLE, color=ACCENT)

    lines: list[str] = []
    lines.append(f"Folder: {game.folder_name}")
    lines.append(f"Directory: {game.game_dir}")

    if game.steamgriddb_id:
        lines.append(f"SteamGridDB ID: {game.steamgriddb_id}")

    if game.targets:
        lines.append("")
        lines.append("Targets:")
        for t in game.targets:
            lines.append(f"  {t.os}/{t.arch}  {t.target}")

    if game.save_paths:
        lines.append("")
        lines.append("Save paths:")
        for sp in game.save_paths:
            if isinstance(sp, dict):
                lines.append(f"  [{sp.get('os', '?')}] {sp.get('path', '')}")

    if game.images:
        lines.append("")
        lines.append("Artwork files:")
        for field in ("cover", "icon", "hero", "logo"):
            val = getattr(game.images, field, None)
            if val:
                lines.append(f"  {field}: {val}")

    if game.notes:
        lines.append("")
        lines.append(f"Notes: {game.notes}")

    if dpg.does_item_exist(TAG_DETAIL_INFO):
        dpg.set_value(TAG_DETAIL_INFO, "\n".join(lines))

    # -- Artwork image ----------------------------------------------------
    _clear_image_group()
    _try_load_artwork(game)

    # -- Edit button (re-add each time) -----------------------------------
    edit_tag = "games_edit_btn"
    if dpg.does_item_exist(edit_tag):
        dpg.delete_item(edit_tag)
    if _on_edit:
        dpg.add_button(
            label="Edit Game",
            tag=edit_tag,
            parent=TAG_DETAIL_PANEL,
            width=140,
            callback=lambda s, a, u: _on_edit(game) if _on_edit else None,
        )


def _clear_image_group() -> None:
    if not dpg.does_item_exist(TAG_IMG_GROUP):
        return
    for child in dpg.get_item_children(TAG_IMG_GROUP, 1) or []:
        dpg.delete_item(child)


def _try_load_artwork(game: Game) -> None:
    """Load the cover image into a texture and display it."""
    cover = game.images.cover if game.images else None
    if not cover:
        return

    img_path = os.path.join(str(game.game_dir), CARTOUCHE_DIR, cover)
    if not os.path.isfile(img_path):
        return

    tex_tag = f"tex_{game.folder_name}_{cover}"
    if tex_tag in _loaded_textures:
        dpg.add_image(_loaded_textures[tex_tag], parent=TAG_IMG_GROUP,
                       width=300, height=450)
        return

    try:
        width, height, _channels, data = dpg.load_image(img_path)
        if width <= 0 or height <= 0:
            return
        texture_id = dpg.add_static_texture(
            width, height, data, parent=_texture_registry_tag, tag=tex_tag,
        )
        _loaded_textures[tex_tag] = texture_id
        dpg.add_image(texture_id, parent=TAG_IMG_GROUP, width=300, height=450)
    except Exception:
        dpg.add_text(f"(could not load {cover})", parent=TAG_IMG_GROUP,
                      color=WARNING)
