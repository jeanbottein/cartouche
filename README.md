# •cartridge

> *Say "dot cartridge".*

One self-contained, controller-friendly binary to carry DRM-free portable games, patches and save backups around. Steam / SteamOS friendly.

---

Every game carries a `.cartridge/` folder with its metadata and artwork, and a folder holding the app, its `system/` settings and your games *is* a cartridge: portable, easy to back up, and named however you like. Call the folder `Potato-games` and that's its label, in the app and in Steam. The app can be renamed too; only the folders inside the cartridge keep fixed names:

```text
Potato-games/          # any name: the cartridge's label
├── cartridge          # the app (any name)
├── games/
├── mods/
├── saves-copies/
├── saves-links/
└── system/            # settings (config.txt), caches
```

---

## Features

- **Scan** — discovers game folders and loads existing metadata
- **Detect** — auto-detects executables when metadata is missing
- **SteamGridDB integration, out of the box** — fetches official names and artwork from [SteamGridDB](https://www.steamgriddb.com/); release builds embed an API key, so there's nothing to set up (your own key in `config.txt` overrides it)
- **Steam shortcuts** — adds games, or the cartridge itself, to Steam as non-Steam shortcuts, complete with artwork, and sets Proton for Windows games
- **Proton support** — plays Windows games on Linux with Steam's own Proton, in the same prefix as their Steam shortcut
- **Steam ROM Manager integration** — generates `manifests.json` for [Steam ROM Manager](https://github.com/SteamGridDB/steam-rom-manager)
- **Patch** — applies BPS patches or file replacements via `patch.json`
- **Save backup** — backs up (or restores) save files, with multiple save paths per game, plus a symlink tree for Syncthing
- **LAN sharing** — packs games into `.cartridge.tar` and copies them between cartridges on the local network
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

### Windows games (Proton)

On Linux, Windows games start from the app with the Proton builds Steam installed (`steamapps/common`, `compatibilitytools.d`), inside the Steam Linux Runtime, exactly as Steam starts their shortcut: the prefix is the shortcut's `compatdata/<appid>`, so saves and settings are the same from Steam or from the app. What a game needs travels with it in `game.json` (also editable in the game editor):

```json
"proton": {
    "version": "proton_9",
    "winetricks": ["d3dx9", "vcrun2019"],
    "env": {"PROTON_USE_WINED3D": "1", "WINEDLLOVERRIDES": "ddraw=n,b"}
}
```

- **`version`**: Steam's name (`proton_9`, `proton_experimental`, `GE-Proton11-5`) or folder name (`Proton 9.0`). Default: the build Steam has for the game's shortcut, else Steam's default Proton, else the newest one installed (a build that isn't installed is never forced on Steam).
- **`winetricks`**: packages installed into the prefix before the first launch (B cancels). Needs `winetricks`, or the Protontricks Flatpak. What they download is taken from `system/winetricks` when it is there; turn on **Keep Proton downloads** (`PROTON_KEEP_DOWNLOADS`) to download into it, so the cartridge brings those files to other machines.
- **`env`**: environment variables (plain `NAME`s), also written into the Steam shortcut's launch options (`KEY=value %command%`). Without them, launch options you set in Steam are left alone.

## Getting started

1. Download the archive for your platform from the [releases](https://github.com/jeanbottein/cartouche/releases): `cartridge-linux-x86_64.tar.gz` (Steam Deck, desktop Linux), `cartridge-linux-aarch64.tar.gz`, `cartridge-macos-arm64.zip` or `cartridge-windows-x86_64.zip`.
2. Extract it where you want your cartridge. You get a ready-made `Cartridge/` folder (rename it: that's its label) holding the app (`Cartridge.AppImage`, `Cartridge.app` or `cartridge.exe`), an empty `system/`, `games/`, `mods/`, `saves-copies/` and `saves-links/`. Keep the app inside that folder: it looks for `system/` right next to itself. To put a folder elsewhere, replace it with a symbolic link.
3. Launch the app (first launch creates `system/config.txt`). If it won't start:
   - **Linux / Steam Deck:** make the AppImage executable (`chmod +x Cartridge.AppImage`), and don't let an AppImage manager (Gear Lever, AppImageLauncher) move it out of the folder.
   - **macOS:** the app isn't notarized. Right-click `Cartridge.app` > Open; if it says it runs from a temporary copy, run `xattr -dr com.apple.quarantine /path/to/Cartridge` once.
   - **Windows:** on the SmartScreen warning, More info > Run anyway.
4. Drop your games into `games/`, adjust the settings (in the UI, or in `system/config.txt`), then run the pipeline from the **Status** tab.

Prefer the bare binary? Build it (see below) and run it in its own folder: on first launch it creates the same layout next to itself.

```text
Usage: cartridge [OPTIONS] [DIR]

  [DIR]                Folder holding the system/ workspace
                       (default: next to the binary, then the current folder)
  -t, --terminal       Run the pipeline in the terminal, without the UI
  -g, --group <NAME>   Phases to run in the terminal (implies -t):
                       all (default), parse, steam, backup, patch
  -n, --dry-run        Preview the Steam sync without changing Steam
      --pack <GAME>    Pack the game folder GAME into GAME.cartridge.tar
      --unpack <GAME>  Unpack GAME.cartridge.tar into the folder GAME
      --serve          Share the games folder on the LAN, without the UI
      --peers          List •cartridge servers found on the LAN, with their games
      --download <PEER> <GAME>
                       Copy GAME from PEER as a package (GAME.cartridge.tar)
      --install <PEER> <GAME>
                       Copy GAME from PEER into the folder GAME
                       PEER: a name shown by --peers, or host[:port]
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
| Back (other tabs: to Library; Library: **hold 1 s to quit**) | Esc / Backspace | B | — |
| Library: add to / remove from Steam; Network, Mods: refresh | H | Y | — |
| Mute / unmute the background music | M | Select | — |
| Switch tab | Q / E or 1 to 5 | LB / RB | — |
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
