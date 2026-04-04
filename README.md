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

### Save Path Variables

When configuring save paths in `game.json`, you generally only need to provide the Windows path (e.g. `%APPDATA%`, `%USERPROFILE%`, or `C:\...`). If the game is run on Linux through Proton, Cartouche will automatically translate common Windows paths to their corresponding Proton prefix equivalents in `~/.local/share/Steam/steamapps/compatdata/<appid>/pfx/drive_c/`.

For example, this `game.json` entry works for both Windows native and Linux Proton seamlessly:
```json
{
  "os": "windows",
  "path": "%USERPROFILE%/AppData/LocalLow/by Sam Eng/SKATE STORY"
}
```

If you do need to write an explicit `linux` path overriding Proton, you can use the built-in variables:
- **`${steamappid}`**: Replaced with the unique AppID Steam assigns to the non-Steam shortcut.
- **`${proton_c}`**: A shorthand for `~/.local/share/Steam/steamapps/compatdata/${steamappid}/pfx/drive_c`

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

- Python 3.10+
- Dependencies: `pip install -r requirements.txt`

## Platform support

| Platform | Status |
|---|---|
| Linux (Steam Deck, desktop) | Fully tested |
| macOS | Works for development |
| Windows | Included but untested |

## Credits

- **Icon**: <a href="https://www.flaticon.com/free-icons/game-cartridge" title="game cartridge icons">Game cartridge icons created by Freepik - Flaticon</a>
- **vdf**: [ValvePython/vdf](https://github.com/ValvePython/vdf) by Rossen Georgiev — MIT License. Used for reading/writing Steam's VDF configuration and shortcut files.
- **requests**: [psf/requests](https://github.com/psf/requests) by Kenneth Reitz — Apache-2.0 License. HTTP client for SteamGridDB API and artwork downloads.
- **python-bps-continued**: [Screwtape/python-bps](https://gitlab.com/Screwtapello/python-bps) — ISC License. Pure Python BPS patch application (replaces external flips binary).
