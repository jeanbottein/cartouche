# AGENTS.md – AI Agent Guide for Cartouche

## Project Overview

Cartouche (French for "cartridge") is a Python CLI tool for managing DRM-free games on Linux (primarily Steam Deck). It uses a pipeline architecture with an in-memory GameDatabase as the central data carrier.

## Pipeline

```
cartouche.py main():
 0. migrator.migrate()         # One-time: launch_manifest.json -> .cartouche/game.json
 1. db = scanner.scan()        # Parse .cartouche/game.json into GameDatabase
 2. detector.detect(db)        # Fill missing targets (exe detection)
 3. enricher.enrich(db, cfg)   # Fetch SteamGridDB data (if API key)
 4. persister.persist(db)      # Write .cartouche/game.json + download images
 5. steam_cleaner.clean()      # Remove stale Steam shortcuts
 6. steam_exporter.export()    # Insert/update Steam shortcuts
 7. manifest_writer.write()    # Create manifests.json (ROM manager compat)
 8. patcher.run(cfg)           # Apply patches
 9. saver.run(db, cfg)         # Backup/restore saves
10. configurer.run(cfg)        # Emulator config mutations
11. run_post_commands(cfg)     # RUN_AFTER_* commands
```

## Architecture

```
cartouche.py               # Entry point: loads config, runs pipeline
lib/
  models.py                # Game, GameTarget, GameImages, GameDatabase
  scanner.py               # Step 1: parse .cartouche/ folders
  detector.py              # Step 2: exe detection
  enricher.py              # Step 3: SteamGridDB API
  persister.py             # Step 4: write .cartouche/ + download images
  steam_vdf.py             # Binary VDF reader/writer
  steam_cleaner.py         # Step 5: remove stale shortcuts
  steam_exporter.py        # Step 6: create/update shortcuts
  steam_compat.py          # Step 6b: set Proton compat for Windows games
  manifest_writer.py       # Step 7: write manifests.json
  migrator.py              # Step 0: migrate old format
  patcher.py               # Step 8: BPS patching + file replacement
  saver.py                 # Step 9: save backup/restore/symlink
  configurer.py            # Step 10: emulator config mutations
  configurer.json          # Declarative emulator config rules
  games_locations.json     # Known game directory search paths
scripts/
  backup.sh
config.txt                 # User configuration (gitignored)
config-default.txt         # Template copied to config.txt if missing
```

## Data Structure

Each game folder gets a `.cartouche/` subfolder:
```
FREEGAMES_PATH/
  MyGame/
    .cartouche/
      game.json    # Game metadata
      cover.png    # Artwork images
      icon.png
      hero.png
      logo.png
    game-executable
```

## Key Conventions

- **No external dependencies.** Only Python standard library.
- **`cfg` dict** parsed from `config.txt` is the runtime config for all modules.
- **GameDatabase** is the central in-memory data carrier (steps 1-7, 9).
- **`.cartouche/game.json`** replaces the old `launch_manifest.json`.
- **Multiple save paths per game** supported (named: "saves", "config", etc.).
- **Declarative emulator rules** in `configurer.json`, not in Python code.
- **Variable substitution** uses `${VARIABLE_NAME}` syntax.
- **`BACKUP_<name>`** config keys define custom directory backups.
- **`RUN_AFTER_<label>`** config keys define post-run shell commands.
- **Ownership tag** in Steam shortcuts is `"cartouche"` (also recognizes legacy `"gamer-sidekick"`).

## Dev Workflow

```bash
python3 cartouche.py            # Run full pipeline
python3 cartouche.py test steam # Dry-run Steam sync
./cartouche.sh                  # Shell wrapper
```

Edit `config.txt` (not `config-default.txt`) for local overrides. `config.txt` is gitignored.

## Testing

No automated test suite. Verification is done manually:
1. Set up `config.txt` with known paths.
2. Run `python3 cartouche.py` and inspect log output.
3. Verify `.cartouche/game.json` created in game folders.
4. Verify backups, manifests, Steam shortcuts as expected.
