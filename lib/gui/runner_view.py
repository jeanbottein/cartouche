"""
Runner view -- pipeline execution with progress and log output.

Runs the pipeline in a background thread, updating progress bars and
a scrolling log area via Dear PyGui's thread-safe value setters.
"""

from __future__ import annotations

import logging
import threading
from typing import Callable

import dearpygui.dearpygui as dpg

from lib.pipeline import PipelineRunner
from .theme import TEXT_SECONDARY, SUCCESS, ERROR, WARNING, TEXT_MUTED, ACCENT

TAG_WINDOW = "runner_view_window"
TAG_OVERALL_PROGRESS = "runner_overall_progress"
TAG_PHASE_PROGRESS = "runner_phase_progress"
TAG_PHASE_LABEL = "runner_phase_label"
TAG_LOG_TEXT = "runner_log_text"
TAG_RUN_BUTTON = "runner_run_button"
TAG_STATUS_LABEL = "runner_status_label"

_runner_thread: threading.Thread | None = None
_cancel_flag = threading.Event()


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
_log_handler.setFormatter(logging.Formatter("%(asctime)s  %(name)s  %(message)s",
                                             datefmt="%H:%M:%S"))


def create(
    cfg: dict,
    on_pipeline_done: Callable[[], None] | None = None,
) -> None:
    """Build the runner view widgets."""

    with dpg.window(
        label="Runner",
        tag=TAG_WINDOW,
        no_title_bar=True,
        no_move=True,
        no_resize=True,
        no_close=True,
        no_collapse=True,
        show=False,
    ):
        dpg.add_text("Pipeline Runner", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=8)

        # -- Controls -----------------------------------------------------
        with dpg.group(horizontal=True):
            dpg.add_button(
                label="Run All",
                tag=TAG_RUN_BUTTON,
                width=140,
                callback=lambda: _start_pipeline(cfg, "all", on_pipeline_done),
            )
            dpg.add_button(
                label="Run Parse",
                width=140,
                callback=lambda: _start_pipeline(cfg, "parse", on_pipeline_done),
            )
            dpg.add_button(
                label="Run Backup",
                width=140,
                callback=lambda: _start_pipeline(cfg, "backup", on_pipeline_done),
            )
            dpg.add_button(
                label="Cancel",
                width=100,
                callback=_request_cancel,
            )

        dpg.add_spacer(height=12)

        # -- Status -------------------------------------------------------
        dpg.add_text("Idle", tag=TAG_STATUS_LABEL, color=TEXT_MUTED)

        # -- Progress bars ------------------------------------------------
        dpg.add_text("Overall progress:", color=TEXT_SECONDARY)
        dpg.add_progress_bar(
            tag=TAG_OVERALL_PROGRESS,
            default_value=0.0,
            width=-1,
        )
        dpg.add_spacer(height=4)

        with dpg.group(horizontal=True):
            dpg.add_text("Current phase:", color=TEXT_SECONDARY)
            dpg.add_text("--", tag=TAG_PHASE_LABEL, color=TEXT_MUTED)
        dpg.add_progress_bar(
            tag=TAG_PHASE_PROGRESS,
            default_value=0.0,
            width=-1,
        )

        dpg.add_spacer(height=12)

        # -- Log output ---------------------------------------------------
        dpg.add_text("Log output:", color=TEXT_SECONDARY)
        dpg.add_input_text(
            tag=TAG_LOG_TEXT,
            multiline=True,
            readonly=True,
            width=-1,
            height=340,
            default_value="Ready.\n",
        )


# -- Pipeline execution ---------------------------------------------------

def _start_pipeline(
    cfg: dict,
    group: str,
    on_done: Callable[[], None] | None,
) -> None:
    global _runner_thread
    if _runner_thread and _runner_thread.is_alive():
        return  # already running

    _cancel_flag.clear()
    _log_handler.clear()

    # Attach our log handler to root so we capture everything
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
        dpg.set_value(TAG_PHASE_PROGRESS, 0.5)  # indeterminate pulse
        from . import status_view
        status_view.set_phase_status(key, "running")

    def on_phase_end(key: str) -> None:
        nonlocal completed_count
        completed_count += 1
        if total > 0:
            dpg.set_value(TAG_OVERALL_PROGRESS, completed_count / total)
        dpg.set_value(TAG_PHASE_PROGRESS, 1.0)
        from . import status_view
        status_view.set_phase_status(key, "completed")

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

            from . import status_view
            status_view.set_last_run_now()
            status_view.refresh_game_count(cfg)
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
