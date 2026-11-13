# AGENTS.md – AI Agent Guide for •cartridge

## Project Overview

•cartridge (French for "cartridge") is a Rust app for managing DRM-free games on Linux (primarily Steam Deck). One binary: a themed macroquad UI by default, terminal mode with `-t`. The backend is a pipeline of phases over an in-memory game list.

## Pipeline

```
core::pipeline::Pipeline (groups: all / parse / steam / backup / patch)
 0. migrations::run_all      # Idempotent data migrations
 1. scanner::scan            # Parse .cartridge/game.json into Vec<Game>
 2. detector::detect         # Fill missing targets (exe detection), restore lost exec bits
    sizer::measure           #   + size on disk into game.json (only when unknown: kept until reinstalled)
 3. sgdb::enrich             # SteamGridDB names/IDs/artwork (if API key)
 4. persister::persist       # Write game.json + download images, always in each game's .cartridge/
 5. steam::clean             # Remove this cartridge's shortcuts for games no longer in Steam
 6. steam::export            # Insert/update shortcuts + Proton compat
    steam_self::register     #   + •cartridge itself (STEAM_ADD_SELF)
 7. manifest::write          # manifests.json (MANIFEST_EXPORT)
 8. mods::run                # Keep the mods the user turned on applied (system/mods.json)
 9. saver::run               # Backup/restore saves, symlink tree
10. commands::run            # RUN_AFTER_* shell commands
```

Migrations: `launch_manifest.json` → `.cartridge/game.json`, nested → flat `savePaths`, arch `x86_64` → `x64`. Add new ones to `migrations::run_all`; keep them idempotent.

## Architecture

```
src/
  main.rs, cli.rs        # Args: UI (default), -t/--terminal, -g/--group, -n/--dry-run, --pack/--unpack, --serve, --peers, --download/--install
  terminal.rs            # Terminal mode + Steam dry-run preview
  core/                  # Backend, no UI code
    app.rs               # App identity, fixed folder names, cartridge id, system/ lookup
    config.rs            # config.txt parse / relative path resolution / in-place updates
    models.rs            # Game, Target, SavePath, Images; game.json (de)serialization
    paths.rs, platform.rs# normpath/expandvars/relpath; OS/arch tags, ELF/PE/Mach-O headers
    migrations.rs scanner.rs detector.rs sgdb.rs persister.rs sizer.rs
    package.rs           # Foo.cartridge.tar: pack/unpack with progress + crc verification
    share/               # LAN sharing: server.rs (HTTP, GET only), discovery.rs (UDP broadcast), client.rs
    vdf.rs               # Binary (shortcuts.vdf) + text (config.vdf) KeyValues
    steam/               # users, selection (steam.json), host (detect/restart), shortcuts (vdf), clean, export, compat (Proton builds)
    steam_self.rs manifest.rs bps.rs saver.rs commands.rs
    proton.rs            # Windows games with Steam's Proton + runtime, in the shortcut's compatdata prefix; winetricks
    patcher.rs mods.rs   # mod file engine (state/apply/restore) + mod model (list, on/off)
    pipeline.rs init.rs logging.rs
  ui/                    # macroquad UI
    app.rs               # Shell: tabs, footer hints, game session lock, log sink
    library.rs editor.rs status.rs settings.rs setup.rs
    form.rs list.rs keyboard.rs browser.rs dialog.rs   # widgets
    network.rs jobs.rs info.rs   # Network tab, background transfers, shared info panel
    mods.rs              # MODS tab: mods with status, A turns one on/off
    style.rs             # Shared look: backdrop, cards, highlight, panels, bars, toasts, chips, badges
    covers.rs launcher.rs music.rs
    engine/              # canvas (design px, drawn at native res), input, text, shape, theme
assets/                  # Embedded: fonts (VT323, Barlow), icon, steam-art, config-default.txt, games_locations.json
testdata/                # Golden fixtures produced by the former Python implementation
```

## Data Structure

```
CARTRIDGE/                                                            # Config::cartridge_dir()
  system/config.txt, README.txt, steamgriddb_cache.json, ui-thumbs/   # workspace: Config::system_dir()
  cartridge (binary / .AppImage / .app)                               # portable: next to system/
  games/MyGame/.cartridge/game.json, cover.png, icon.png, hero.png, logo.png
  games/MyGame/.cartridge/unpacked_from.json   # id of the package it came from
  games/Other.cartridge.tar                    # packaged game (not unpacked yet)
```

Folder names are fixed, whatever the binary is called: per-game metadata is always `.cartridge/` (part of the game and package format), the workspace is always `system/`. `system/` only counts with a `config.txt` or our README (the name is common). A `.cartridge/` folder next to the app is not a workspace (`core::app::workspace_in`).

Packages: a plain tar of `Foo/` with `.cartridge/package.json` first and `.cartridge/checksums.json` last. The scanner merges `Foo.cartridge.tar` and `Foo/` into one game (`models::Install`). Packaged-only games are read-only: phases that write (detector, sgdb, persister, editor) skip them. Staging uses hidden `.Foo…part` names, which the scanner ignores.

## Key Conventions

