"""
Main GUI entry point for Cartouche.

Creates the Dear PyGui context, viewport, navigation tab bar, and
wires each view module together.  Call ``run_gui(cfg)`` to launch.
"""

from __future__ import annotations

from typing import Any, Callable

import dearpygui.dearpygui as dpg

from lib.gui import theme, status_view, games_view, settings_view, controller

VIEWPORT_WIDTH = 1280
VIEWPORT_HEIGHT = 800
VIEWPORT_TITLE = "Cartouche"

TAG_PRIMARY = "primary_window"
TAG_TAB_BAR = "main_tab_bar"

_VIEW_TAGS: dict[str, str] = {
    "Status": status_view.TAG_WINDOW,
    "Games": games_view.TAG_WINDOW,
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
    theme.set_global_scale(1.0)

    # -- Build views (order matters: tags must exist before tab bar) ------
    _build_status_view(cfg)
    _build_games_view(cfg)
    _build_settings_view(cfg)

    # -- Primary window with tab bar navigation ---------------------------
    with dpg.window(tag=TAG_PRIMARY):
        with dpg.tab_bar(tag=TAG_TAB_BAR, callback=_on_tab_changed):
            dpg.add_tab(label="Status", tag="tab_Status")
            dpg.add_tab(label="Games", tag="tab_Games")
            dpg.add_tab(label="Settings", tag="tab_Settings")

    dpg.set_primary_window(TAG_PRIMARY, True)

    # -- Start Dear PyGui -------------------------------------------------
    dpg.setup_dearpygui()
    dpg.show_viewport()

    # -- Gamepad controller -----------------------------------------------
    controller.configure(
        view_names=list(_VIEW_TAGS.keys()),
        switch_view_callback=_switch_view,
    )

    # Show default view (after viewport is shown so sizes are correct)
    _switch_view("Status")

    # Handle resizing to keep the active view full-screen
    dpg.set_viewport_resize_callback(_on_viewport_resize)

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


def _build_games_view(cfg: dict) -> None:
    games_view.create(cfg=cfg)


def _build_settings_view(cfg: dict) -> None:
    """Build the settings view using the new settings_view module."""
    settings_view.create(
        cfg=cfg,
        on_saved=lambda: _on_settings_saved(cfg),
    )


def _on_settings_saved(cfg: dict) -> None:
    """Callback after settings are saved."""
    # We might need to reload the config if other parts of the app depend on it.
    # For now, we'll just show a message.
    pass


# -- Navigation -----------------------------------------------------------

def _on_tab_changed(sender: int | str, app_data: int | str, user_data: Any = None) -> None:
    """Called when the user clicks a tab."""
    # app_data is usually the tag of the selected tab.
    # We check if it matches the string tag or if it's an alias.
    for name in _VIEW_TAGS:
        target_tab = f"tab_{name}"
        if app_data == target_tab or dpg.get_item_alias(app_data) == target_tab:
            _switch_view(name)
            return


def _on_viewport_resize() -> None:
    """Ensure the active view remains correctly sized when the window changes."""
    # Find which view is currently shown
    for name, tag in _VIEW_TAGS.items():
        if dpg.does_item_exist(tag) and dpg.is_item_shown(tag):
            _switch_view(name)
            break


def _switch_view(name: str) -> None:
    """Show the window for *name* and hide all others."""
    vp_w = dpg.get_viewport_client_width()
    vp_h = dpg.get_viewport_client_height()
    tab_bar_height = 36  # approximate tab bar + padding

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
    """Kick off the pipeline on the Status view."""
    status_view.start_pipeline(cfg, group, lambda: _on_pipeline_done(cfg))


def _on_pipeline_done(cfg: dict) -> None:
    """Refresh dependent views after pipeline completion."""
    status_view.refresh_game_count(cfg)
    games_view.refresh(cfg)
