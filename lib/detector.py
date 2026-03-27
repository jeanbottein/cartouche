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
import subprocess
import sys

from .models import Game, GameTarget, GameDatabase
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.detector")


# ── Architecture filter constants ────────────────────────────────────────

if sys.platform.startswith("linux") or sys.platform == "darwin":
    _machine = platform.machine().lower()
    if "arm" in _machine or "aarch64" in _machine:
        EXEC_FILTERS = ["arm64", "aarch64", "x86-64", "x86", ""]
    elif "64" in _machine or "x86_64" in _machine or "amd64" in _machine:
        EXEC_FILTERS = ["x86-64", "x86", "arm64", "aarch64", ""]
    else:
        EXEC_FILTERS = ["x86", "x86-64", "arm64", "aarch64", ""]
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

def _os_tag():
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform.startswith("win"):
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    return "other"


def _arch_tag():
    m = platform.machine().lower()
    if "arm" in m or "aarch64" in m:
        return "arm64"
    if "64" in m or "x86_64" in m or "amd64" in m:
        return "x86_64"
    if "86" in m or "i386" in m or "i686" in m:
        return "x86"
    return "other"


def _arch_from_filter(arch_filter, exe_path):
    f = (arch_filter or "").lower()
    if f in ("arm64", "aarch64"):
        return "arm64"
    if f in ("x86-64", "x86_64"):
        return "x86_64"
    if f == "x86":
        return "x86"
    base = os.path.basename(exe_path).lower()
    if "arm64" in base or "aarch64" in base:
        return "arm64"
    if "x86_64" in base or "amd64" in base or "64" in base:
        return "x86_64"
    if "x86" in base or "32" in base:
        return "x86"
    return _arch_tag()


# ── Executable finding ───────────────────────────────────────────────────

def _run_find_exe(cmd):
    try:
        executables = subprocess.check_output(cmd, shell=True, text=True).splitlines()
    except subprocess.CalledProcessError:
        return None
    if not executables:
        return None
    return executables


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


def _get_bin_posix(game_dir, maxdepth, arch_filter=""):
    if arch_filter == "x86":
        cmd = f"""find "{game_dir}" -maxdepth {maxdepth} -type f -executable | \
            grep -E "\\.x86$" | grep -v "x86_64" """
    else:
        cmd = f"""find "{game_dir}" -maxdepth {maxdepth} -type f -executable -exec file {{}} + | \
            grep executable | grep "{arch_filter}"  | sed "s#:.*##" | \
            grep -v "/java/" | grep -v "/jre/" | grep -v "/lib/" """
    return _find_best(game_dir, _run_find_exe(cmd))


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


# ── Multi-target collection ──────────────────────────────────────────────

def _collect_targets(game_dir: str) -> list[GameTarget]:
    """Collect all executable targets for a game directory."""
    os_tag = _os_tag()
    real_game_dir = _get_real_first_path(game_dir)
    targets = []

    if sys.platform.startswith("win"):
        exe = _get_target(game_dir)
        if exe:
            targets.append(GameTarget(
                os=os_tag,
                arch=_arch_tag(),
                target=_format_path(exe, game_dir),
                start_in=_format_path(os.path.dirname(exe), game_dir),
            ))
        return targets

    seen = set()
    for arch_filter in EXEC_FILTERS:
        exe = _get_bin(real_game_dir, 3, arch_filter)
        if not exe or exe in seen:
            continue
        seen.add(exe)

        # FIX: If it's an .EXE file, it's a Windows target even if on Linux
        target_os = os_tag
        if exe.lower().endswith(".exe"):
            target_os = "windows"

        arch = _arch_from_filter(arch_filter, exe)
        targets.append(GameTarget(
            os=target_os,
            arch=arch,
            target=_format_path(exe, game_dir),
            start_in=_format_path(os.path.dirname(exe), game_dir),
        ))
    return targets


# ── Pick best target for current platform ────────────────────────────────

def _pick_target_entry(targets: list[GameTarget]) -> GameTarget | None:
    if not targets:
        return None
    os_tag = _os_tag()
    arch_tag = _arch_tag()

    same_os = [t for t in targets if t.os.lower() == os_tag]
    if not same_os:
        same_os = [t for t in targets if not t.os.strip() or t.os.lower() == "any"]
    pool = same_os or targets

    same_arch = [t for t in pool if t.arch.lower() == arch_tag]
    if not same_arch:
        same_arch = [t for t in pool if not t.arch.strip() or t.arch.lower() == "any"]
    pool = same_arch or pool

    return pool[0]


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
