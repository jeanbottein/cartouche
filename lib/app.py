"""
Single source of truth for the application name.

Derived from the binary/script filename: everything before the first
'-' or '.'. This lets distribution files live in the same folder with
suffixes (e.g. cartouche-linux-x86_64, cartouche.v2) while still
resolving to "cartouche". Falls back to "cartouche" if the result is empty.
"""
import re
import sys
from pathlib import Path

if getattr(sys, 'frozen', False):
    _filename = Path(sys.executable).name
else:
    _filename = Path(sys.argv[0]).name

APP_NAME = re.split(r'[-.]', _filename)[0] or "cartouche"
