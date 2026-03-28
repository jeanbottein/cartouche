"""
Main GUI entry point for Cartouche.

Creates the Dear PyGui context, viewport, navigation tab bar, and
wires each view module together.  Call ``run_gui(cfg)`` to launch.
"""

from __future__ import annotations

import dearpygui.dearpygui as dpg

from lib.gui import theme, status_view, runner_view, games_view, game_edit, controller

VIEWPORT_WIDTH = 1280
VIEWPORT_HEIGHT = 800
VIEWPORT_TITLE = "Cartouche"

TAG_PRIMARY = "primary_window"
TAG_TAB_BAR = "main_tab_bar"

_VIEW_TAGS: dict[str, str] = {
    "Status": status_view.TAG_WINDOW,
    "Games": games_view.TAG_WINDOW,
    "Runner": runner_view.TAG_WINDOW,
    "Settings": "settings_view_window",
}


def run_gui(cfg: dict) -> None:
    """Initialise Dear PyGui and run the application loop."""

    dpg.create_context()
    dpg.create_viewport(
        title=VIEWPORT_TITLE,
        width=VIEWPORT_WIDTH,
        height=VIEWPORT_HEIGHT,
        resizable=True,
    )

    # -- Theme and font scale for Steam Deck readability ------------------
    theme.apply_theme()
    theme.set_global_scale(1.15)

    # -- Build views (order matters: tags must exist before tab bar) ------
    _build_status_view(cfg)
    _build_runner_view(cfg)
    _build_games_view(cfg)
    _build_settings_view(cfg)

    # -- Primary window with tab bar navigation ---------------------------
    with dpg.window(tag=TAG_PRIMARY):
        with dpg.tab_bar(tag=TAG_TAB_BAR, callback=_on_tab_changed):
            dpg.add_tab(label="Status", tag="tab_Status")
            dpg.add_tab(label="Games", tag="tab_Games")
            dpg.add_tab(label="Runner", tag="tab_Runner")
            dpg.add_tab(label="Settings", tag="tab_Settings")

    dpg.set_primary_window(TAG_PRIMARY, True)

    # Show default view
    _switch_view("Status")

    # -- Gamepad controller -----------------------------------------------
    controller.configure(
        view_names=list(_VIEW_TAGS.keys()),
        switch_view_callback=_switch_view,
    )

    # -- Start Dear PyGui -------------------------------------------------
    dpg.setup_dearpygui()
    dpg.show_viewport()

    while dpg.is_dearpygui_running():
        controller.poll()
        dpg.render_dearpygui_frame()

    dpg.destroy_context()


# -- View construction helpers -------------------------------------------

def _build_status_view(cfg: dict) -> None:
    status_view.create(
        cfg=cfg,
        on_run_all=lambda: _run_pipeline(cfg, "all"),
        on_run_parse=lambda: _run_pipeline(cfg, "parse"),
        on_run_backup=lambda: _run_pipeline(cfg, "backup"),
    )


def _build_runner_view(cfg: dict) -> None:
    runner_view.create(cfg=cfg, on_pipeline_done=lambda: _on_pipeline_done(cfg))


def _build_games_view(cfg: dict) -> None:
    games_view.create(
        cfg=cfg,
        on_edit_game=lambda game: game_edit.open_editor(
            game,
            on_saved=lambda: games_view.refresh(cfg),
        ),
    )


def _build_settings_view(cfg: dict) -> None:
    """Minimal settings panel showing current configuration values."""
    from .theme import TEXT_SECONDARY, TEXT_MUTED

    with dpg.window(
        label="Settings",
        tag="settings_view_window",
        no_title_bar=True,
        no_move=True,
        no_resize=True,
        no_close=True,
        no_collapse=True,
        show=False,
    ):
        dpg.add_text("Settings", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=8)

        dpg.add_text("Configuration", color=TEXT_SECONDARY)
        dpg.add_separator()
        dpg.add_spacer(height=4)

        config_path = cfg.get("_CONFIG_PATH", "(unknown)")
        dpg.add_text(f"Config file: {config_path}", color=TEXT_MUTED)
        dpg.add_spacer(height=4)

        for key in sorted(cfg.keys()):
            if key.startswith("_"):
                continue
            dpg.add_text(f"{key} = {cfg[key]}", color=TEXT_MUTED, wrap=700)


# -- Navigation -----------------------------------------------------------

def _on_tab_changed(sender: int | str, app_data: int | str) -> None:
    """Called when the user clicks a tab."""
    # app_data is the tag of the selected tab
    for name in _VIEW_TAGS:
        if app_data == f"tab_{name}":
            _switch_view(name)
            return


def _switch_view(name: str) -> None:
    """Show the window for *name* and hide all others."""
    vp_w = dpg.get_viewport_client_width()
    vp_h = dpg.get_viewport_client_height()
    tab_bar_height = 50  # approximate tab bar + padding

    for view_name, tag in _VIEW_TAGS.items():
        if not dpg.does_item_exist(tag):
            continue
        if view_name == name:
            dpg.configure_item(tag, show=True, pos=[0, tab_bar_height],
                                width=vp_w, height=vp_h - tab_bar_height)
        else:
            dpg.configure_item(tag, show=False)

    # Sync tab bar selection
    tab_tag = f"tab_{name}"
    if dpg.does_item_exist(TAG_TAB_BAR) and dpg.does_item_exist(tab_tag):
        dpg.set_value(TAG_TAB_BAR, tab_tag)


# -- Pipeline triggers ----------------------------------------------------

def _run_pipeline(cfg: dict, group: str) -> None:
    """Switch to Runner view and kick off the pipeline."""
    _switch_view("Runner")
    # Reuse runner_view's internal start (import to call directly)
    runner_view._start_pipeline(cfg, group, lambda: _on_pipeline_done(cfg))


def _on_pipeline_done(cfg: dict) -> None:
    """Refresh dependent views after pipeline completion."""
    status_view.refresh_game_count(cfg)
    games_view.refresh(cfg)
