"""
Step 3: Fetch data from SteamGridDB.

If an API key is available, queries SteamGridDB for game metadata
(official names, IDs) and artwork URLs. Updates the in-memory
GameDatabase. Manages the SGDB cache to minimize API calls.
"""

import json
import logging
import os
import urllib.request
import urllib.error

from .models import GameDatabase, GameImages
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.enricher")

USER_AGENT = f"{APP_NAME}/1.0"


# ── SteamGridDB API ─────────────────────────────────────────────────────

def _steamgriddb_request(endpoint, api_key):
    """Make a GET request to SteamGridDB API. Returns parsed JSON or None."""
    url = f"https://www.steamgriddb.com/api/v2/{endpoint}"
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {api_key}",
        "User-Agent": USER_AGENT,
    })
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            if data.get("success") and data.get("data"):
                return data["data"]
    except (urllib.error.URLError, urllib.error.HTTPError, OSError) as e:
        logger.debug(f"SteamGridDB request failed for {endpoint}: {e}")
    return None


def search_game_id(game_name, api_key):
    """Search SteamGridDB for a game. Returns (id, official_name) or (None, None)."""
    encoded = urllib.request.quote(game_name)
    data = _steamgriddb_request(f"search/autocomplete/{encoded}", api_key)
    if data:
        return data[0].get("id"), data[0].get("name")
    return None, None


def fetch_artwork_urls(game_id, api_key):
    """
    Fetch artwork URLs for a game from SteamGridDB.
    Returns dict with keys: grid, poster, hero, logo, icon (URL or None each).
    """
    endpoints = [
        ("grid", "grids", "dimensions=460x215,920x430"),
        ("poster", "grids", "dimensions=600x900,342x482"),
        ("hero", "heroes", ""),
        ("logo", "logos", ""),
        ("icon", "icons", ""),
    ]
    result = {}
    for art_type, endpoint, query in endpoints:
        q = f"?{query}" if query else ""
        data = _steamgriddb_request(f"{endpoint}/game/{game_id}{q}", api_key)
        if not data and query:
            data = _steamgriddb_request(f"{endpoint}/game/{game_id}", api_key)
        result[art_type] = data[0]["url"] if data else None
    return result


def _get_extension(url):
    """Extract file extension from URL."""
    path = url.split("?")[0]
    _, ext = os.path.splitext(path)
    return ext or ".png"


# ── Cache management ─────────────────────────────────────────────────────

def load_sgdb_cache(project_root):
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    if os.path.exists(cache_path):
        try:
            with open(cache_path, 'r') as f:
                return json.load(f)
        except Exception as e:
            logger.warning(f"Failed to load SteamGridDB cache: {e}")
    return {}


def save_sgdb_cache(project_root, cache):
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    try:
        with open(cache_path, 'w') as f:
            json.dump(cache, f, indent=2)
    except Exception as e:
        logger.warning(f"Failed to save SteamGridDB cache: {e}")


def get_sgdb_info(name, api_key, cache, manifest_id=None):
    """
    Returns (game_id, urls_dict, official_name). Uses cache to prevent API calls.
    Updates cache in-place if fresh data is fetched.
    Returns (None, {}, None) if not found.
    """
    key = name.lower()
    if key in cache:
        cached = cache[key]
        urls = cached.get("urls", {})
        if "name" in cached and "icon" in urls and "poster" in urls:
            return cached.get("game_id"), urls, cached.get("name")

        game_id = cached.get("game_id")
        if game_id:
            needs_update = False
            if "name" not in cached:
                _, official_name = search_game_id(name, api_key)
                if official_name:
                    cached["name"] = official_name
                    needs_update = True
            if "icon" not in urls or "poster" not in urls:
                fresh_urls = fetch_artwork_urls(game_id, api_key)
                cached["urls"] = fresh_urls
                needs_update = True

            if needs_update:
                return game_id, cached.get("urls", {}), cached.get("name")
        elif cached.get("game_id") is None and "game_id" in cached:
            return None, {}, None

    if manifest_id:
        game_id = manifest_id
        official_name = name
    else:
        game_id, official_name = search_game_id(name, api_key)

    if game_id:
        urls = fetch_artwork_urls(game_id, api_key)
        cache[key] = {"game_id": game_id, "urls": urls, "name": official_name}
        return game_id, urls, official_name
    else:
        cache[key] = {"game_id": None, "urls": {}, "name": None}
        return None, {}, None


