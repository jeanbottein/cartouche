"""
Steam non-Steam shortcut manager for gamer-sidekick.

Reads manifests.json (produced by manifester.py) and synchronises the entries
with Steam's shortcuts.vdf.  Every shortcut created by this module is tagged
with "gamer-sidekick" so it can later be identified and removed when the
corresponding game is uninstalled.

Binary VDF reader/writer is implemented with the standard library only
(struct), following the format used by Steam's shortcuts.vdf files.

Reference: GameSync by Maikeru86 (MIT) – adapted for stdlib-only usage.
"""

import os
import struct
import json
import zlib
import logging
import urllib.request
import urllib.error

logger = logging.getLogger("exposer")

# ── Tag used to identify shortcuts created by gamer-sidekick ──────────────
OWNERSHIP_TAG = "gamer-sidekick"

# ── Binary VDF type bytes ─────────────────────────────────────────────────
_TYPE_MAP    = 0x00  # nested map
_TYPE_STR    = 0x01  # null-terminated string
_TYPE_INT32  = 0x02  # 4-byte signed int (little-endian)
_TYPE_END    = 0x08  # end of current map


# ═══════════════════════════════════════════════════════════════════════════
# Binary VDF reader / writer  (stdlib only)
# ═══════════════════════════════════════════════════════════════════════════

def _read_string(data, pos):
    """Read a null-terminated UTF-8 string starting at *pos*."""
    end = data.index(b'\x00', pos)
    return data[pos:end].decode('utf-8', errors='replace'), end + 1


def _read_int32(data, pos):
    """Read a 4-byte little-endian signed integer."""
    val = struct.unpack_from('<i', data, pos)[0]
    return val, pos + 4


def _read_map(data, pos):
    """Recursively read a binary VDF map and return an OrderedDict-like dict."""
    result = {}
    while pos < len(data):
        type_byte = data[pos]
        pos += 1

        if type_byte == _TYPE_END:
            return result, pos

        # Read key
        key, pos = _read_string(data, pos)

        if type_byte == _TYPE_MAP:
            val, pos = _read_map(data, pos)
        elif type_byte == _TYPE_STR:
            val, pos = _read_string(data, pos)
        elif type_byte == _TYPE_INT32:
            val, pos = _read_int32(data, pos)
        else:
            raise ValueError(f"Unknown VDF type byte 0x{type_byte:02x} at offset {pos - 1}")

        result[key] = val

    return result, pos


def binary_vdf_load(f):
    """Load a binary VDF file and return its contents as nested dicts."""
    data = f.read()
    if not data:
        return {}
    result, _ = _read_map(data, 0)
    return result


def _write_string(f, key, value):
    f.write(struct.pack('B', _TYPE_STR))
    f.write(key.encode('utf-8') + b'\x00')
    f.write(value.encode('utf-8') + b'\x00')


def _write_int32(f, key, value):
    f.write(struct.pack('B', _TYPE_INT32))
    f.write(key.encode('utf-8') + b'\x00')
    f.write(struct.pack('<i', value))


def _write_map(f, key, value):
    f.write(struct.pack('B', _TYPE_MAP))
    f.write(key.encode('utf-8') + b'\x00')
    _write_map_contents(f, value)
    f.write(struct.pack('B', _TYPE_END))


def _write_map_contents(f, d):
    for k, v in d.items():
        if isinstance(v, dict):
            _write_map(f, k, v)
        elif isinstance(v, int):
            _write_int32(f, k, v)
        elif isinstance(v, str):
            _write_string(f, k, v)
        else:
            _write_string(f, k, str(v))


def binary_vdf_dump(obj, f):
    """Write nested dicts as a binary VDF file."""
    for k, v in obj.items():
        if isinstance(v, dict):
            _write_map(f, k, v)
        elif isinstance(v, int):
            _write_int32(f, k, v)
        elif isinstance(v, str):
            _write_string(f, k, v)
    f.write(struct.pack('B', _TYPE_END))


# ═══════════════════════════════════════════════════════════════════════════
# Steam path helpers
# ═══════════════════════════════════════════════════════════════════════════

