"""
Data models for cartouche.

Central dataclasses used throughout the pipeline to represent games
and their metadata in memory.
"""

from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional, List, Dict

from .app import APP_NAME

CARTOUCHE_DIR = f".{APP_NAME}"
GAME_JSON = "game.json"
SCHEMA_VERSION = 2


@dataclass
class GameTarget:
    """A platform-specific executable target for a game."""
    os: str
    arch: str
    target: str
    start_in: str
    launch_options: str = ""

    def to_dict(self) -> dict:
        return {
            "os": self.os,
            "arch": self.arch,
            "target": self.target,
            "startIn": self.start_in,
            "launchOptions": self.launch_options,
        }

    @classmethod
    def from_dict(cls, d: dict) -> "GameTarget":
        return cls(
            os=d.get("os", ""),
            arch=d.get("arch", ""),
            target=d.get("target", ""),
            start_in=d.get("startIn", ""),
            launch_options=d.get("launchOptions", ""),
        )


@dataclass
class GameImages:
    """Filenames for Steam artwork images stored in .cartouche/."""
    cover: Optional[str] = None    # Grid/poster artwork
    icon: Optional[str] = None     # Icon
    hero: Optional[str] = None     # Hero banner
    logo: Optional[str] = None     # Logo overlay

    def to_dict(self) -> dict:
        d = {}
        if self.cover:
            d["cover"] = self.cover
        if self.icon:
            d["icon"] = self.icon
        if self.hero:
            d["hero"] = self.hero
        if self.logo:
            d["logo"] = self.logo
        return d

    @classmethod
    def from_dict(cls, d: dict) -> "GameImages":
        return cls(
            cover=d.get("cover"),
            icon=d.get("icon"),
            hero=d.get("hero"),
            logo=d.get("logo"),
        )


@dataclass
class Game:
    """In-memory representation of a game."""
    # Identity
    folder_name: str                 # The subfolder name under FREEGAMES_PATH
    game_dir: Path                   # Absolute path to game folder

    # Metadata
    title: str = ""                  # Display title (may be SGDB official name)

    # Targets
    targets: List[GameTarget] = field(default_factory=list)

    # Save paths - flat list of {"os": "...", "path": "..."} dicts
    save_paths: List[Dict[str, str]] = field(default_factory=list)

    # SteamGridDB
    steamgriddb_id: Optional[int] = None
    images: GameImages = field(default_factory=GameImages)

    # User-editable
    notes: str = ""

    # State tracking
    has_cartouche: bool = False      # True if .cartouche/game.json exists on disk
    needs_persist: bool = False      # True if in-memory data differs from disk

    # Runtime-resolved (computed, not persisted)
    resolved_target: Optional[str] = None     # Absolute path to best exe for current OS/arch
    resolved_start_in: Optional[str] = None   # Absolute working directory
    resolved_launch_options: str = ""
    resolved_target_os: str = ""                                  # OS of the picked target
    resolved_save_paths: List[str] = field(default_factory=list)  # Absolute paths for current OS

    def __post_init__(self):
        if isinstance(self.game_dir, str):
            self.game_dir = Path(self.game_dir)
        if self.title is None:
            self.title = ""

    def __hash__(self):
        return hash(self.folder_name)

    def __eq__(self, other):
        if not isinstance(other, Game):
            return False
        return self.folder_name == other.folder_name

    @property
    def cartouche_dir(self) -> Path:
        return self.game_dir / CARTOUCHE_DIR

    @property
    def game_json_path(self) -> Path:
        return self.cartouche_dir / GAME_JSON

    def to_dict(self) -> dict:
        """Serialize to the game.json schema (only persisted fields)."""
        d = {
            "schema_version": SCHEMA_VERSION,
            "title": self.title,
            "targets": [t.to_dict() for t in self.targets],
            "savePaths": list(self.save_paths),
            "images": self.images.to_dict(),
        }
        if self.steamgriddb_id is not None:
            d["steamgriddb_id"] = self.steamgriddb_id
        if self.notes:
            d["notes"] = self.notes
        return d


class GameDatabase:
    """In-memory collection of all discovered games."""

    def __init__(self):
        self.games: List[Game] = []
        self._by_folder: Dict[str, Game] = {}

    def add(self, game: Game):
        self.games.append(game)
        self._by_folder[game.folder_name] = game

    def get_by_folder(self, folder_name: str) -> Optional[Game]:
        return self._by_folder.get(folder_name)

    def incomplete_games(self) -> List[Game]:
        """Games that still need executable detection."""
        return [g for g in self.games if not g.targets]

    def games_needing_enrichment(self) -> List[Game]:
        """Games missing SteamGridDB data."""
        return [g for g in self.games if g.steamgriddb_id is None]

    def dirty_games(self) -> List[Game]:
        """Games with unsaved changes."""
        return [g for g in self.games if g.needs_persist]

    def games_with_targets(self) -> List[Game]:
        """Games that have at least one resolved target."""
        return [g for g in self.games if g.resolved_target]

    def __len__(self) -> int:
        return len(self.games)

    def __iter__(self):
        return iter(self.games)
