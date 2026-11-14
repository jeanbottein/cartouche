//! Pipeline step 3: SteamGridDB metadata (official names, IDs, artwork URLs),
//! with a JSON cache in `system/steamgriddb_cache.json` to spare API calls.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use log::{debug, info, warn};
use serde_json::{json, Map, Value};

use super::app::APP_NAME;
use super::config::Config;
use super::models::{ArtworkUrls, Game, ImageSlot, Images};
use super::paths;

const API: &str = "https://www.steamgriddb.com/api/v2";
const CACHE_FILE: &str = "steamgriddb_cache.json";

/// SteamGridDB didn't answer once: enrichment stops asking until the next
/// launch, instead of waiting for a timeout per game on every run (offline).
static UNREACHABLE: AtomicBool = AtomicBool::new(false);

include!(concat!(env!("OUT_DIR"), "/api_key.rs"));

fn built_in_key() -> String {
    let key: Vec<u8> = KEY.iter().zip(NONCE).map(|(k, n)| k ^ n).collect();
    String::from_utf8_lossy(&key).into_owned()
}

/// API key: `config.txt` > `STEAMGRIDDB_API_KEY` env var > built-in default.
pub fn api_key(cfg: &Config) -> Option<String> {
    let from_config = cfg.get("STEAMGRIDDB_API_KEY").map(str::to_string);
    let from_env = || std::env::var("STEAMGRIDDB_API_KEY").ok();
    [from_config, from_env(), Some(built_in_key())]
        .into_iter()
        .flatten()
        .map(|k| k.trim().to_string())
        .find(|k| !k.is_empty())
}

pub fn user_agent() -> String {
    format!("{APP_NAME}/1.0")
}

pub fn http_get(url: &str, timeout_secs: u64) -> Result<ureq::Response, String> {
    ureq::get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .set("User-Agent", &user_agent())
        .call()
        .map_err(|e| e.to_string())
}

/// SteamGridDB gave no answer (offline, bad key, server error): nothing was
/// learned, so nothing may be cached as "not found".
#[derive(Debug)]
struct Unreachable;

/// GET an API endpoint; its `data` (`null` when there is none).
fn request_data(endpoint: &str, key: &str) -> Result<Value, Unreachable> {
    let response = ureq::get(&format!("{API}/{endpoint}"))
        .timeout(Duration::from_secs(10))
        .set("Authorization", &format!("Bearer {key}"))
        .set("User-Agent", &user_agent())
        .call();
    api_data(response).inspect_err(|_| debug!("SteamGridDB gave no answer for {endpoint}"))
}

/// A 404 is an answer: that game or artwork doesn't exist.
fn api_data(response: Result<ureq::Response, ureq::Error>) -> Result<Value, Unreachable> {
    let text = match response {
        Ok(response) => response.into_string().map_err(|_| Unreachable)?,
        Err(ureq::Error::Status(404, _)) => return Ok(Value::Null),
        Err(_) => return Err(Unreachable),
    };
    let body: Value = serde_json::from_str(&text).map_err(|_| Unreachable)?;
    Ok(body
        .get("data")
        .filter(|_| body.get("success").and_then(Value::as_bool) == Some(true))
        .cloned()
        .unwrap_or(Value::Null))
}

/// GET an API endpoint; its `data` list (empty when there is none).
fn request(endpoint: &str, key: &str) -> Result<Vec<Value>, Unreachable> {
    let data = request_data(endpoint, key)?;
    Ok(data.as_array().cloned().unwrap_or_default())
}

/// Gregorian year of a Unix timestamp (days-to-civil, Howard Hinnant).
fn year_of_timestamp(secs: i64) -> i64 {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_index = (5 * doy + 2) / 153; // 0 = March
    yoe + era * 400 + i64::from(month_index >= 10)
}

/// Release year of a SteamGridDB game object (`release_date` is a Unix timestamp).
fn release_year(game: &Value) -> Option<u32> {
    let secs = game.get("release_date")?.as_i64()?;
    u32::try_from(year_of_timestamp(secs)).ok()
}

/// Release year of a known SteamGridDB game.
fn fetch_year(game_id: i64, key: &str) -> Result<Option<u32>, Unreachable> {
    Ok(release_year(&request_data(
        &format!("games/id/{game_id}"),
        key,
    )?))
}

/// Percent-encode a path segment (like `urllib.parse.quote`, keeping `/`).
fn quote(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub id: i64,
    pub name: Option<String>,
    pub year: Option<u32>,
}

