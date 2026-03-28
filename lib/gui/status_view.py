"""
Status view -- the home/default screen for Cartouche.

Shows app title, game count, quick-action buttons for pipeline
execution, phase status indicators, and last-run timestamp.
"""

from __future__ import annotations

import os
from datetime import datetime
from typing import Callable

import dearpygui.dearpygui as dpg

from lib import scanner
from .theme import TEXT_SECONDARY, SUCCESS, ERROR, WARNING, TEXT_MUTED

TAG_WINDOW = "status_view_window"
TAG_GAME_COUNT = "status_game_count"
TAG_LAST_RUN = "status_last_run"
_PHASE_STATUS_TAGS: dict[str, str] = {}

VERSION = "1.0.0"


def create(
    cfg: dict,
    on_run_all: Callable[[], None],
    on_run_parse: Callable[[], None],
    on_run_backup: Callable[[], None],
) -> None:
    """Build the status view widgets inside a Dear PyGui window."""

    games_dir: str = cfg.get("FREEGAMES_PATH", "")
    game_count = _count_games(games_dir)

    with dpg.window(
        label="Status",
        tag=TAG_WINDOW,
        no_title_bar=True,
        no_move=True,
        no_resize=True,
        no_close=True,
        no_collapse=True,
    ):
        dpg.add_text("Cartouche", color=(230, 235, 242, 255))
        dpg.add_text(f"v{VERSION}  --  DRM-free game manager for Steam Deck",
                      color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=10)

        # -- Game count ---------------------------------------------------
        if not games_dir or not os.path.isdir(games_dir):
            dpg.add_text("FREEGAMES_PATH is not configured or invalid.",
                          color=ERROR)
        else:
            dpg.add_text(f"Games directory: {games_dir}", color=TEXT_MUTED)
            dpg.add_text(
                f"Games found: {game_count}",
                tag=TAG_GAME_COUNT,
                color=SUCCESS if game_count > 0 else WARNING,
            )

        dpg.add_spacer(height=16)

        # -- Quick actions ------------------------------------------------
        dpg.add_text("Quick Actions", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=6)

        with dpg.group(horizontal=True):
            dpg.add_button(label="Run All Phases", width=180,
                           callback=lambda: on_run_all())
            dpg.add_button(label="Run Parse Only", width=180,
                           callback=lambda: on_run_parse())
            dpg.add_button(label="Run Backup Only", width=180,
                           callback=lambda: on_run_backup())

        dpg.add_spacer(height=20)

        # -- Phase status indicators --------------------------------------
        dpg.add_text("Phase Status", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=6)

        from lib.pipeline import PipelineRunner
        for phase_key, phase_label in PipelineRunner.PHASES:
            tag = f"status_phase_{phase_key}"
            _PHASE_STATUS_TAGS[phase_key] = tag
            with dpg.group(horizontal=True):
                dpg.add_text(f"  [{phase_label}]", color=TEXT_MUTED)
                dpg.add_text("not yet run", tag=tag, color=TEXT_MUTED)

        dpg.add_spacer(height=16)

        # -- Last run timestamp -------------------------------------------
        dpg.add_text("Last run: never", tag=TAG_LAST_RUN, color=TEXT_MUTED)


# -- Public helpers -------------------------------------------------------

def set_phase_status(phase_key: str, status: str) -> None:
    """Update the status indicator for a phase.

    *status* should be ``"running"``, ``"completed"``, or ``"error"``.
    """
    tag = _PHASE_STATUS_TAGS.get(phase_key)
    if not tag or not dpg.does_item_exist(tag):
        return

    color_map = {
        "running": WARNING,
        "completed": SUCCESS,
        "error": ERROR,
    }
    dpg.set_value(tag, status)
    dpg.configure_item(tag, color=color_map.get(status, TEXT_MUTED))


def set_last_run_now() -> None:
    """Stamp the 'last run' label with the current time."""
    if dpg.does_item_exist(TAG_LAST_RUN):
        now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        dpg.set_value(TAG_LAST_RUN, f"Last run: {now}")


def refresh_game_count(cfg: dict) -> None:
    """Recount games and update the label."""
    if not dpg.does_item_exist(TAG_GAME_COUNT):
        return
    count = _count_games(cfg.get("FREEGAMES_PATH", ""))
    dpg.set_value(TAG_GAME_COUNT, f"Games found: {count}")
    dpg.configure_item(TAG_GAME_COUNT,
                       color=SUCCESS if count > 0 else WARNING)


# -- Internal helpers -----------------------------------------------------

def _count_games(games_dir: str) -> int:
    if not games_dir or not os.path.isdir(games_dir):
        return 0
    return sum(
        1 for item in os.listdir(games_dir)
        if not item.startswith(".") and os.path.isdir(os.path.join(games_dir, item))
    )
