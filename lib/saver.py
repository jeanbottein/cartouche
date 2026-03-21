import os
import json
import logging
import shutil
import re

from . import manifester

logger = logging.getLogger("saver")


WINDOWS_RESERVED_NAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
}


def _resolve_base_path(path: str) -> str:
    path = os.path.expandvars(os.path.expanduser(path))
    return os.path.abspath(path)


def _resolve_save_path(save_path: str, manifest_path: str) -> str:
    if not save_path:
        return ""
    save_path = os.path.expandvars(os.path.expanduser(save_path))
    if not os.path.isabs(save_path):
        manifest_dir = os.path.dirname(manifest_path)
        save_path = os.path.join(manifest_dir, save_path)
    return os.path.normpath(save_path)


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
                # If we can't stat, fall back to copying
                shutil.copy2(src, dst)
                return

            # If destination is at least as new and same size, treat as identical
            if (
                src_stat.st_size == dst_stat.st_size
                and src_stat.st_mtime <= dst_stat.st_mtime
            ):
                return

        shutil.copy2(src, dst)
    except Exception as e:
        logger.error(f"❌ Error copying {src} -> {dst}: {e}")


def _copy_tree_one_way(src_root: str, dst_root: str) -> None:
    files = _build_file_map(src_root)
    if not files:
        return
    for rel, src_path in files.items():
        dst_path = os.path.join(dst_root, rel)
        _copy_file(src_path, dst_path)



def _sync_one_manifest(manifest_path: str, saves_root: str, strategy: str) -> None:
    try:
        with open(manifest_path, "r") as f:
            manifest = json.load(f)
    except Exception as e:
        logger.error(f"❌ Error reading manifest {manifest_path}: {e}")
        return

    title = manifest.get("title") or os.path.basename(os.path.dirname(manifest_path))
    raw_save = manifest.get("savePath", "")
    try:
        if hasattr(manifester, "_pick_save_path"):
            save_path = manifester._pick_save_path(raw_save)
        else:
            save_path = raw_save
    except Exception as e:
        logger.error(f"❌ {title}: error resolving savePath {raw_save!r}: {e}")
        return

    if not save_path:
        logger.info(f"ℹ️ {title}: no savePath defined, skipping")
        return

    src_dir = _resolve_save_path(save_path, manifest_path)
    dst_dir = os.path.join(saves_root, _sanitize_title(title))

    if strategy in ("backup", "sync"):
        if strategy == "sync":
            logger.info(f"ℹ️ {title}: 'sync' strategy now behaves like 'backup' (use SAVESLINK_PATH + Syncthing for bidirectional sync)")
        if not os.path.isdir(src_dir):
            logger.info(f"ℹ️ {title}: source save directory {src_dir} not found, skipping backup")
            return
        src_files = _build_file_map(src_dir)
        if not src_files:
            logger.info(f"ℹ️ {title}: source save directory {src_dir} is empty, skipping backup")
            return
        dst_files = _build_file_map(dst_dir) if os.path.isdir(dst_dir) else {}

        os.makedirs(dst_dir, exist_ok=True)
        logger.info(f"🤖 Backing up saves for {title}")

        # Copy/update files from source into backup
        for rel, src_path in src_files.items():
            dst_path = os.path.join(dst_dir, rel)
            _copy_file(src_path, dst_path)

        # Remove files from backup that no longer exist in source
        for rel, dst_path in dst_files.items():
            if rel not in src_files:
                try:
                    os.remove(dst_path)
                except OSError as e:
                    logger.error(f"❌ Error removing obsolete backup file {dst_path}: {e}")

        logger.info(f"✅ {title}: backup updated")
        return

    if strategy == "restore":
        if not os.path.isdir(dst_dir):
            logger.info(f"ℹ️ {title}: backup directory {dst_dir} not found, skipping restore")
            return
        os.makedirs(src_dir, exist_ok=True)
        logger.warning(f"⚠️ Restoring saves for {title} from backup (overwrites existing files)")
        _copy_tree_one_way(dst_dir, src_dir)
        logger.info(f"✅ {title}: restore completed")
        return


