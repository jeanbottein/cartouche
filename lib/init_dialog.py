"""
First-run initialization dialog for the .{APP_NAME}/ config directory.

When no existing .{APP_NAME}/ directory is found, presents the user with
two location choices. Tries a tkinter GUI first; falls back to a plain
CLI prompt on headless systems or when tkinter is unavailable.

Public API:
    run_init_dialog(script_dir, cwd) -> Path
        Creates and returns the chosen .{APP_NAME}/ directory path.
        Calls sys.exit(0) if the user declines to initialize.
"""

import shutil
import sys
from pathlib import Path

from .app import APP_NAME, get_script_dir, get_icon_path


# ── Default config template ───────────────────────────────────────────────────

def _find_default_conf() -> Path | None:
    """Locate the bundled {APP_NAME}-default.conf (MEIPASS then script dir)."""
    if getattr(sys, 'frozen', False):
        candidate = Path(sys._MEIPASS) / 'lib' / f"{APP_NAME}-default.conf"
        if candidate.exists():
            return candidate
    candidate = get_script_dir() / 'lib' / f"{APP_NAME}-default.conf"
    return candidate if candidate.exists() else None


def _initialize_dir(parent: Path) -> Path:
    """Create parent/.{APP_NAME}/, seed the conf file, return the dir."""
    app_dir = parent / f".{APP_NAME}"
    app_dir.mkdir(parents=True, exist_ok=True)
    conf_dst = app_dir / f"{APP_NAME}.conf"
    if not conf_dst.exists():
        default = _find_default_conf()
        if default:
            shutil.copy2(default, conf_dst)
        else:
            conf_dst.touch()
    return app_dir


# ── GUI dialog ────────────────────────────────────────────────────────────────

def _run_gui(script_dir: Path, cwd: Path) -> Path:
    import tkinter as tk
    from tkinter import ttk

    choices = [
        (f"Next to binary — {script_dir}", script_dir),
        (f"Current directory — {cwd}", cwd),
    ]

    result: list[Path] = []

    root = tk.Tk()
    root.title(f"Initialize {APP_NAME}")
    root.resizable(False, False)

    icon_path = get_icon_path()
    if icon_path:
        try:
            full_img = tk.PhotoImage(file=str(icon_path))
            # 512 / 8 = 64px, much better for a dialog
            icon_img = full_img.subsample(8)
            root.iconphoto(True, icon_img)
        except Exception:
            icon_img = None
    else:
        icon_img = None

    frame = ttk.Frame(root, padding=16)
    frame.grid(sticky="nsew")

    if icon_img:
        # Display icon at the top
        icon_label = ttk.Label(frame, image=icon_img)
        icon_label.grid(row=0, column=0, columnspan=2, pady=(0, 12))
        
        # Attribution
        attr_label = ttk.Label(
            frame, 
            text="Game cartridge icons created by Freepik - Flaticon",
            font=("TkDefaultFont", 8),
            foreground="gray"
        )
        attr_label.grid(row=5, column=0, columnspan=2, pady=(8, 0))

    ttk.Label(
        frame,
        text=f"No .{APP_NAME}/ directory found.\nWhere should it be initialized?",
        justify="left",
    ).grid(row=1, column=0, columnspan=2, sticky="w", pady=(0, 12))

    var = tk.IntVar(value=0)
    for i, (label, _) in enumerate(choices):
        ttk.Radiobutton(frame, text=label, variable=var, value=i).grid(
            row=i + 1, column=0, columnspan=2, sticky="w"
        )

    def on_init():
        result.append(choices[var.get()][1])
        root.destroy()

    def on_exit():
        root.destroy()
        sys.exit(0)

    btn_frame = ttk.Frame(frame)
    btn_frame.grid(row=len(choices) + 1, column=0, columnspan=2, pady=(16, 0))
    ttk.Button(btn_frame, text="Initialize", command=on_init).grid(row=0, column=0, padx=4)
    ttk.Button(btn_frame, text="Exit", command=on_exit).grid(row=0, column=1, padx=4)

    root.mainloop()

    if not result:
        sys.exit(0)
    return _initialize_dir(result[0])


# ── CLI fallback ──────────────────────────────────────────────────────────────

def _run_cli(script_dir: Path, cwd: Path) -> Path:
    choices = [
        (f"Next to binary  ({script_dir})", script_dir),
        (f"Current directory  ({cwd})", cwd),
    ]

    print(f"\nNo .{APP_NAME}/ directory found.")
    print("Where should it be initialized?\n")
    for i, (label, _) in enumerate(choices, 1):
        print(f"  {i}. {label}")
    print("  q. Exit\n")

    while True:
        try:
            answer = input("Choice [1]: ").strip() or "1"
        except EOFError:
            sys.exit(0)

        if answer.lower() == "q":
            sys.exit(0)
        if answer.isdigit() and 1 <= int(answer) <= len(choices):
            return _initialize_dir(choices[int(answer) - 1][1])
        print(f"Please enter a number between 1 and {len(choices)}, or q.")


# ── Public entry point ────────────────────────────────────────────────────────

def run_init_dialog(script_dir: Path, cwd: Path) -> Path:
    """
    Show the initialization dialog (GUI preferred, CLI fallback).
    Creates and returns the chosen .{APP_NAME}/ directory.
    Calls sys.exit(0) if the user declines.
    """
    try:
        return _run_gui(script_dir, cwd)
    except Exception:
        return _run_cli(script_dir, cwd)
