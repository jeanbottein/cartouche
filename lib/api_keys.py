"""
API key management with build-time injection support.

Release binaries embed an XOR-encrypted default SteamGridDB API key,
injected by scripts/inject_api_key.py during CI builds. Users can
override via config.txt or the STEAMGRIDDB_API_KEY environment variable.

When running from source (no injection), these placeholders are empty
and the user must provide their own key.
"""

import os

# Placeholders — replaced by scripts/inject_api_key.py at build time.
# DO NOT put real keys here; they are injected from GitHub Secrets.
_K = b''  # XOR-encrypted key
_N = b''  # Random nonce


def _decrypt() -> str:
    """Recover the API key by XOR-ing the encrypted blob with the nonce."""
    if not _K or not _N:
        return ""
    return bytes(a ^ b for a, b in zip(_K, _N)).decode("utf-8", errors="replace")


def get_steamgriddb_key(cfg: dict) -> str:
    """
    Return the SteamGridDB API key.

    Priority: config.txt > STEAMGRIDDB_API_KEY env var > built-in default.
    """
    key = cfg.get("STEAMGRIDDB_API_KEY", "").strip()
    if key:
        return key
    key = os.environ.get("STEAMGRIDDB_API_KEY", "").strip()
    if key:
        return key
    return _decrypt()