fn search_hit(game: &Value) -> Option<SearchHit> {
    Some(SearchHit {
        id: game.get("id")?.as_i64()?,
        name: game.get("name").and_then(Value::as_str).map(str::to_string),
        year: release_year(game),
    })
}

fn try_search_games(name: &str, key: &str) -> Result<Vec<SearchHit>, Unreachable> {
    let data = request(&format!("search/autocomplete/{}", quote(name)), key)?;
    Ok(data.iter().filter_map(search_hit).collect())
}

/// Games matching `name`, best first (empty when none, or offline).
pub fn search_games(name: &str, key: &str) -> Vec<SearchHit> {
    try_search_games(name, key).unwrap_or_default()
}

fn search_game(name: &str, key: &str) -> Result<Option<SearchHit>, Unreachable> {
    Ok(try_search_games(name, key)?.into_iter().next())
}

/// URL of the highest-scored entry (first on ties).
fn pick_best(data: &[Value]) -> Option<String> {
    let score = |d: &Value| d.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let best = data.iter().fold(None::<&Value>, |best, d| match best {
        Some(b) if score(b) >= score(d) => Some(b),
        _ => Some(d),
    })?;
    best.get("url").and_then(Value::as_str).map(str::to_string)
}

fn content_filters(cfg: &Config) -> String {
    let flag = |key| if cfg.is_true(key) { "any" } else { "false" };
    format!(
        "nsfw={}&humor={}&epilepsy={}",
        flag("STEAMGRIDDB_NSFW"),
        flag("STEAMGRIDDB_HUMOR"),
        flag("STEAMGRIDDB_EPILEPSY")
    )
}

/// Best artwork URL per art type (every type present, `None` when not found).
fn fetch_artwork_urls(game_id: i64, key: &str, cfg: &Config) -> Result<ArtworkUrls, Unreachable> {
    let filters = content_filters(cfg);
    let endpoints = [
        (
            "grid",
            "grids",
            format!("dimensions=460x215,920x430&{filters}"),
            true,
        ),
        (
            "poster",
            "grids",
            format!("dimensions=600x900,342x482&{filters}"),
            true,
        ),
        ("hero", "heroes", filters.clone(), false),
        ("logo", "logos", filters.clone(), false),
        ("icon", "icons", filters.clone(), false),
    ];
    endpoints
        .iter()
        .map(|(art, endpoint, query, fallback)| {
            let mut data = request(&format!("{endpoint}/game/{game_id}?{query}"), key)?;
            if data.is_empty() && *fallback {
                data = request(&format!("{endpoint}/game/{game_id}?{filters}"), key)?;
            }
            Ok((art.to_string(), pick_best(&data)))
        })
        .collect()
}

/// File extension of an artwork URL (defaults to `.png`).
fn url_extension(url: &str) -> String {
    let path = url.split('?').next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    let leading_dots = name.len() - name.trim_start_matches('.').len();
    match name[leading_dots..].rfind('.') {
        Some(i) => name[leading_dots + i..].to_string(),
        None => ".png".to_string(),
    }
}

/// Local `.cartridge/` filenames for the artwork that has a URL.
pub fn image_names(urls: &ArtworkUrls) -> Images {
    let mut images = Images::default();
    for slot in ImageSlot::ALL {
        if let Some(Some(url)) = urls.get(slot.art_type()) {
            *images.slot_mut(slot) = Some(format!("{}{}", slot.name(), url_extension(url)));
        }
    }
    images
}

// ── Cache ────────────────────────────────────────────────────────────────

pub fn load_cache(system_dir: &Path) -> Map<String, Value> {
    let path = system_dir.join(CACHE_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Map::new();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        warn!("Failed to load SteamGridDB cache: {e}");
        Map::new()
    })
}

fn save_cache(system_dir: &Path, cache: &Map<String, Value>) {
    let text = serde_json::to_string_pretty(cache).expect("JSON serialization cannot fail");
    if let Err(e) = paths::write_atomic(system_dir.join(CACHE_FILE), text) {
        warn!("Failed to save SteamGridDB cache: {e}");
    }
}

fn urls_of(entry: &Value) -> ArtworkUrls {
    entry
        .get("urls")
        .and_then(Value::as_object)
        .map(|urls| {
            urls.iter()
                .map(|(k, v)| (k.clone(), v.as_str().map(str::to_string)))
                .collect()
        })
        .unwrap_or_default()
}

