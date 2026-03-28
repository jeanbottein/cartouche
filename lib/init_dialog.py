"""
First-run initialization wizard for the .{APP_NAME}/ config directory.

Shows a setup wizard (Dear PyGui preferred, tkinter fallback, CLI last).
The user enables features, optionally customises folder names, then clicks
Confirm to create all directories and seed config.txt.

Public API:
    run_init_dialog(script_dir, cwd) -> Path
        Creates and returns the chosen .{APP_NAME}/ directory.
        Calls sys.exit(0) if the user cancels.
"""

import sys
import shutil
from pathlib import Path

from .app import APP_NAME, get_script_dir, get_icon_path


# ── Feature definitions ──────────────────────────────────────────────────────

WIZARD_FEATURES = [
    {
        "key": "FREEGAMES_PATH",
        "label": "Games",
        "default": "games",
        "description": (
            "Your DRM-free game library. Cartouche will scan this folder,\n"
            "detect executables, and fetch artwork from SteamGridDB."
        ),
        "enabled": True,
    },
    {
        "key": "PATCHES_PATH",
        "label": "Mods & Patches",
        "default": "mods",
        "description": (
            "Patches and file replacements for your games (BPS format)."
        ),
        "enabled": False,
    },
    {
        "key": "SAVESCOPY_PATH",
        "label": "Save Copies",
        "default": "saves-copies",
        "description": (
            "Backup copies of your save files.\n"
            "Cartouche copies them here after each run."
        ),
        "enabled": False,
    },
    {
        "key": "SAVESLINK_PATH",
        "label": "Save Links",
        "default": "saves-links",
        "description": (
            "A folder of symlinks pointing to your save directories.\n"
            "Works with Syncthing or similar tools for cloud sync."
        ),
        "enabled": False,
    },
]


# ── Config helpers ────────────────────────────────────────────────────────────

def _find_default_conf() -> Path | None:
    """Locate the bundled config-default.txt (MEIPASS then script dir)."""
    candidates = []
    if getattr(sys, 'frozen', False):
        candidates.append(Path(sys._MEIPASS) / 'lib' / 'config-default.txt')
    candidates.append(get_script_dir() / 'lib' / 'config-default.txt')
    for c in candidates:
        if c.exists():
            return c
    return None


def _write_config(conf_path: Path, feature_paths: dict) -> None:
    """Write config.txt with wizard-configured paths at the top followed by defaults."""
    parts = []
    if feature_paths:
        parts.append("# Paths configured by the setup wizard\n")
        for key, rel in feature_paths.items():
            parts.append(f"{key}={rel}\n")
        parts.append("\n")

    default = _find_default_conf()
    if default:
        parts.append(default.read_text())

    conf_path.write_text("".join(parts))


def _initialize_dir(parent: Path, feature_paths: dict | None = None) -> Path:
    """Create .cartouche/, feature folders, and config.txt. Return the app dir."""
    app_dir = parent / f".{APP_NAME}"
    app_dir.mkdir(parents=True, exist_ok=True)

    # Create enabled feature folders next to .cartouche/
    for rel in (feature_paths or {}).values():
        (parent / rel).mkdir(parents=True, exist_ok=True)

    # Seed config.txt (cartouche.py won't overwrite if it already exists)
    conf_path = app_dir / "config.txt"
    if not conf_path.exists():
        _write_config(conf_path, feature_paths or {})

    return app_dir


# ── Dear PyGui wizard ─────────────────────────────────────────────────────────

