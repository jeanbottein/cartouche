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
        name = name[:100].rstrip(". ")
    if not name:
        name = "game"
    return name


def _build_file_map(root: str) -> dict:
    if not os.path.isdir(root):
        return {}
    return {
        os.path.relpath(os.path.join(dp, f), root): os.path.join(dp, f)
        for dp, _, filenames in os.walk(root)
        for f in filenames
    }


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
            if src_stat.st_size == dst_stat.st_size and src_stat.st_mtime <= dst_stat.st_mtime:
                return
        shutil.copy2(src, dst)
    except OSError as e:
        logger.error(f"Error copying {src} -> {dst}: {e}")


def _copy_tree_one_way(src_root: str, dst_root: str) -> None:
    for rel, src_path in _build_file_map(src_root).items():
        _copy_file(src_path, os.path.join(dst_root, rel))


# ── Directory sync strategies ────────────────────────────────────────────

def _backup_dir(title: str, src_dir: str, dst_dir: str) -> None:
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
        _copy_file(src_path, os.path.join(dst_dir, rel))

    for rel, dst_path in dst_files.items():
        if rel not in src_files:
            try:
                os.remove(dst_path)
            except OSError as e:
                logger.error(f"  Error removing obsolete backup file {dst_path}: {e}")


def _restore_dir(title: str, src_dir: str, dst_dir: str) -> None:
    if not os.path.isdir(dst_dir):
        logger.info(f"  {title}: backup {dst_dir} not found, skipping restore")
        return
    os.makedirs(src_dir, exist_ok=True)
    logger.warning(f"  Restoring: {title} (overwrites existing files)")
    _copy_tree_one_way(dst_dir, src_dir)


def _sync_directory(title: str, src_dir: str, dst_dir: str, strategy: str) -> None:
    if strategy in ("backup", "sync"):
        if strategy == "sync":
            logger.info(f"  {title}: 'sync' strategy behaves like 'backup' (use SAVESLINK_PATH + Syncthing for bidirectional sync)")
        _backup_dir(title, src_dir, dst_dir)
    elif strategy == "restore":
        _restore_dir(title, src_dir, dst_dir)


# ── Symlink tree ─────────────────────────────────────────────────────────

def _create_symlink(link_path: str, target_path: str) -> None:
    try:
        if os.path.islink(link_path):
            if os.readlink(link_path) == target_path:
                return
            os.remove(link_path)
        elif os.path.exists(link_path):
            logger.warning(f"  {link_path} exists and is not a symlink, skipping")
            return
        os.makedirs(os.path.dirname(link_path), exist_ok=True)
        os.symlink(target_path, link_path)
    except OSError as e:
        logger.error(f"  Error creating symlink {link_path} -> {target_path}: {e}")


def _remove_stale_symlinks(link_root: str, active_entries: set) -> None:
    try:
        for entry in os.listdir(link_root):
            if entry in active_entries:
                continue
            full = os.path.join(link_root, entry)
            if os.path.islink(full):
                logger.info(f"  Removing stale symlink: {entry}")
                try:
                    os.remove(full)
                except OSError as e:
                    logger.error(f"  Error removing stale symlink {full}: {e}")
    except OSError:
        pass


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
            _create_symlink(os.path.join(link_root, game_title), source_path)
        else:
            game_dir = os.path.join(link_root, game_title)
            os.makedirs(game_dir, exist_ok=True)
            _create_symlink(os.path.join(game_dir, sub_name), source_path)
        created_top_level.add(game_title)

    _remove_stale_symlinks(link_root, created_top_level)

    if created_top_level:
        logger.info(f"  Symlink tree updated at {link_root} ({len(created_top_level)} entries)")


# ── run() helpers ────────────────────────────────────────────────────────

def _resolve_strategy(raw: str | None) -> str:
    strategy = (raw or "backup").strip().lower()
    if strategy not in {"backup", "sync", "restore"}:
        logger.warning(f"Invalid SAVESCOPY_STRATEGY '{strategy}', falling back to 'backup'")
        return "backup"
    return strategy


def _backup_config_file(config_path: str | None, machine_name: str, saves_root: str) -> str | None:
    """Copy config.txt to saves_root; returns the backup filename or None."""
    if not machine_name or not config_path or not os.path.isfile(config_path):
        return None
    backup_name = f"{_sanitize_title(machine_name)}_config.txt"
    _copy_file(config_path, os.path.join(saves_root, backup_name))
    logger.info(f"  Config backed up as {backup_name}")
    return backup_name


def _process_game_saves(db: GameDatabase, saves_root: str, strategy: str) -> list:
    """Sync all game save directories; returns symlink_entries list."""
    symlink_entries = []
    games_processed = 0

    for game in db.games:
        if not game.resolved_save_paths:
            continue
        game_title = _sanitize_title(game.title)
        multi = len(game.resolved_save_paths) > 1

        for save_path in game.resolved_save_paths:
            sub_name = _sanitize_title(os.path.basename(save_path))
            if multi:
                dst_dir = os.path.join(saves_root, game_title, sub_name)
                label   = f"{game.title}/{sub_name}"
            else:
                dst_dir = os.path.join(saves_root, game_title)
                label   = game.title
            _sync_directory(label, save_path, dst_dir, strategy)
            symlink_entries.append((game_title, sub_name if multi else None, save_path))

        games_processed += 1

    if games_processed:
        logger.info(f"Processed saves for {games_processed} game(s)")
    return symlink_entries


def _collect_custom_backup_dirs(config: dict) -> dict:
    """Extract BACKUP_<name>=<path> entries from config."""
    return {
        key[len("BACKUP_"):]: value
        for key, value in config.items()
        if key.startswith("BACKUP_") and key[len("BACKUP_"):] and value
    }


# ── Public entry point ───────────────────────────────────────────────────

def run(db: GameDatabase, config: dict) -> None:
    """
    Backup/restore game saves using the GameDatabase.

    1 save path  → SAVESCOPY_PATH/<GameTitle>/
    2+ save paths → SAVESCOPY_PATH/<GameTitle>/<basename>/
    """
    saves_root = config.get("SAVESCOPY_PATH")
    if not saves_root:
        logger.info("SAVESCOPY_PATH not configured, skipping save backup")
        return

    saves_root = _resolve_base_path(saves_root)
    os.makedirs(saves_root, exist_ok=True)

    strategy    = _resolve_strategy(config.get("SAVESCOPY_STRATEGY"))
    link_root   = config.get("SAVESLINK_PATH")
    config_path = config.get("_CONFIG_PATH")
    machine_name = (config.get("MACHINE_NAME") or "").strip()

    logger.info(f"Running saver with strategy='{strategy}'")

    config_backup_name = _backup_config_file(config_path, machine_name, saves_root)
    symlink_entries    = _process_game_saves(db, saves_root, strategy)

    custom_backups = _collect_custom_backup_dirs(config)
    if custom_backups:
        logger.info(f"Processing {len(custom_backups)} custom directory backup(s)")
        for custom_name, source_path in custom_backups.items():
            src_dir = _resolve_base_path(source_path)
            dst_dir = os.path.join(saves_root, _sanitize_title(custom_name))
            _sync_directory(custom_name, src_dir, dst_dir, strategy)
            symlink_entries.append((_sanitize_title(custom_name), None, src_dir))

    if not (link_root and symlink_entries):
        return

    link_root = _resolve_base_path(link_root)
    logger.info(f"Building symlink tree at {link_root}")
    _build_symlink_tree(symlink_entries, link_root)

    if config_backup_name and config_path and os.path.isfile(config_path):
        _create_symlink(os.path.join(link_root, config_backup_name), config_path)