fn is_complete(entry: &Value) -> bool {
    let urls = urls_of(entry);
    entry.get("name").is_some()
        && entry.get("year").is_some()
        && urls.contains_key("icon")
        && urls.contains_key("poster")
}

pub struct SgdbInfo {
    pub id: i64,
    pub urls: ArtworkUrls,
    pub name: Option<String>,
    pub year: Option<u32>,
}

fn read_entry(entry: &Value) -> Option<SgdbInfo> {
    Some(SgdbInfo {
        id: entry.get("game_id")?.as_i64()?,
        urls: urls_of(entry),
        name: entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        year: entry
            .get("year")
            .and_then(Value::as_u64)
            .and_then(|y| u32::try_from(y).ok()),
    })
}

/// Fill what a partial cache entry is missing; what SteamGridDB doesn't
/// answer stays missing, to ask again next run.
fn refresh_entry(name: &str, entry: &mut Value, key: &str, cfg: &Config) {
    if entry.get("name").is_none() {
        if let Ok(Some(official)) = search_game(name, key).map(|hit| hit.and_then(|h| h.name)) {
            entry["name"] = json!(official);
        }
    }
    let Some(id) = entry.get("game_id").and_then(Value::as_i64) else {
        return;
    };
    if entry.get("year").is_none() {
        if let Ok(year) = fetch_year(id, key) {
            entry["year"] = json!(year); // null: looked up, unknown
        }
    }
    let urls = urls_of(entry);
    if !urls.contains_key("icon") || !urls.contains_key("poster") {
        if let Ok(urls) = fetch_artwork_urls(id, key, cfg) {
            entry["urls"] = json!(urls);
        }
    }
}

fn fetch_entry(
    name: &str,
    known_id: Option<i64>,
    key: &str,
    cfg: &Config,
) -> Result<Value, Unreachable> {
    let found = match known_id {
        Some(id) => Some(SearchHit {
            id,
            name: Some(name.to_string()),
            year: fetch_year(id, key)?,
        }),
        None => search_game(name, key)?,
    };
    Ok(match found {
        Some(hit) => json!({
            "game_id": hit.id,
            "urls": fetch_artwork_urls(hit.id, key, cfg)?,
            "name": hit.name,
            "year": hit.year,
        }),
        None => json!({"game_id": null, "urls": {}, "name": null}),
    })
}

/// A cache entry for another game than the one the game.json names (the user
/// picked another match): the game's own ID wins.
fn contradicts(entry: &Value, known_id: Option<i64>) -> bool {
    known_id.is_some_and(|id| entry.get("game_id").and_then(Value::as_i64) != Some(id))
}

/// SteamGridDB info for `name`, served from the cache when possible.
pub fn lookup(
    name: &str,
    known_id: Option<i64>,
    key: &str,
    cache: &mut Map<String, Value>,
    cfg: &Config,
) -> Option<SgdbInfo> {
    let cache_key = name.to_lowercase();
    let stale = cache
        .get(&cache_key)
        .is_none_or(|entry| contradicts(entry, known_id));
    if stale {
        let fetched = fetch_entry(name, known_id, key, cfg)
            .map_err(|_| UNREACHABLE.store(true, Ordering::Relaxed))
            .ok()?;
        cache.insert(cache_key.clone(), fetched);
    }
    let entry = cache.get_mut(&cache_key)?;
    if !is_complete(entry) && entry.get("game_id").is_some_and(|id| !id.is_null()) {
        refresh_entry(name, entry, key, cfg);
    }
    read_entry(entry)
}

// ── Enrichment ───────────────────────────────────────────────────────────

fn official_name(info: &SgdbInfo) -> Option<&str> {
    info.name.as_deref().filter(|n| !n.is_empty())
}

fn would_change(game: &Game, info: &SgdbInfo) -> bool {
    let new_images = image_names(&info.urls);
    game.steamgriddb_id != Some(info.id)
        || official_name(info).is_some_and(|n| n != game.title)
        || (game.year.is_none() && info.year.is_some())
        || ImageSlot::ALL
            .iter()
            .any(|&s| new_images.get(s).is_some() && game.images.get(s).is_none())
}

