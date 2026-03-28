"""
Step 9: Save file backup/restore.

Backs up, restores, or syncs game save files based on the GameDatabase
and config. Also handles custom directory backups and symlink trees.
"""

import os
import json
import logging
import shutil
import re

from .models import GameDatabase
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.saver")


WINDOWS_RESERVED_NAMES = {
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
}


def _resolve_base_path(path: str) -> str:
    path = os.path.expandvars(os.path.expanduser(path))
    return os.path.abspath(path)


def _sanitize_title(title: str) -> str:
    if not title:
        title = "game"
    name = title.strip()
    name = "".join(
        c if c not in '<>:"/\\|?*' and ord(c) >= 32 else "_" for c in name
    )
    name = re.sub(r"\s+", "_", name)
    name = name.rstrip(". ")
    if not name:
        name = "game"
    upper = name.upper()
    if upper in WINDOWS_RESERVED_NAMES:
        name = f"{name}_game"
    if len(name) > 100:
        name = name[:100]
    return name


def _build_file_map(root: str) -> dict:
    files = {}
    if not os.path.isdir(root):
        return files
    for dirpath, dirnames, filenames in os.walk(root):
        for fname in filenames:
            full = os.path.join(dirpath, fname)
            rel = os.path.relpath(full, root)
            files[rel] = full
    return files


def _copy_file(src: str, dst: str) -> None:
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    try:
        if os.path.exists(dst):
            try:
                src_stat = os.stat(src)
                dst_stat = os.stat(dst)
            except OSError:
                shutil.copy2(src, dst)
                return
            if (
                src_stat.st_size == dst_stat.st_size
                and src_stat.st_mtime <= dst_stat.st_mtime
            ):
                return
        shutil.copy2(src, dst)
    except OSError as e:
        logger.error(f"Error copying {src} -> {dst}: {e}")


def _copy_tree_one_way(src_root: str, dst_root: str) -> None:
    files = _build_file_map(src_root)
    if not files:
        return
    for rel, src_path in files.items():
        dst_path = os.path.join(dst_root, rel)
        _copy_file(src_path, dst_path)


def _sync_directory(title: str, src_dir: str, dst_dir: str, strategy: str) -> None:
    """
    Sync a single directory according to strategy.
    Used for both game saves and custom directory backups.
    """
    if strategy in ("backup", "sync"):
        if strategy == "sync":
            logger.info(f"  {title}: 'sync' strategy behaves like 'backup' (use SAVESLINK_PATH + Syncthing for bidirectional sync)")
        if not os.path.isdir(src_dir):
            logger.info(f"  {title}: source {src_dir} not found, skipping")
            return
        src_files = _build_file_map(src_dir)
        if not src_files:
            logger.info(f"  {title}: source {src_dir} is empty, skipping")
            return
        dst_files = _build_file_map(dst_dir) if os.path.isdir(dst_dir) else {}

        os.makedirs(dst_dir, exist_ok=True)
        logger.info(f"  Backing up: {title}")

        for rel, src_path in src_files.items():
            dst_path = os.path.join(dst_dir, rel)
            _copy_file(src_path, dst_path)

        for rel, dst_path in dst_files.items():
            if rel not in src_files:
                try:
                    os.remove(dst_path)
                except OSError as e:
                    logger.error(f"  Error removing obsolete backup file {dst_path}: {e}")

        return

    if strategy == "restore":
        if not os.path.isdir(dst_dir):
            logger.info(f"  {title}: backup {dst_dir} not found, skipping restore")
            return
        os.makedirs(src_dir, exist_ok=True)
        logger.warning(f"  Restoring: {title} (overwrites existing files)")
        _copy_tree_one_way(dst_dir, src_dir)
        return


def _create_symlink(link_path: str, target_path: str) -> None:
    try:
        if os.path.islink(link_path):
            current_target = os.readlink(link_path)
            if current_target == target_path:
                return
            os.remove(link_path)
        elif os.path.exists(link_path):
            logger.warning(f"  {link_path} exists and is not a symlink, skipping")
            return
        os.makedirs(os.path.dirname(link_path), exist_ok=True)
        os.symlink(target_path, link_path)
    except OSError as e:
        logger.error(f"  Error creating symlink {link_path} -> {target_path}: {e}")


