"""
Step 4: Persist game data to .cartouche/ folders.

Writes game.json and downloads artwork images for games that have
been modified in memory (needs_persist = True).
"""

import json
import logging
import os
import urllib.request
import urllib.error

from .models import GameDatabase, CARTOUCHE_DIR, GAME_JSON
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.persister")

USER_AGENT = f"{APP_NAME}/1.0"


def _download_file(url, local_path):
    """Download a file from URL to local_path. Returns True on success."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(req, timeout=15) as resp:
            with open(local_path, 'wb') as f:
                f.write(resp.read())
        return True
    except (urllib.error.URLError, OSError) as e:
        logger.warning(f"Failed to download {url}: {e}")
        return False


def _download_images(game, cartouche_dir: str):
    """Download artwork images from URLs stored during enrichment."""
    urls = getattr(game, "_artwork_urls", None)
    if not urls:
        return

    image_map = {
        "cover": urls.get("poster") or urls.get("grid"),
        "icon": urls.get("icon"),
        "hero": urls.get("hero"),
        "logo": urls.get("logo"),
    }

    for field_name, url in image_map.items():
        if not url:
            continue
        filename = getattr(game.images, field_name)
        if not filename:
            continue
        local_path = os.path.join(cartouche_dir, filename)
        if os.path.isfile(local_path):
            continue
        if _download_file(url, local_path):
            logger.info(f"  Downloaded {field_name}: {filename}")

    # Clean up temporary attribute
    if hasattr(game, "_artwork_urls"):
        del game._artwork_urls


def persist(db: GameDatabase):
    """
    Write .cartouche/game.json and download images for all dirty games.
    """
    dirty = db.dirty_games()
    if not dirty:
        return

    logger.info(f"Persisting {len(dirty)} game(s)")

    for game in dirty:
        cartouche_dir = str(game.cartouche_dir)
        game_json_path = str(game.game_json_path)

        os.makedirs(cartouche_dir, exist_ok=True)

        # Write game.json
        try:
            with open(game_json_path, "w") as f:
                json.dump(game.to_dict(), f, indent=4)
            logger.info(f"  Saved: {game.folder_name}/{CARTOUCHE_DIR}/{GAME_JSON}")
        except Exception as e:
            logger.error(f"  Failed to write {game_json_path}: {e}")
            continue

        # Download images
        _download_images(game, cartouche_dir)

        game.has_cartouche = True
        game.needs_persist = False