def find_steam_userdata_dirs():
    """Return a list of all Steam userdata/<id>/config directories found."""
    candidates = [
        os.path.expanduser("~/.steam/steam/userdata"),
        os.path.expanduser("~/.local/share/Steam/userdata"),
    ]
    results = []
    for base in candidates:
        if not os.path.isdir(base):
            continue
        for uid in os.listdir(base):
            config_dir = os.path.join(base, uid, "config")
            if os.path.isdir(config_dir):
                results.append(config_dir)
    return results


def _get_shortcuts_path(config_dir):
    return os.path.join(config_dir, "shortcuts.vdf")


def _get_grid_dir(config_dir):
    """Return the grid folder path for storing artwork images."""
    return os.path.join(os.path.dirname(config_dir), "config", "grid")


# ═══════════════════════════════════════════════════════════════════════════
# SteamGridDB artwork  (stdlib urllib only)
# ═══════════════════════════════════════════════════════════════════════════

def _steamgriddb_request(endpoint, api_key):
    """Make a GET request to SteamGridDB API. Returns parsed JSON or None."""
    url = f"https://www.steamgriddb.com/api/v2/{endpoint}"
    req = urllib.request.Request(url, headers={
        "Authorization": f"Bearer {api_key}",
        "User-Agent": "gamer-sidekick/1.0"
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
    """Search SteamGridDB for a game and return (id, official_name) or (None, None)."""
    encoded = urllib.request.quote(game_name)
    data = _steamgriddb_request(f"search/autocomplete/{encoded}", api_key)
    if data:
        return data[0].get("id"), data[0].get("name")
    return None, None


def fetch_artwork_urls(game_id, api_key):
    """
    Fetch artwork URLs for a game from SteamGridDB.
    Returns dict with keys: grid, hero, logo (URL or None each).
    """
    result = {}
    for art_type, endpoint in [("grid", "grids"), ("hero", "heroes"), ("logo", "logos"), ("icon", "icons")]:
        data = _steamgriddb_request(f"{endpoint}/game/{game_id}", api_key)
        result[art_type] = data[0]["url"] if data else None
    return result


def _download_file(url, local_path):
    """Download a file from URL to local_path. Returns True on success."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "gamer-sidekick/1.0"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            with open(local_path, 'wb') as f:
                f.write(resp.read())
        return True
    except (urllib.error.URLError, OSError) as e:
        logger.warning(f"⚠️  Failed to download {url}: {e}")
        return False


def _get_extension(url):
    """Extract file extension from URL."""
    path = url.split("?")[0]  # strip query params
    _, ext = os.path.splitext(path)
    return ext or ".png"


def load_sgdb_cache(project_root):
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    if os.path.exists(cache_path):
        try:
            with open(cache_path, 'r') as f:
                return json.load(f)
        except Exception as e:
            logger.warning(f"⚠️  Failed to load steamgriddb cache: {e}")
    return {}


def save_sgdb_cache(project_root, cache):
    cache_path = os.path.join(project_root, "steamgriddb_cache.json")
    try:
        with open(cache_path, 'w') as f:
            json.dump(cache, f, indent=2)
    except Exception as e:
        logger.warning(f"⚠️  Failed to save steamgriddb cache: {e}")


def get_sgdb_info(name, api_key, cache, manifest_id=None):
    """
    Returns (game_id, urls_dict, official_name). Uses cache to prevent API calls.
    Updates cache in-place if fresh data is fetched or name/icon is missing.
    Returns (None, {}, None) if not found.
    """
    key = name.lower()
    if key in cache:
        cached = cache[key]
        urls = cached.get("urls", {})
        if "name" in cached and "icon" in urls:
            return cached.get("game_id"), urls, cached.get("name")
        
        # If ID exists but name or icon is missing, we need to fetch it (migration to newer features)
        game_id = cached.get("game_id")
        if game_id:
            needs_update = False
            if "name" not in cached:
                _, official_name = search_game_id(name, api_key)
                if official_name:
                    cached["name"] = official_name
                    needs_update = True
            if "icon" not in urls:
                fresh_urls = fetch_artwork_urls(game_id, api_key)
                cached["urls"] = fresh_urls
                needs_update = True
                
            if needs_update:
                return game_id, cached.get("urls", {}), cached.get("name")
        elif cached.get("game_id") is None and "game_id" in cached:
            # Explicitly searched and not found before
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


def save_artwork(appid, urls, grid_dir):
    """
    Save grid/hero/logo artwork from urls.
    Files are saved to grid_dir with Steam's naming convention.
    """
    os.makedirs(grid_dir, exist_ok=True)
    name_map = {
        "grid": f"{appid}p",
        "hero": f"{appid}_hero",
        "logo": f"{appid}_logo",
        "icon": f"{appid}_icon",
    }

    for art_type, prefix in name_map.items():
        url = urls.get(art_type)
        if not url:
            continue
        ext = _get_extension(url)
        dest = os.path.join(grid_dir, f"{prefix}{ext}")
        # Skip if already downloaded
        if any(os.path.exists(os.path.join(grid_dir, f"{prefix}{e}")) for e in [".png", ".jpg", ".jpeg", ".webp", ".ico"]):
            continue
        if _download_file(url, dest):
            logger.info(f"  🖼️  {art_type}: {os.path.basename(dest)}")


# ═══════════════════════════════════════════════════════════════════════════
# AppID generation  (matches Steam's algorithm for non-Steam shortcuts)
# ═══════════════════════════════════════════════════════════════════════════

def generate_appid(app_name, exe_path):
    """Generate a stable non-Steam shortcut appid (signed 32-bit)."""
    unique = (exe_path + app_name).encode('utf-8')
    crc = zlib.crc32(unique) & 0xFFFFFFFF
    return (crc | 0x80000000) & 0xFFFFFFFF


def _signed32(val):
    """Convert an unsigned 32-bit int to a signed 32-bit int."""
    if val >= 0x80000000:
        return val - 0x100000000
    return val


# ═══════════════════════════════════════════════════════════════════════════
# Shortcut helpers
# ═══════════════════════════════════════════════════════════════════════════

def _has_ownership_tag(shortcut):
    """Return True if this shortcut was created by gamer-sidekick."""
    tags = shortcut.get("tags", {})
    if isinstance(tags, dict):
        return OWNERSHIP_TAG in tags.values()
    return False


def _next_index(shortcuts_dict):
    """Return the next numeric string key for a shortcuts dict."""
    if not shortcuts_dict:
        return "0"
    indices = [int(k) for k in shortcuts_dict if k.isdigit()]
    return str(max(indices) + 1) if indices else "0"


def _next_tag_index(tags_dict):
    """Return the next numeric string key for a tags dict."""
    if not tags_dict:
        return "0"
    indices = [int(k) for k in tags_dict if k.isdigit()]
    return str(max(indices) + 1) if indices else "0"


def _make_shortcut_entry(app_name, exe_path, start_dir, launch_options="", icon_path=""):
    """Build a shortcut dict entry in the format Steam expects."""
    appid = generate_appid(app_name, exe_path)
    tags = {"0": OWNERSHIP_TAG}
    return {
        "appid": _signed32(appid),
        "AppName": app_name,
        "Exe": f'"{exe_path}"',
        "StartDir": f'"{start_dir}"',
        "icon": icon_path,
        "ShortcutPath": "",
        "LaunchOptions": launch_options,
        "IsHidden": 0,
        "AllowDesktopConfig": 1,
        "AllowOverlay": 1,
        "OpenVR": 0,
        "Devkit": 0,
        "DevkitGameID": "",
        "DevkitOverrideAppID": 0,
        "LastPlayTime": 0,
        "tags": tags,
    }


# ═══════════════════════════════════════════════════════════════════════════
# Core sync logic
# ═══════════════════════════════════════════════════════════════════════════

def load_shortcuts(shortcuts_path):
    """Load the shortcuts.vdf file, returning the inner dict."""
    if not os.path.exists(shortcuts_path):
        return {}
    with open(shortcuts_path, 'rb') as f:
        data = binary_vdf_load(f)
    return data.get("shortcuts", {})


def save_shortcuts(shortcuts_path, shortcuts_dict):
    """Write the shortcuts dict back to shortcuts.vdf."""
    os.makedirs(os.path.dirname(shortcuts_path), exist_ok=True)
    with open(shortcuts_path, 'wb') as f:
        binary_vdf_dump({"shortcuts": shortcuts_dict}, f)


def _get_appname(shortcut):
    """Get the app name from a shortcut, handling both key casings."""
    return shortcut.get("AppName") or shortcut.get("appname") or ""


def sync_shortcuts(shortcuts_dict, manifests, api_key=None, cache=None, grid_dir=None):
    """
    Synchronise shortcuts with manifests.
    If api_key is provided, uses official names from SteamGridDB.
    Returns (updated_dict, added_count, removed_count).
    """
    # Build set of manifest exe paths for fast lookup
    manifest_by_name = {}
    for game in manifests:
        name = game.get("title", "")
        target = game.get("target", "")
        if name and target:
            manifest_by_name[name.lower()] = game

    # ── Identify existing gamer-sidekick shortcuts ────────────────────
    owned_keys = []          # keys of shortcuts we own
    owned_names = set()      # lowercase app names of shortcuts we own
    owned_exes = {}          # exe -> key mapping for owned shortcuts
    for key, shortcut in shortcuts_dict.items():
        if _has_ownership_tag(shortcut):
            owned_keys.append(key)
            name = _get_appname(shortcut).lower()
            owned_names.add(name)
            exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
            owned_exes[exe] = key

    # ── Remove stale shortcuts (by Exe path) ──────────────────────────
    removed = 0
    keys_to_remove = []
    
    # We remove if the EXE is no longer in manifests
    manifest_targets = {g.get("target") for g in manifests if g.get("target")}
    for key in owned_keys:
        shortcut = shortcuts_dict[key]
        exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
        if exe not in manifest_targets:
            keys_to_remove.append(key)
            logger.info(f"🗑️  Removing stale shortcut: {_get_appname(shortcut)}")
            removed += 1

    for key in keys_to_remove:
        del shortcuts_dict[key]

    # Reindex to keep keys contiguous (Steam expects "0", "1", "2", …)
    if keys_to_remove:
        reindexed = {}
        for i, (_, v) in enumerate(sorted(shortcuts_dict.items(), key=lambda x: int(x[0]) if x[0].isdigit() else 0)):
            reindexed[str(i)] = v
        shortcuts_dict = reindexed

    # Rebuild maps after removal
    owned_names = set()
    owned_exes = {}
    for key, shortcut in shortcuts_dict.items():
        if _has_ownership_tag(shortcut):
            owned_names.add(_get_appname(shortcut).lower())
            exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
            owned_exes[exe] = key

    # ── Add or Update shortcuts ───────────────────────────────────────
    added = 0
    for game in manifests:
        name = game.get("title", "")
        target = game.get("target", "")
        start_in = game.get("startIn", "")
        launch_opts = game.get("launchOptions", "")

        if not name or not target:
            continue

        # Use official name if available
        final_name = name
        icon_path = ""
        if api_key and cache is not None:
            _, urls, official_name = get_sgdb_info(name, api_key, cache, game.get("steamgriddb_id"))
            if official_name:
                final_name = official_name
            if grid_dir and "icon" in urls and urls["icon"]:
                ext = _get_extension(urls["icon"])
                appid = generate_appid(final_name, target)
                icon_path = os.path.join(grid_dir, f"{appid}_icon{ext}")

        # If we already have this EXE, check if the name matches or icon needs updating
        if target in owned_exes:
            key = owned_exes[target]
            existing_shortcut = shortcuts_dict[key]
            
            name_changed = _get_appname(existing_shortcut) != final_name
            icon_changed = existing_shortcut.get("icon", "") != icon_path
            
            if name_changed or icon_changed:
                logger.info(f"🔄 Updating shortcut: {_get_appname(existing_shortcut)} (name: {name_changed}, icon: {icon_changed})")
                shortcuts_dict[key] = _make_shortcut_entry(final_name, target, start_in, launch_opts, icon_path)
            continue

        idx = _next_index(shortcuts_dict)
        shortcuts_dict[idx] = _make_shortcut_entry(final_name, target, start_in, launch_opts, icon_path)
        owned_names.add(final_name.lower())
        owned_exes[target] = idx
        logger.info(f"➕ Added shortcut: {final_name} (matching '{name}')")
        added += 1

    return shortcuts_dict, added, removed


# ═══════════════════════════════════════════════════════════════════════════
# Public helpers (for external callers)
# ═══════════════════════════════════════════════════════════════════════════

def list_owned_shortcuts(shortcuts_dict):
    """Return a list of (key, appname) for all gamer-sidekick owned shortcuts."""
    result = []
    for key, shortcut in shortcuts_dict.items():
        if _has_ownership_tag(shortcut):
            result.append((key, _get_appname(shortcut)))
    return result


# ═══════════════════════════════════════════════════════════════════════════
# Dry-run / test mode
# ═══════════════════════════════════════════════════════════════════════════

def test_steam(config: dict):
    """
    Dry-run mode: print all shortcut info that would be written to Steam,
    including SteamGridDB artwork URLs if an API key is configured.
    Does NOT modify shortcuts.vdf.
    """
    games_dir = config.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        logger.warning("🤖 Steam test: FREEGAMES_PATH not configured or invalid")
        return

    manifests_path = os.path.join(games_dir, "manifests.json")
    if not os.path.exists(manifests_path):
        logger.warning("🤖 Steam test: manifests.json not found. Run manifester first.")
        return

    try:
        with open(manifests_path, 'r') as f:
            manifests = json.load(f)
    except Exception as e:
        logger.error(f"❌ Steam test: Failed to load manifests.json: {e}")
        return

    api_key = config.get("STEAMGRIDDB_API_KEY", "").strip()
    project_root = config.get("_CONFIG_PATH", "")
    if project_root:
        project_root = os.path.dirname(project_root)
    else:
        project_root = os.getcwd()

    sgdb_cache = load_sgdb_cache(project_root)
    cache_start_len = len(sgdb_cache)

    logger.info(f"🧪 Steam test: {len(manifests)} games from manifests.json")
    logger.info(f"   SteamGridDB API key: {'configured' if api_key else 'NOT SET (set STEAMGRIDDB_API_KEY in config.txt)'}")
    logger.info("")

    for game in manifests:
        name = game.get("title", "")
        target = game.get("target", "")
        start_in = game.get("startIn", "")
        launch_opts = game.get("launchOptions", "")

        if not name or not target:
            continue

        sgdb_id_from_manifest = game.get("steamgriddb_id")

        # Use official name if available
        final_name = name
        official_mapped = False
        if api_key:
            game_id, urls, official_name = get_sgdb_info(name, api_key, sgdb_cache, sgdb_id_from_manifest)
            if official_name:
                final_name = official_name
                official_mapped = (official_name != name)

        appid = generate_appid(final_name, target)
        signed = _signed32(appid)

        label = f"📦 {name}"
        if official_mapped:
            label = f"📦 {name} → {final_name}"
        
        logger.info(f"  {label}")
        logger.info(f"     AppID:    {appid} (signed: {signed})")
        logger.info(f"     Exe:      {target}")
        logger.info(f"     StartDir: {start_in}")
        if launch_opts:
            logger.info(f"     Options:  {launch_opts}")
        logger.info(f"     Tags:     [{OWNERSHIP_TAG}]")

        # SteamGridDB details
        if api_key:
            if game_id:
                if official_name:
                    logger.info(f"     SGDB Name: {official_name}")
                logger.info(f"     SGDB ID:   {game_id}")
                for art_type, url in urls.items():
                    if url:
                        ext = _get_extension(url)
                        if art_type == "grid":
                            filename = f"{appid}p{ext}"
                        elif art_type == "hero":
                            filename = f"{appid}_hero{ext}"
                        elif art_type == "logo":
                            filename = f"{appid}_logo{ext}"
                        else:
                            filename = f"{appid}_icon{ext}"
                        logger.info(f"     🖼️  {art_type}: {url}")
                        logger.info(f"        → {filename}")
                    else:
                        logger.info(f"     🖼️  {art_type}: not found")
            else:
                logger.info(f"     SteamGridDB: game not found")
        logger.info("")

    if len(sgdb_cache) > cache_start_len:
        save_sgdb_cache(project_root, sgdb_cache)

    # Show existing owned shortcuts
    config_dirs = find_steam_userdata_dirs()
    for config_dir in config_dirs:
        shortcuts_path = _get_shortcuts_path(config_dir)
        shortcuts = load_shortcuts(shortcuts_path)
        owned = list_owned_shortcuts(shortcuts)
        uid = os.path.basename(os.path.dirname(config_dir))
        if owned:
            logger.info(f"  🔗 Steam user {uid}: {len(owned)} existing gamer-sidekick shortcuts")
            for _, appname in owned:
                logger.info(f"     • {appname}")
        else:
            logger.info(f"  🔗 Steam user {uid}: no gamer-sidekick shortcuts yet")


# ═══════════════════════════════════════════════════════════════════════════
# Module entry point
# ═══════════════════════════════════════════════════════════════════════════

def _load_manifests(config):
    """Load manifests.json, returning the list or None on error."""
    games_dir = config.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        return None

    manifests_path = os.path.join(games_dir, "manifests.json")
    if not os.path.exists(manifests_path):
        logger.warning("🤖 Steam exposer: manifests.json not found")
        return None

    try:
        with open(manifests_path, 'r') as f:
            return json.load(f)
    except Exception as e:
        logger.error(f"❌ Steam exposer: Failed to load manifests.json: {e}")
        return None


def run(config: dict):
    if config.get("STEAM_EXPOSE", "False").lower() != "true":
        return

    manifests = _load_manifests(config)
    if not manifests:
        return

    api_key = config.get("STEAMGRIDDB_API_KEY", "").strip()

    manifest_ids_by_exe = {}
    for g in manifests:
        t = g.get("target")
        i = g.get("steamgriddb_id")
        if t and i:
            manifest_ids_by_exe[t] = i

    # Find Steam userdata directories
    config_dirs = find_steam_userdata_dirs()
    if not config_dirs:
        logger.warning("🤖 Steam exposer: No Steam userdata directories found. Skipping.")
        return

    # Optionally filter to a specific Steam user ID
    steam_userid = config.get("STEAM_USERID", "").strip()
    if steam_userid:
        config_dirs = [d for d in config_dirs if f"/{steam_userid}/" in d]
        if not config_dirs:
            logger.warning(f"🤖 Steam exposer: STEAM_USERID={steam_userid} not found")
            return

    project_root = config.get("_CONFIG_PATH", "")
    if project_root:
        project_root = os.path.dirname(project_root)
    else:
        project_root = os.getcwd()

    sgdb_cache = load_sgdb_cache(project_root)
    cache_start_len = len(sgdb_cache)

    total_added = 0
    total_removed = 0

    for config_dir in config_dirs:
        grid_dir = _get_grid_dir(config_dir)
        shortcuts_path = _get_shortcuts_path(config_dir)
        shortcuts = load_shortcuts(shortcuts_path)
        shortcuts, added, removed = sync_shortcuts(shortcuts, manifests, api_key, sgdb_cache, grid_dir)
        total_added += added
        total_removed += removed

        if added or removed:
            save_shortcuts(shortcuts_path, shortcuts)
            uid = os.path.basename(os.path.dirname(config_dir))
            logger.info(
                f"✅ Steam user {uid}: +{added} added, -{removed} removed"
            )

        # Fetch artwork for owned shortcuts if API key is available
        if api_key:
            grid_dir = _get_grid_dir(config_dir)
            for shortcut in shortcuts.values():
                if not _has_ownership_tag(shortcut):
                    continue
                name = _get_appname(shortcut)
                exe = shortcut.get("Exe", shortcut.get("exe", "")).strip('"')
                appid = generate_appid(name, exe)
                manifest_id = manifest_ids_by_exe.get(exe)
                game_id, urls, _ = get_sgdb_info(name, api_key, sgdb_cache, manifest_id)
                if game_id:
                    save_artwork(appid, urls, grid_dir)

    if len(sgdb_cache) > cache_start_len:
        save_sgdb_cache(project_root, sgdb_cache)

    if total_added or total_removed:
        logger.info(
            f"✅ Steam expose complete: {total_added} added, {total_removed} removed"
        )
    else:
        logger.info("🤖 Steam exposer: shortcuts already up to date")
