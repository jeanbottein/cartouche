"""
Single source of truth for the application name.

Derived at runtime from the binary stem (frozen) or the main script
stem (source run), so renaming cartouche → potato propagates everywhere.
"""
import sys
from pathlib import Path

if getattr(sys, 'frozen', False):
    APP_NAME = Path(sys.executable).stem
else:
    APP_NAME = Path(sys.argv[0]).stem
