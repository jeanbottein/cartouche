"""
Step 2: Calculate missing data (executable detection).

For games without targets, detects executables in the game folder,
determines architecture, and populates the Game's targets and
resolved fields.
"""

import difflib
import logging
import os
import platform
import sys

from .models import Game, GameTarget, GameDatabase
from .app import APP_NAME
from .platform_info import os_tag, arch_tag, detect_binary_arch, is_executable

logger = logging.getLogger(f"{APP_NAME}.detector")


# ── Architecture filter constants (platform-computed at import time) ──────

def _build_exec_filters() -> list[str]:
    """Return ordered architecture preferences for the current POSIX platform."""
    if not (sys.platform.startswith("linux") or sys.platform == "darwin"):
        return [""]
    m = platform.machine().lower()
    if "arm" in m or "aarch64" in m:
        return ["arm64", "x64", ""]
    return ["x64", "arm64", ""]


def _build_win_arch_groups() -> list[list[str]]:
    """Return ordered architecture token groups for Windows executable selection."""
    if not sys.platform.startswith("win"):
        return []
    m = platform.machine().lower()
    if "arm" in m:
        return [["arm64", "aarch64"], ["x64", "amd64", "win64", "64"], ["x86", "win32", "32"]]
    if "64" in m or "amd64" in m or "x86_64" in m:
        return [["x64", "amd64", "win64", "64"], ["x86", "win32", "32"], ["arm64", "aarch64"]]
    return [["x86", "win32", "32"], ["x64", "amd64", "win64", "64"], ["arm64", "aarch64"]]


EXEC_FILTERS    = _build_exec_filters()
WIN_ARCH_GROUPS = _build_win_arch_groups()


# ── OS/Arch helpers ──────────────────────────────────────────────────────

def _arch_from_binary(exe_path: str) -> str:
    """Detect architecture from binary headers, falling back to filename heuristics."""
    arch = detect_binary_arch(exe_path)
    if arch:
        return arch
    base = os.path.basename(exe_path).lower()
    if "arm64" in base or "aarch64" in base:
        return "arm64"
    if "x86_64" in base or "amd64" in base or "64" in base:
        return "x64"
    return arch_tag()


# ── Executable finding ───────────────────────────────────────────────────

def _find_best(game_dir: str, files: list | None) -> str | None:
    if not files:
        return None
    if len(files) == 1:
        return files[0]
    folder_base = os.path.basename(game_dir)
    return max(
        files,
        key=lambda f: difflib.SequenceMatcher(None, folder_base, os.path.splitext(os.path.basename(f))[0]).ratio(),
    )


_SKIP_DIRS = {"java", "jre", "lib", "__pycache__"}


def _walk_executables(game_dir: str, maxdepth: int):
    """Yield (dirpath, filenames) for directories within maxdepth, skipping hidden/skip dirs."""
    root_depth = game_dir.rstrip(os.sep).count(os.sep)
    for dirpath, dirnames, filenames in os.walk(game_dir):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= maxdepth:
            dirnames[:] = []
            continue
        dirnames[:] = [d for d in dirnames if d not in _SKIP_DIRS and not d.startswith('.')]
        yield dirpath, filenames


def _is_matching_arch(exe_path: str, arch_filter: str) -> bool:
    """Check whether an executable matches the desired architecture filter."""
    if not arch_filter:
        return True
    bin_arch = detect_binary_arch(exe_path)
    return (bin_arch or _arch_from_binary(exe_path)) == arch_filter


def _is_runnable(name: str, full_path: str) -> bool:
    """Check whether a file looks like a runnable executable."""
    if name.lower().endswith(".exe"):
        return is_executable(full_path)
    return os.access(full_path, os.X_OK) and is_executable(full_path)


def _get_bin_posix(game_dir: str, maxdepth: int, arch_filter: str = "") -> str | None:
    candidates = [
        full
        for dirpath, filenames in _walk_executables(game_dir, maxdepth)
        for name in filenames
        if (full := os.path.join(dirpath, name))
        and _is_runnable(name, full)
        and _is_matching_arch(full, arch_filter)
    ]
    return _find_best(game_dir, candidates or None)


def _get_bin_windows(game_dir: str, maxdepth: int) -> str | None:
    candidates = [
        os.path.join(dirpath, name)
        for dirpath, filenames in _walk_executables(game_dir, maxdepth)
        for name in filenames
        if name.lower().endswith(".exe")
    ]
    if not candidates:
        return None
    for group in WIN_ARCH_GROUPS or [[]]:
        if group:
            group_candidates = [
                p for p in candidates
                if any(token in os.path.basename(p).lower() for token in group)
            ]
            if group_candidates:
                return _find_best(game_dir, group_candidates)
    return _find_best(game_dir, candidates)


