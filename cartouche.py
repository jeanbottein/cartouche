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
            config_map[k] = v.strip()
    return config_map


def ensure_config_file(script_dir: Path) -> Path:
    config_path = script_dir / 'config.txt'
    default_path = script_dir / 'config-default.txt'

    if config_path.exists():
        logger.info("Loaded config from %s", config_path)
        return config_path

    if default_path.exists():
        try:
            shutil.copy2(default_path, config_path)
            logger.info("config.txt missing. Copied defaults from %s", default_path.name)
        except OSError as exc:
            logger.error("Failed to copy %s -> %s: %s", default_path, config_path, exc)
    else:
        logger.warning("config.txt missing and config-default.txt not found. Using empty configuration")

    return config_path


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
        logger.warning("FREEGAMES_PATH not configured or invalid")
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

        label = game.title
        if game.title != game.original_title:
            label = f"{game.original_title} -> {game.title}"

        logger.info("  %s", label)
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
            logger.info("  Steam user %s: %d existing cartouche shortcuts", uid, len(owned))
            for _, appname in owned:
                logger.info("     - %s", appname)
        else:
            logger.info("  Steam user %s: no cartouche shortcuts yet", uid)


def main():
    logging.basicConfig(level=logging.INFO, format='%(message)s')
    script_dir = Path(__file__).resolve().parent
    config_path = ensure_config_file(script_dir)
    cfg = load_config_map(config_path)
    cfg["_CONFIG_PATH"] = str(config_path)
    cfg["_SCRIPT_DIR"] = str(script_dir)

    # CLI modes
    args = sys.argv[1:]
    if len(args) >= 2 and args[0] == "test" and args[1] == "steam":
        test_steam(cfg)
        return

    games_dir = cfg.get("FREEGAMES_PATH")
    if not games_dir or not os.path.isdir(games_dir):
        logger.warning("FREEGAMES_PATH not configured or invalid")
        return

    # Step 0: Migration (one-time)
    migrator.migrate(games_dir)

    # Step 1: Parse games into in-memory database
    db = scanner.scan(games_dir)

    # Step 2: Calculate missing data (exe detection)
    detector.detect(db)

    # Step 3: Fetch from SteamGridDB (if API key available)
    enricher.enrich(db, cfg)

    # Step 4: Persist .cartouche/game.json + images
    if cfg.get("PERSIST_DATA", "True").lower() != "false":
        persister.persist(db)

    # Step 5: Clean old Steam shortcuts
    steam_cleaner.clean(db, cfg)

    # Step 6: Export to Steam
    steam_exporter.export(db, cfg)

    # Step 7: ROM manager manifest
    if cfg.get("MANIFEST_EXPORT", "True").lower() != "false":
        manifest_path = cfg.get("MANIFEST_PATH", os.path.join(games_dir, "manifests.json"))
        manifest_writer.write(db, manifest_path)

    # Step 8: Patcher
    patcher.run(cfg)

    # Step 9: Saver (backup/restore)
    saver.run(db, cfg)

    # Step 10: Configurer (emulator config mutations)
    configurer.run(cfg)

    # Step 11: Post commands
    run_post_commands(cfg)


if __name__ == "__main__":
    main()
