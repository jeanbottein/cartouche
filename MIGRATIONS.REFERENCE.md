# Quick Reference: Migrations

**File:** `lib/migrations.py` — Central coordinator

**How to add a migration:**
1. Create `lib/my_migration.py` with `migrate_all_games(games_dir: str) -> int`
2. Add to `MIGRATIONS` list in `lib/migrations.py`
3. Returns count of items migrated (or -1 on failure)
4. Runs automatically as phase 0 of the pipeline

**Current migrations:**
- `migrator.migrate()` — `launch_manifest.json` → `.cartouche/game.json`
- `save_paths_migrator.migrate_all_games()` — Flatten nested save paths format

**When migrations run:** Every pipeline run, automatically. They're idempotent (safe to run multiple times).

**For details:** See `MIGRATIONS.md`
