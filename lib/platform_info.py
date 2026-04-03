"""
Centralized OS/architecture detection and binary format parsing.

Provides a single source of truth for platform detection and
executable architecture identification via ELF/PE/Mach-O headers.
No external dependencies — uses only struct, platform, and sys.
"""

import platform
import struct
import sys


# ── Current platform detection ──────────────────────────────────────────

def os_tag() -> str:
    """Return the canonical OS tag for the current platform."""
    if sys.platform.startswith("linux"):  return "linux"
    if sys.platform.startswith("win"):    return "windows"
    if sys.platform == "darwin":           return "macos"
    return "other"


def arch_tag() -> str:
    """Return the canonical architecture tag for the current platform."""
    m = platform.machine().lower()
    if "arm" in m or "aarch64" in m:
        return "arm64"
    if "64" in m or "x86_64" in m or "amd64" in m or "86" in m or "i386" in m or "i686" in m:
        return "x64"
    return "other"


# ── Binary format look-up tables ─────────────────────────────────────────

_ELF_ARCH = {
    0x03: "x64",    # x86 → x64
    0x3E: "x64",    # x86_64 → x64
    0xB7: "arm64",
    0x28: "arm64",  # ARM → arm64
}

_PE_ARCH = {
    0x014C: "x64",  # x86 → x64
    0x8664: "x64",  # x86_64 → x64
    0xAA64: "arm64",
}

_MACHO_ARCH = {
    7:          "x64",    # CPU_TYPE_X86 → x64
    0x01000007: "x64",    # CPU_TYPE_X86_64 → x64
    12:         "arm64",  # CPU_TYPE_ARM → arm64
    0x0100000C: "arm64",  # CPU_TYPE_ARM64
}

_MACHO_MAGICS = {0xFEEDFACE, 0xFEEDFACF}
_EXEC_MAGICS  = _MACHO_MAGICS | {0xCAFEBABE}


# ── Per-format arch parsers ──────────────────────────────────────────────

def _read_elf_arch(header: bytes) -> str | None:
    if len(header) < 20:
        return None
    return _ELF_ARCH.get(struct.unpack_from('<H', header, 18)[0])


def _read_pe_arch(f, header: bytes) -> str | None:
    if len(header) < 64:
        return None
    e_lfanew = struct.unpack_from('<I', header, 60)[0]
    f.seek(e_lfanew)
    if f.read(4) != b'PE\x00\x00':
        return None
    machine_bytes = f.read(2)
    if len(machine_bytes) < 2:
        return None
    return _PE_ARCH.get(struct.unpack('<H', machine_bytes)[0])


def _read_macho_arch(header: bytes) -> str | None:
    if len(header) < 8:
        return None
    for fmt in ('<I', '>I'):
        magic = struct.unpack_from(fmt, header, 0)[0]
        if magic in _MACHO_MAGICS:
            return _MACHO_ARCH.get(struct.unpack_from(fmt, header, 4)[0])
    return None


# ── Public API ───────────────────────────────────────────────────────────

def detect_binary_arch(path: str) -> str | None:
    """
    Detect the architecture of a binary by reading its file header.
    Returns a canonical arch string or None if format is unrecognised.
    """
    try:
        with open(path, 'rb') as f:
            header = f.read(64)
            if len(header) < 4:
                return None
            if header[:4] == b'\x7fELF':
                return _read_elf_arch(header)
            if header[:2] == b'MZ':
                return _read_pe_arch(f, header)
            return _read_macho_arch(header)
    except (OSError, struct.error):
        return None


def is_executable(path: str) -> bool:
    """
    Check if a file is a recognised executable (ELF, PE, or Mach-O).
    Uses header magic bytes — does not rely on file permissions.
    """
    try:
        with open(path, 'rb') as f:
            magic = f.read(4)
        if len(magic) < 2:
            return False
        if magic[:4] == b'\x7fELF' or magic[:2] == b'MZ':
            return True
        if len(magic) >= 4:
            val_le = struct.unpack_from('<I', magic, 0)[0]
            val_be = struct.unpack_from('>I', magic, 0)[0]
            return val_le in _EXEC_MAGICS or val_be in _EXEC_MAGICS
    except OSError:
        pass
    return False
