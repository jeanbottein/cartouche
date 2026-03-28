"""
First-run initialization wizard for the .{APP_NAME}/ config directory.

Shows a setup wizard (Dear PyGui preferred, tkinter fallback, CLI last).
The user picks a gamespace location, enables features, sets directories
(absolute or relative to the gamespace), then clicks Confirm to create
all directories and seed config.txt.

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
        "description": "DRM-free game library. Relative to gamespace or absolute.",
        "enabled": True,
    },
    {
        "key": "PATCHES_PATH",
        "label": "Mods & Patches",
        "default": "mods",
        "description": "Patches and file replacements (BPS format). Relative or absolute.",
        "enabled": False,
    },
    {
        "key": "SAVESCOPY_PATH",
        "label": "Save Copies",
        "default": "saves-copies",
        "description": "Backup copies of save files. Relative or absolute.",
        "enabled": False,
    },
    {
        "key": "SAVESLINK_PATH",
        "label": "Save Links",
        "default": "saves-links",
        "description": "Symlinks to save directories for Syncthing sync. Relative or absolute.",
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
    """Create .cartouche/, feature directories, and config.txt. Return the app dir."""
    app_dir = parent / f".{APP_NAME}"
    app_dir.mkdir(parents=True, exist_ok=True)

    # Create enabled feature directories (absolute or relative to gamespace)
    for val in (feature_paths or {}).values():
        p = Path(val)
        target = p if p.is_absolute() else parent / p
        target.mkdir(parents=True, exist_ok=True)

    # Seed config.txt (cartouche.py won't overwrite if it already exists)
    conf_path = app_dir / "config.txt"
    if not conf_path.exists():
        _write_config(conf_path, feature_paths or {})

    return app_dir


# ── Dear PyGui wizard ─────────────────────────────────────────────────────────

def _run_dpg_wizard(script_dir: Path, cwd: Path) -> Path:
    import dearpygui.dearpygui as dpg

    # Build gamespace location list (deduplicate when script_dir == cwd)
    locations: list[tuple[str, Path]] = []
    if script_dir.resolve() != cwd.resolve():
        locations.append(("Next to binary", script_dir))
    locations.append(("Current directory", cwd))

    result: list[tuple[Path, dict]] = []
    _pending_browse_key: list[str] = []  # one-element mutable cell

    # ── callbacks ─────────────────────────────────────────────────────────

    def _get_parent() -> Path:
        if dpg.get_value("portable_check"):
            return script_dir.resolve()
        raw = dpg.get_value("gamespace_path").strip() or str(cwd)
        return Path(raw).expanduser().resolve()

    def _on_portable_toggled(sender, app_data, user_data):
        """Switch gamespace path between script_dir (portable) and cwd."""
        if app_data:
            dpg.hide_item("gamespace_path")
            dpg.hide_item("gamespace_browse_btn")
        else:
            dpg.set_value("gamespace_path", str(cwd))
            dpg.show_item("gamespace_path")
            dpg.show_item("gamespace_browse_btn")

    def _on_check(sender, app_data, user_data):
        if app_data:
            dpg.show_item(f"feat_path_{user_data}")
            dpg.show_item(f"feat_browse_{user_data}")
        else:
            dpg.hide_item(f"feat_path_{user_data}")
            dpg.hide_item(f"feat_browse_{user_data}")

    def _on_browse(sender, app_data, user_data):
        _pending_browse_key.clear()
        _pending_browse_key.append(user_data)
        dpg.show_item("wiz_file_dialog")

    def _on_dir_selected(sender, app_data):
        key = _pending_browse_key[0] if _pending_browse_key else None
        chosen = app_data.get("file_path_name", "")
        if not chosen:
            return
        if key == "__gamespace__":
            dpg.set_value("gamespace_path", str(Path(chosen).resolve()))
        elif key:
            # Make relative to gamespace if possible
            try:
                rel = Path(chosen).relative_to(_get_parent())
                chosen = str(rel)
            except ValueError:
                pass  # keep absolute
            dpg.set_value(f"feat_path_{key}", chosen)

    def _on_confirm():
        parent = _get_parent()
        # Reflect the resolved absolute path back into the field
        dpg.set_value("gamespace_path", str(parent))
        paths = {}
        for feat in WIZARD_FEATURES:
            key = feat["key"]
            if dpg.get_value(f"feat_check_{key}"):
                val = dpg.get_value(f"feat_path_{key}").strip() or feat["default"]
                paths[key] = val
        result.append((parent, paths))
        dpg.stop_dearpygui()

    def _on_cancel():
        dpg.stop_dearpygui()

    # ── build UI ──────────────────────────────────────────────────────────

    dpg.create_context()

    W, H = 820, 500
    dpg.create_viewport(
        title=f"Initialize {APP_NAME.capitalize()}",
        width=W, height=H,
        resizable=True,
    )

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
            dpg.add_theme_style(dpg.mvStyleVar_FrameRounding, 4)
            dpg.add_theme_style(dpg.mvStyleVar_WindowRounding, 4)
            dpg.add_theme_style(dpg.mvStyleVar_FramePadding, 6, 3)
            dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, 6, 4)
            dpg.add_theme_style(dpg.mvStyleVar_WindowPadding, 14, 10)
            dpg.add_theme_style(dpg.mvStyleVar_IndentSpacing, 16)
    dpg.bind_theme(wiz_theme)
    dpg.set_global_font_scale(1.0)

    COL = W - 40  # content width with window padding
    PAD = 0

    with dpg.window(tag="wiz_win", no_title_bar=True, no_resize=True,
                    no_move=True, no_close=True, no_scrollbar=True,
                    no_scroll_with_mouse=True, width=W, height=H, pos=[0, 0]):

        # File dialog for directory browsing
        with dpg.file_dialog(tag="wiz_file_dialog", directory_selector=True,
                             show=False, callback=_on_dir_selected,
                             width=700, height=450):
            pass

        with dpg.group(indent=PAD):

            # ── Header ────────────────────────────────────────────────────
            ICON_SIZE = 36
            icon_path = get_icon_path()
            icon_tex = None
            if icon_path and icon_path.exists():
                try:
                    iw, ih, _, idata = dpg.load_image(str(icon_path))
                    with dpg.texture_registry():
                        icon_tex = dpg.add_static_texture(iw, ih, idata)
                except Exception:
                    pass

            with dpg.table(header_row=False, borders_innerV=False,
                           borders_outerV=False, borders_innerH=False,
                           borders_outerH=False):
                dpg.add_table_column(width_fixed=True,
                                     init_width_or_weight=ICON_SIZE + 8)
                dpg.add_table_column(width_fixed=False)

                with dpg.table_row():
                    if icon_tex is not None:
                        dpg.add_image(icon_tex, width=ICON_SIZE, height=ICON_SIZE)
                    else:
                        dpg.add_text("")
                    with dpg.group():
                        dpg.add_text(f"Welcome to {APP_NAME.capitalize()}")
                        dpg.add_text("Configure your gamespace, then click Confirm.",
                                     color=(160, 168, 180, 255))

            dpg.add_separator()
            dpg.add_spacer(height=8)

            # ── Gamespace location ─────────────────────────────────────────
            dpg.add_text("Gamespace location", color=(160, 168, 180, 255))
            dpg.add_spacer(height=4)

            LABEL_W  = 135
            BROWSE_W = 72
            PATH_W   = COL - LABEL_W - BROWSE_W - 12

            with dpg.group(horizontal=True):
                dpg.add_input_text(
                    tag="gamespace_path",
                    default_value=str(cwd),
                    width=COL - BROWSE_W - 6,
                    hint="/absolute/path",
                )
                dpg.add_button(
                    label="Browse",
                    tag="gamespace_browse_btn",
                    callback=_on_browse,
                    user_data="__gamespace__",
                    width=BROWSE_W,
                )
            dpg.add_spacer(height=4)
            dpg.add_checkbox(
                label=f"  Portable  (place .{APP_NAME}/ next to the {APP_NAME} binary)",
                tag="portable_check",
                default_value=False,
                callback=_on_portable_toggled,
            )
            dpg.add_spacer(height=4)
            dpg.add_text(
                f"A .{APP_NAME}/ directory will be created here to store settings and app files.",
                color=(110, 118, 130, 255), wrap=COL,
            )
            dpg.add_separator()
            dpg.add_spacer(height=8)

            # ── Features ──────────────────────────────────────────────────
            dpg.add_text("Directories  (relative to gamespace or absolute)",
                         color=(160, 168, 180, 255))
            dpg.add_spacer(height=4)

            for feat in WIZARD_FEATURES:
                key = feat["key"]
                with dpg.group(horizontal=True):
                    dpg.add_checkbox(
                        label="",
                        tag=f"feat_check_{key}",
                        default_value=True,
                        callback=_on_check,
                        user_data=key,
                    )
                    # Fixed-width label cell to align all path fields
                    dpg.add_text(feat["label"],
                                 tag=f"feat_label_{key}",
                                 indent=-1)
                    dpg.add_spacer(width=LABEL_W - len(feat["label"]) * 7 - 24)
                    dpg.add_input_text(
                        tag=f"feat_path_{key}",
                        default_value=feat["default"],
                        width=PATH_W,
                        hint="relative or /absolute/path",
                    )
                    dpg.add_button(
                        label="Browse",
                        tag=f"feat_browse_{key}",
                        callback=_on_browse,
                        user_data=key,
                        width=BROWSE_W,
                    )
                dpg.add_text(feat["description"], color=(110, 118, 130, 255),
                             wrap=COL)
                dpg.add_spacer(height=4)

            dpg.add_separator()
            dpg.add_spacer(height=8)

            # ── Buttons ───────────────────────────────────────────────────
            with dpg.group(horizontal=True):
                dpg.add_spacer(width=COL - 214)
                dpg.add_button(label="Cancel", width=100, callback=_on_cancel)
                dpg.add_spacer(width=4)
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
    root.resizable(True, True)

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
        text="Configure your gamespace, then click Confirm.",
        foreground="gray",
    ).pack(anchor="w", pady=(2, 8))
    ttk.Separator(outer).pack(fill="x", pady=(0, 8))

    # Location
    ttk.Label(outer, text="Gamespace location", foreground="gray").pack(anchor="w")
    loc_var = tk.IntVar(value=len(locations) - 1)
    for i, (lbl, path) in enumerate(locations):
        ttk.Radiobutton(
            outer,
            text=f"{lbl}  —  {path}",
            variable=loc_var,
            value=i,
        ).pack(anchor="w", padx=8)
    ttk.Label(
        outer,
        text=f"A .{APP_NAME}/ directory will be created here to store settings and app files.",
        foreground="gray",
        font=("TkDefaultFont", 8),
    ).pack(anchor="w", padx=8, pady=(2, 0))

    ttk.Separator(outer).pack(fill="x", pady=8)

    # Features
    ttk.Label(outer, text="Directories (relative to gamespace or absolute)",
              foreground="gray").pack(anchor="w")

    def _get_tk_parent() -> Path:
        return locations[loc_var.get()][1]

    feat_vars: dict[str, tuple[tk.BooleanVar, tk.StringVar]] = {}
    for feat in WIZARD_FEATURES:
        key = feat["key"]
        enabled_var = tk.BooleanVar(value=True)
        path_var = tk.StringVar(value=feat["default"])
        feat_vars[key] = (enabled_var, path_var)

        row = ttk.Frame(outer)
        row.pack(fill="x", pady=(6, 0))

        path_entry = ttk.Entry(row, textvariable=path_var, width=28)

        def _toggle(ev=None, ev_=enabled_var, pe=path_entry):
            pe.configure(state="normal" if ev_.get() else "disabled")

        def _browse(pv=path_var):
            chosen = filedialog.askdirectory(title="Select directory")
            if chosen:
                try:
                    rel = str(Path(chosen).relative_to(_get_tk_parent()))
                    pv.set(rel)
                except ValueError:
                    pv.set(chosen)

        ttk.Checkbutton(
            row,
            text=f"  {feat['label']}",
            variable=enabled_var,
            command=_toggle,
        ).pack(side="left")

        path_entry.pack(side="left", padx=(8, 4))
        ttk.Button(row, text="Browse", command=_browse).pack(side="left")

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

    # Gamespace location
    print(f"\nGamespace location (a .{APP_NAME}/ directory will be created here to store settings and app files):\n")
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

    # Directories
    paths = {}
    print("\nConfigure directories (relative to gamespace or absolute, Enter to keep default):\n")
    for feat in WIZARD_FEATURES:
        try:
            ans = input(f"  {feat['label']} [{feat['default']}] (leave blank to skip): ").strip()
        except EOFError:
            sys.exit(0)

        if ans.lower() in ("", feat["default"]):
            paths[feat["key"]] = feat["default"]
        elif ans.lower() not in ("n", "no", "skip"):
            paths[feat["key"]] = ans
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
