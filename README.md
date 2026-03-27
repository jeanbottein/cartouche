# Cartouche

> *French for "cartridge" — because nothing beats the nostalgia of blowing into a plastic slot and praying your game boots.*

A no-nonsense tool for managing DRM-free games, emulator configs, patches, and save backups on Linux (and especially Steam Deck).

---

Don't like the name? **Change it.** Renaming things is literally what Cartouche does best — it'll happily override any game title with whatever you put in `game.json`. Fork it, rename it "GamingBlob", the tool won't judge you. It already survived being called *gamer-sidekick*.

---

## Features

- **Scan** — discovers game folders and loads existing metadata
- **Detect** — auto-detects executables when metadata is missing
- **Enrich** — fetches official names and artwork from [SteamGridDB](https://www.steamgriddb.com/)
- **Steam sync** — adds games to Steam as non-Steam shortcuts, complete with artwork
- **Patch** — applies BPS patches or file replacements via `patch.json`
- **Save backup** — backs up (or restores) save files, with multiple save paths per game
- **Emulator config** — auto-configures Dolphin, Ryujinx, Cemu, and RetroArch from `config.txt`
- **Manifest export** — generates `manifests.json` for [Steam ROM Manager](https://github.com/SteamGridDB/steam-rom-manager)

Metadata and artwork are stored in a `.cartouche/` subfolder inside each game's directory — no central database, no magic, just files.

## Getting started

1. Clone this repo
2. Copy `config-default.txt` to `config.txt` and fill in your paths
3. Run:
   ```bash
   ./cartouche.sh          # Linux / macOS
   python3 cartouche.py    # anywhere

   cartouche.bat           # Windows (untested, godspeed)
   ```
4. Dry-run Steam sync before committing:
   ```bash
   python3 cartouche.py test steam
   ```

## Requirements

- Python 3.6+ (stdlib only — no `pip install` needed)
- Standard Linux utilities (`find`, `file`)
- For BPS patching: `flips` binary in `bin/` or system PATH
- For SteamGridDB enrichment: a free [API key](https://www.steamgriddb.com/profile/preferences/api)

## Platform support

| Platform | Status |
|---|---|
| Linux (Steam Deck, desktop) | Fully tested |
| macOS | Works for development |
| Windows | Included but untested |

## Credits

- **Icon**: <a href="https://www.flaticon.com/free-icons/game-cartridge" title="game cartridge icons">Game cartridge icons created by Freepik - Flaticon</a>
- **vdf** (Python library): [ValvePython/vdf](https://github.com/ValvePython/vdf) by Rossen Georgiev — MIT License. Used for reading/writing Steam's text-format VDF configuration files.
- **Binary VDF format reference**: [GameSync](https://github.com/Maikeru86/GameSync) by Maikeru86 — MIT License. The binary `shortcuts.vdf` reader/writer (`lib/steam_vdf.py`) was adapted from this project.
