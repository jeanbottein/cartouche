# Cartouche Sub-Agents

Specialized agents for different areas of the codebase. Each agent has focused context and common debugging patterns.

## Agents

- **pipeline-debugger** — Trace/fix any of the 12 pipeline phases, migrations system
- **steam-integration** — Shortcuts, artwork sync, Proton, AppID issues
- **save-manager** — Backup/restore/symlink save file operations
- **emulator-config** — `configurer.json` rules, variable substitution
- **metadata-enricher** — SteamGridDB API, artwork fetching, cache
- **gui-developer** — Dear PyGui views, MVC controller, theme, games editor

## Key Documentation

For all agents to reference:
- **CLAUDE.md** — Main architecture overview, pipeline phases
- **MIGRATIONS.md** — Data migration system (how to add migrations)
- **MIGRATIONS.REFERENCE.md** — Quick reference for migrations

## Shared Knowledge

### Migration System

The pipeline has a **migration phase** (step 0) that runs idempotent data migrations. All migrations are registered in `lib/migrations.py`:

```python
MIGRATIONS = [
    ("launch_manifest", migrator.migrate),
    ("save_paths_format", save_paths_migrator.migrate_all_games),
]
```

**To add a new migration:**
1. Create `lib/my_migration.py` with `migrate_all_games(games_dir: str) -> int`
2. Add to `MIGRATIONS` list
3. Return count of items migrated
4. It will run automatically with the pipeline

See `MIGRATIONS.md` for detailed guide and examples.

### Games View (GUI)

The games view now has **inline editing** of game metadata:
- Title, SteamGridDB ID
- Targets (OS, arch, exe, start dir, launch options)
- Save paths (OS, path)
- Add/remove rows with `+ Add` buttons
- Delete rows with red X buttons
- Browse for executables and directories with `...` buttons
- Automatic save to `.cartouche/game.json`

File: `lib/gui/games_view.py`

### Pipeline

12 phases run in order:
1. **migrate** — Data migrations (see above)
2. **scan** — Parse games into database
3. **detect** — Find missing executables
4. **enrich** — Fetch SteamGridDB data
5. **persist** — Write JSON and download images
6. **steam_clean** — Remove stale Steam shortcuts
7. **steam_export** — Add/update Steam shortcuts
8. **manifest** — Export ROM manager manifests
9. **patch** — Apply patches
10. **save** — Backup/restore saves
11. **configure** — Mutate emulator configs
12. **post_commands** — Run user shell commands

See `CLAUDE.md` for details.