def _get_bin(game_dir: str, maxdepth: int, arch_filter: str = "") -> str | None:
    if sys.platform.startswith("win"):
        return _get_bin_windows(game_dir, maxdepth)
    return _get_bin_posix(game_dir, maxdepth, arch_filter)


def _get_real_first_path(game_dir: str, _depth: int = 0) -> str:
    """Recursively descend if directory contains a single subfolder and no files."""
    if _depth > 10:
        return game_dir
    from .models import CARTOUCHE_DIR
    game_dir = os.path.normpath(game_dir)
    entries     = [e for e in os.listdir(game_dir) if not e.startswith('.') and e != CARTOUCHE_DIR]
    directories = [e for e in entries if os.path.isdir(os.path.join(game_dir, e))]
    files       = [e for e in entries if os.path.isfile(os.path.join(game_dir, e))]
    if len(directories) == 1 and len(files) == 0:
        return _get_real_first_path(os.path.join(game_dir, directories[0]), _depth + 1)
    return game_dir


def _get_title(game_dir: str) -> str:
    return os.path.basename(_get_real_first_path(game_dir))


def _get_target(game_dir: str) -> str | None:
    real_game_dir = _get_real_first_path(game_dir)
    for depth in range(1, 4):
        for arch in EXEC_FILTERS:
            exe = _get_bin(real_game_dir, depth, arch)
            if exe is not None:
                return exe
    return None


def _format_path(full_path: str, base_path: str) -> str:
    return os.path.relpath(full_path, base_path)


# ── Collect all executables ──────────────────────────────────────────────

def _find_all_executables(game_dir: str, maxdepth: int = 3) -> list[str]:
    return [
        full
        for dirpath, filenames in _walk_executables(game_dir, maxdepth)
        for name in filenames
        if (full := os.path.join(dirpath, name))
        and (name.lower().endswith(".exe") or os.access(full, os.X_OK))
        and is_executable(full)
    ]


# ── Multi-target collection ──────────────────────────────────────────────

def _collect_targets(game_dir: str) -> list[GameTarget]:
    """
    Collect executable targets for a game directory.

    Rules:
    - x86 binaries are reported as x64 (x64 covers legacy x86)
    - Only one target per (os, arch) pair is kept
    - Best name match (vs folder name) wins within each (os, arch) group
    """
    cur_os = os_tag()
    real_game_dir = _get_real_first_path(game_dir)
    targets: list[GameTarget] = []

    if sys.platform.startswith("win"):
        exe = _get_target(game_dir)
        if exe:
            targets.append(GameTarget(
                os=cur_os,
                arch=arch_tag(),
                target=_format_path(exe, game_dir),
                start_in=_format_path(os.path.dirname(exe), game_dir),
            ))
        return targets

    all_exes = _find_all_executables(real_game_dir)
    if not all_exes:
        return targets

    groups: dict[tuple[str, str], list[str]] = {}
    for exe in all_exes:
        target_os = "windows" if exe.lower().endswith(".exe") else cur_os
        key = (target_os, _arch_from_binary(exe))
        groups.setdefault(key, []).append(exe)

    for (target_os, arch), exes in groups.items():
        best = _find_best(real_game_dir, exes)
        if best:
            targets.append(GameTarget(
                os=target_os,
                arch=arch,
                target=_format_path(best, game_dir),
                start_in=_format_path(os.path.dirname(best), game_dir),
            ))

    return targets


# ── Pick best target for current platform ────────────────────────────────

def _filter_targets(targets: list[GameTarget], key_fn, value: str) -> list[GameTarget]:
    """Narrow targets by exact match on key_fn, falling back to blank/any, then the full list."""
    exact = [t for t in targets if key_fn(t).lower() == value]
    if exact:
        return exact
    fallback = [t for t in targets if not key_fn(t).strip() or key_fn(t).lower() == "any"]
    return fallback or targets


def _pick_target_entry(targets: list[GameTarget]) -> GameTarget | None:
    if not targets:
        return None
    pool = _filter_targets(targets, lambda t: t.os, os_tag())
    pool = _filter_targets(pool, lambda t: t.arch, arch_tag())
    return pool[0]


# ── Public single-game API ───────────────────────────────────────────────

def collect_targets(game_dir: str) -> list[GameTarget]:
    """Collect all detected executable targets for a single game directory."""
    return _collect_targets(game_dir)


