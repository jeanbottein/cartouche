import json
import os
import shutil
import glob
import zlib
import logging
from pathlib import Path

from bps.apply import apply_to_files as bps_apply

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.patcher")


# ── Game directory discovery ─────────────────────────────────────────────

def _load_game_locations() -> list[str]:
    json_path = Path(__file__).resolve().parent / 'games_locations.json'
    with open(json_path, 'r') as f:
        locations = json.load(f)
    all_paths = locations.get('steam_directories', []) + locations.get('other_game_directories', [])
    resolved = []
    for path in all_paths:
        expanded = os.path.expandvars(path)
        resolved.extend(glob.glob(expanded) if '*' in expanded else [expanded])
    return resolved


def load_games_locations():
    return _load_game_locations()


def get_game_dirs() -> list[str]:
    return [p for p in _load_game_locations() if os.path.exists(p)]


# ── CRC helpers ──────────────────────────────────────────────────────────

def calculate_crc32(filename: str) -> int:
    with open(filename, 'rb') as f:
        return zlib.crc32(f.read()) & 0xFFFFFFFF


def check_file_status(file_path: str, target_crc32: str | None, patched_crc32: str | None) -> str:
    actual = calculate_crc32(file_path)
    try:
        if patched_crc32 and actual == int(patched_crc32, 16):
            return "already_patched"
        if not target_crc32 or actual == int(target_crc32, 16):
            return "ready"
    except ValueError:
        logger.error(f"❌ Invalid CRC hex value — target={target_crc32!r} patched={patched_crc32!r}")
        return "mismatch"
    logger.warning(f"❌ CRC mismatch. Expected: {target_crc32}, Got: {actual:08X}")
    return "mismatch"


# ── File-replacement patching ────────────────────────────────────────────

def _backup_exists(backup_file: str) -> bool:
    return os.path.exists(backup_file)


def _target_matches_backup(target_file: str, backup_file: str) -> bool:
    return calculate_crc32(target_file) == calculate_crc32(backup_file)


def _target_matches_source(target_file: str, source_file: str) -> bool:
    return calculate_crc32(target_file) == calculate_crc32(source_file)


def _do_replace(source_file: str, target_file: str) -> None:
    shutil.copy2(source_file, target_file)
    logger.info(f"✅ {target_file} replaced")


def apply_replacement(source_file: str, target_file: str) -> None:
    backup_file = f"{target_file}.backup"
    if not _backup_exists(backup_file):
        shutil.copy2(target_file, backup_file)
        _do_replace(source_file, target_file)
        return
    if _target_matches_backup(target_file, backup_file):
        _do_replace(source_file, target_file)
    elif _target_matches_source(target_file, source_file):
        logger.info(f"✅ {target_file} already replaced")
    else:
        logger.error(f"❌ {target_file} backup exists but target file differ from patch")


# ── BPS patching ─────────────────────────────────────────────────────────

def _apply_bps_patch(patch_file: str, source_file: str, output_file: str) -> bool:
    try:
        with open(patch_file, 'rb') as pf, \
             open(source_file, 'rb') as sf, \
             open(output_file, 'wb') as of:
            bps_apply(pf, sf, of)
        return True
    except Exception as e:
        logger.error(f"❌ BPS apply failed: {e}")
        return False


def _parse_crc_int(hex_str: str | None, label: str) -> tuple[int | None, bool]:
    """Returns (value_or_None, is_valid). Returns (None, False) on parse error."""
    if not hex_str:
        return None, True
    try:
        return int(hex_str, 16), True
    except ValueError:
        logger.error(f"❌ Invalid {label} hex value: {hex_str!r}")
        return None, False


def _report_backup_state(target_file: str, patched_crc32: str | None) -> None:
    """Log whether the already-modified target is our patch or something unexpected."""
    patched_crc, valid = _parse_crc_int(patched_crc32, "patched_crc32")
    if not valid:
        return
    if patched_crc and calculate_crc32(target_file) == patched_crc:
        logger.info(f"✅ {target_file} already patched")
    else:
        logger.error(f"❌ {target_file} backup exists but target file differ from patch")


def patch_file_with_backup_check(patch_info: dict, source_file: str, target_file: str) -> None:
    backup_file = f"{target_file}.backup"

    target_crc, valid = _parse_crc_int(patch_info.get('target_crc32'), "target_crc32")
    if not valid:
        return
    if target_crc is not None and calculate_crc32(target_file) != target_crc:
        logger.error(f"❌ CRC32 mismatch for {target_file}. Expected: {patch_info['target_crc32']}, Got: {calculate_crc32(target_file):08X}")
        return

    # Backup exists and target has already been modified — report state, don't re-patch
    if _backup_exists(backup_file) and not _target_matches_backup(target_file, backup_file):
        _report_backup_state(target_file, patch_info.get('patched_crc32'))
        return

    if not _backup_exists(backup_file):
        shutil.copy2(target_file, backup_file)

    patched_file = f"{target_file}.patched"
    if _apply_bps_patch(source_file, target_file, patched_file):
        os.replace(patched_file, target_file)
        logger.info(f"✅ {target_file} patched")
    else:
        if os.path.exists(patched_file):
            os.remove(patched_file)


def apply_patch_to_file(patch_info: dict, source_file: str, target_file: str) -> None:
    if patch_info['method'] == 'replace':
        apply_replacement(source_file, target_file)
    elif patch_info['method'] == 'patch':
        patch_file_with_backup_check(patch_info, source_file, target_file)


# ── Patch discovery and dispatch ─────────────────────────────────────────

def _find_target_file(target_relative: str) -> str | None:
    return next(
        (path for d in get_game_dirs()
         if os.path.exists(path := os.path.join(d, target_relative))),
        None,
    )


def _load_patch_json(json_file: str) -> list | None:
    try:
        with open(json_file, 'r') as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        logger.error(f"Failed to read {json_file}: {e}")
        return None


def process_single_patch(patch_info: dict, patch_folder: str) -> None:
    source_file = os.path.join(patch_folder, patch_info['file'])
    if not os.path.exists(source_file):
        logger.error(f"❌ {source_file} does not exist")
        return

    target_file = _find_target_file(patch_info['target'])
    if not target_file:
        logger.info(f"❌ {patch_info['target']} not found")
        return

    status = check_file_status(target_file, patch_info.get('target_crc32'), patch_info.get('patched_crc32'))
    if status == "already_patched":
        logger.info(f"✅ {target_file} already patched")
    elif status == "ready":
        apply_patch_to_file(patch_info, source_file, target_file)
    else:
        logger.warning(f"❌ {target_file}: skipping patch due to CRC mismatch")


def run(config: dict) -> None:
    patches_dir = config.get('PATCHES_PATH')
    if not patches_dir or not os.path.isdir(patches_dir):
        logger.warning("🤖 PATCHES_PATH not configured or invalid")
        return

    logger.info(f"🤖 Looking for patches in {patches_dir}")
    patch_count = 0

    for root, dirs, files in os.walk(patches_dir):
        if 'patch.json' not in files:
            continue
        json_file = os.path.join(root, 'patch.json')
        logger.info(f"📦 Processing {os.path.relpath(root, patches_dir)}")
        patches = _load_patch_json(json_file)
        if patches is None:
            continue
        for patch in patches:
            process_single_patch(patch, root)
        patch_count += 1

    if patch_count == 0:
        logger.info("ℹ️  No patch.json files found")