def _run_dpg_wizard(script_dir: Path, cwd: Path) -> Path:
    import dearpygui.dearpygui as dpg

    # Build location list (deduplicate when script_dir == cwd)
    locations: list[tuple[str, Path]] = []
    if script_dir.resolve() != cwd.resolve():
        locations.append(("Next to binary", script_dir))
    locations.append(("Current directory", cwd))

    loc_labels = [f"{lbl}  —  {path}" for lbl, path in locations]
    default_loc = loc_labels[-1]

    result: list[tuple[Path, dict]] = []

    # ── callbacks ─────────────────────────────────────────────────────────

    def _get_parent() -> Path:
        sel = dpg.get_value("loc_radio")
        try:
            idx = loc_labels.index(sel)
        except ValueError:
            idx = len(locations) - 1
        return locations[idx][1]

    def _on_check(sender, app_data, user_data):
        dpg.configure_item(f"feat_path_{user_data}", enabled=app_data)

    def _on_confirm():
        parent = _get_parent()
        paths = {}
        for feat in WIZARD_FEATURES:
            key = feat["key"]
            if dpg.get_value(f"feat_check_{key}"):
                rel = dpg.get_value(f"feat_path_{key}").strip() or feat["default"]
                paths[key] = rel
        result.append((parent, paths))
        dpg.stop_dearpygui()

    def _on_cancel():
        dpg.stop_dearpygui()

    # ── build UI ──────────────────────────────────────────────────────────

    dpg.create_context()

    W, H = 700, 680
    dpg.create_viewport(
        title=f"Initialize {APP_NAME.capitalize()}",
        width=W, height=H,
        min_width=W, max_width=W,
        min_height=H, max_height=H,
        resizable=False,
    )

    # Apply a simple dark theme matching the main app
    with dpg.theme() as wiz_theme:
        with dpg.theme_component(dpg.mvAll):
            dpg.add_theme_color(dpg.mvThemeCol_WindowBg,      (23, 26, 33, 255))
            dpg.add_theme_color(dpg.mvThemeCol_FrameBg,       (42, 46, 56, 255))
            dpg.add_theme_color(dpg.mvThemeCol_FrameBgHovered,(55, 60, 72, 255))
            dpg.add_theme_color(dpg.mvThemeCol_Button,        (86, 156, 214, 255))
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, (106, 176, 234, 255))
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive,  (66, 136, 194, 255))
            dpg.add_theme_color(dpg.mvThemeCol_Text,          (230, 235, 242, 255))
            dpg.add_theme_color(dpg.mvThemeCol_TextDisabled,  (110, 118, 130, 255))
            dpg.add_theme_color(dpg.mvThemeCol_CheckMark,     (86, 156, 214, 255))
            dpg.add_theme_color(dpg.mvThemeCol_Border,        (60, 65, 78, 255))
            dpg.add_theme_color(dpg.mvThemeCol_Separator,     (60, 65, 78, 255))
            dpg.add_theme_style(dpg.mvStyleVar_FrameRounding, 6)
            dpg.add_theme_style(dpg.mvStyleVar_WindowRounding, 6)
            dpg.add_theme_style(dpg.mvStyleVar_FramePadding, 10, 6)
            dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, 10, 8)
            dpg.add_theme_style(dpg.mvStyleVar_WindowPadding, 20, 16)
            dpg.add_theme_style(dpg.mvStyleVar_IndentSpacing, 22)
    dpg.bind_theme(wiz_theme)
    dpg.set_global_font_scale(1.1)

    with dpg.window(tag="wiz_win", no_title_bar=True, no_resize=True,
                    no_move=True, no_close=True, width=W, height=H, pos=[0, 0]):

        # ── Header ────────────────────────────────────────────────────────
        dpg.add_spacer(height=6)
        dpg.add_text(f"Welcome to {APP_NAME.capitalize()}")
        dpg.add_text(
            "Let's set up your workspace. Enable the features you want and\n"
            "adjust folder names if needed, then click Confirm.",
            color=(160, 168, 180, 255),
        )
        dpg.add_spacer(height=10)
        dpg.add_separator()
        dpg.add_spacer(height=10)

        # ── Location ──────────────────────────────────────────────────────
        dpg.add_text("Workspace location", color=(160, 168, 180, 255))
        dpg.add_spacer(height=4)
        dpg.add_radio_button(
            items=loc_labels,
            tag="loc_radio",
            default_value=default_loc,
            indent=4,
        )
        dpg.add_spacer(height=10)
        dpg.add_separator()
        dpg.add_spacer(height=10)

        # ── Features ──────────────────────────────────────────────────────
        dpg.add_text("Features", color=(160, 168, 180, 255))
        dpg.add_spacer(height=6)

        for feat in WIZARD_FEATURES:
            key = feat["key"]

            dpg.add_checkbox(
                label=f"  {feat['label']}",
                tag=f"feat_check_{key}",
                default_value=feat["enabled"],
                callback=_on_check,
                user_data=key,
            )
            with dpg.group(indent=22):
                dpg.add_text(feat["description"], color=(130, 138, 150, 255), wrap=W - 80)
                dpg.add_spacer(height=2)
                with dpg.group(horizontal=True):
                    dpg.add_text("Folder name:", color=(160, 168, 180, 255))
                    dpg.add_input_text(
                        tag=f"feat_path_{key}",
                        default_value=feat["default"],
                        width=W - 240,
                        enabled=feat["enabled"],
                    )
            dpg.add_spacer(height=8)

        dpg.add_separator()
        dpg.add_spacer(height=10)

        # ── Buttons ───────────────────────────────────────────────────────
        with dpg.group(horizontal=True):
            dpg.add_spacer(width=W - 260)
            dpg.add_button(label="Cancel", width=100, callback=_on_cancel)
            dpg.add_spacer(width=8)
            dpg.add_button(label="Confirm", width=110, callback=_on_confirm)

    dpg.set_primary_window("wiz_win", True)
    dpg.setup_dearpygui()
    dpg.show_viewport()
    dpg.start_dearpygui()
    dpg.destroy_context()

    if not result:
        sys.exit(0)

    parent, paths = result[0]
    return _initialize_dir(parent, paths)