- **Compatibility is the contract:** game.json (4-space JSON), shortcuts.vdf, AppIDs (`crc32(exe+title) | 0x80000000`, computed once and kept in game.json `steam_appid` so a rename keeps the shortcut, Proton prefix and saves: always go through `steam::appid_for`/`appid_of`), path normalization and save layout must stay byte-compatible with existing cartridges. Golden tests in `testdata/` guard this.
- **Fixed layout, no path settings:** `games/`, `mods/`, `saves-copies/`, `saves-links/` sit next to `system/` (`core::app::FOLDERS`, `Config::games_dir()`/`mods_dir()`/...). Users who want a folder elsewhere replace it with a symlink. `RUN_AFTER_*` commands still get `${FREEGAMES_PATH}`, `${PATCHES_PATH}`, `${SAVESCOPY_PATH}`, `${SAVESLINK_PATH}` as read-only folder variables.
- **`Config`** keeps raw values (Settings editor) and resolved values (only `BACKUP_*` sources are paths, resolved against the cartridge folder).
- **UI look:** layout is in design pixels (432 px tall canvas), rendered at the window's native resolution, so 480p and 4K share one layout. Screens draw through `ui::style` and `theme::*()` colors, never raw colors; `THEME` (Settings > DISPLAY) swaps palette, typeface and corners, never positions. Add a theme to `engine::theme::THEMES`. `WINDOW_MODE`: borderless (fullscreen, default) or a resizable window; gamescope is always fullscreen.
- **Windows games:** the UI runs them itself with Steam's Proton (`core::proton`), the way Steam runs the shortcut (runtime `_v2-entry-point` → `proton waitforexitandrun`, `STEAM_COMPAT_DATA_PATH=compatdata/<shortcut appid>`), so prefix and saves are shared with the Steam shortcut. Per-game settings: `"proton": {version, winetricks, env}` in game.json. Build: game.json `version` → the shortcut's own `CompatToolMapping` entry → Steam's default (`0`) → newest Proton (`proton::Installed::tool_for`), so Steam and cartridge share the prefix's build. A non-empty `env` (plain names only: Steam runs LaunchOptions in a shell) owns the shortcut's LaunchOptions; otherwise options set in Steam are kept. `version` pins the shortcut's compat tool only when installed. winetricks downloads: `W_CACHE` is `system/winetricks` with `PROTON_KEEP_DOWNLOADS`, else the user cache with links to what `system/winetricks` holds.
- Flags: `is_true` for opt-in keys (STEAM_ADD_SELF, SHARE_ENABLED, ...), `is_not_false` for opt-out keys (MANIFEST_EXPORT, MUSIC_ENABLED).
- **Steam, per game:** the user adds/removes each game (Library, Y), remembered in `system/steam.json`; the pipeline keeps exactly those shortcuts current. Ownership tag `cartridge:<id>` (`system/cartridge-id`) plus the folder-name label as a visible tag; the self-shortcut (`STEAM_ADD_SELF`, first SETTINGS option) uses `<owner tag>-self` and is applied as soon as it's toggled. Steam reads shortcuts at startup: the UI says "restart Steam" when it runs.
- **Logging:** `log` macros; `core::logging` prints to stderr and mirrors to the UI Status log.
- **Minimal dependencies:** macroquad, gilrs (feature `gamepad`), rodio (feature `music`, ALSA at build time), serde/serde_json, image, ureq, crc32fast, tar, log.

## Dev Workflow

The host (Bazzite) has no Rust toolchain on purpose: build in the container.

```bash
./container.sh                         # cargo build --release (any cargo args work)
./container.sh test
./container.sh clippy --all-targets
./target/release/cartridge             # run on the host (UI)
```

Releases (`v*` tags, or run the Release workflow by hand) ship ready-to-use
cartridge archives (`package.sh`, template in `packaging/cartridge/`): a
`Cartridge/` folder with the app (`Cartridge.AppImage` via `appimage.sh`,
Apple Silicon `Cartridge.app` via `macapp.sh`, `cartridge.exe`), READMEs and an
empty `system/`. No `config.txt` is shipped: the app creates it on first
launch, so extracting a newer release over a cartridge keeps its settings.
CI extracts each archive and runs the app inside it. Bare binaries are only
published with `PUBLISH_BARE_BINARIES: true` in `release.yml`. In an AppImage
or `.app` the binary sits in a mount/bundle: `core::app::launched_file()`
returns the `.AppImage` / `.app` instead, for portable mode and the Steam
self-shortcut.

With a native toolchain instead:

```bash
cargo run                              # UI
cargo run -- -t                        # full pipeline in the terminal
cargo run -- -n                        # Steam dry-run preview
cargo test && cargo clippy --all-targets
cargo build --no-default-features      # without libudev/ALSA (no gamepad, no music)
```

Edit `system/config.txt` (seeded from `assets/config-default.txt`).

## Testing

Unit tests live next to the code (`cargo test`); filesystem tests use `core::testutil::TempDir`. For UI changes, run under Xvfb (`Xvfb :99 &`, `DISPLAY=:99 cargo run`) and drive with `xdotool` / screenshot with `import -window root`.
