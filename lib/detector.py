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


# ── Architecture filter constants ────────────────────────────────────────

if sys.platform.startswith("linux") or sys.platform == "darwin":
    _machine = platform.machine().lower()
    if "arm" in _machine or "aarch64" in _machine:
        EXEC_FILTERS = ["arm64", "x64", ""]
    elif "64" in _machine or "x86_64" in _machine or "amd64" in _machine:
        EXEC_FILTERS = ["x64", "arm64", ""]
    else:
        EXEC_FILTERS = ["x64", "arm64", ""]
else:
    EXEC_FILTERS = [""]

if sys.platform.startswith("win"):
    _win_machine = platform.machine().lower()
    if "arm" in _win_machine:
        WIN_ARCH_GROUPS = [
            ["arm64", "aarch64"],
            ["x64", "amd64", "win64", "64"],
            ["x86", "win32", "32"],
        ]
    elif "64" in _win_machine or "amd64" in _win_machine or "x86_64" in _win_machine:
        WIN_ARCH_GROUPS = [
            ["x64", "amd64", "win64", "64"],
            ["x86", "win32", "32"],
            ["arm64", "aarch64"],
        ]
    else:
        WIN_ARCH_GROUPS = [
            ["x86", "win32", "32"],
            ["x64", "amd64", "win64", "64"],
            ["arm64", "aarch64"],
        ]
else:
    WIN_ARCH_GROUPS = []


# ── OS/Arch helpers ──────────────────────────────────────────────────────

def _arch_from_binary(exe_path: str) -> str:
    """Detect architecture from binary headers, falling back to filename heuristics."""
    arch = detect_binary_arch(exe_path)
    if arch:
        return arch  # already normalised by platform_info
    base = os.path.basename(exe_path).lower()
    if "arm64" in base or "aarch64" in base:
        return "arm64"
    # x86, x86_64, amd64 all map to x64
    if "x86_64" in base or "amd64" in base or "64" in base:
        return "x64"
    if "x86" in base or "32" in base:
        return "x64"
    return arch_tag()


# ── Executable finding ───────────────────────────────────────────────────


def _find_best(game_dir, files):
    if files is None:
        return None
    if len(files) == 1:
        return files[0]

    folder_base = os.path.basename(game_dir)
    best_match = None
    highest_score = 0.0

    for file in files:
        file_base = os.path.splitext(os.path.basename(file))[0]
        score = difflib.SequenceMatcher(None, folder_base, file_base).ratio()
        if score > highest_score:
            highest_score = score
            best_match = file

    return best_match


_SKIP_DIRS = {"java", "jre", "lib", "__pycache__"}


def _get_bin_posix(game_dir, maxdepth, arch_filter=""):
    root_depth = game_dir.rstrip(os.sep).count(os.sep)
    candidates = []

    for dirpath, dirnames, filenames in os.walk(game_dir):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= maxdepth:
            dirnames[:] = []
            continue
        dirnames[:] = [d for d in dirnames if d not in _SKIP_DIRS and not d.startswith('.')]

        for name in filenames:
            full = os.path.join(dirpath, name)
            is_exe = name.lower().endswith(".exe")
            if not is_exe and not os.access(full, os.X_OK):
                continue
            if not is_executable(full):
                continue
            if arch_filter:
                bin_arch = detect_binary_arch(full)
                if bin_arch and bin_arch != arch_filter:
                    continue
                if not bin_arch:
                    # Fallback: infer arch from filename
                    arch_guess = _arch_from_binary(full)
                    if arch_guess != arch_filter:
                        continue
            candidates.append(full)

    return _find_best(game_dir, candidates or None)


def _get_bin_windows(game_dir, maxdepth):
    root_depth = game_dir.rstrip(os.sep).count(os.sep)
    candidates = []
    for dirpath, dirnames, filenames in os.walk(game_dir):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= maxdepth:
            dirnames[:] = []
        for name in filenames:
            if not name.lower().endswith(".exe"):
                continue
            full = os.path.join(dirpath, name)
            candidates.append(full)
    if not candidates:
        return None

    for group in WIN_ARCH_GROUPS or [[]]:
        if group:
            group_candidates = []
            for path in candidates:
                base = os.path.basename(path).lower()
                for token in group:
                    if token in base:
                        group_candidates.append(path)
                        break
            if group_candidates:
                return _find_best(game_dir, group_candidates)

    return _find_best(game_dir, candidates)


