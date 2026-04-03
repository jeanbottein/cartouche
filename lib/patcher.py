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

def load_games_locations():
    """Load game directory locations from JSON file."""
    json_path = Path(__file__).resolve().parent / 'games_locations.json'
    with open(json_path, 'r') as f:
        locations = json.load(f)

    all_dirs = locations.get('steam_directories', []) + locations.get('other_game_directories', [])
    resolved_dirs = []

    for path in all_dirs:
        resolved_path = os.path.expandvars(path)
        if '*' in resolved_path:
            expanded_paths = glob.glob(resolved_path)
            resolved_dirs.extend(expanded_paths)
        else:
            resolved_dirs.append(resolved_path)
    
    return resolved_dirs

def calculate_crc32(filename):
    with open(filename, 'rb') as file:
        return zlib.crc32(file.read()) & 0xFFFFFFFF

def get_game_dirs():
    game_locations = load_games_locations()
    return [p for p in game_locations if os.path.exists(p)]

def check_file_status(file_path, target_crc32, patched_crc32):
    actual_crc32 = calculate_crc32(file_path)
    try:
        if patched_crc32 and actual_crc32 == int(patched_crc32, 16):
            return "already_patched"

        if not target_crc32 or actual_crc32 == int(target_crc32, 16):
            return "ready"
    except ValueError:
        logger.error(f"❌ Invalid CRC hex value — target={target_crc32!r} patched={patched_crc32!r}")
        return "mismatch"

    logger.warning(f"❌ CRC mismatch. Expected: {target_crc32}, Got: {actual_crc32:08X}")
    return "mismatch"

def apply_replacement(source_file, target_file):
    backup_file = f"{target_file}.backup"
    
    if os.path.exists(backup_file):
        target_crc32 = calculate_crc32(target_file)
        backup_crc32 = calculate_crc32(backup_file)
        
        if target_crc32 != backup_crc32:
            source_crc32 = calculate_crc32(source_file)
            if target_crc32 == source_crc32:
                logger.info(f"✅ {target_file} already replaced")
                return
            else:
                logger.error(f"❌ {target_file} backup exists but target file differ from patch")
                return
    else:
        shutil.copy2(target_file, backup_file)
    
    shutil.copy2(source_file, target_file)
    logger.info(f"✅ {target_file} replaced")

def patch_file_with_backup_check(patch_info, source_file, target_file):
    backup_file = f"{target_file}.backup"

    target_crc32_expected = patch_info.get('target_crc32')
    if target_crc32_expected:
        actual_crc32 = calculate_crc32(target_file)
        try:
            expected_crc32 = int(target_crc32_expected, 16)
        except ValueError:
            logger.error(f"❌ Invalid target_crc32 hex value: {target_crc32_expected!r}")
            return
        if actual_crc32 != expected_crc32:
            logger.error(f"❌ CRC32 mismatch for {target_file}. Expected: {target_crc32_expected}, Got: {actual_crc32:08X}")
            return

    if os.path.exists(backup_file):
        target_crc32 = calculate_crc32(target_file)
        backup_crc32 = calculate_crc32(backup_file)

        if target_crc32 != backup_crc32:
            patched_crc32 = patch_info.get('patched_crc32')
            try:
                patched_crc32_int = int(patched_crc32, 16) if patched_crc32 else None
            except ValueError:
                logger.error(f"❌ Invalid patched_crc32 hex value: {patched_crc32!r}")
                return
            if patched_crc32_int and target_crc32 == patched_crc32_int:
                logger.info(f"✅ {target_file} already patched")
                return
            else:
                logger.error(f"❌ {target_file} backup exists but target file differ from patch")
                return
    else:
        shutil.copy2(target_file, backup_file)

    patched_file = f"{target_file}.patched"
    try:
        with open(source_file, 'rb') as patch_f, \
             open(target_file, 'rb') as source_f, \
             open(patched_file, 'wb') as target_f:
            bps_apply(patch_f, source_f, target_f)
        os.replace(patched_file, target_file)
        logger.info(f"✅ {target_file} patched")
    except Exception as e:
        if os.path.exists(patched_file):
            os.remove(patched_file)
        logger.error(f"❌ Error patching {target_file}: {e}")

def apply_patch_to_file(patch_info, source_file, target_file):
    if patch_info['method'] == 'replace':
        apply_replacement(source_file, target_file)
    elif patch_info['method'] == 'patch':
        patch_file_with_backup_check(patch_info, source_file, target_file)


def process_single_patch(patch_info, patch_folder):
    source_file = os.path.join(patch_folder, patch_info['file'])
    if not os.path.exists(source_file):
        logger.error(f"❌ {source_file} does not exist")
        return
    
    for games_folder in get_game_dirs():
        target_file = os.path.join(games_folder, patch_info['target'])
        if not os.path.exists(target_file):
            continue
            
        status = check_file_status(target_file, patch_info.get('target_crc32'), patch_info.get('patched_crc32'))
        
        if status == "already_patched":
            logger.info(f"✅ {target_file} already patched")
        elif status == "ready":
            apply_patch_to_file(patch_info, source_file, target_file)
        else:
            logger.warning(f"❌ {target_file}: skipping patch due to CRC mismatch")

        return
    
    logger.info(f"❌ {patch_info['target']} not found")

def run(config: dict):
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
        relative_path = os.path.relpath(root, patches_dir)
        logger.info(f"📦 Processing {relative_path}")

        try:
            with open(json_file, 'r') as f:
                patches = json.load(f)
        except (json.JSONDecodeError, OSError) as e:
            logger.error(f"Failed to read {json_file}: {e}")
            continue

        patch_folder = os.path.dirname(json_file)
        for patch in patches:
            process_single_patch(patch, patch_folder)

        patch_count += 1

    if patch_count == 0:
        logger.info("ℹ️  No patch.json files found")
