#!/usr/bin/env python3
"""
Cartouche - Game library manager.

Manages a collection of DRM-free games: detects executables, fetches
artwork from SteamGridDB, syncs to Steam as non-Steam shortcuts,
backs up save files, applies patches, and configures emulators.
"""

from pathlib import Path
from typing import Dict
import logging
import os
import shutil
import subprocess
import sys

from lib import migrator
from lib import scanner
from lib import detector
from lib import enricher
from lib import persister
from lib import steam_cleaner
from lib import steam_exporter
from lib import manifest_writer
from lib import patcher
from lib import saver
from lib import configurer
from lib.app import APP_NAME, get_script_dir, find_app_dir
from lib.init_dialog import run_init_dialog

logger = logging.getLogger(__name__)


def load_config_map(config_path: Path) -> Dict[str, str]:
    if not config_path.exists():
        logger.warning("Config file %s not found; continuing with empty configuration", config_path)
        return {}
    lines = (l.strip() for l in config_path.read_text().splitlines())
    pairs = (l.split('=', 1) for l in lines if l and not l.startswith('#') and '=' in l)
    config_map = {}
    for k, v in pairs:
        k = k.strip()
        if k:
            if '#' in v:
                v = v.split('#')[0]
            val = v.strip()
            if val:
                config_map[k] = val
    return config_map


def parse_args(argv: list) -> tuple:
    """
    Split argv on '--' into a positional directory and mode flags.

    Usage:
        cartouche                            normal run, auto-detect .{APP_NAME}/
        cartouche /path/to/dir               normal run, explicit directory
        cartouche -- test steam              dry-run, auto-detect
        cartouche /path/to/dir -- test steam dry-run, explicit directory
    """
    if '--' in argv:
        sep    = argv.index('--')
        before = argv[:sep]
        after  = argv[sep + 1:]
    else:
        before = argv
        after  = []

    cli_dir = None
    if before:
        p = Path(before[0]).expanduser()
        if p.is_dir():
            cli_dir = p.resolve()

    dry_run = len(after) >= 2 and after[0] == 'test' and after[1] == 'steam'
    return cli_dir, dry_run


def _seed_conf(app_dir: Path, conf_path: Path) -> None:
    script_dir = get_script_dir()
    default = script_dir / f'{APP_NAME}-default.conf'
    if not default.exists() and getattr(sys, 'frozen', False):
        default = Path(sys._MEIPASS) / f'{APP_NAME}-default.conf'
    if default.exists():
        try:
            shutil.copy2(default, conf_path)
            logger.info("%s missing. Copied defaults from %s", conf_path.name, default.name)
        except OSError as exc:
            logger.error("Failed to copy %s -> %s: %s", default, conf_path, exc)
    else:
        logger.warning("%s missing and no default template found. Using empty configuration.",
                       conf_path.name)


def resolve_config_path(cli_dir: Path | None) -> Path:
    """Locate or create the .{APP_NAME}/ directory and return the conf file path inside it."""
    app_dir = find_app_dir(cli_dir)
    if app_dir is None:
        app_dir = run_init_dialog(get_script_dir(), Path.cwd())
    conf_path = app_dir / f'{APP_NAME}.conf'
    if not conf_path.exists():
        _seed_conf(app_dir, conf_path)
    else:
        logger.info("Loaded config from %s", conf_path)
    return conf_path


def run_post_commands(config: Dict[str, str]) -> None:
    """Run commands defined by RUN_AFTER_<name>=<command> entries, sorted by key."""
    commands = sorted(
        [(k, v) for k, v in config.items() if k.startswith("RUN_AFTER_") and v],
        key=lambda x: x[0],
    )
    if not commands:
        return

    logger.info("Running %d post-command(s)", len(commands))
    for key, cmd in commands:
        label = key[len("RUN_AFTER_"):]
        for cfg_key, cfg_val in config.items():
            cmd = cmd.replace(f"${{{cfg_key}}}", cfg_val)

        logger.info("  %s: %s", label, cmd)
        try:
            result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
            if result.stdout.strip():
                logger.info(result.stdout.strip())
            if result.returncode != 0:
                logger.warning("  %s: exited with code %d", label, result.returncode)
                if result.stderr.strip():
                    logger.warning("   %s", result.stderr.strip())
            else:
                logger.info("  %s: done", label)
        except Exception as e:
            logger.error("  %s: failed to execute: %s", label, e)


