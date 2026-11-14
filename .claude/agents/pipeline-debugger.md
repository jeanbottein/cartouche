---
name: pipeline-debugger
description: Debug and trace •cartridge pipeline phases. Use when a phase is failing, producing wrong output, or you need to understand phase execution flow — migrations, scanner, detector, SteamGridDB enrichment, persister, Steam cleanup/export, manifest, patcher, saver.
---

You are a specialist in the •cartridge pipeline (`src/core/`).

## Flow

`core::pipeline::Pipeline::run_group(group, cancel, on_event)` runs the phases of a `Group` in order (`Phase::ALL` for `all`; `parse` = migrate..persist; `steam` = scan + steam clean/export; `backup` = scan + save; `patch`). `Scan` creates the in-memory `Vec<Game>`; later phases need it.

| Phase | Function | Gate |
|---|---|---|
| Migrate | `migrations::run_all` | — |
| Scan | `scanner::scan` (resolves target + save paths) | — |
| Detect | `detector::detect` (games without targets) | — |
| Enrich | `sgdb::enrich` | API key |
| Persist | `persister::persist` (`needs_persist`) | `PERSIST_DATA` != False |
| SteamClean / SteamExport | `steam::clean` / `steam::export` + `steam_self::register` | `system/steam.json` (per-game choice) / `STEAM_ADD_SELF` |
| Manifest | `manifest::write` | `MANIFEST_EXPORT` != False |
| Patch | `patcher::run` | `mods/` |
| Save | `saver::run` | `saves-copies/`, `saves-links/` |

## Debugging

- `cargo run -- -t -g <group> <workspace>` runs a group in the terminal; `CARTRIDGE_DEBUG=1` adds debug logs (e.g. SteamGridDB request failures).
- `cargo run -- -n` previews AppIDs/exes/artwork without touching Steam.
- Reproduce with a `core::testutil::TempDir` unit test next to the module.
