"""
Step 3: Fetch data from SteamGridDB.

If an API key is available, queries SteamGridDB for game metadata
(official names, IDs) and artwork URLs. Updates the in-memory
GameDatabase. Manages the SGDB cache to minimize API calls.
"""

import json
import logging
import os

import requests

from .models import GameDatabase, GameImages
from .app import APP_NAME
from .api_keys import get_steamgriddb_key

logger = logging.getLogger(f"{APP_NAME}.enricher")

USER_AGENT = f"{APP_NAME}/1.0"


# ── SteamGridDB API ──────────────────────────────────────────────────────

def _steamgriddb_request(endpoint: str, api_key: str):
    """Make a GET request to SteamGridDB API. Returns parsed data list or None."""
    url = f"https://www.steamgriddb.com/api/v2/{endpoint}"
    try:
        resp = requests.get(url, headers={
            "Authorization": f"Bearer {api_key}",
            "User-Agent": USER_AGENT,
        }, timeout=10)
        resp.raise_for_status()
        data = resp.json()
        if data.get("success") and data.get("data"):
            return data["data"]
    except requests.RequestException as e:
        logger.debug(f"SteamGridDB request failed for {endpoint}: {e}")
    return None


def search_game_id(game_name: str, api_key: str) -> tuple:
    """Search SteamGridDB for a game. Returns (id, official_name) or (None, None)."""
    encoded = requests.utils.quote(game_name)
    data = _steamgriddb_request(f"search/autocomplete/{encoded}", api_key)
    if data:
        return data[0].get("id"), data[0].get("name")
    return None, None


def _pick_best(data: list) -> str | None:
    """Pick the entry with the highest score from a SteamGridDB result list."""
    if not data:
        return None
    return max(data, key=lambda d: d.get("score", 0)).get("url")


def _build_content_filters(cfg: dict) -> str:
    """Build SteamGridDB content filter query string from config."""
    def flag(key):
        return "any" if str(cfg.get(key, "")).lower() == "true" else "false"
    return f"nsfw={flag('STEAMGRIDDB_NSFW')}&humor={flag('STEAMGRIDDB_HUMOR')}&epilepsy={flag('STEAMGRIDDB_EPILEPSY')}"


def fetch_artwork_urls(game_id: int, api_key: str, cfg: dict | None = None) -> dict:
    """Fetch artwork URLs for a game from SteamGridDB; returns dict keyed by art type."""
    filters = _build_content_filters(cfg or {})
    endpoints = [
        ("grid",   "grids",  f"dimensions=460x215,920x430&{filters}", True),
        ("poster", "grids",  f"dimensions=600x900,342x482&{filters}", True),
        ("hero",   "heroes", filters,                                  False),
        ("logo",   "logos",  filters,                                  False),
        ("icon",   "icons",  filters,                                  False),
    ]
    result = {}
    for art_type, endpoint, query, fallback in endpoints:
        data = _steamgriddb_request(f"{endpoint}/game/{game_id}?{query}", api_key)
        if not data and fallback:
            data = _steamgriddb_request(f"{endpoint}/game/{game_id}?{filters}", api_key)
        result[art_type] = _pick_best(data)
    return result


def _get_extension(url: str) -> str:
    """Extract file extension from URL."""
    path = url.split("?")[0]
    _, ext = os.path.splitext(path)
    return ext or ".png"


# ── Cache management ─────────────────────────────────────────────────────

def load_sgdb_cache(project_root: str) -> dict:
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    if not os.path.exists(cache_path):
        return {}
    try:
        with open(cache_path, 'r') as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        logger.warning(f"Failed to load SteamGridDB cache: {e}")
        return {}


def save_sgdb_cache(project_root: str, cache: dict) -> None:
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    try:
        with open(cache_path, 'w') as f:
            json.dump(cache, f, indent=2)
    except OSError as e:
        logger.warning(f"Failed to save SteamGridDB cache: {e}")


# ── SGDB info retrieval with cache ───────────────────────────────────────

def _cache_is_complete(cached: dict) -> bool:
    urls = cached.get("urls", {})
    return "name" in cached and "icon" in urls and "poster" in urls


def _refresh_cached_entry(key: str, cached: dict, api_key: str, cfg) -> tuple:
    """Fill missing fields in a partial cache entry; returns (game_id, urls, name)."""
    game_id = cached.get("game_id")
    urls = cached.get("urls", {})

    if "name" not in cached:
        _, official_name = search_game_id(key, api_key)
        if official_name:
            cached["name"] = official_name

    if "icon" not in urls or "poster" not in urls:
        cached["urls"] = fetch_artwork_urls(game_id, api_key, cfg)

    return game_id, cached.get("urls", {}), cached.get("name")


