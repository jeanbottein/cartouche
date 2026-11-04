CARTOUCHE
=========

A cartridge for your DRM-free games: this folder holds the app, its settings
and your games. Move it, copy it to another drive or back it up as one piece.

  Cartouche app   cartouche.exe (Windows), Cartouche.app (macOS),
                  Cartouche.AppImage (Linux / Steam Deck)
  system/         settings (config.txt, created on first launch), caches
  games/          one folder per game
  mods/           patches and file replacements
  saves-copies/   backups of your save files
  saves-links/    links to the live save folders (for Syncthing & co)

START
  Launch the app. Add games to games/, then run the pipeline from the
  STATUS tab: it finds each game's executable, fetches artwork and (if you
  enable it in SETTINGS) adds the games to Steam.

KEEP IT TOGETHER
  The app looks for system/ right next to itself. Keep them in the same
  folder; to use another cartridge, launch the app inside that one.

UPDATING
  Extract a newer release over this folder: the app and these READMEs are
  replaced, your settings and games are left untouched.

FIRST LAUNCH, PER SYSTEM
  Linux / Steam Deck: if the AppImage won't start, make it executable
  (right-click > Properties > Permissions, or: chmod +x Cartouche.AppImage).
  Don't let an AppImage manager (Gear Lever, AppImageLauncher) move it:
  away from this folder it can't find its settings.

  macOS: the app isn't notarized by Apple. Move the Cartouche folder where
  you want it with Finder first, then right-click Cartouche.app > Open. If it
  says it runs from a temporary copy, run once in Terminal:
      xattr -dr com.apple.quarantine /path/to/Cartouche

  Windows: SmartScreen may warn about an unknown app: More info > Run anyway.

HELP
  cartouche --help (or the app's SETTINGS tab), and
  https://github.com/jeanbottein/cartouche
