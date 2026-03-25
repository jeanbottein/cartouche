# AGENTS.md – AI Agent Guide for Gamer Sidekick

## Project Overview

Gamer Sidekick is a Python CLI tool for managing DRM-free games on Linux (primarily Steam Deck). It has four independent modules, each invoked sequentially by `gamer-sidekick.py`:

| Module | File | Responsibility |
|---|---|---|
| Manifester | `lib/manifester.py` | Scans game directories, generates `launch_manifest.json` and `manifests.json` |
| Saver | `lib/saver.py` | Backs up / restores / symlinks game save directories |
| Patcher | `lib/patcher.py` | Applies file replacements or BPS patches to game files |
| Steamer | `lib/steamer.py` | Syncs discovered games as non-Steam shortcuts in Steam (binary VDF) |
| Configurer | `lib/configurer.py` | Patches emulator config files (text & binary) based on `config.txt` |

All modules receive a single `cfg: Dict[str, str]` parsed from `config.txt`.

## Architecture

```
gamer-sidekick.py        # Entry point: loads config, calls each module's run(cfg)
lib/
  __init__.py
  manifester.py          # Manifest generation
  steamer.py             # Steam non-Steam shortcut sync
  saver.py               # Save backup / restore / symlink
  patcher.py             # BPS patching + file replacement
  configurer.py          # Emulator config mutations
  configurer.json        # Declarative emulator config rule definitions
  games_locations.json   # Known game save path locations
scripts/
  install_bios.sh
  move_rom_zips.sh
  backup.sh
config.txt               # User configuration (gitignored)
config-default.txt       # Template copied to config.txt if missing
```

## Key Conventions

- **No external dependencies.** Only Python standard library. Do not add third-party packages.
- **`cfg` dict is the sole runtime input.** All modules read settings exclusively from the `cfg: Dict[str, str]` passed to their `run(cfg)` entry point.
- **Declarative emulator rules live in `configurer.json`**, not in Python code. Prefer adding new emulator rules there instead of hardcoding logic.
- **`games_locations.json`** maps game titles to their save paths. Expand it for new games rather than hardcoding paths.
- **Variable substitution** uses `${VARIABLE_NAME}` syntax in config values and in `configurer.json` patterns/values.
- **Config keys prefixed `BACKUP_`** define custom directory backups (e.g., `BACKUP_shaders=/path`).
- **Config keys prefixed `RUN_AFTER_`** define post-run shell commands (sorted alphabetically by key).
- **Strategy values**: `backup` (safe), `sync` (alias for backup), `restore` (dangerous one-way restore).

## Dev Workflow

- Run the tool locally:
  ```bash
  python3 gamer-sidekick.py
  # or
  ./gamer-sidekick.sh
  ```
- Edit `config.txt` (not `config-default.txt`) for local overrides. `config.txt` is gitignored.
- For binary patching, the `flips` binary must be at `bin/flips` or on `$PATH`.

## Adding New Features

### New emulator support (Configurer)
Add entries to `lib/configurer.json`. Each entry supports:
- `type`: `"text"` (regex replacement) or `"hexadecimal"` (binary search-and-replace)
- `pattern`: regex (text) or hex string (binary), supporting `${VAR}` substitution
- `value`: replacement string, supporting `${VAR}` substitution
- `paths`: list of file paths to try (first match wins); supports `~` and env vars

### New game save path (Saver)
Add an entry to `lib/games_locations.json` mapping game title → save path.

### New patch (Patcher)
Add a `patch.json` inside the game's directory under `PATCHES_PATH`. Fields: `file`, `target`, `method` (`replace` or `patch`), and optional `target_crc32` / `patched_crc32`.

### New post-run automation
Add `RUN_AFTER_<label>=<shell command>` to `config.txt`. Commands are sorted by label and run after all modules complete. `${VAR}` substitution from `cfg` is supported.

## Important Paths (Runtime)

| Config Key | Purpose |
|---|---|
| `FREEGAMES_PATH` | Root directory scanned by the Manifester |
| `PATCHES_PATH` | Root directory containing per-game `patch.json` files |
| `SAVESCOPY_PATH` | Root for save backups |
| `SAVESCOPY_STRATEGY` | `backup` / `sync` / `restore` |
| `SAVESLINK_PATH` | Optional: root for symlink tree (for Syncthing) |
| `BACKUP_<name>` | Custom directory to include in saver operations |

## Testing

There is no automated test suite. Verification is done manually:
1. Set up a representative `config.txt` with known paths.
2. Run `python3 gamer-sidekick.py` and inspect log output.
3. Verify file system outcomes (backups created, configs mutated, manifests generated).
4. For patching, confirm CRC32 values match expected values in `patch.json`.