def _sync_custom_directory(custom_name: str, source_dir: str, saves_root: str, strategy: str) -> None:
    """
    Sync a custom directory to the backup location.
    
    Args:
        custom_name: Name to use for the backup subdirectory
        source_dir: Source directory path to backup
        saves_root: Root backup directory path
        strategy: Backup strategy (backup, sync, or restore)
    """
    src_dir = _resolve_base_path(source_dir)
    dst_dir = os.path.join(saves_root, _sanitize_title(custom_name))
    
    if strategy in ("backup", "sync"):
        if strategy == "sync":
            logger.info(f"ℹ️ {custom_name}: 'sync' strategy now behaves like 'backup' (use SAVESLINK_PATH + Syncthing for bidirectional sync)")
        if not os.path.isdir(src_dir):
            logger.info(f"ℹ️ {custom_name}: source directory {src_dir} not found, skipping backup")
            return
        src_files = _build_file_map(src_dir)
        if not src_files:
            logger.info(f"ℹ️ {custom_name}: source directory {src_dir} is empty, skipping backup")
            return
        dst_files = _build_file_map(dst_dir) if os.path.isdir(dst_dir) else {}

        os.makedirs(dst_dir, exist_ok=True)
        logger.info(f"🤖 Backing up custom directory: {custom_name}")

        # Copy/update files from source into backup
        for rel, src_path in src_files.items():
            dst_path = os.path.join(dst_dir, rel)
            _copy_file(src_path, dst_path)

        # Remove files from backup that no longer exist in source
        for rel, dst_path in dst_files.items():
            if rel not in src_files:
                try:
                    os.remove(dst_path)
                except OSError as e:
                    logger.error(f"❌ Error removing obsolete backup file {dst_path}: {e}")

        logger.info(f"✅ {custom_name}: backup updated")
        return

    if strategy == "restore":
        if not os.path.isdir(dst_dir):
            logger.info(f"ℹ️ {custom_name}: backup directory {dst_dir} not found, skipping restore")
            return
        os.makedirs(src_dir, exist_ok=True)
        logger.warning(f"⚠️ Restoring custom directory: {custom_name} from backup (overwrites existing files)")
        _copy_tree_one_way(dst_dir, src_dir)
        logger.info(f"✅ {custom_name}: restore completed")
        return


def _create_symlink(link_path: str, target_path: str) -> None:
    """Create or update a symlink at link_path pointing to target_path."""
    try:
        if os.path.islink(link_path):
            current_target = os.readlink(link_path)
            if current_target == target_path:
                return  # Already correct
            os.remove(link_path)
        elif os.path.exists(link_path):
            # Something that isn't a symlink exists here; skip to avoid data loss
            logger.warning(f"⚠️ {link_path} exists and is not a symlink, skipping")
            return
        os.makedirs(os.path.dirname(link_path), exist_ok=True)
        os.symlink(target_path, link_path)
    except Exception as e:
        logger.error(f"❌ Error creating symlink {link_path} -> {target_path}: {e}")


def _build_symlink_tree(symlink_entries: list, link_root: str) -> None:
    """
    Build a symlink tree under link_root.

    Args:
        symlink_entries: list of (sanitized_name, original_source_path) tuples
        link_root: root directory for the symlink tree
    """
    os.makedirs(link_root, exist_ok=True)

    created_names = set()
    for name, source_path in symlink_entries:
        link_path = os.path.join(link_root, name)
        if not os.path.isdir(source_path):
            logger.info(f"ℹ️ {name}: source {source_path} does not exist, skipping symlink")
            continue
        _create_symlink(link_path, source_path)
        created_names.add(name)

    # Clean up stale symlinks (symlinks in link_root that no longer correspond to any entry)
    try:
        for entry in os.listdir(link_root):
            full = os.path.join(link_root, entry)
            if os.path.islink(full) and entry not in created_names:
                logger.info(f"🤖 Removing stale symlink: {full}")
                try:
                    os.remove(full)
                except OSError as e:
                    logger.error(f"❌ Error removing stale symlink {full}: {e}")
    except OSError as e:
        logger.error(f"❌ Error listing symlink directory {link_root}: {e}")

    if created_names:
        logger.info(f"✅ Symlink tree updated at {link_root} ({len(created_names)} entries)")


