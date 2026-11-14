•CARTRIDGE ("dot cartridge")
============================

A cartridge for your DRM-free games: this folder holds the app, its settings
and your games. Move it, copy it to another drive or back it up as one piece.
Name the folder as you like (e.g. Potato-games): that name is the cartridge's
label in the app and in Steam.

  the app         cartridge.exe (Windows), Cartridge.app (macOS),
                  Cartridge.AppImage (Linux / Steam Deck)
  system/         settings (config.txt, created on first launch), caches
  games/          one folder per game
  mods/           patches and file replacements
  saves-copies/   backups of your save files
  saves-links/    links to the live save folders (for Syncthing & co)

START
  Launch the app and put each game in its own folder in games/: the app
  finds its executable and fetches its artwork by itself, at launch and as
  soon as a copy is done. Press Y on a game in the LIBRARY to add it to Steam.

STEAM ARTWORK FOR THE CARTRIDGE
  With "Add <folder name> to Steam" on (SETTINGS), the cartridge appears in
  Steam under its label. Drop cover.png, wide.png, hero.png, logo.png or
  icon.png into system/ to use your own images; the next launch installs them.

KEEP IT TOGETHER
  The app looks for system/ right next to itself. Keep them in the same
  folder; to use another cartridge, launch the app inside that one.

TWO CARTRIDGES, ONE STEAM
  Each cartridge has an id (system/cartridge-id), so their Steam shortcuts
  never mix. A copied cartridge keeps the id: if you run a copy next to the
  original on the same machine, delete system/cartridge-id in the copy.

UPDATING
  Extract a newer release over this folder: the app and these READMEs are
  replaced, your settings and games are left untouched.

FIRST LAUNCH, PER SYSTEM
  Linux / Steam Deck: if the AppImage won't start, make it executable
  (right-click > Properties > Permissions, or: chmod +x Cartridge.AppImage).
  Don't let an AppImage manager (Gear Lever, AppImageLauncher) move it:
  away from this folder it can't find its settings.

  macOS: the app isn't notarized by Apple. Move the •cartridge folder where
  you want it with Finder first, then right-click Cartridge.app > Open. If it
  says it runs from a temporary copy, run once in Terminal:
      xattr -dr com.apple.quarantine /path/to/•cartridge

  Windows: SmartScreen may warn about an unknown app: More info > Run anyway.

HELP
  cartridge --help (or the app's SETTINGS tab), and
  https://github.com/jeanbottein/cartouche