# ── tkinter fallback ──────────────────────────────────────────────────────────

def _run_tkinter_wizard(script_dir: Path, cwd: Path) -> Path:
    import tkinter as tk
    from tkinter import ttk, filedialog

    locations: list[tuple[str, Path]] = []
    if script_dir.resolve() != cwd.resolve():
        locations.append(("Next to binary", script_dir))
    locations.append(("Current directory", cwd))

    result: list[tuple[Path, dict]] = []

    root = tk.Tk()
    root.title(f"Initialize {APP_NAME.capitalize()}")
    root.resizable(False, False)

    icon_path = get_icon_path()
    if icon_path:
        try:
            full_img = tk.PhotoImage(file=str(icon_path))
            icon_img = full_img.subsample(8)
            root.iconphoto(True, icon_img)
        except Exception:
            pass

    outer = ttk.Frame(root, padding=16)
    outer.pack(fill="both", expand=True)

    # Header
    ttk.Label(
        outer,
        text=f"Welcome to {APP_NAME.capitalize()}",
        font=("TkDefaultFont", 12, "bold"),
    ).pack(anchor="w")
    ttk.Label(
        outer,
        text=(
            "Enable the features you want and adjust folder names,\n"
            "then click Confirm."
        ),
        foreground="gray",
    ).pack(anchor="w", pady=(2, 8))
    ttk.Separator(outer).pack(fill="x", pady=(0, 8))

    # Location
    ttk.Label(outer, text="Workspace location", foreground="gray").pack(anchor="w")
    loc_var = tk.IntVar(value=len(locations) - 1)
    for i, (lbl, path) in enumerate(locations):
        ttk.Radiobutton(
            outer,
            text=f"{lbl}  —  {path}",
            variable=loc_var,
            value=i,
        ).pack(anchor="w", padx=8)

    ttk.Separator(outer).pack(fill="x", pady=8)

    # Features
    ttk.Label(outer, text="Features", foreground="gray").pack(anchor="w")

    feat_vars: dict[str, tuple[tk.BooleanVar, tk.StringVar]] = {}
    for feat in WIZARD_FEATURES:
        key = feat["key"]
        enabled_var = tk.BooleanVar(value=feat["enabled"])
        path_var = tk.StringVar(value=feat["default"])
        feat_vars[key] = (enabled_var, path_var)

        row = ttk.Frame(outer)
        row.pack(fill="x", pady=(6, 0))

        path_entry = ttk.Entry(row, textvariable=path_var, width=22)

        def _toggle(ev=None, ev_=enabled_var, pe=path_entry):
            pe.configure(state="normal" if ev_.get() else "disabled")

        ttk.Checkbutton(
            row,
            text=f"  {feat['label']}",
            variable=enabled_var,
            command=_toggle,
        ).pack(side="left")

        ttk.Label(row, text="Folder:").pack(side="left", padx=(16, 4))
        path_entry.pack(side="left")
        if not feat["enabled"]:
            path_entry.configure(state="disabled")

        ttk.Label(
            outer,
            text=feat["description"],
            foreground="gray",
            font=("TkDefaultFont", 8),
        ).pack(anchor="w", padx=28, pady=(0, 4))

    ttk.Separator(outer).pack(fill="x", pady=8)

    def _on_confirm():
        idx = loc_var.get()
        parent = locations[idx][1]
        paths = {}
        for feat in WIZARD_FEATURES:
            key = feat["key"]
            ev, pv = feat_vars[key]
            if ev.get():
                rel = pv.get().strip() or feat["default"]
                paths[key] = rel
        result.append((parent, paths))
        root.destroy()

    def _on_cancel():
        root.destroy()

    btn_frame = ttk.Frame(outer)
    btn_frame.pack(anchor="e")
    ttk.Button(btn_frame, text="Cancel", command=_on_cancel).pack(side="left", padx=4)
    ttk.Button(btn_frame, text="Confirm", command=_on_confirm).pack(side="left", padx=4)

    root.mainloop()

    if not result:
        sys.exit(0)

    parent, paths = result[0]
    return _initialize_dir(parent, paths)


