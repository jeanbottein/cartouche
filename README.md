# Cartouche

A comprehensive tool for managing DRM-free games, emulator configurations, and game patches on Linux gaming systems like Steam Deck.

> **Note**: This project was developed using vibe coding techniques with extensive curation work. While primarily tested on Linux (Steam Deck), Windows compatibility is included but untested - it may work but hasn't been verified.

## Overview

Cartouche (French for "cartridge") uses a pipeline architecture with an in-memory game database:

1. **Scanner** - Discovers games and loads existing `.cartouche/game.json` metadata
2. **Detector** - Automatically detects executables for games missing metadata
3. **Enricher** - Fetches official names and artwork from SteamGridDB
4. **Persister** - Saves game metadata and artwork to `.cartouche/` folders
5. **Steam Exporter** - Syncs games as non-Steam shortcuts in your Steam library
6. **Manifest Writer** - Generates `manifests.json` for ROM manager compatibility
7. **Patcher** - Applies patches and file replacements to games
8. **Saver** - Manages save backups with multiple save paths per game
9. **Configurer** - Automatically configures emulator settings

## Installation & Usage

1. Clone or download this repository
2. Edit `config.txt` with your paths and preferences
3. Run the main script:
   ```bash
   # Linux/macOS
   ./cartouche.sh
   # or
   python3 cartouche.py

   # Windows
   cartouche.bat
   # or
   python cartouche.py
   ```
4. Dry-run Steam sync:
   ```bash
   python3 cartouche.py test steam
   ```

## Per-Game Data Structure

Each game folder gets a `.cartouche/` subfolder containing all metadata and artwork:

```
FREEGAMES_PATH/
  MyGame/
    .cartouche/
      game.json    # Game metadata (title, targets, save paths, SGDB info)
      cover.png    # Grid/poster artwork
      icon.png     # Icon
      hero.png     # Hero banner
      logo.png     # Logo overlay
    game-executable
```

### game.json Schema

```json
{
    "title": "Official Game Name",
    "steamgriddb_id": 13136,
    "targets": [
        {
            "os": "linux",
            "arch": "x86_64",
            "target": "game.bin",
            "startIn": ".",
            "launchOptions": ""
        }
    ],
    "savePaths": [
        {
            "name": "saves",
            "paths": [
                { "os": "linux", "path": "~/.local/share/MyGame/Saves" }
            ]
        },
        {
            "name": "config",
            "paths": [
                { "os": "linux", "path": "~/.config/MyGame" }
            ]
        }
    ],
    "images": {
        "cover": "cover.png",
        "icon": "icon.png",
        "hero": "hero.png",
        "logo": "logo.png"
    }
}
```

## Configuration File (config.txt)

```ini
# Paths
FREEGAMES_PATH=/run/media/deck/SteamDeck-SD/linux-games
PATCHES_PATH=/run/media/deck/SteamDeck-SD/mods
SAVESCOPY_PATH=/run/media/deck/SteamDeck-SD/cartouche-backup
SAVESCOPY_STRATEGY=backup  # backup (default), sync (alias for backup), or restore (dangerous)
SAVESLINK_PATH=/run/media/deck/SteamDeck-SD/cartouche-sync  # optional, for Syncthing

# Steam integration
STEAM_EXPOSE=True
STEAMGRIDDB_API_KEY=your_api_key_here

# Persistence control
PERSIST_DATA=True          # Set to False to skip writing .cartouche/ folders
MANIFEST_EXPORT=True       # Set to False to skip writing manifests.json
MANIFEST_PATH=             # Custom path for manifests.json (default: FREEGAMES_PATH/manifests.json)

# Custom directory backups
BACKUP_gamescope-shaders=/home/deck/.local/share/gamescope/reshade/Shaders

# Emulator settings
DOLPHIN_GC_LANGUAGE=2
RYUJINX_LANGUAGE_CODE=fr_FR
CEMU_CONSOLE_LANGUAGE=2
RETROARCH_USER_LANGUAGE=2
```

## Modules

### Configurer

Automatically modifies emulator configuration files (Dolphin, Ryujinx, Cemu, RetroArch) based on preferences in `config.txt`. Supports text-based and binary modifications with `${VARIABLE}` substitution.

### Patcher

Applies file patches and replacements using `patch.json` configuration files placed in `PATCHES_PATH`.

```json
[
    { "file": "patch.bps", "target": "Game/data.win", "method": "patch", "target_crc32": "D3D27C56", "patched_crc32": "1655BF6C" },
    { "file": "replacement.ogg", "target": "Game/music.ogg", "method": "replace" }
]
```

### Saver

Manages per-game save backups with support for **multiple save paths per game**. Each game can declare separate save directories (e.g., saves, config, screenshots) that are backed up independently.

**Strategies:**
- `backup` (default) - One-way mirror from original to backup
- `sync` - Alias for backup (use `SAVESLINK_PATH` + Syncthing for bidirectional)
- `restore` - One-way restore from backup to original (dangerous)

**Symlink Tree** (`SAVESLINK_PATH`): Creates symlinks to original save directories for use with Syncthing.

**Custom Directory Backups**: `BACKUP_<name>=<path>` entries in config.

### Migration from gamer-sidekick

Cartouche automatically migrates old `launch_manifest.json` files to the new `.cartouche/game.json` format on first run. Existing Steam shortcuts tagged with `"gamer-sidekick"` are recognized and managed.

## Directory Structure

```
cartouche/
  cartouche.py               # Main script
  cartouche.sh               # Shell wrapper (Linux/macOS)
  cartouche.bat              # Batch script (Windows)
  cartouche.ps1              # PowerShell script (Windows)
  config.txt                 # Configuration file (gitignored)
  config-default.txt         # Template
  lib/
    models.py                # Data models (Game, GameDatabase, etc.)
    scanner.py               # Game discovery
    detector.py              # Executable detection
    enricher.py              # SteamGridDB integration
    persister.py             # .cartouche/ writer
    steam_vdf.py             # Binary VDF reader/writer
    steam_cleaner.py         # Stale shortcut removal
    steam_exporter.py        # Steam shortcut creation
    manifest_writer.py       # manifests.json generator
    migrator.py              # Old format migration
    patcher.py               # Game patching
    saver.py                 # Save backup/restore
    configurer.py            # Emulator config mutations
    configurer.json          # Emulator config rules
    games_locations.json     # Game directory search paths
  scripts/
    backup.sh
```

## Requirements

- Python 3.6+ (no external packages - stdlib only)
- Standard Linux utilities (find, file, etc.)
- For patching: **flips** binary (`bin/flips` or system PATH)

## Platform Support

- **Linux**: Fully tested (Steam Deck, desktop Linux)
- **macOS**: Development only
- **Windows**: Included but untested

## Steam ROM Manager Integration

1. Install [Steam ROM Manager](https://github.com/SteamGridDB/steam-rom-manager)
2. Run cartouche to generate manifests
3. Configure a parser to use the generated `manifests.json`
4. Parse and add games to Steam

## Credits

- **Icon**: <a href="https://www.flaticon.com/free-icons/game-cartridge" title="game cartridge icons">Game cartridge icons created by Freepik - Flaticon</a>
