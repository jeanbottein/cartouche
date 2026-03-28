"""
Game editor -- modal window for editing a single game's metadata.

Provides input fields for title, notes, SteamGridDB ID, and tables
for targets and save paths.  Saves via the persister module.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Any, Callable

import dearpygui.dearpygui as dpg

from lib.models import Game, GameTarget, CARTOUCHE_DIR, GAME_JSON
from .theme import TEXT_SECONDARY, TEXT_MUTED, ACCENT, SUCCESS, ERROR

logger = logging.getLogger(__name__)

TAG_WINDOW = "game_edit_window"
TAG_TITLE_INPUT = "game_edit_title"
TAG_NOTES_INPUT = "game_edit_notes"
TAG_SGDB_INPUT = "game_edit_sgdb"
TAG_TARGETS_GROUP = "game_edit_targets"
TAG_SAVES_GROUP = "game_edit_saves"
TAG_STATUS = "game_edit_status"

_current_game: Game | None = None
_on_saved: Callable[[], None] | None = None


def open_editor(
    game: Game,
    on_saved: Callable[[], None] | None = None,
) -> None:
    """Open (or recreate) the editor window for *game*."""
    global _current_game, _on_saved
    _current_game = game
    _on_saved = on_saved

    # Tear down any previous editor
    if dpg.does_item_exist(TAG_WINDOW):
        dpg.delete_item(TAG_WINDOW)

    with dpg.window(
        label=f"Edit: {game.title}",
        tag=TAG_WINDOW,
        modal=True,
        width=800,
        height=600,
        no_resize=False,
        on_close=_on_close,
    ):
        dpg.add_text("Edit Game Metadata", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=6)

        # -- Title --------------------------------------------------------
        dpg.add_text("Title:", color=TEXT_MUTED)
        dpg.add_input_text(
            tag=TAG_TITLE_INPUT,
            default_value=game.title,
            width=-1,
        )

        # -- Notes --------------------------------------------------------
        dpg.add_text("Notes:", color=TEXT_MUTED)
        dpg.add_input_text(
            tag=TAG_NOTES_INPUT,
            default_value=game.notes or "",
            multiline=True,
            width=-1,
            height=60,
        )

        # -- SteamGridDB ID ----------------------------------------------
        dpg.add_text("SteamGridDB ID:", color=TEXT_MUTED)
        dpg.add_input_text(
            tag=TAG_SGDB_INPUT,
            default_value=str(game.steamgriddb_id or ""),
            width=200,
            decimal=True,
        )

        dpg.add_spacer(height=10)

        # -- Targets table ------------------------------------------------
        dpg.add_text("Targets:", color=TEXT_MUTED)
        with dpg.child_window(tag=TAG_TARGETS_GROUP, height=140, border=True):
            _build_targets_table(game)

        dpg.add_spacer(height=6)

        # -- Save paths table ---------------------------------------------
        dpg.add_text("Save Paths:", color=TEXT_MUTED)
        with dpg.child_window(tag=TAG_SAVES_GROUP, height=120, border=True):
            _build_saves_table(game)

        dpg.add_spacer(height=12)

        # -- Buttons ------------------------------------------------------
        dpg.add_text("", tag=TAG_STATUS, color=TEXT_MUTED)
        with dpg.group(horizontal=True):
            dpg.add_button(label="Save", width=120, callback=_do_save)
            dpg.add_button(label="Cancel", width=120, callback=_on_close)


# -- Table builders -------------------------------------------------------

def _build_targets_table(game: Game) -> None:
    if not game.targets:
        dpg.add_text("No targets defined.", parent=TAG_TARGETS_GROUP,
                      color=TEXT_MUTED)
        return

    with dpg.table(
        parent=TAG_TARGETS_GROUP,
        header_row=True,
        borders_innerH=True,
        borders_outerH=True,
        borders_innerV=True,
        borders_outerV=True,
        resizable=True,
    ):
        dpg.add_table_column(label="OS", width_fixed=True, init_width_or_weight=70)
        dpg.add_table_column(label="Arch", width_fixed=True, init_width_or_weight=70)
        dpg.add_table_column(label="Target")
        dpg.add_table_column(label="Start In")
        dpg.add_table_column(label="Launch Opts")

        for t in game.targets:
            with dpg.table_row():
                dpg.add_text(t.os)
                dpg.add_text(t.arch)
                dpg.add_text(t.target)
                dpg.add_text(t.start_in)
                dpg.add_text(t.launch_options)


def _build_saves_table(game: Game) -> None:
    if not game.save_paths:
        dpg.add_text("No save paths defined.", parent=TAG_SAVES_GROUP,
                      color=TEXT_MUTED)
        return

    with dpg.table(
        parent=TAG_SAVES_GROUP,
        header_row=True,
        borders_innerH=True,
        borders_outerH=True,
        borders_innerV=True,
        borders_outerV=True,
        resizable=True,
    ):
        dpg.add_table_column(label="OS", width_fixed=True, init_width_or_weight=80)
        dpg.add_table_column(label="Path")

        for sp in game.save_paths:
            if not isinstance(sp, dict):
                continue
            with dpg.table_row():
                dpg.add_text(sp.get("os", ""))
                dpg.add_text(sp.get("path", ""))


# -- Save / close callbacks -----------------------------------------------

def _do_save(sender: int | str = None, app_data: Any = None, user_data: Any = None) -> None:
    if _current_game is None:
        return

    game = _current_game
    game.title = dpg.get_value(TAG_TITLE_INPUT) or game.title
    game.notes = dpg.get_value(TAG_NOTES_INPUT) or ""

    sgdb_raw = dpg.get_value(TAG_SGDB_INPUT).strip()
    game.steamgriddb_id = int(sgdb_raw) if sgdb_raw.isdigit() else None

    # Write game.json directly
    cartouche_dir = os.path.join(str(game.game_dir), CARTOUCHE_DIR)
    os.makedirs(cartouche_dir, exist_ok=True)
    json_path = os.path.join(cartouche_dir, GAME_JSON)

    try:
        with open(json_path, "w") as fh:
            json.dump(game.to_dict(), fh, indent=4)
        game.has_cartouche = True
        game.needs_persist = False
        _set_status("Saved successfully.", SUCCESS)
        logger.info("Saved %s", json_path)
        if _on_saved:
            _on_saved()
    except Exception as exc:
        _set_status(f"Save failed: {exc}", ERROR)
        logger.error("Failed to save %s: %s", json_path, exc)


def _on_close(sender: int | str = None, app_data: Any = None, user_data: Any = None) -> None:
    if dpg.does_item_exist(TAG_WINDOW):
        dpg.configure_item(TAG_WINDOW, show=False)


def _set_status(text: str, color: tuple[int, ...]) -> None:
    if dpg.does_item_exist(TAG_STATUS):
        dpg.set_value(TAG_STATUS, text)
        dpg.configure_item(TAG_STATUS, color=color)
