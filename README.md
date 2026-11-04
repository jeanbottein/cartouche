# •cartridge

> *Say "dot cartridge". Because nothing beats the nostalgia of blowing into a plastic slot and praying your game boots.*

A no-nonsense tool for managing DRM-free games, patches, and save backups on Linux (and especially Steam Deck). One self-contained binary: a retro, controller-friendly UI by default, or a plain terminal run with `-t`.

---

Every game carries a `.cartridge/` folder with its metadata and artwork, and a folder holding the app, its `system/` settings and your games *is* a cartridge: portable, easy to back up, and named however you like. Call the folder `Potato-games` and that's its label, in the app and in Steam. It already survived being called *gamer-sidekick* and *cartouche*.

---

## Features

- **Scan** — discovers game folders and loads existing metadata
- **Detect** — auto-detects executables when metadata is missing
- **Enrich** — fetches official names and artwork from [SteamGridDB](https://www.steamgriddb.com/)
- **Steam sync** — adds games to Steam as non-Steam shortcuts, complete with artwork, and sets Proton for Windows games
- **Self shortcut** — `STEAM_ADD_SELF=True` adds the cartridge itself to Steam under its label, with retro artwork (or yours: drop `cover.png`, `hero.png`, ... into `system/`)
- **Patch** — applies BPS patches or file replacements via `patch.json`
- **Save backup** — backs up (or restores) save files, with multiple save paths per game, plus a symlink tree for Syncthing
- **Manifest export** — generates `manifests.json` for [Steam ROM Manager](https://github.com/SteamGridDB/steam-rom-manager)
- **Post-run commands** — `RUN_AFTER_<name>=<command>` entries run after everything else
- **Retro UI** — browse and launch games, edit every game (targets, save paths, artwork), run the pipeline with live logs, and change every setting — with a gamepad (on-screen keyboard included), keyboard or mouse

Metadata and artwork are stored in a `.cartridge/` subfolder inside each game's directory — no central database, no magic, just files.

### Save Path Variables

When configuring save paths in `game.json`, you generally only need to provide the Windows path (e.g. `%APPDATA%`, `%USERPROFILE%`, or `C:\...`). If the game is run on Linux through Proton, •cartridge will automatically translate common Windows paths to their corresponding Proton prefix equivalents in `~/.local/share/Steam/steamapps/compatdata/<appid>/pfx/drive_c/`.

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

## Getting started

1. Download the binary for your platform from the releases (or build it, see below).
2. Run it. On first launch it asks where to put the workspace (a `system/` folder with `config.txt`) and which folders to create. •cartridge is portable: keep it in its own folder, next to `system/` and your games.
3. Fill in the settings (in the UI, or by editing `system/config.txt`), then run the pipeline from the **Status** tab.

```text
Usage: cartridge [OPTIONS] [DIR]

  [DIR]                Folder holding the system/ workspace
                       (default: next to the binary, then the current folder)
  -t, --terminal       Run the pipeline in the terminal, without the UI
  -g, --group <NAME>   Phases to run in the terminal (implies -t):
                       all (default), parse, steam, backup, patch
  -n, --dry-run        Preview the Steam sync without changing Steam
  -h, --help           Print this help
  -V, --version        Print the version
```

Without a display (e.g. over SSH) the UI isn't available, so •cartridge runs the full pipeline in the terminal instead. Set `CARTRIDGE_DEBUG=1` for verbose logs.

### Controls

| Action | Keyboard | Gamepad | Mouse |
|---|---|---|---|
| Move | Arrows / WASD | D-pad / Left stick | Hover / wheel |
| Confirm (Library: play) | Enter / Space | A | Click the focused row |
| Edit (Library: edit game) | X | X | — |
| Back (Library: **hold 1 s to quit**) | Esc / Backspace | B | — |
| Switch tab | Q / E or 1 2 3 | LB / RB | — |
| Toggle wide / square layout | Tab | Select | — |
| Fullscreen | F | — | — |

In text fields, the physical keyboard types directly; on a gamepad use the on-screen keyboard (A types, B deletes, X space, Y shift, Start accepts).

**Steam Game Mode:** gamescope gives the app the resolution set in the shortcut's *Properties → Game Resolution*. Set it to **Native**, otherwise Steam upscales a lower resolution with its own filter, softening the pixels.

## Building

**In a container (recommended, nothing installed on the host):** needs only podman or docker.

```bash
./container.sh                  # cargo build --release -> target/release/cartridge
./container.sh test             # any cargo command
./container.sh run -- -t        # (terminal mode only: the container has no display)
```

The toolchain and build libraries live in the image (`Containerfile`, Debian 12, so the binary runs on SteamOS and other distros); the cargo cache stays in `target/`. Remove everything with `rm -rf target && podman rmi cartridge-build`.

**Natively:** with the Rust toolchain ([rustup](https://rustup.rs)):

```bash
cargo run                       # UI
cargo run -- -t                 # terminal mode
cargo test
./release.sh                    # dist/cartridge-<os>-<arch>
```

Linux build deps: gamepad support (`gilrs`) needs `libudev` and the UI needs X11/OpenGL: `sudo apt-get install -y libudev-dev libx11-dev libxi-dev libgl1-mesa-dev`. Without them, build keyboard/mouse-only with `cargo build --no-default-features`.

Set `STEAMGRIDDB_API_KEY` at build time (also passed through by `container.sh`) to embed a default (obfuscated) API key; users can still override it in `config.txt` or the environment.

## Platform support

| Platform | Status |
|---|---|
| Linux (Steam Deck, desktop) | Fully tested |
| macOS | Works for development |
| Windows | Included but untested |

## Credits

- **Icon**: <a href="https://www.flaticon.com/free-icons/game-cartridge" title="game cartridge icons">Game cartridge icons created by Freepik - Flaticon</a>
- **Font**: VT323 by The VT323 Project Authors — SIL Open Font License 1.1 (`assets/VT323-OFL.txt`).
- **Rust crates**: [macroquad](https://github.com/not-fl3/macroquad) (UI), [gilrs](https://gitlab.com/gilrs-project/gilrs) (gamepads), [ureq](https://github.com/algesten/ureq) (HTTP), [image](https://github.com/image-rs/image), [serde_json](https://github.com/serde-rs/json), [crc32fast](https://github.com/srijs/rust-crc32fast), [log](https://github.com/rust-lang/log).
