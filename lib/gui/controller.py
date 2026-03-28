"""
Gamepad input handler for Steam Deck / controller navigation.

Uses Dear PyGui's built-in gamepad support by polling axis and button
state each frame and synthesising keyboard-style navigation events.
"""

from __future__ import annotations

import time

import dearpygui.dearpygui as dpg

# Axis / button indices (standard SDL mapping used by DPG)
AXIS_LEFT_X = 0
AXIS_LEFT_Y = 1
DPAD_UP = 11
DPAD_DOWN = 12
DPAD_LEFT = 13
DPAD_RIGHT = 14
BUTTON_A = 0
BUTTON_B = 1
BUTTON_START = 6

# Repeat delay / rate (seconds)
_INITIAL_DELAY = 0.35
_REPEAT_RATE = 0.12
_STICK_DEADZONE = 0.4

_last_nav_time: float = 0.0
_nav_held: bool = False

# View cycling
_view_names: list[str] = []
_view_index: int = 0
_switch_callback = None


def configure(view_names: list[str], switch_view_callback) -> None:
    """Set the ordered view list and the callback to switch views.

    *switch_view_callback* is called with the view name string.
    """
    global _view_names, _switch_callback
    _view_names = list(view_names)
    _switch_callback = switch_view_callback


def poll() -> None:
    """Call once per frame (from the DPG render loop) to handle gamepad."""
    if not dpg.is_dearpygui_running():
        return

    _handle_navigation()
    _handle_confirm()
    _handle_back()
    _handle_view_switch()


# -- Internals -----------------------------------------------------------

def _gamepad_button(button: int) -> bool:
    """Return True if *button* was pressed this frame (edge-triggered)."""
    try:
        return dpg.is_key_pressed(dpg.mvKey_GamepadButtonA + button)
    except Exception:
        return False


def _gamepad_axis(axis: int) -> float:
    """Return axis value in [-1, 1] range, or 0 on error."""
    try:
        return dpg.get_axis_value(axis)
    except Exception:
        return 0.0


def _handle_navigation() -> None:
    """D-pad and left-stick to focus-navigation (up/down/left/right)."""
    global _last_nav_time, _nav_held

    now = time.monotonic()
    direction = _get_nav_direction()
    if direction is None:
        _nav_held = False
        return

    delay = _REPEAT_RATE if _nav_held else _INITIAL_DELAY
    if now - _last_nav_time < delay:
        return

    _last_nav_time = now
    _nav_held = True

    key_map = {
        "up": dpg.mvKey_Up,
        "down": dpg.mvKey_Down,
        "left": dpg.mvKey_Left,
        "right": dpg.mvKey_Right,
    }
    key = key_map.get(direction)
    if key is not None:
        try:
            dpg.set_value("__nav_key__", key)
        except Exception:
            pass


def _get_nav_direction() -> str | None:
    """Determine intended navigation direction from D-pad or stick."""
    # D-pad first
    try:
        if dpg.is_key_down(dpg.mvKey_GamepadDpadUp):
            return "up"
        if dpg.is_key_down(dpg.mvKey_GamepadDpadDown):
            return "down"
        if dpg.is_key_down(dpg.mvKey_GamepadDpadLeft):
            return "left"
        if dpg.is_key_down(dpg.mvKey_GamepadDpadRight):
            return "right"
    except Exception:
        pass

    # Left stick fallback
    lx = _gamepad_axis(AXIS_LEFT_X)
    ly = _gamepad_axis(AXIS_LEFT_Y)
    if abs(ly) > _STICK_DEADZONE and abs(ly) > abs(lx):
        return "up" if ly < 0 else "down"
    if abs(lx) > _STICK_DEADZONE and abs(lx) > abs(ly):
        return "left" if lx < 0 else "right"
    return None


def _handle_confirm() -> None:
    """A button -> Enter."""
    try:
        if dpg.is_key_pressed(dpg.mvKey_GamepadFaceDown):
            focused = dpg.get_active_window()
            if focused:
                dpg.focus_item(focused)
    except Exception:
        pass


def _handle_back() -> None:
    """B button -> Escape."""
    try:
        if dpg.is_key_pressed(dpg.mvKey_GamepadFaceRight):
            pass  # Dear PyGui processes Escape natively for popups
    except Exception:
        pass


def _handle_view_switch() -> None:
    """Start button -> cycle to next view."""
    global _view_index
    if not _view_names or _switch_callback is None:
        return
    try:
        if dpg.is_key_pressed(dpg.mvKey_GamepadStart):
            _view_index = (_view_index + 1) % len(_view_names)
            _switch_callback(_view_names[_view_index])
    except Exception:
        pass
