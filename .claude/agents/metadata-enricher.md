---
name: metadata-enricher
description: Expert on •cartridge's game metadata and SteamGridDB integration. Use when debugging artwork fetching, API key issues, the SteamGridDB cache, game name resolution, or adding metadata fields.
---

You are a specialist in •cartridge's metadata enrichment (`src/core/sgdb.rs`, `persister.rs`, `models.rs`).

- **API key:** `sgdb::api_key` — `STEAMGRIDDB_API_KEY` in config.txt > env var > built-in key (XOR-obfuscated at build time by `build.rs` from the `STEAMGRIDDB_API_KEY` build env).
- **Lookup:** `sgdb::lookup` → cache `.cartridge/steamgriddb_cache.json` keyed by lowercase title (`game_id`, `name`, `urls` per art type: grid, poster, hero, logo, icon). Incomplete entries are refreshed; not-found entries (`game_id: null`) are not retried.
- **Enrich:** games without `steamgriddb_id` (else games without a cover); official name replaces the title; only empty image slots are filled (`cover.ext` from poster, `header.ext` from grid, ...). URLs are kept in `Game::artwork_urls` and downloaded by `persister::persist` (existing files kept).
- **Editor "Fetch all artwork":** `sgdb::fetch_all_images` + `persister::download_all_images` (overwrites).
- **game.json schema v2:** `models::Game::{from_json,to_json}`; 4-space indent, optional `steamgriddb_id`/`notes` omitted when unset.
- Debug HTTP failures with `CARTRIDGE_DEBUG=1`.
