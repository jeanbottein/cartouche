# AGENTS.md – AI Agent Guide for Cartouche

## Project Overview

Cartouche (French for "cartridge") is a Rust app for managing DRM-free games on Linux (primarily Steam Deck). One binary: a retro macroquad UI by default, terminal mode with `-t`. The backend is a pipeline of phases over an in-memory game list.

## Pipeline

```
core::pipeline::Pipeline (groups: all / parse / steam / backup / patch)
 0. migrations::run_all      # Idempotent data migrations
 1. scanner::scan            # Parse .cartouche/game.json into Vec<Game>
 2. detector::detect         # Fill missing targets (exe detection)
 3. sgdb::enrich             # SteamGridDB names/IDs/artwork (if API key)
 4. persister::persist       # Write game.json + download images (PERSIST_DATA)
 5. steam::clean             # Remove stale Steam shortcuts (STEAM_EXPOSE)
 6. steam::export            # Insert/update shortcuts + Proton compat
    steam_self::register     #   + Cartouche itself (STEAM_ADD_SELF)
 7. manifest::write          # manifests.json (MANIFEST_EXPORT)
 8. patcher::run             # BPS patches / file replacements
 9. saver::run               # Backup/restore saves, symlink tree
10. commands::run            # RUN_AFTER_* shell commands
```

Migrations: `launch_manifest.json` → `.cartouche/game.json`, nested → flat `savePaths`, arch `x86_64` → `x64`. Add new ones to `migrations::run_all`; keep them idempotent.

## Architecture

```
src/
  main.rs, cli.rs        # Args: UI (default), -t/--terminal, -g/--group, -n/--dry-run
  terminal.rs            # Terminal mode + Steam dry-run preview
  core/                  # Backend, no UI code
    app.rs               # App name (from binary filename), workspace dir lookup
    config.rs            # config.txt parse / relative path resolution / in-place updates
    models.rs            # Game, Target, SavePath, Images; game.json (de)serialization
    paths.rs, platform.rs# normpath/expandvars/relpath; OS/arch tags, ELF/PE/Mach-O headers
    migrations.rs scanner.rs detector.rs sgdb.rs persister.rs
    vdf.rs               # Binary (shortcuts.vdf) + text (config.vdf) KeyValues
    steam.rs steam_self.rs manifest.rs bps.rs patcher.rs saver.rs commands.rs
    pipeline.rs init.rs logging.rs
  ui/                    # macroquad UI
    app.rs               # Shell: tabs, footer hints, game session lock, log sink
    library.rs editor.rs status.rs settings.rs setup.rs
    form.rs list.rs keyboard.rs browser.rs dialog.rs   # widgets
    covers.rs launcher.rs engine/ (canvas, input, text, theme)
assets/                  # Embedded: font, icon, steam-art, config-default.txt, games_locations.json
testdata/                # Golden fixtures produced by the former Python implementation
```

## Data Structure

```
WORKSPACE/
  .cartouche/config.txt, steamgriddb_cache.json, ui-thumbs/
  games/MyGame/.cartouche/game.json, cover.png, icon.png, hero.png, logo.png
```

## Key Conventions

- **Compatibility is the contract:** game.json (4-space JSON), shortcuts.vdf, AppIDs (`crc32(exe+title) | 0x80000000`), path normalization and save layout must stay byte-compatible with existing workspaces. Golden tests in `testdata/` guard this.
- **`Config`** keeps raw values (Settings editor) and resolved values (relative path keys + `BACKUP_*` resolved against the workspace).
- Flags: `is_true` for opt-in keys (STEAM_EXPOSE, ...), `is_not_false` for opt-out keys (PERSIST_DATA, MANIFEST_EXPORT).
- **Ownership tag** in Steam shortcuts is the app name (also recognizes legacy `"gamer-sidekick"`); the self shortcut uses `<app>-self`.
- **Logging:** `log` macros; `core::logging` prints to stderr and mirrors to the UI Status log.
- **Minimal dependencies:** macroquad, gilrs (feature `gamepad`), serde/serde_json, image, ureq, crc32fast, log.

## Dev Workflow

The host (Bazzite) has no Rust toolchain on purpose: build in the container.

```bash
./container.sh                         # cargo build --release (any cargo args work)
./container.sh test
./container.sh clippy --all-targets
./target/release/cartouche             # run on the host (UI)
```

With a native toolchain instead:

```bash
cargo run                              # UI
cargo run -- -t                        # full pipeline in the terminal
cargo run -- -n                        # Steam dry-run preview
cargo test && cargo clippy --all-targets
cargo build --no-default-features      # without libudev (no gamepad)
```

Edit `.cartouche/config.txt` (seeded from `assets/config-default.txt`).

## Testing

Unit tests live next to the code (`cargo test`); filesystem tests use `core::testutil::TempDir`. For UI changes, run under Xvfb (`Xvfb :99 &`, `DISPLAY=:99 cargo run`) and drive with `xdotool` / screenshot with `import -window root`.
