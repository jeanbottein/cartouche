"""
Dark terminal-style theme for Dear PyGui.

Palette derived from the app icon: dark amber for window chrome,
green accents for buttons/interactive elements, blue for tab selection.
Rounded corners and readable fonts sized for 1280x800 (Steam Deck).
"""

import dearpygui.dearpygui as dpg

# -- Color palette --------------------------------------------------------
BG_DARKEST = (18, 13, 8, 255)
BG_DARK = (26, 19, 12, 255)
BG_MID = (38, 28, 18, 255)
BG_LIGHT = (52, 40, 26, 255)

TEXT_PRIMARY = (230, 235, 242, 255)
TEXT_SECONDARY = (160, 168, 180, 255)
TEXT_MUTED = (110, 118, 130, 255)

# Blue — tabs only
ACCENT = (86, 156, 214, 255)
ACCENT_HOVER = (106, 176, 234, 255)
ACCENT_ACTIVE = (66, 136, 194, 255)

# Green — buttons, checkmarks, sliders (from icon's green face)
GREEN = (38, 168, 108, 255)
GREEN_HOVER = (54, 195, 128, 255)
GREEN_ACTIVE = (26, 138, 86, 255)

# Amber — title bar chrome (from icon's orange body, kept very dark)
AMBER_DARK = (60, 30, 6, 255)
AMBER = (160, 82, 16, 255)

SUCCESS = (78, 185, 120, 255)
ERROR = (214, 86, 86, 255)
WARNING = (214, 180, 86, 255)

BORDER = (65, 48, 30, 255)
SCROLLBAR = (32, 24, 15, 255)
SCROLLBAR_GRAB = (70, 52, 32, 255)

# -- Sizing constants -----------------------------------------------------
ROUNDING = 6
FRAME_PADDING = (10, 6)
ITEM_SPACING = (10, 8)
WINDOW_PADDING = (16, 16)


def apply_theme() -> int:
    """Create and bind a global Steam-like dark theme. Returns the theme id."""

    with dpg.theme() as global_theme:
        with dpg.theme_component(dpg.mvAll):
            # Window
            dpg.add_theme_color(dpg.mvThemeCol_WindowBg, BG_DARKEST)
            dpg.add_theme_color(dpg.mvThemeCol_ChildBg, BG_DARK)
            dpg.add_theme_color(dpg.mvThemeCol_PopupBg, BG_MID)
            dpg.add_theme_color(dpg.mvThemeCol_MenuBarBg, BG_DARK)

            # Text
            dpg.add_theme_color(dpg.mvThemeCol_Text, TEXT_PRIMARY)
            dpg.add_theme_color(dpg.mvThemeCol_TextDisabled, TEXT_MUTED)

            # Borders
            dpg.add_theme_color(dpg.mvThemeCol_Border, BORDER)
            dpg.add_theme_color(dpg.mvThemeCol_BorderShadow, (0, 0, 0, 0))

            # Frame (inputs, combos, etc.)
            dpg.add_theme_color(dpg.mvThemeCol_FrameBg, BG_MID)
            dpg.add_theme_color(dpg.mvThemeCol_FrameBgHovered, BG_LIGHT)
            dpg.add_theme_color(dpg.mvThemeCol_FrameBgActive, BG_LIGHT)

            # Title bar (amber tint — from icon's orange body)
            dpg.add_theme_color(dpg.mvThemeCol_TitleBg, AMBER_DARK)
            dpg.add_theme_color(dpg.mvThemeCol_TitleBgActive, AMBER_DARK)
            dpg.add_theme_color(dpg.mvThemeCol_TitleBgCollapsed, BG_DARKEST)

            # Buttons (green — from icon's green face)
            dpg.add_theme_color(dpg.mvThemeCol_Button, GREEN)
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, GREEN_HOVER)
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, GREEN_ACTIVE)

            # Headers / tabs
            dpg.add_theme_color(dpg.mvThemeCol_Header, BG_MID)
            dpg.add_theme_color(dpg.mvThemeCol_HeaderHovered, BG_LIGHT)
            dpg.add_theme_color(dpg.mvThemeCol_HeaderActive, ACCENT_ACTIVE)
            dpg.add_theme_color(dpg.mvThemeCol_Tab, BG_MID)
            dpg.add_theme_color(dpg.mvThemeCol_TabHovered, ACCENT_HOVER)
            dpg.add_theme_color(dpg.mvThemeCol_TabActive, ACCENT)
            dpg.add_theme_color(dpg.mvThemeCol_TabUnfocused, BG_DARK)
            dpg.add_theme_color(dpg.mvThemeCol_TabUnfocusedActive, BG_MID)

            # Table
            dpg.add_theme_color(dpg.mvThemeCol_TableHeaderBg, BG_MID)
            dpg.add_theme_color(dpg.mvThemeCol_TableBorderStrong, BORDER)
            dpg.add_theme_color(dpg.mvThemeCol_TableBorderLight, (65, 48, 30, 128))
            dpg.add_theme_color(dpg.mvThemeCol_TableRowBg, BG_DARK)
            dpg.add_theme_color(dpg.mvThemeCol_TableRowBgAlt, BG_DARKEST)

            # Scrollbar
            dpg.add_theme_color(dpg.mvThemeCol_ScrollbarBg, SCROLLBAR)
            dpg.add_theme_color(dpg.mvThemeCol_ScrollbarGrab, SCROLLBAR_GRAB)
            dpg.add_theme_color(dpg.mvThemeCol_ScrollbarGrabHovered, BG_LIGHT)
            dpg.add_theme_color(dpg.mvThemeCol_ScrollbarGrabActive, GREEN)

            # Separator / resize grip
            dpg.add_theme_color(dpg.mvThemeCol_Separator, BORDER)
            dpg.add_theme_color(dpg.mvThemeCol_ResizeGrip, (0, 0, 0, 0))
            dpg.add_theme_color(dpg.mvThemeCol_ResizeGripHovered, GREEN_HOVER)
            dpg.add_theme_color(dpg.mvThemeCol_ResizeGripActive, GREEN)

            # Check / slider (green)
            dpg.add_theme_color(dpg.mvThemeCol_CheckMark, GREEN)
            dpg.add_theme_color(dpg.mvThemeCol_SliderGrab, GREEN)
            dpg.add_theme_color(dpg.mvThemeCol_SliderGrabActive, GREEN_HOVER)

            # Progress bar (green)
            dpg.add_theme_color(dpg.mvThemeCol_PlotHistogram, GREEN)

            # -- Style --
            dpg.add_theme_style(dpg.mvStyleVar_WindowRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_ChildRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_FrameRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_PopupRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_ScrollbarRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_GrabRounding, ROUNDING)
            dpg.add_theme_style(dpg.mvStyleVar_TabRounding, ROUNDING)

            dpg.add_theme_style(dpg.mvStyleVar_FramePadding, *FRAME_PADDING)
            dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, *ITEM_SPACING)
            dpg.add_theme_style(dpg.mvStyleVar_WindowPadding, *WINDOW_PADDING)
            dpg.add_theme_style(dpg.mvStyleVar_ScrollbarSize, 14)
            dpg.add_theme_style(dpg.mvStyleVar_IndentSpacing, 20)

    dpg.bind_theme(global_theme)
    return global_theme


def set_global_scale(scale: float = 1.0) -> None:
    """Adjust the global UI scale (useful for high-DPI or Steam Deck)."""
    dpg.set_global_font_scale(scale)