def _get_bin(game_dir, maxdepth, arch_filter=""):
    if sys.platform.startswith("win"):
        return _get_bin_windows(game_dir, maxdepth)
    return _get_bin_posix(game_dir, maxdepth, arch_filter)


def _get_real_first_path(game_dir):
    """Recursively descend if directory contains a single subfolder and no files."""
    from .models import CARTOUCHE_DIR
    game_dir = os.path.normpath(game_dir)
    entries = [
        entry for entry in os.listdir(game_dir)
        if not entry.startswith('.') and entry != CARTOUCHE_DIR
    ]
    directories = [e for e in entries if os.path.isdir(os.path.join(game_dir, e))]
    files = [e for e in entries if os.path.isfile(os.path.join(game_dir, e))]

    if len(directories) == 1 and len(files) == 0:
        return _get_real_first_path(os.path.join(game_dir, directories[0]))

    return game_dir


def _get_title(game_dir):
    return os.path.basename(_get_real_first_path(game_dir))


def _get_target(game_dir):
    """Find the best executable for the current platform."""
    real_game_dir = _get_real_first_path(game_dir)
    for depth in range(1, 4):
        for arch in EXEC_FILTERS:
            exe = _get_bin(real_game_dir, depth, arch)
            if exe is not None:
                return exe
    return None


def _format_path(full_path, base_path):
    return os.path.relpath(full_path, base_path)


# ── Collect all executables ──────────────────────────────────────────────

def _find_all_executables(game_dir: str, maxdepth: int = 3) -> list[str]:
    """Find all recognized executables in game_dir up to maxdepth."""
    root_depth = game_dir.rstrip(os.sep).count(os.sep)
    results = []

    for dirpath, dirnames, filenames in os.walk(game_dir):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= maxdepth:
            dirnames[:] = []
            continue
        dirnames[:] = [d for d in dirnames if d not in _SKIP_DIRS and not d.startswith('.')]

        for name in filenames:
            full = os.path.join(dirpath, name)
            is_exe = name.lower().endswith(".exe")
            if not is_exe and not os.access(full, os.X_OK):
                continue
            if not is_executable(full):
                continue
            results.append(full)

    return results


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

    # Collect all executables, then group by (os, arch)
    all_exes = _find_all_executables(real_game_dir)
    if not all_exes:
        return targets

    # Group executables by (os, arch)
    groups: dict[tuple[str, str], list[str]] = {}
    for exe in all_exes:
        target_os = cur_os
        if exe.lower().endswith(".exe"):
            target_os = "windows"

        arch = _arch_from_binary(exe)
        key = (target_os, arch)
        groups.setdefault(key, []).append(exe)

    # Pick the best match per (os, arch) group
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

def _pick_target_entry(targets: list[GameTarget]) -> GameTarget | None:
    if not targets:
        return None
    cur_os = os_tag()
    cur_arch = arch_tag()

    same_os = [t for t in targets if t.os.lower() == cur_os]
    if not same_os:
        same_os = [t for t in targets if not t.os.strip() or t.os.lower() == "any"]
    pool = same_os or targets

    same_arch = [t for t in pool if t.arch.lower() == cur_arch]
    if not same_arch:
        same_arch = [t for t in pool if not t.arch.strip() or t.arch.lower() == "any"]
    pool = same_arch or pool

    return pool[0]


# ── Public single-game API ───────────────────────────────────────────────

def collect_targets(game_dir: str) -> list[GameTarget]:
    """
    Collect all detected executable targets for a single game directory.
    Returns a list of GameTarget objects (may be empty).
    """
    return _collect_targets(game_dir)


# ── Main entry point ─────────────────────────────────────────────────────

def detect(db: GameDatabase):
    """
    For each game without targets, detect executables and populate
    targets + resolved fields.
    """
    incomplete = db.incomplete_games()
    if not incomplete:
        return

    logger.info(f"Detecting executables for {len(incomplete)} game(s)")

    for game in incomplete:
        game_dir = str(game.game_dir)
        targets = _collect_targets(game_dir)

        if not targets:
            logger.info(f"  No executable found: {game.folder_name}")
            continue

        game.targets = targets
        game.title = _get_title(game_dir)
        game.needs_persist = True

        # Resolve best target for current platform
        best = _pick_target_entry(targets)
        if best:
            game.resolved_target = os.path.normpath(os.path.join(game_dir, best.target))
            game.resolved_start_in = os.path.normpath(os.path.join(game_dir, best.start_in))
            game.resolved_launch_options = best.launch_options
            game.resolved_target_os = best.os

        logger.info(f"  Detected: {game.title} ({len(targets)} target(s))")
