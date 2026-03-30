"""
Pipeline runner for cartouche.

Wraps the multi-step pipeline into a callable class with named phases,
progress callbacks, and phase grouping. Used by both the CLI batch
mode and the GUI.
"""

import logging
import os
from typing import Callable

from . import migrations, scanner, detector, enricher, persister
from . import steam_cleaner, steam_exporter, manifest_writer
from . import patcher, saver, configurer
from .models import GameDatabase
from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.pipeline")


class PipelineRunner:
    """Orchestrates the cartouche pipeline with per-phase control."""

    PHASES = [
        ("migrate",       "Migration"),
        ("scan",          "Scanning"),
        ("detect",        "Detection"),
        ("enrich",        "Enrichment"),
        ("persist",       "Persistence"),
        ("steam_clean",   "Steam cleanup"),
        ("steam_export",  "Steam export"),
        ("manifest",      "Manifest"),
        ("patch",         "Patcher"),
        ("save",          "Saver"),
        ("configure",     "Configurer"),
        ("post_commands", "Post commands"),
    ]

    GROUPS = {
        "all":    ["migrate", "scan", "detect", "enrich", "persist",
                   "steam_clean", "steam_export", "manifest",
                   "patch", "save", "configure", "post_commands"],
        "parse":  ["migrate", "scan", "detect", "enrich", "persist"],
        "steam":  ["steam_clean", "steam_export"],
        "backup": ["save"],
        "patch":  ["patch"],
        "configure": ["configure"],
    }

    def __init__(self, cfg: dict, games_dir: str,
                 on_phase_start: Callable[[str, str], None] | None = None,
                 on_phase_end: Callable[[str], None] | None = None):
        self.cfg = cfg
        self.games_dir = games_dir
        self.db: GameDatabase | None = None
        self._on_phase_start = on_phase_start
        self._on_phase_end = on_phase_end
        self._post_commands_fn: Callable | None = None

    def set_post_commands_fn(self, fn: Callable):
        """Set the function for running post commands (from cartouche.py)."""
        self._post_commands_fn = fn

    def run_phase(self, phase_name: str):
        """Run a single named phase."""
        label = dict(self.PHASES).get(phase_name, phase_name)
        if self._on_phase_start:
            self._on_phase_start(phase_name, label)

        logger.info(f"\n--- {label.upper()} ---")
        getattr(self, f"_phase_{phase_name}")()

        if self._on_phase_end:
            self._on_phase_end(phase_name)

    def run_all(self):
        """Run all phases in order."""
        self.run_group("all")

    def run_group(self, group: str):
        """Run a named group of phases."""
        phase_names = self.GROUPS.get(group, [group])
        for name in phase_names:
            self.run_phase(name)

    # ── Individual phase implementations ──────────────────────────────────

    def _phase_migrate(self):
        migrations.run_all_migrations(self.games_dir)

    def _phase_scan(self):
        self.db = scanner.scan(self.games_dir)

    def _phase_detect(self):
        if self.db:
            detector.detect(self.db)

    def _phase_enrich(self):
        if self.db:
            enricher.enrich(self.db, self.cfg)

    def _phase_persist(self):
        if self.db and self.cfg.get("PERSIST_DATA", "True").lower() != "false":
            persister.persist(self.db)

    def _phase_steam_clean(self):
        if self.db:
            steam_cleaner.clean(self.db, self.cfg)

    def _phase_steam_export(self):
        if self.db:
            steam_exporter.export(self.db, self.cfg)

    def _phase_manifest(self):
        if self.db and self.cfg.get("MANIFEST_EXPORT", "True").lower() != "false":
            manifest_path = self.cfg.get(
                "MANIFEST_PATH",
                os.path.join(self.games_dir, "manifests.json"),
            )
            manifest_writer.write(self.db, manifest_path)

    def _phase_patch(self):
        patcher.run(self.cfg)

    def _phase_save(self):
        if self.db:
            saver.run(self.db, self.cfg)

    def _phase_configure(self):
        configurer.run(self.cfg)

    def _phase_post_commands(self):
        if self._post_commands_fn:
            self._post_commands_fn(self.cfg)
