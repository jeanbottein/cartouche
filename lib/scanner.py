"""
Step 1: Parse games into in-memory database.

Scans FREEGAMES_PATH for game folders, reads existing .cartouche/game.json
files, and builds a GameDatabase.
"""

import json
import logging
import os
import sys
import platform

from .models import (
    Game, GameTarget, SavePath, GameImages, GameDatabase,
    CARTOUCHE_DIR, GAME_JSON,
)

logger = logging.getLogger("cartouche.scanner")


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


def _pick_target_entry(targets: list) -> dict | None:
    """Select the best target entry for the current OS/arch."""
    if not targets:
        return None
    os_tag = _os_tag()
    arch_tag = _arch_tag()

    same_os = [t for t in targets if (t.get("os") or "").lower() == os_tag]
    if not same_os:
        same_os = [t for t in targets if not (t.get("os") or "").strip() or (t.get("os") or "").lower() == "any"]
    pool = same_os or targets

    same_arch = [t for t in pool if (t.get("arch") or "").lower() == arch_tag]
    if not same_arch:
        same_arch = [t for t in pool if not (t.get("arch") or "").strip() or (t.get("arch") or "").lower() == "any"]
    pool = same_arch or pool

    return pool[0]


def _pick_save_path(save_path_entry: dict) -> str:
    """Pick the appropriate path from a SavePath entry for the current OS."""
    paths = save_path_entry.get("paths", [])
    if not paths:
        return ""

    os_tag = _os_tag()
    same_os = [p for p in paths if (p.get("os") or "").lower() == os_tag]
    if not same_os:
        same_os = [p for p in paths if not (p.get("os") or "").strip() or (p.get("os") or "").lower() == "any"]
    pool = same_os or paths

    for entry in pool:
        path = entry.get("path", "")
        if path:
            return path
    return ""


def _resolve_save_path(save_path: str, game_dir: str) -> str:
    """Resolve a save path (expand vars, make absolute)."""
    if not save_path:
        return ""
    save_path = os.path.expandvars(os.path.expanduser(save_path))
    if not os.path.isabs(save_path):
        save_path = os.path.join(game_dir, save_path)
    return os.path.normpath(save_path)


def _load_game_json(game_dir: str) -> Game | None:
    """Load a game from its .cartouche/game.json file."""
    cartouche_dir = os.path.join(game_dir, CARTOUCHE_DIR)
    game_json_path = os.path.join(cartouche_dir, GAME_JSON)

    if not os.path.isfile(game_json_path):
        return None

    try:
        with open(game_json_path, "r") as f:
            data = json.load(f)
    except Exception as e:
        logger.error(f"Failed to read {game_json_path}: {e}")
        return None

    folder_name = os.path.basename(game_dir)

    # Parse targets
    targets = [GameTarget.from_dict(t) for t in data.get("targets", [])]

    # Parse save paths (new format)
    save_paths = []
    for sp in data.get("savePaths", []):
        save_paths.append(SavePath.from_dict(sp))

    # Parse images
    images = GameImages.from_dict(data.get("images", {}))

    # Detect which image files actually exist on disk
    for img_field in ("cover", "icon", "hero", "logo"):
        filename = getattr(images, img_field)
        if filename:
            img_path = os.path.join(cartouche_dir, filename)
            if not os.path.isfile(img_path):
                setattr(images, img_field, None)

    game = Game(
        folder_name=folder_name,
        game_dir=game_dir,
        title=data.get("title", folder_name),
        targets=targets,
        save_paths=save_paths,
        steamgriddb_id=data.get("steamgriddb_id"),
        images=images,
        has_cartouche=True,
    )

    return game


def _resolve_runtime_fields(game: Game):
    """Resolve runtime fields (best target, save paths) for the current platform."""
    game_dir = str(game.game_dir)

    # Resolve best target
    if game.targets:
        target_dicts = [t.to_dict() for t in game.targets]
        best = _pick_target_entry(target_dicts)
        if best:
            target_path = best.get("target", "")
            start_in_path = best.get("startIn", "") or os.path.dirname(target_path)
            game.resolved_target = os.path.normpath(os.path.join(game_dir, target_path))
            game.resolved_start_in = os.path.normpath(os.path.join(game_dir, start_in_path))
            game.resolved_launch_options = best.get("launchOptions", "")

    # Resolve save paths
    game.resolved_save_paths = []
    for sp in game.save_paths:
        path_str = _pick_save_path(sp.to_dict())
        if path_str:
            abs_path = _resolve_save_path(path_str, game_dir)
            if abs_path:
                game.resolved_save_paths.append((sp.name, abs_path))


def scan(games_dir: str) -> GameDatabase:
    """
    Scan games_dir for game folders and build a GameDatabase.

    For each subfolder:
    - If .cartouche/game.json exists, load it
    - Otherwise, create a skeleton Game (to be filled by detector)
    """
    db = GameDatabase()

    if not games_dir or not os.path.isdir(games_dir):
        logger.warning("FREEGAMES_PATH not configured or invalid")
        return db

    logger.info(f"Scanning for games in {games_dir}")

    for item in sorted(os.listdir(games_dir)):
        if item.startswith("."):
            continue
        item_path = os.path.join(games_dir, item)
        if not os.path.isdir(item_path):
            continue

        game = _load_game_json(item_path)
        if game:
            _resolve_runtime_fields(game)
            db.add(game)
            logger.info(f"  Loaded: {game.title}")
        else:
            # Skeleton game - needs detection
            game = Game(
                folder_name=item,
                game_dir=item_path,
                title=item,
            )
            db.add(game)
            logger.info(f"  Discovered: {item} (needs detection)")

    logger.info(f"Found {len(db)} game(s)")
    return db