# ── Save Path Auto-Detection ─────────────────────────────────────────────

_PROTON_SEARCH_DIRS = [
    ("users/steamuser/AppData/LocalLow", "%USERPROFILE%/AppData/LocalLow"),
    ("users/steamuser/AppData/Local", "%LOCALAPPDATA%"),
    ("users/steamuser/AppData/Roaming", "%APPDATA%"),
    ("users/steamuser/Documents", "%USERPROFILE%/Documents"),
    ("users/steamuser/Saved Games", "%USERPROFILE%/Saved Games"),
]

_PROTON_IGNORE_DIRS = {
    "Microsoft", "Temp", "dxvk", "CrashDumps", ".cef", "CEF",
    "desktop.ini", "Public", "Windows", "Contacts", "Favorites",
    "Links", "Pictures", "Music", "Videos", "Searches",
    "Steam", "SteamVR",
}

_MAX_PROTON_SEARCH_DEPTH = 2


def _alphanum_key(text: str) -> str:
    """Reduce a string to lowercase alphanumerics for fuzzy matching."""
    return "".join(c for c in text.lower() if c.isalnum())


def _resolve_proton_c_path(game_title: str, exe_path: str) -> str | None:
    """Build and validate the Proton C: drive path for a game."""
    from .steam_exporter import generate_appid
    from .steam_helpers import PROTON_PREFIX_TEMPLATE
    appid = generate_appid(game_title, exe_path)
    proton_c = os.path.expanduser(PROTON_PREFIX_TEMPLATE.format(appid=appid))
    return proton_c if os.path.isdir(proton_c) else None


def _search_proton_dir(proton_c_disk: str, title_clean: str,
                       base_dir: str, win_pattern: str) -> list[dict]:
    """Walk one Proton search directory for folders matching the game title."""
    full_base = os.path.join(proton_c_disk, base_dir)
    if not os.path.isdir(full_base):
        return []

    results = []
    root_depth = full_base.rstrip(os.sep).count(os.sep)
    for dirpath, dirnames, _ in os.walk(full_base):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= _MAX_PROTON_SEARCH_DEPTH:
            dirnames[:] = []
        dirnames[:] = [d for d in dirnames if d not in _PROTON_IGNORE_DIRS and not d.startswith('.')]

        clean_curr = _alphanum_key(os.path.basename(dirpath))
        if not clean_curr or title_clean not in clean_curr:
            continue

        rel_match = os.path.relpath(dirpath, full_base).replace('\\', '/')
        win_path = f"{win_pattern}/{rel_match}" if rel_match != "." else win_pattern
        results.append({"os": "windows", "path": win_path})
    return results


def _deduplicate(items: list[dict]) -> list[dict]:
    """Deduplicate a list of dicts preserving insertion order."""
    seen = []
    for item in items:
        if item not in seen:
            seen.append(item)
    return seen


def detect_proton_save_paths(game_title: str, exe_path: str) -> list[dict]:
    """
    Auto-detect game save locations within the corresponding Proton prefix.
    Returns a list of dicts with 'os' and 'path' mapping to standard Windows environments.
    """
    if not game_title or not exe_path:
        return []

    proton_c_disk = _resolve_proton_c_path(game_title, exe_path)
    if not proton_c_disk:
        return []

    title_clean = _alphanum_key(game_title)
    if not title_clean:
        return []

    results = [
        match
        for base_dir, win_pattern in _PROTON_SEARCH_DIRS
        for match in _search_proton_dir(proton_c_disk, title_clean, base_dir, win_pattern)
    ]
    return _deduplicate(results)


# ── Main entry point ─────────────────────────────────────────────────────

def detect(db: GameDatabase) -> None:
    """For each game without targets, detect executables and populate targets + resolved fields."""
    incomplete = db.incomplete_games()
    if not incomplete:
        return

    logger.info(f"Detecting executables for {len(incomplete)} game(s)")

    for game in incomplete:
        game_dir = str(game.game_dir)
        targets  = _collect_targets(game_dir)

        if not targets:
            logger.info(f"  No executable found: {game.folder_name}")
            continue

        game.targets     = targets
        game.title       = _get_title(game_dir)
        game.needs_persist = True

        best = _pick_target_entry(targets)
        if best:
            game.resolved_target         = os.path.normpath(os.path.join(game_dir, best.target))
            game.resolved_start_in       = os.path.normpath(os.path.join(game_dir, best.start_in))
            game.resolved_launch_options = best.launch_options
            game.resolved_target_os      = best.os

        logger.info(f"  Detected: {game.title} ({len(targets)} target(s))")
