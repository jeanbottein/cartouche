"""
Single source of truth for the application name.

Derived from the binary/script filename: everything before the first
'-' or '.'. This lets distribution files live in the same folder with
suffixes (e.g. cartouche-linux-x86_64, cartouche.v2) while still
resolving to "cartouche". Falls back to "cartouche" if the result is empty.
"""
import re
import sys
from pathlib import Path

if getattr(sys, 'frozen', False):
    _filename = Path(sys.executable).name
else:
    _filename = Path(sys.argv[0]).name

APP_NAME = re.split(r'[-.]', _filename)[0] or "cartouche"


def get_script_dir() -> Path:
    """Return the directory containing the binary (frozen) or the entry-point script."""
    if getattr(sys, 'frozen', False):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parent.parent


def _is_app_dir(p: Path) -> bool:
    """True if p looks like an app config dir (not a per-game data dir)."""
    return p.is_dir() and not (p / "game.json").exists()


def find_app_dir(cli_dir: Path | None = None) -> Path | None:
    """
    Locate the .{APP_NAME}/ config directory.

    Resolution order (first valid match wins):
      1. cli_dir/.{APP_NAME}/      explicit CLI arg
      2. binary_dir/.{APP_NAME}/   portable mode
      3. cwd/.{APP_NAME}/          ambient fallback

    A candidate is rejected if it contains game.json (per-game data dir).
    Returns None when no valid directory is found.
    """
    candidates = []
    if cli_dir:
        candidates.append(cli_dir / f".{APP_NAME}")
    candidates.append(get_script_dir() / f".{APP_NAME}")
    candidates.append(Path.cwd() / f".{APP_NAME}")
    return next((p for p in candidates if _is_app_dir(p)), None)


def get_icon_path() -> Path | None:
    """Return the path to the bundled app-icon.png (MEIPASS then lib)."""
    script_dir = get_script_dir()
    if getattr(sys, 'frozen', False):
        candidate = Path(sys._MEIPASS) / 'lib' / 'app-icon.png'
    else:
        candidate = script_dir / 'lib' / 'app-icon.png'
    return candidate if candidate.exists() else None
