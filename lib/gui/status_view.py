"""
Status view -- the home/default screen for Cartouche.

Shows app title, game count, quick-action buttons for pipeline
execution, phase status indicators, progress bars, and log output.
"""

from __future__ import annotations

import logging
import os
import threading
from datetime import datetime
from typing import Any, Callable

import dearpygui.dearpygui as dpg

from lib import scanner
from lib.pipeline import PipelineRunner
from .theme import TEXT_SECONDARY, SUCCESS, ERROR, WARNING, TEXT_MUTED, ACCENT

TAG_WINDOW = "status_view_window"
TAG_GAME_COUNT = "status_game_count"
TAG_LAST_RUN = "status_last_run"
TAG_OVERALL_PROGRESS = "status_overall_progress"
TAG_PHASE_PROGRESS = "status_phase_progress"
TAG_PHASE_LABEL = "status_phase_label"
TAG_LOG_TEXT = "status_log_text"
TAG_STATUS_LABEL = "status_status_label"

_PHASE_STATUS_TAGS: dict[str, str] = {}
_runner_thread: threading.Thread | None = None
_cancel_flag = threading.Event()

VERSION = "1.1.0"


class _GuiLogHandler(logging.Handler):
    """Logging handler that appends messages to the DPG log widget."""

    def __init__(self, max_lines: int = 500) -> None:
        super().__init__()
        self._lines: list[str] = []
        self._max = max_lines

    def emit(self, record: logging.LogRecord) -> None:
        msg = self.format(record)
        self._lines.append(msg)
        if len(self._lines) > self._max:
            self._lines = self._lines[-self._max:]
        if dpg.does_item_exist(TAG_LOG_TEXT):
            dpg.set_value(TAG_LOG_TEXT, "\n".join(self._lines))

    def clear(self) -> None:
        self._lines.clear()
        if dpg.does_item_exist(TAG_LOG_TEXT):
            dpg.set_value(TAG_LOG_TEXT, "")


_log_handler = _GuiLogHandler()
_log_handler.setFormatter(logging.Formatter("%(asctime)s  %(message)s",
                                             datefmt="%H:%M:%S"))


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
        with dpg.group(horizontal=True):
            with dpg.group(width=400):
                dpg.add_text("Cartouche", color=(230, 235, 242, 255))
                dpg.add_text(f"v{VERSION}  --  DRM-free game manager",
                              color=TEXT_SECONDARY)
                dpg.add_separator()
                dpg.add_spacer(height=10)

                # -- Game count ---------------------------------------------------
                if not games_dir or not os.path.isdir(games_dir):
                    dpg.add_text("FREEGAMES_PATH is invalid.", color=ERROR)
                else:
                    dpg.add_text(f"Games found: {game_count}",
                                 tag=TAG_GAME_COUNT,
                                 color=SUCCESS if game_count > 0 else WARNING)

                dpg.add_spacer(height=16)

                # -- Quick actions ------------------------------------------------
                dpg.add_text("Quick Actions", color=TEXT_SECONDARY)
                dpg.add_separator()
                dpg.add_spacer(height=6)

                with dpg.group(horizontal=True):
                    dpg.add_button(label="Run All", width=120,
                                   callback=lambda s, a, u: on_run_all())
                    dpg.add_button(label="Parse", width=120,
                                   callback=lambda s, a, u: on_run_parse())
                    dpg.add_button(label="Backup", width=120,
                                   callback=lambda s, a, u: on_run_backup())
                
                dpg.add_button(label="Cancel Current Run", width=370,
                               callback=lambda s, a, u: _request_cancel())

                dpg.add_spacer(height=20)

                # -- Phase status indicators --------------------------------------
                dpg.add_text("Phase Status", color=TEXT_SECONDARY)
                dpg.add_separator()
                dpg.add_spacer(height=6)

                for phase_key, phase_label in PipelineRunner.PHASES:
                    tag = f"status_phase_{phase_key}"
                    _PHASE_STATUS_TAGS[phase_key] = tag
                    with dpg.group(horizontal=True):
                        dpg.add_text(f"  [{phase_label}]", color=TEXT_MUTED)
                        dpg.add_text("not yet run", tag=tag, color=TEXT_MUTED)

                dpg.add_spacer(height=16)
                dpg.add_text("Last run: never", tag=TAG_LAST_RUN, color=TEXT_MUTED)

            dpg.add_spacer(width=20)

            # -- Progress and Logs (Right side) -------------------------------
            with dpg.group(width=-1):
                dpg.add_text("Pipeline Progress", color=TEXT_SECONDARY)
                dpg.add_separator()
                dpg.add_spacer(height=8)

                dpg.add_text("Idle", tag=TAG_STATUS_LABEL, color=TEXT_MUTED)
                
                dpg.add_text("Overall progress:", color=TEXT_SECONDARY)
                dpg.add_progress_bar(tag=TAG_OVERALL_PROGRESS, default_value=0.0, width=-1)
                
                with dpg.group(horizontal=True):
                    dpg.add_text("Current phase:", color=TEXT_SECONDARY)
                    dpg.add_text("--", tag=TAG_PHASE_LABEL, color=TEXT_MUTED)
                dpg.add_progress_bar(tag=TAG_PHASE_PROGRESS, default_value=0.0, width=-1)

                dpg.add_spacer(height=12)
                dpg.add_text("Log Output:", color=TEXT_SECONDARY)
                dpg.add_input_text(
                    tag=TAG_LOG_TEXT,
                    multiline=True,
                    readonly=True,
                    width=-1,
                    height=-1,  # fill remaining
                    default_value="Ready.\n",
                )


