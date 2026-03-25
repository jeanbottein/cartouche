import os
import json
import logging
from lib import steamer
from lib import manifester

logger = logging.getLogger("retrofiter")

def run(config: dict):
    if config.get("STEAM_EXPOSE", "False").lower() != "true":
        return
        
    games_dir = config.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        return

    api_key = config.get("STEAMGRIDDB_API_KEY", "").strip()
    if not api_key:
        return

    project_root = config.get("_CONFIG_PATH", "")
    if project_root:
        project_root = os.path.dirname(project_root)
    else:
        project_root = os.getcwd()

    sgdb_cache = steamer.load_sgdb_cache(project_root)
    cache_start_len = len(sgdb_cache)
    
    manifest_paths = manifester.find_manifests(games_dir)
    updated = 0
    
    for path in manifest_paths:
        try:
            with open(path, "r") as f:
                manifest = json.load(f)
        except Exception as e:
            continue
            
        current_title = manifest.get("title")
        if not current_title:
            current_title = os.path.basename(os.path.dirname(path))
            
        # We need to find if there is an official name and ID
        game_id, urls, official_name = steamer.get_sgdb_info(current_title, api_key, sgdb_cache)
        
        changes = False
        if official_name and manifest.get("title") != official_name:
            manifest["title"] = official_name
            changes = True
            
        if game_id and manifest.get("steamgriddb_id") != game_id:
            manifest["steamgriddb_id"] = game_id
            changes = True
            
        if changes:
            try:
                with open(path, "w") as f:
                    json.dump(manifest, f, indent=4)
                logger.info(f"🔄 Retrofitted {os.path.basename(os.path.dirname(path))}: title='{manifest.get('title')}', id={manifest.get('steamgriddb_id')}")
                updated += 1
            except Exception as e:
                logger.error(f"❌ Failed to save retrofitted manifest {path}: {e}")
                
    if len(sgdb_cache) > cache_start_len:
        steamer.save_sgdb_cache(project_root, sgdb_cache)
        
    if updated > 0:
        logger.info(f"✅ Retrofiter updated {updated} manifests")