fn apply(game: &mut Game, info: SgdbInfo) {
    game.steamgriddb_id = Some(info.id);
    if let Some(name) = official_name(&info) {
        game.title = name.to_string();
    }
    game.year = game.year.or(info.year);
    let new_images = image_names(&info.urls);
    for slot in ImageSlot::ALL {
        if game.images.get(slot).is_none() {
            *game.images.slot_mut(slot) = new_images.get(slot).map(str::to_string);
        }
    }
    if !info.urls.is_empty() {
        game.artwork_urls = info.urls;
    }
}

/// An installed game SteamGridDB may tell more about: no ID, cover or year
/// yet. What it answered is cached, so asking again about a game it knows
/// nothing more of costs no request.
fn wants_enrichment(game: &Game) -> bool {
    game.is_installed()
        && (game.steamgriddb_id.is_none() || game.images.cover.is_none() || game.year.is_none())
}

/// Add SteamGridDB names, IDs, release years and artwork to games that lack them.
pub fn enrich(games: &mut [Game], cfg: &Config) {
    let Some(key) = api_key(cfg) else { return };
    let todo: Vec<&mut Game> = games.iter_mut().filter(|g| wants_enrichment(g)).collect();
    if todo.is_empty() || UNREACHABLE.load(Ordering::Relaxed) {
        return;
    }
    // Read once there is work: enrichment runs per batch of games.
    let system_dir = cfg.system_dir();
    let mut cache = load_cache(&system_dir);
    let snapshot = cache.clone();
    info!("Enriching {} game(s) from SteamGridDB", todo.len());

    for game in todo {
        if UNREACHABLE.load(Ordering::Relaxed) {
            warn!("SteamGridDB unreachable: trying again at the next launch");
            break;
        }
        let Some(found) = lookup(&game.title, game.steamgriddb_id, &key, &mut cache, cfg) else {
            info!("  Not found: {}", game.title);
            continue;
        };
        if would_change(game, &found) {
            let id = found.id;
            apply(game, found);
            game.needs_persist = true;
            info!(
                "  Enriched: {} -> {} (SGDB #{id})",
                game.folder_name, game.title
            );
        } else if game.images.cover.is_none() {
            info!("  No artwork found on SteamGridDB for: {}", game.title);
        }
    }

    if cache != snapshot {
        save_cache(&system_dir, &cache);
    }
}