def _fetch_and_cache(name: str, key: str, cache: dict, api_key: str, manifest_id, cfg) -> tuple:
    """Fetch fresh SGDB data, populate cache, return (game_id, urls, official_name)."""
    if manifest_id:
        game_id, official_name = manifest_id, name
    else:
        game_id, official_name = search_game_id(name, api_key)

    if game_id:
        urls = fetch_artwork_urls(game_id, api_key, cfg)
        cache[key] = {"game_id": game_id, "urls": urls, "name": official_name}
        return game_id, urls, official_name

    cache[key] = {"game_id": None, "urls": {}, "name": None}
    return None, {}, None


def get_sgdb_info(name: str, api_key: str, cache: dict, manifest_id=None, cfg=None) -> tuple:
    """
    Returns (game_id, urls_dict, official_name). Uses cache to prevent API calls.
    Updates cache in-place if fresh data is fetched.
    Returns (None, {}, None) if not found.
    """
    key = name.lower()

    if key in cache:
        cached = cache[key]
        if _cache_is_complete(cached):
            return cached.get("game_id"), cached.get("urls", {}), cached.get("name")
        if cached.get("game_id"):
            return _refresh_cached_entry(key, cached, api_key, cfg)
        if "game_id" in cached and cached["game_id"] is None:
            return None, {}, None

    return _fetch_and_cache(name, key, cache, api_key, manifest_id, cfg)


# ── Image filename mapping ───────────────────────────────────────────────

IMAGE_FIELD_MAP = {
    "poster": "cover",
    "grid":   "cover",
    "icon":   "icon",
    "hero":   "hero",
    "logo":   "logo",
}


def _urls_to_image_filenames(urls: dict) -> GameImages:
    """Convert SGDB artwork URLs to local filenames for .cartouche/ storage."""
    images = GameImages()
    field_url_keys = [
        ("cover",  ["poster"]),
        ("icon",   ["icon"]),
        ("hero",   ["hero"]),
        ("logo",   ["logo"]),
        ("header", ["grid"]),
    ]
    for field, keys in field_url_keys:
        url = next((urls.get(k) for k in keys if urls.get(k)), None)
        if url:
            setattr(images, field, f"{field}{_get_extension(url)}")
    return images


# ── Game enrichment ──────────────────────────────────────────────────────

def _apply_sgdb_data(game, game_id: int, urls: dict, official_name: str | None) -> bool:
    """Update game fields from SGDB data. Returns True if any field changed."""
    changed = False

    if game.steamgriddb_id != game_id:
        game.steamgriddb_id = game_id
        changed = True

    if official_name and game.title != official_name:
        game.title = official_name
        changed = True

    new_images = _urls_to_image_filenames(urls)
    for field in ("cover", "icon", "hero", "logo", "header"):
        new_val = getattr(new_images, field)
        if new_val and not getattr(game.images, field):
            setattr(game.images, field, new_val)
            changed = True

    if urls:
        game._artwork_urls = urls  # type: ignore[attr-defined]

    return changed


# ── Main entry point ─────────────────────────────────────────────────────

def enrich(db: GameDatabase, cfg: dict) -> None:
    """
    Enrich games with SteamGridDB data: official names, IDs, artwork URLs.
    Only runs if STEAMGRIDDB_API_KEY is configured.
    """
    api_key = get_steamgriddb_key(cfg)
    if not api_key:
        return

    project_root = os.path.dirname(cfg["_CONFIG_PATH"]) if cfg.get("_CONFIG_PATH") else os.getcwd()
    sgdb_cache   = load_sgdb_cache(project_root)
    cache_snapshot = json.dumps(sgdb_cache, sort_keys=True)

    games = db.games_needing_enrichment() or [g for g in db.games if not g.images.cover]
    if not games:
        return

    logger.info(f"Enriching {len(games)} game(s) from SteamGridDB")

    for game in games:
        game_id, urls, official_name = get_sgdb_info(
            game.title, api_key, sgdb_cache, game.steamgriddb_id, cfg=cfg
        )
        if not game_id:
            logger.info(f"  Not found: {game.title}")
            continue

        if _apply_sgdb_data(game, game_id, urls, official_name):
            game.needs_persist = True
            logger.info(f"  Enriched: {game.folder_name} -> {game.title} (SGDB #{game_id})")
        elif not game.images.cover:
            logger.info(f"  No artwork found on SteamGridDB for: {game.title}")

    if json.dumps(sgdb_cache, sort_keys=True) != cache_snapshot:
        save_sgdb_cache(project_root, sgdb_cache)