def _resolve_manifest_save_path(manifest_path: str) -> tuple:
    """Resolve a manifest's title and save path. Returns (title, src_dir) or (None, None)."""
    try:
        with open(manifest_path, "r") as f:
            manifest = json.load(f)
    except Exception as e:
        logger.error(f"❌ Error reading manifest {manifest_path}: {e}")
        return None, None

    title = manifest.get("title") or os.path.basename(os.path.dirname(manifest_path))
    raw_save = manifest.get("savePath", "")
    try:
        if hasattr(manifester, "_pick_save_path"):
            save_path = manifester._pick_save_path(raw_save)
        else:
            save_path = raw_save
    except Exception as e:
        logger.error(f"❌ {title}: error resolving savePath {raw_save!r}: {e}")
        return None, None

    if not save_path:
        return None, None

    src_dir = _resolve_save_path(save_path, manifest_path)
    return title, src_dir


def run(config: dict) -> None:
    games_dir = config.get("FREEGAMES_PATH")
    saves_root = config.get("SAVESCOPY_PATH")
    link_root = config.get("SAVESLINK_PATH")

    raw_strategy = (config.get("SAVESCOPY_STRATEGY") or "backup").strip().lower()
    if raw_strategy not in {"backup", "sync", "restore"}:
        logger.warning(
            f"\x10 Invalid SAVESCOPY_STRATEGY '{raw_strategy}', falling back to 'backup'"
        )
        strategy = "backup"
    else:
        strategy = raw_strategy

    if not games_dir or not os.path.isdir(games_dir):
        logger.warning("🤖 FREEGAMES_PATH not configured or invalid")
        return

    if not saves_root:
        logger.warning("🤖 SAVESCOPY_PATH not configured")
        return

    saves_root = _resolve_base_path(saves_root)
    os.makedirs(saves_root, exist_ok=True)

    # Copy config.txt to backup/sync folder prefixed with machine name
    machine_name = (config.get("MACHINE_NAME") or "").strip()
    config_path = config.get("_CONFIG_PATH")  # injected by gamer-sidekick.py
    if machine_name and config_path and os.path.isfile(config_path):
        config_backup_name = f"{_sanitize_title(machine_name)}_config.txt"
        config_dst = os.path.join(saves_root, config_backup_name)
        _copy_file(config_path, config_dst)
        logger.info(f"✅ Config backed up as {config_backup_name}")
    elif not machine_name:
        logger.info("ℹ️ MACHINE_NAME not set, skipping config backup")

    manifests = manifester.find_manifests(games_dir)
    if not manifests:
        logger.info("🤖 No launch_manifest.json found, nothing to process")
        return

    logger.info(f"🤖 Running saver with strategy='{strategy}' to {saves_root}")
    for manifest_path in manifests:
        _sync_one_manifest(manifest_path, saves_root, strategy)

    # Process custom directory backups
    custom_backups = {}
    for key, value in config.items():
        if key.startswith("BACKUP_"):
            custom_name = key[len("BACKUP_"):]
            if custom_name and value:
                custom_backups[custom_name] = value

    if custom_backups:
        logger.info(f"🤖 Processing {len(custom_backups)} custom directory backup(s)")
        for custom_name, source_path in custom_backups.items():
            _sync_custom_directory(custom_name, source_path, saves_root, strategy)

    # Build symlink tree if SAVESLINK_PATH is configured
    if link_root:
        link_root = _resolve_base_path(link_root)
        logger.info(f"🤖 Building symlink tree at {link_root}")

        symlink_entries = []

        # Add game save directories
        for manifest_path in manifests:
            title, src_dir = _resolve_manifest_save_path(manifest_path)
            if title and src_dir:
                symlink_entries.append((_sanitize_title(title), src_dir))

        # Add custom directory entries
        for custom_name, source_path in custom_backups.items():
            src_dir = _resolve_base_path(source_path)
            symlink_entries.append((_sanitize_title(custom_name), src_dir))

        _build_symlink_tree(symlink_entries, link_root)

        # Also copy config to symlink folder
        if machine_name and config_path and os.path.isfile(config_path):
            config_link_dst = os.path.join(link_root, config_backup_name)
            _copy_file(config_path, config_link_dst)