/// Re-fetch every artwork slot for one game (editor "Fetch images"); returns the
/// new filenames and their URLs, or why nothing was fetched.
pub fn fetch_all_images(game_id: i64, cfg: &Config) -> Result<(Images, ArtworkUrls), String> {
    let key = api_key(cfg).ok_or("No API key configured.")?;
    let urls = fetch_artwork_urls(game_id, &key, cfg)
        .map_err(|_| "SteamGridDB is unreachable (offline?).")?;
    let images = image_names(&urls);
    if ImageSlot::ALL.iter().all(|&s| images.get(s).is_none()) {
        return Err("No images found on SteamGridDB.".into());
    }
    Ok((images, urls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Install;

    fn urls(pairs: &[(&str, Option<&str>)]) -> ArtworkUrls {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
            .collect()
    }

    #[test]
    fn image_names_use_slot_and_url_extension() {
        let images = image_names(&urls(&[
            ("poster", Some("https://cdn/x/abc.jpg?v=2")),
            ("grid", Some("https://cdn/y/def.png")),
            ("icon", Some("https://cdn/z/noext")),
            ("hero", None),
        ]));
        assert_eq!(images.cover.as_deref(), Some("cover.jpg"));
        assert_eq!(images.header.as_deref(), Some("header.png"));
        assert_eq!(images.icon.as_deref(), Some("icon.png"));
        assert_eq!(images.hero, None);
    }

    fn response(status: u16, body: &str) -> ureq::Response {
        ureq::Response::new(status, "", body).unwrap()
    }

    #[test]
    fn an_answer_is_its_data_even_when_empty() {
        let answer = api_data(Ok(response(200, r#"{"success": true, "data": []}"#)));
        assert_eq!(answer.unwrap(), json!([]));
    }

    #[test]
    fn not_found_is_an_answer_with_no_data() {
        let answer = api_data(Err(ureq::Error::Status(404, response(404, "{}"))));
        assert_eq!(answer.unwrap(), Value::Null);
    }

    #[test]
    fn server_errors_and_bad_bodies_are_no_answer() {
        assert!(api_data(Err(ureq::Error::Status(500, response(500, "")))).is_err());
        assert!(api_data(Err(ureq::Error::Status(401, response(401, "")))).is_err());
        assert!(api_data(Ok(response(200, "<html>"))).is_err());
    }

    #[test]
    fn quote_matches_urllib() {
        assert_eq!(
            quote("Hollow Knight: Silk/Song!"),
            "Hollow%20Knight%3A%20Silk/Song%21"
        );
    }

    #[test]
    fn pick_best_takes_highest_score_first_on_ties() {
        let data = vec![
            json!({"score": 1, "url": "a"}),
            json!({"score": 5, "url": "b"}),
            json!({"score": 5, "url": "c"}),
        ];
        assert_eq!(pick_best(&data).as_deref(), Some("b"));
    }

    #[test]
    fn cached_entries_skip_the_network() {
        let cfg = Config::from_raw("/ws/system/config.txt".into(), Default::default());
        let mut cache = Map::new();
        cache.insert(
            "celeste".into(),
            json!({"game_id": 7, "name": "Celeste", "year": 2018, "urls": {"icon": "https://i/1.ico", "poster": null}}),
        );
        let info = lookup("Celeste", None, "unused", &mut cache, &cfg).unwrap();
        assert_eq!(
            (info.id, info.name.as_deref(), info.year),
            (7, Some("Celeste"), Some(2018))
        );
        assert_eq!(image_names(&info.urls).icon.as_deref(), Some("icon.ico"));

        cache.insert(
            "unknown".into(),
            json!({"game_id": null, "urls": {}, "name": null}),
        );
        assert!(lookup("Unknown", None, "unused", &mut cache, &cfg).is_none());
    }

    #[test]
    fn search_hits_keep_order_and_skip_entries_without_id() {
        let hits: Vec<SearchHit> = [
            json!({"id": 2, "name": "Celeste", "release_date": 1_516_838_400}),
            json!({"name": "No id"}),
            json!({"id": 9, "name": "Celeste Classic"}),
        ]
        .iter()
        .filter_map(search_hit)
        .collect();
        let summary: Vec<_> = hits.iter().map(|h| (h.id, h.year)).collect();
        assert_eq!(summary, [(2, Some(2018)), (9, None)]);
    }

    #[test]
    fn cache_entry_for_another_game_than_the_known_id_is_stale() {
        let entry = json!({"game_id": 7});
        assert!(contradicts(&entry, Some(8)));
        assert!(!contradicts(&entry, Some(7)));
        assert!(!contradicts(&entry, None));
        assert!(contradicts(&json!({"game_id": null}), Some(7)));
    }

    #[test]
    fn every_game_missing_something_is_enriched_whatever_the_others() {
        let complete = Game {
            steamgriddb_id: Some(7),
            year: Some(2018),
            images: Images {
                cover: Some("cover.png".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let no_cover = Game {
            images: Images::default(),
            ..complete.clone()
        };
        let packaged = Game {
            install: Install::Packaged {
                package: "/g/X.cartridge.tar".into(),
            },
            ..Default::default()
        };
        assert!(wants_enrichment(&Game::default()));
        assert!(wants_enrichment(&no_cover));
        assert!(wants_enrichment(&Game {
            year: None,
            ..complete.clone()
        }));
        assert!(!wants_enrichment(&complete));
        assert!(!wants_enrichment(&packaged));
    }

    #[test]
    fn apply_keeps_existing_images() {
        let mut game = Game {
            title: "celeste".into(),
            ..Default::default()
        };
        game.images.cover = Some("mine.png".into());
        let info = SgdbInfo {
            id: 7,
            name: Some("Celeste".into()),
            urls: urls(&[("poster", Some("u/p.jpg")), ("hero", Some("u/h.jpg"))]),
            year: Some(2018),
        };
        assert!(would_change(&game, &info));
        apply(&mut game, info);
        assert_eq!(game.title, "Celeste");
        assert_eq!(game.images.cover.as_deref(), Some("mine.png"));
        assert_eq!(game.images.hero.as_deref(), Some("hero.jpg"));
    }

    #[test]
    fn release_year_reads_unix_timestamps() {
        // 2018-01-25 and 1999-12-31T23:59:59Z
        assert_eq!(
            release_year(&json!({"release_date": 1_516_838_400})),
            Some(2018)
        );
        assert_eq!(
            release_year(&json!({"release_date": 946_684_799})),
            Some(1999)
        );
        assert_eq!(release_year(&json!({"release_date": null})), None);
    }
}