# ── Image filename mapping ───────────────────────────────────────────────

# Maps SGDB art type to local filename in .cartouche/
IMAGE_FIELD_MAP = {
    "poster": "cover",   # poster/grid -> cover
    "grid": "cover",     # fallback if poster not available
    "icon": "icon",
    "hero": "hero",
    "logo": "logo",
}


def _urls_to_image_filenames(urls: dict) -> GameImages:
    """Convert SGDB artwork URLs to local filenames for .cartouche/ storage."""
    images = GameImages()

    # Poster is preferred for cover, grid is fallback
    poster_url = urls.get("poster")
    grid_url = urls.get("grid")
    cover_url = poster_url or grid_url
    if cover_url:
        ext = _get_extension(cover_url)
        images.cover = f"cover{ext}"

    icon_url = urls.get("icon")
    if icon_url:
        ext = _get_extension(icon_url)
        images.icon = f"icon{ext}"

    hero_url = urls.get("hero")
    if hero_url:
        ext = _get_extension(hero_url)
        images.hero = f"hero{ext}"

    logo_url = urls.get("logo")
    if logo_url:
        ext = _get_extension(logo_url)
        images.logo = f"logo{ext}"

    return images


# ── Main entry point ─────────────────────────────────────────────────────

def enrich(db: GameDatabase, cfg: dict):
    """
    Enrich games with SteamGridDB data: official names, IDs, artwork URLs.
    Only runs if STEAMGRIDDB_API_KEY is configured.
    """
    api_key = cfg.get("STEAMGRIDDB_API_KEY", "").strip()
    if not api_key:
        return

    project_root = cfg.get("_CONFIG_PATH", "")
    if project_root:
        project_root = os.path.dirname(project_root)
    else:
        project_root = os.getcwd()

    sgdb_cache = load_sgdb_cache(project_root)
    cache_start_len = len(json.dumps(sgdb_cache))

    games = db.games_needing_enrichment()
    if not games:
        # Even complete games might need image enrichment
        games = [g for g in db.games if not g.images.cover]

    if not games:
        return

    logger.info(f"Enriching {len(games)} game(s) from SteamGridDB")

    for game in games:
        game_id, urls, official_name = get_sgdb_info(
            game.title, api_key, sgdb_cache, game.steamgriddb_id
        )

        if not game_id:
            logger.info(f"  Not found: {game.title}")
            continue

        changed = False

        if game.steamgriddb_id != game_id:
            game.steamgriddb_id = game_id
            changed = True

        if official_name and game.title != official_name:
            game.title = official_name
            changed = True

        # Compute image filenames from URLs
        new_images = _urls_to_image_filenames(urls)
        if new_images.cover and not game.images.cover:
            game.images.cover = new_images.cover
            changed = True
        if new_images.icon and not game.images.icon:
            game.images.icon = new_images.icon
            changed = True
        if new_images.hero and not game.images.hero:
            game.images.hero = new_images.hero
            changed = True
        if new_images.logo and not game.images.logo:
            game.images.logo = new_images.logo
            changed = True

        # Store URLs temporarily for the persister to download
        if urls:
            game._artwork_urls = urls  # type: ignore[attr-defined]

        if changed:
            game.needs_persist = True
            logger.info(f"  Enriched: {game.folder_name} -> {game.title} (SGDB #{game_id})")
        else:
            if not game.images.cover:
                logger.info(f"  No artwork found on SteamGridDB for: {game.title}")

    # Save cache if it changed
    cache_json = json.dumps(sgdb_cache)
    if len(cache_json) != cache_start_len:
        save_sgdb_cache(project_root, sgdb_cache)
