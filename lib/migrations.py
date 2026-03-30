"""
Cartouche data migrations coordinator.

Runs all data migrations in sequence. Add new migrations here as they're needed.
Each migration should be idempotent (safe to run multiple times).
"""

import logging
from typing import Callable

from . import migrator, save_paths_migrator, arch_migrator

logger = logging.getLogger(__name__)


# Registry of all migrations in order
MIGRATIONS: list[tuple[str, Callable[[str], int | None]]] = [
    ("launch_manifest", migrator.migrate),
    ("save_paths_format", save_paths_migrator.migrate_all_games),
    ("arch_x64", arch_migrator.migrate_all_games),
]


def run_all_migrations(games_dir: str) -> dict[str, int]:
    """
    Run all registered migrations in sequence.
    Returns a dict mapping migration name to count of items migrated.
    """
    results = {}

    for name, migration_fn in MIGRATIONS:
        try:
            count = migration_fn(games_dir)
            results[name] = count or 0
            logger.info(f"Migration '{name}': {results[name]} item(s)")
        except Exception as exc:
            logger.error(f"Migration '{name}' failed: {exc}", exc_info=True)
            results[name] = -1  # Indicate failure

    return results