# -- Pipeline execution ---------------------------------------------------

def start_pipeline(
    cfg: dict,
    group: str,
    on_done: Callable[[], None] | None = None,
) -> None:
    """Start the pipeline in a background thread."""
    global _runner_thread
    if _runner_thread and _runner_thread.is_alive():
        return

    _cancel_flag.clear()
    _log_handler.clear()

    root_logger = logging.getLogger()
    if _log_handler not in root_logger.handlers:
        root_logger.addHandler(_log_handler)

    games_dir: str = cfg.get("FREEGAMES_PATH", "")
    phases = PipelineRunner.GROUPS.get(group, PipelineRunner.GROUPS["all"])
    total = len(phases)

    _set_status("Running...", WARNING)
    dpg.set_value(TAG_OVERALL_PROGRESS, 0.0)
    dpg.set_value(TAG_PHASE_PROGRESS, 0.0)

    completed_count = 0

    def on_phase_start(key: str, label: str) -> None:
        nonlocal completed_count
        if dpg.does_item_exist(TAG_PHASE_LABEL):
            dpg.set_value(TAG_PHASE_LABEL, label)
        dpg.set_value(TAG_PHASE_PROGRESS, 0.5)
        set_phase_status(key, "running")

    def on_phase_end(key: str) -> None:
        nonlocal completed_count
        completed_count += 1
        if total > 0:
            dpg.set_value(TAG_OVERALL_PROGRESS, completed_count / total)
        dpg.set_value(TAG_PHASE_PROGRESS, 1.0)
        set_phase_status(key, "completed")

    def _run() -> None:
        try:
            runner = PipelineRunner(
                cfg=cfg,
                games_dir=games_dir,
                on_phase_start=on_phase_start,
                on_phase_end=on_phase_end,
            )
            for phase_name in phases:
                if _cancel_flag.is_set():
                    _set_status("Cancelled", ERROR)
                    return
                runner.run_phase(phase_name)

            _set_status("Completed", SUCCESS)
            dpg.set_value(TAG_OVERALL_PROGRESS, 1.0)
            set_last_run_now()
            refresh_game_count(cfg)
        except Exception as exc:
            _set_status(f"Error: {exc}", ERROR)
            logging.getLogger(__name__).error(f"Pipeline error: {exc}", exc_info=True)
        finally:
            if on_done:
                on_done()

    _runner_thread = threading.Thread(target=_run, daemon=True)
    _runner_thread.start()


def _request_cancel() -> None:
    _cancel_flag.set()
    _set_status("Cancelling...", WARNING)


def _set_status(text: str, color: tuple[int, ...]) -> None:
    if dpg.does_item_exist(TAG_STATUS_LABEL):
        dpg.set_value(TAG_STATUS_LABEL, text)
        dpg.configure_item(TAG_STATUS_LABEL, color=color)


# -- Public helpers -------------------------------------------------------

def set_phase_status(phase_key: str, status: str) -> None:
    """Update the status indicator for a phase."""
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
