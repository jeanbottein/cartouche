# Data Migrations

Cartouche uses a migration coordinator (`lib/migrations.py`) to run all data migrations in sequence. Each migration is idempotent — safe to run every time.

## How Migrations Work

Migrations run as **Step 0** of the pipeline (`_phase_migrate`), before scanning. The coordinator:
1. Iterates through registered migrations in order
2. Calls each migration function with the games directory
3. Logs success/failure and count of items migrated
4. Continues even if one migration fails (logged as `-1`)

## Current Migrations

### 1. launch_manifest → game.json (`migrator.migrate()`)

**Purpose:** One-time migration from old `launch_manifest.json` format to `.cartouche/game.json`.

**When it runs:** When `.cartouche/game.json` doesn't exist but `launch_manifest.json` does.

**What it does:**
- Reads `launch_manifest.json` from game root
- Converts to `Game` model
- Writes `.cartouche/game.json`
- Deletes the old `launch_manifest.json`

### 2. Flatten save paths (`save_paths_migrator.migrate_all_games()`)

**Purpose:** Convert nested save paths format to flat format.

**Old format:**
```json
{
  "savePaths": [
    {
      "name": "saves",
      "paths": [
        {"os": "linux", "path": "~/.local/share/Game/saves"}
      ]
    }
  ]
}
```

**New format:**
```json
{
  "savePaths": [
    {"os": "linux", "path": "~/.local/share/Game/saves"}
  ]
}
```

**When it runs:** When `game.json` has the old nested format.

**What it does:**
- Scans all game folders for `.cartouche/game.json`
- Checks if `savePaths` uses old nested format
- Flattens `[{name, paths: []}]` → `[{os, path}, ...]`
- Rewrites the file
- Handles games already in new format (skips them)

---

## Adding a New Migration

### Step 1: Create a migration module

File: `lib/my_migration.py`

```python
"""
Description of what this migration does.
"""

import logging
from pathlib import Path
from lib.models import CARTOUCHE_DIR, GAME_JSON

logger = logging.getLogger(__name__)

def migrate_all_games(games_dir: str) -> int:
    """
    Migrate all games in the directory.
    Returns the count of games that were migrated.
    """
    if not games_dir or not os.path.isdir(games_dir):
        return 0

    count = 0
    for item in os.listdir(games_dir):
        game_path = Path(games_dir) / item
        if not game_path.is_dir() or item.startswith("."):
            continue

        if _migrate_single_game(game_path):
            count += 1

    return count


def _migrate_single_game(game_dir: Path) -> bool:
    """Migrate a single game. Return True if migrated, False if skipped or error."""
    game_json_path = game_dir / CARTOUCHE_DIR / GAME_JSON
    if not game_json_path.exists():
        return False

    try:
        with open(game_json_path, "r") as fh:
            data = json.load(fh)
    except Exception as exc:
        logger.error(f"Failed to read {game_json_path}: {exc}")
        return False

    # Check if migration is needed
    if _already_migrated(data):
        return False

    # Apply migration
    try:
        _apply_migration(data)
        with open(game_json_path, "w") as fh:
            json.dump(data, fh, indent=4)
        return True
    except Exception as exc:
        logger.error(f"Migration failed for {game_json_path}: {exc}")
        return False


def _already_migrated(data: dict) -> bool:
    """Check if this game has already been migrated."""
    # Return True if data is already in new format
    pass


def _apply_migration(data: dict) -> None:
    """Mutate data dict in-place to apply the migration."""
    pass
```

### Step 2: Register in `migrations.py`

Edit `lib/migrations.py` and add your migration to the `MIGRATIONS` list:

```python
from . import my_migration

MIGRATIONS: list[tuple[str, Callable[[str], int | None]]] = [
    ("launch_manifest", migrator.migrate),
    ("save_paths_format", save_paths_migrator.migrate_all_games),
    ("my_migration", my_migration.migrate_all_games),  # ← Add here
]
```

### Step 3: Test

```bash
python3 cartouche.py
# Watch logs: "Migration 'my_migration': N item(s)"
```

---

## Migration Best Practices

1. **Idempotent:** Safe to run multiple times. Check `_already_migrated()` before applying.
2. **Reversible:** Include a comment about how to reverse if needed.
3. **Logged:** Log start, success, and errors. Use `logger.info()` / `logger.error()`.
4. **Isolated:** Each migration is independent. No cross-migration dependencies.
5. **Fast:** Process as efficiently as possible (avoid redundant I/O).
6. **Documented:** Add a docstring explaining what changed and why.

---

## Seeing Migrations in Action

**CLI:**
```bash
python3 cartouche.py
# Output: "--- MIGRATION ---"
# Then: "Migration 'launch_manifest': 0 item(s)"
#       "Migration 'save_paths_format': 2 item(s)"
```

**GUI:**
Status view shows "Migration" as the first phase. Progress updates as each migration runs.

---

## Troubleshooting

**Migration returned -1 (failed):**
- Check logs for the specific error
- Verify the migration function handles all edge cases
- Ensure files are readable/writable

**Game data corrupted:**
- Git history is your friend: `git checkout lib/migrations.py` to revert
- Always back up user game folders before major migrations

**A migration should be idempotent but runs every time:**
- Check `_already_migrated()` logic — it may always return False
- Verify the condition that determines if a game needs migration
