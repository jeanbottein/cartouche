#!/usr/bin/env python3
"""
Build helper: inject a SteamGridDB API key into lib/api_keys.py.

Reads the key from the STEAMGRIDDB_API_KEY environment variable,
XOR-encrypts it with a random nonce, and writes both byte literals
into lib/api_keys.py. This prevents `strings` from finding the key
in the compiled binary.

Usage (CI):
    STEAMGRIDDB_API_KEY=xxx python scripts/inject_api_key.py

If the env var is not set, does nothing (binary ships without a default key).
"""

import os
import sys
from pathlib import Path

API_KEYS_PATH = Path(__file__).resolve().parent.parent / "lib" / "api_keys.py"


def main():
    key = os.environ.get("STEAMGRIDDB_API_KEY", "").strip()
    if not key:
        print("STEAMGRIDDB_API_KEY not set — skipping API key injection")
        return

    key_bytes = key.encode("utf-8")
    nonce = os.urandom(len(key_bytes))
    encrypted = bytes(a ^ b for a, b in zip(key_bytes, nonce))

    source = API_KEYS_PATH.read_text()
    source = source.replace("_K = b''", f"_K = {encrypted!r}")
    source = source.replace("_N = b''", f"_N = {nonce!r}")
    API_KEYS_PATH.write_text(source)

    print(f"Injected SteamGridDB API key ({len(key)} chars, XOR-encrypted)")


if __name__ == "__main__":
    main()