# ── CLI fallback ──────────────────────────────────────────────────────────────

def _run_cli(script_dir: Path, cwd: Path) -> Path:
    locations: list[tuple[str, Path]] = []
    if script_dir.resolve() != cwd.resolve():
        locations.append(("Next to binary", script_dir))
    locations.append(("Current directory", cwd))

    print(f"\nInitialize {APP_NAME.capitalize()}")
    print("=" * 40)

    # Location
    print("\nWhere should .cartouche/ be created?\n")
    for i, (lbl, path) in enumerate(locations, 1):
        print(f"  {i}. {lbl}  ({path})")
    print("  q. Exit\n")

    while True:
        try:
            ans = input(f"Choice [{len(locations)}]: ").strip() or str(len(locations))
        except EOFError:
            sys.exit(0)
        if ans.lower() == "q":
            sys.exit(0)
        if ans.isdigit() and 1 <= int(ans) <= len(locations):
            parent = locations[int(ans) - 1][1]
            break
        print(f"  Please enter 1–{len(locations)} or q.")

    # Features
    paths = {}
    print("\nEnable features (Enter to keep default, leave blank to skip):\n")
    for feat in WIZARD_FEATURES:
        default_yn = "Y/n" if feat["enabled"] else "y/N"
        try:
            ans = input(f"  {feat['label']} [{default_yn}]: ").strip().lower()
        except EOFError:
            sys.exit(0)

        enabled = feat["enabled"] if not ans else ans in ("y", "yes")
        if enabled:
            try:
                rel = input(f"    Folder name [{feat['default']}]: ").strip()
            except EOFError:
                sys.exit(0)
            paths[feat["key"]] = rel or feat["default"]
        print()

    return _initialize_dir(parent, paths)


# ── Public entry point ────────────────────────────────────────────────────────

def run_init_dialog(script_dir: Path, cwd: Path) -> Path:
    """
    Show the initialization wizard (DPG preferred, tkinter fallback, CLI last).
    Creates and returns the chosen .{APP_NAME}/ directory.
    Calls sys.exit(0) if the user cancels.
    """
    try:
        return _run_dpg_wizard(script_dir, cwd)
    except Exception:
        pass
    try:
        return _run_tkinter_wizard(script_dir, cwd)
    except Exception:
        pass
    return _run_cli(script_dir, cwd)