def test_steam(cfg: dict):
    """Dry-run mode: show what would be synced to Steam."""
    games_dir = cfg.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        logger.error("FREEGAMES_PATH not configured or invalid. Stopping the app.")
        return

    # Run pipeline up to enrichment
    migrator.migrate(games_dir)
    db = scanner.scan(games_dir)
    detector.detect(db)
    enricher.enrich(db, cfg)

    games = db.games_with_targets()
    logger.info("Steam test: %d game(s) with targets", len(games))

    api_key = cfg.get("STEAMGRIDDB_API_KEY", "").strip()
    logger.info("SteamGridDB API key: %s", "configured" if api_key else "NOT SET")
    logger.info("")

    for game in games:
        appid = steam_exporter.generate_appid(game.title, game.resolved_target)
        signed = steam_exporter._signed32(appid)

        logger.info("  %s", game.title)
        logger.info("     AppID:    %d (signed: %d)", appid, signed)
        logger.info("     Exe:      %s", game.resolved_target)
        logger.info("     StartDir: %s", game.resolved_start_in)
        if game.resolved_launch_options:
            logger.info("     Options:  %s", game.resolved_launch_options)
        logger.info("     SGDB ID:  %s", game.steamgriddb_id or "none")
        for field in ("cover", "icon", "hero", "logo"):
            img = getattr(game.images, field)
            logger.info("     %s: %s", field, img or "none")
        logger.info("")

    # Show existing shortcuts
    config_dirs = steam_cleaner.find_steam_userdata_dirs()
    for config_dir in config_dirs:
        shortcuts_path = os.path.join(config_dir, "shortcuts.vdf")
        shortcuts = steam_cleaner.load_shortcuts(shortcuts_path)
        owned = [(k, steam_cleaner._get_appname(s)) for k, s in shortcuts.items() if steam_cleaner._has_ownership_tag(s)]
        uid = os.path.basename(os.path.dirname(config_dir))
        if owned:
            logger.info("  Steam user %s: %d existing %s shortcuts", uid, len(owned), APP_NAME)
            for _, appname in owned:
                logger.info("     - %s", appname)
        else:
            logger.info("  Steam user %s: no %s shortcuts yet", uid, APP_NAME)


def main():
    logging.basicConfig(level=logging.INFO, format='%(message)s')
    cli_dir, dry_run = parse_args(sys.argv[1:])
    config_path = resolve_config_path(cli_dir)
    cfg = load_config_map(config_path)
    cfg["_CONFIG_PATH"] = str(config_path)
    cfg["_SCRIPT_DIR"] = str(get_script_dir())

    if dry_run:
        test_steam(cfg)
        return

    games_dir = cfg.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        logger.error("FREEGAMES_PATH not configured or invalid. Stopping the app.")
        return

    # Step 0: Migration (one-time)
    logger.info("\n--- MIGRATION ---")
    migrator.migrate(games_dir)

    # Step 1: Parse games into in-memory database
    logger.info("\n--- SCANNING ---")
    db = scanner.scan(games_dir)

    # Step 2: Calculate missing data (exe detection)
    logger.info("\n--- DETECTION ---")
    detector.detect(db)

    # Step 3: Fetch from SteamGridDB (if API key available)
    logger.info("\n--- ENRICHMENT ---")
    enricher.enrich(db, cfg)

    # Step 4: Persist .cartouche/game.json + images
    logger.info("\n--- PERSISTENCE ---")
    if cfg.get("PERSIST_DATA", "True").lower() != "false":
        persister.persist(db)

    # Step 5: Clean old Steam shortcuts
    logger.info("\n--- STEAM CLEANUP ---")
    steam_cleaner.clean(db, cfg)

    # Step 6: Export to Steam
    logger.info("\n--- STEAM EXPORT ---")
    steam_exporter.export(db, cfg)

    # Step 7: ROM manager manifest
    logger.info("\n--- MANIFEST ---")
    if cfg.get("MANIFEST_EXPORT", "True").lower() != "false":
        manifest_path = cfg.get("MANIFEST_PATH", os.path.join(games_dir, "manifests.json"))
        manifest_writer.write(db, manifest_path)

    # Step 8: Patcher
    logger.info("\n--- PATCHER ---")
    patcher.run(cfg)

    # Step 9: Saver (backup/restore)
    logger.info("\n--- SAVER ---")
    saver.run(db, cfg)

    # Step 10: Configurer (emulator config mutations)
    logger.info("\n--- CONFIGURER ---")
    configurer.run(cfg)

    # Step 11: Post commands
    logger.info("\n--- POST COMMANDS ---")
    run_post_commands(cfg)


if __name__ == "__main__":
    main()
