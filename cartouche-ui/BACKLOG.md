# Cartouche Retro UI — Backlog

A new **game-like, retro (SNES / ZSNES-era) frontend** for Cartouche.

---

## Context

Cartouche today ships a Dear PyGui desktop GUI (`lib/gui/`) plus a 12-phase Python
pipeline (`lib/pipeline.py`). The GUI works but feels like a tool, not a console.
We want a frontend that **feels like a 90s game**: pixel font, snappy menus, full
controller play including on-screen text entry — equally at home on a Steam Deck
handheld or a desktop with mouse + keyboard.

**Decisions locked in for this backlog:**

| Decision | Choice |
|---|---|
| Backend | **Reuse the Python backend.** The new app reads/writes `.cartouche/game.json` + `config.txt` directly for browsing/editing, and shells out to `python cartouche.py` for pipeline runs. No backend rewrite. |
| Tech stack | **Rust + [macroquad](https://github.com/not-fl3/macroquad)** — tiny, fast 2D, trivial cross-compile, built-in gamepad/keyboard/mouse. |
| Rollout | **Replace** the Dear PyGui GUI once parity is reached. Both coexist until cutover. |
| Location | This repo, under **`cartouche-ui/`** (a Rust crate). |

**Outcome we want:** boots to a controller-navigable game library, runs the whole
Cartouche pipeline with live progress, and lets you edit every game/setting the old
GUI did — at a steady 60fps, 480p-native look that scales crisply to any display.

---

## Non-Functional Requirements (apply to every story)

- **Resolution:** Logical canvas renders at a **480p base** and is scaled to the
  window. Default **854×480 (16:9 widescreen)**. **Square mode** (e.g. 480×480 /
  640×640) gives *more vertical space* rather than cropping — layouts reflow, they
  don't letterbox content away.
- **Scaling:** Up-scale with **nearest-neighbor**; prefer integer scale factors,
  letterbox the remainder. Pixel font stays crisp at 720p/1080p/1440p/4K.
- **Font:** A retro **bitmap font**, no anti-aliasing. (Source a CC0/OFL pixel font.)
- **Input:** **Gamepad-first.** *Every* action must be reachable on a controller,
  including text entry via an **on-screen keyboard**. Mouse and physical keyboard
  are first-class too. One unified action layer (see ENGINE-3).
- **Feel:** **Snappy.** Menu animations are minimal and short (~100–150 ms cursor
  tween / panel slide). No action should feel like it waits on an animation.
- **Frame rate:** Cap at **60fps** by default, **configurable** in settings
  (e.g. 30 / 60 / 120 / uncapped + vsync toggle).
- **Portability:** Builds for **x64 + arm64** on **Windows, macOS, Linux**.
  Single self-contained binary with embedded assets where possible.

---

## Architecture Overview

```
cartouche-ui/  (Rust crate, macroquad)
├── engine/      virtual canvas, scaling, input layer, widget toolkit, theme, tweens
├── bridge/      data <-> backend: game.json + config.txt I/O, subprocess runner
├── screens/     status / library / editor / settings
└── assets/      pixel font, sprites, palette  (embedded)

         reads/writes directly            shells out
   ┌──────────────────────────┐   ┌────────────────────────────┐
   │ .cartouche/game.json     │   │ python cartouche.py -- batch │
   │ config.txt               │   │ ... (pipeline phases)        │
   │ artwork .png             │   │ structured progress on stdout│
   └──────────────────────────┘   └────────────────────────────┘
```

Two backend touch-points, by design:

1. **Direct file I/O (fast path):** discovering games (subdirs of `FREEGAMES_PATH`
   with `.cartouche/game.json`), reading/writing game metadata, reading/writing
   `config.txt`, loading artwork PNGs. No Python process needed — keeps editing
   instant. Mirrors `schema_version: 2` exactly (see `lib/models.py`,
   `lib/persister.py`).
2. **Subprocess (heavy path):** running pipeline phases (scan/detect/enrich/persist/
   steam/manifest/patch/save/configure/post). Invokes the existing CLI
   (`cartouche.py -- batch`, `-- test steam`) and streams progress.

> ⚠️ The current CLI only emits human log lines. **A small backend change is
> required** to give the frontend machine-readable progress and to expose
> per-game actions (auto-detect, fetch-images) without running the whole pipeline.
> Captured below as the **BACKEND** epic — these are Python changes in this repo.

---

## Milestones

- **M0 — Walking skeleton:** window opens, scales correctly (wide + square), pixel
  font renders, controller moves a cursor. (ENGINE-1..3, SETUP-1..2)
- **M1 — Library + Status (read + run):** browse games as cover art, run the
  pipeline with live progress + logs. (LIBRARY-1..2, STATUS-1..2, BRIDGE-1..3,
  BACKEND-1)
- **M2 — Editor (write):** edit metadata/targets/save-paths/images, on-screen
  keyboard, file/dir picker, per-game auto-detect + fetch-images. (EDITOR-1..6,
  ENGINE-4..6, BRIDGE-4, BACKEND-2)
- **M3 — Settings:** full config editing incl. UI/display settings. (SETTINGS-1..3)
- **M4 — Parity + cutover:** parity checklist, packaging, polish, retire `lib/gui`.
  (POLISH-1..5)

---

## Epics & Stories

Sizing: **S** ≈ <½ day · **M** ≈ 1–2 days · **L** ≈ 3–5 days. IDs are stable refs.

### SETUP — Project & build
- **SETUP-1 (S):** Create `cartouche-ui/` Cargo crate, add `macroquad`, render a
  blank window at 60fps. Add to repo `.gitignore` (`target/`).
- **SETUP-2 (M):** Cross-compile matrix in `.github/workflows` for
  win/mac/linux × x64/arm64. Produce artifacts per target.
- **SETUP-3 (S):** Asset embedding (font/sprites baked into binary via
  `include_bytes!` / macroquad asset loading). README for building & running.

### ENGINE — Retro UI toolkit (the reusable core)
- **ENGINE-1 (M):** Virtual canvas + scaling. Render to a 480p offscreen target,
  blit nearest-neighbor with integer scale + letterbox to the window.
- **ENGINE-2 (M):** **Aspect modes.** 16:9 widescreen (default) and square. Square
  *adds vertical rows*; a responsive layout/grid primitive so screens reflow rather
  than crop. Toggle live.
- **ENGINE-3 (L):** **Unified input layer.** Map gamepad (d-pad/stick/A/B/X/Y/
  L/R/Start/Select), keyboard, and mouse into a single `Action` enum
  (`Up/Down/Left/Right/Confirm/Back/Menu/PrevTab/NextTab/...`) with repeat-delay/
  -rate (match existing `lib/gui/controller.py`: ~0.35s initial, ~0.12s repeat,
  0.4 deadzone). Focus-cursor model with traversal.
- **ENGINE-4 (L):** **Widget toolkit:** label, button, list/menu, text field,
  checkbox, dropdown/combo, slider, tabs, scroll panel, modal/popup, image/
  thumbnail, progress bar. All focus- and gamepad-navigable.
- **ENGINE-5 (M):** **On-screen keyboard** for gamepad text entry (QWERTY +
  symbols + backspace/space/shift/enter), with physical-keyboard passthrough when a
  text field is focused. Reusable by any text widget.
- **ENGINE-6 (S):** **Theme system** — retro palette (reuse the existing
  brown/amber/green scheme from `lib/gui/theme.py` as the default), centralized
  colors + metrics. Pluggable for future palettes.
- **ENGINE-7 (S):** **Tween/animation** helpers (ease cursor move, panel slide,
  fade) capped short for snappiness; globally disable-able.
- **ENGINE-8 (S, optional):** Menu SFX (blip/confirm/back), toggleable in settings.

### BRIDGE — Data & backend integration
- **BRIDGE-1 (M):** `serde` models mirroring `schema_version: 2`: `Game`,
  `GameTarget` (os/arch/target/startIn/launchOptions), `GameImages` (cover/icon/
  hero/logo/header), `savePaths` (`{os, path}`), `steamgriddb_id`, `notes`.
  Round-trip read/write `.cartouche/game.json` byte-compatibly with `persister.py`.
- **BRIDGE-2 (S):** Game discovery — enumerate `FREEGAMES_PATH` subdirs that have
  `.cartouche/game.json`; build the in-memory library list. Load artwork PNGs as
  textures (lazy/threaded).
- **BRIDGE-3 (M):** **Subprocess runner.** Spawn `python3 cartouche.py … -- batch`
  (and `-- test steam`), stream stdout, parse structured progress (BACKEND-1),
  support **cancel** (kill the process tree). Surface phase status + log lines.
- **BRIDGE-4 (M):** `config.txt` parser/writer preserving comments, key order, and
  inline comments (match `load_config_map` semantics in `cartouche.py`). Handle
  `BACKUP_*`, `RUN_AFTER_*`, `${VAR}` round-trips losslessly.
- **BRIDGE-5 (S):** Locate the Python interpreter / entry point robustly
  (bundled venv vs system `python3`), with a clear error screen if missing.

### BACKEND — Required Python changes (in this repo)
- **BACKEND-1 (M):** Add a **machine-readable progress mode** to the pipeline CLI,
  e.g. `cartouche.py -- batch --progress=json` emitting one JSON event per line
  (`{"phase","status","progress","message"}`) by hooking the existing
  `PipelineRunner` phase callbacks. Keep human logging as default. Also expose
  phase **groups** (`all` / `parse` / `backup` / `steam`) as CLI args.
- **BACKEND-2 (M):** Add **per-game actions** so editor buttons don't run the whole
  pipeline: a small CLI (e.g. `-- detect <folder>`, `-- enrich <folder>`,
  `-- fetch-images <folder>`) that returns JSON, reusing `detector.py` /
  `enricher.py`. Mirrors the existing Games-view "Auto-Detect" / "Fetch Images".
- **BACKEND-3 (S):** JSON output for `test steam` preview (AppIDs, resolved exe,
  artwork status) so the frontend can show a Steam dry-run screen.

### STATUS — Home / run screen (replaces `status_view.py`)
- **STATUS-1 (M):** Home screen: app title + version, game count, last-run time,
  and action buttons **Run All / Parse / Backup / Steam Sync** + **Cancel**.
- **STATUS-2 (M):** Live run view: per-phase status list (12 phases), overall +
  current-phase progress bars, scrolling log panel (cap ~500 lines), driven by
  BRIDGE-3 events. Refresh library on completion.

### LIBRARY — Game browser (replaces left panel of `games_view.py`)
- **LIBRARY-1 (L):** **Cover-art library** — the marquee retro screen. Grid /
  carousel of game covers (use `images.cover`, fallback placeholder), title under
  the focused item, smooth cursor. Reflows in square mode (more rows). Gamepad +
  mouse + keyboard nav, fast scroll.
- **LIBRARY-2 (S):** Search / filter (text field via on-screen keyboard) and sort
  (title). Jump-to-letter for big libraries.
- **LIBRARY-3 (S):** Select a game → open EDITOR. Quick info overlay (targets count,
  has-art, save-paths count).

### EDITOR — Game detail editor (replaces right panel of `games_view.py`)
- **EDITOR-1 (M):** Metadata: editable **title**, **SteamGridDB ID**, "open SGDB
  page" action (opens browser), **notes**. Save writes `game.json` (BRIDGE-1).
- **EDITOR-2 (M):** **Targets** editor — rows of os(combo)/arch(combo)/target/
  startIn/launchOptions with add/delete; **Auto-Detect** (BACKEND-2). File picker
  for target, dir picker for startIn.
- **EDITOR-3 (M):** **Save-paths** editor — rows of os(combo)/path with add/delete/
  open; **Auto-Detect** Proton saves (BACKEND-2).
- **EDITOR-4 (M):** **Artwork** — 5 slots (icon/cover/hero/logo/header) with
  thumbnails, **Fetch Images** (BACKEND-2), per-slot delete with confirm modal
  ("entry only" vs "entry + file"), matching old behavior.
- **EDITOR-5 (M):** **File/Directory browser** widget — fully gamepad-navigable,
  filters (e.g. `.exe`/`.sh`), used by EDITOR-2/3 and SETTINGS-2.
- **EDITOR-6 (S):** Dirty-state tracking + Save/confirm-discard on back.

### SETTINGS — Config editor (replaces `settings_view.py`)
- **SETTINGS-1 (L):** Grouped settings matching the old categories — **General**
  (MACHINE_NAME, PERSIST_DATA), **Paths** (FREEGAMES_PATH, PATCHES_PATH),
  **Save/Sync** (SAVESCOPY_PATH, SAVESCOPY_STRATEGY combo, SAVESLINK_PATH),
  **Steam/Integration** (STEAM_EXPOSE, STEAM_USERID, PROTON_VERSION,
  STEAMGRIDDB_API_KEY masked + clear, NSFW/HUMOR/EPILEPSY, MANIFEST_EXPORT,
  MANIFEST_PATH), **Emulators** (Dolphin/Ryujinx/Cemu/RetroArch keys), **Custom**
  (`BACKUP_*` / `RUN_AFTER_*` key=value rows, add/remove). Field types: text /
  password / checkbox / combo / number / path(+picker). Help text per field.
  Save via BRIDGE-4.
- **SETTINGS-2 (S):** Path fields use the EDITOR-5 picker.
- **SETTINGS-3 (M):** **UI/Display settings** (new, stored in the app's own config):
  aspect mode (wide/square), window scale/resolution, **fps cap** (30/60/120/
  uncapped) + vsync, animations on/off, SFX on/off, theme. Applied live.

### POLISH — Parity, performance, cutover
- **POLISH-1 (M):** Feature-parity checklist vs `lib/gui/` (Status/Games/Settings);
  close every gap.
- **POLISH-2 (S):** Performance pass — hold 60fps, async asset/texture loading, no
  hitches when scrolling large libraries.
- **POLISH-3 (M):** Packaging per platform: Linux (AppImage + Steam Deck friendly),
  macOS (`.app`), Windows (`.exe`); bundle/locate the Python backend; update
  `release.sh`.
- **POLISH-4 (S):** First-run / init flow (mirror `init_dialog.py`) — prompt for
  `FREEGAMES_PATH` when no config exists.
- **POLISH-5 (S):** **Cutover** — make the retro UI the default launch, retire
  `lib/gui/` and drop `dearpygui` from `requirements.txt`; update `CLAUDE.md` /
  `README.md` / dev workflow.

---

## Feature-Parity Map (old GUI → new)

| Old (Dear PyGui) | New screen |
|---|---|
| `status_view.py` (run pipeline, progress, logs, cancel) | **STATUS-1/2** |
| `games_view.py` left list | **LIBRARY-1/2/3** |
| `games_view.py` right editor (title/SGDB/targets/saves/images/notes) | **EDITOR-1..6** |
| `settings_view.py` (all config keys + custom) | **SETTINGS-1/2** |
| `controller.py` (gamepad nav) | **ENGINE-3** (+ on-screen kbd ENGINE-5) |
| `theme.py` (palette) | **ENGINE-6** |
| file/dir dialogs | **EDITOR-5** |
| (none) display/fps settings | **SETTINGS-3** |

---

## Verification

Per milestone, validate end-to-end against a real `config.txt` + `FREEGAMES_PATH`:

- **M0:** `cargo run` opens a window; toggle wide/square; controller moves the
  cursor; pixel font is crisp at multiple window sizes; steady 60fps.
- **M1:** Library shows real covers from `.cartouche/`; "Run All" streams the same
  phases/logs as `python3 cartouche.py -- batch`; Cancel stops the run; library
  refreshes after.
- **M2:** Edit a game → reopen confirms persisted `game.json`; diff vs the Python
  `persister.py` output is byte-identical for the same data; on-screen keyboard
  enters text on controller; Auto-Detect / Fetch Images mutate the game correctly.
- **M3:** Change a setting → `config.txt` updated with comments/order preserved
  (diff only the intended line); display settings (aspect/fps/vsync) apply live.
- **M4:** Parity checklist green; packaged binary runs on a clean machine per OS;
  Steam Deck launches it in Game Mode with controller only; `dearpygui` removed and
  Python test suite still passes (`pytest`).

> Throughout, cross-check the existing Python tests (~205) remain green for any
> BACKEND-* changes: `python3 -m pytest`.
