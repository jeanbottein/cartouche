# •cartridge Sub-Agents

Specialized agents for different areas of the codebase.

- **pipeline-debugger** — Trace/fix pipeline phases and migrations (`src/core/pipeline.rs` and phase modules)
- **steam-integration** — Shortcuts, artwork sync, Proton, AppIDs (`src/core/steam*.rs`, `vdf.rs`)
- **save-manager** — Save backup/restore/symlinks (`src/core/saver.rs`)
- **metadata-enricher** — SteamGridDB, artwork, cache (`src/core/sgdb.rs`, `persister.rs`)
- **ui-developer** — Themed macroquad UI: screens, widgets, input (`src/ui/`)

See `CLAUDE.md` for the architecture overview and conventions.