def _build_symlink_tree(symlink_entries: list, link_root: str) -> None:
    """
    Build a symlink tree mirroring the backup folder structure.

    symlink_entries is a list of (game_title, sub_name_or_None, source_path).
    - sub_name is None → LINK_ROOT/game_title → source_path
    - sub_name is set  → LINK_ROOT/game_title/sub_name → source_path
    """
    os.makedirs(link_root, exist_ok=True)
    created_top_level = set()

    for game_title, sub_name, source_path in symlink_entries:
        if not os.path.isdir(source_path):
            continue
        if sub_name is None:
            # Single save path: direct symlink
            link_path = os.path.join(link_root, game_title)
            _create_symlink(link_path, source_path)
            created_top_level.add(game_title)
        else:
            # Multiple save paths: game subfolder with symlinks inside
            game_dir = os.path.join(link_root, game_title)
            os.makedirs(game_dir, exist_ok=True)
            link_path = os.path.join(game_dir, sub_name)
            _create_symlink(link_path, source_path)
            created_top_level.add(game_title)

    try:
        for entry in os.listdir(link_root):
            full = os.path.join(link_root, entry)
            if entry not in created_top_level and (os.path.islink(full) or os.path.isdir(full)):
                if os.path.islink(full):
                    logger.info(f"  Removing stale symlink: {entry}")
                    try:
                        os.remove(full)
                    except OSError as e:
                        logger.error(f"  Error removing stale symlink {full}: {e}")
    except OSError:
        pass

    if created_top_level:
        logger.info(f"  Symlink tree updated at {link_root} ({len(created_top_level)} entries)")


def run(db: GameDatabase, config: dict) -> None:
    """
    Backup/restore game saves using the GameDatabase.

    1 save path  → SAVESCOPY_PATH/<GameTitle>/
    2+ save paths → SAVESCOPY_PATH/<GameTitle>/<basename>/
    """
    saves_root = config.get("SAVESCOPY_PATH")
    link_root = config.get("SAVESLINK_PATH")

    raw_strategy = (config.get("SAVESCOPY_STRATEGY") or "backup").strip().lower()
    if raw_strategy not in {"backup", "sync", "restore"}:
        logger.warning(f"Invalid SAVESCOPY_STRATEGY '{raw_strategy}', falling back to 'backup'")
        strategy = "backup"
    else:
        strategy = raw_strategy

    if not saves_root:
        logger.info("SAVESCOPY_PATH not configured, skipping save backup")
        return

    saves_root = _resolve_base_path(saves_root)
    os.makedirs(saves_root, exist_ok=True)

    machine_name = (config.get("MACHINE_NAME") or "").strip()
    config_path = config.get("_CONFIG_PATH")
    config_backup_name = None
    if machine_name and config_path and os.path.isfile(config_path):
        config_backup_name = f"{_sanitize_title(machine_name)}_config.txt"
        config_dst = os.path.join(saves_root, config_backup_name)
        _copy_file(config_path, config_dst)
        logger.info(f"  Config backed up as {config_backup_name}")

    symlink_entries = []
    games_processed = 0

    logger.info(f"Running saver with strategy='{strategy}'")

    for game in db.games:
        if not game.resolved_save_paths:
            continue

        game_title = _sanitize_title(game.title)
        multi = len(game.resolved_save_paths) > 1

        for save_path in game.resolved_save_paths:
            sub_name = _sanitize_title(os.path.basename(save_path))

            if multi:
                dst_dir = os.path.join(saves_root, game_title, sub_name)
                label = f"{game.title}/{sub_name}"
            else:
                dst_dir = os.path.join(saves_root, game_title)
                label = game.title

            _sync_directory(label, save_path, dst_dir, strategy)
            symlink_entries.append((game_title, sub_name if multi else None, save_path))

        games_processed += 1

    if games_processed:
        logger.info(f"Processed saves for {games_processed} game(s)")

    custom_backups = {}
    for key, value in config.items():
        if key.startswith("BACKUP_"):
            custom_name = key[len("BACKUP_"):]
            if custom_name and value:
                custom_backups[custom_name] = value

    if custom_backups:
        logger.info(f"Processing {len(custom_backups)} custom directory backup(s)")
        for custom_name, source_path in custom_backups.items():
            src_dir = _resolve_base_path(source_path)
            dst_dir = os.path.join(saves_root, _sanitize_title(custom_name))
            _sync_directory(custom_name, src_dir, dst_dir, strategy)
            symlink_entries.append((_sanitize_title(custom_name), None, src_dir))

    if link_root and symlink_entries:
        link_root = _resolve_base_path(link_root)
        logger.info(f"Building symlink tree at {link_root}")
        _build_symlink_tree(symlink_entries, link_root)

        if config_backup_name and config_path and os.path.isfile(config_path):
            config_link_dst = os.path.join(link_root, config_backup_name)
            _copy_file(config_path, config_link_dst)
