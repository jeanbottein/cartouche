"""
Centralized OS/architecture detection and binary format parsing.

Provides a single source of truth for platform detection and
executable architecture identification via ELF/PE/Mach-O headers.
No external dependencies — uses only struct, platform, and sys.
"""

import os
import platform
import struct
import sys


# ── Current platform detection ──────────────────────────────────────────

def os_tag() -> str:
    """Return the canonical OS tag for the current platform."""
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform.startswith("win"):
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    return "other"


def arch_tag() -> str:
    """Return the canonical architecture tag for the current platform."""
    m = platform.machine().lower()
    if "arm" in m or "aarch64" in m:
        return "arm64"
    if "64" in m or "x86_64" in m or "amd64" in m:
        return "x86_64"
    if "86" in m or "i386" in m or "i686" in m:
        return "x86"
    return "other"


# ── Binary executable detection ─────────────────────────────────────────

# ELF e_machine values
_ELF_ARCH = {
    0x03: "x86",
    0x3E: "x86_64",
    0xB7: "arm64",
    0x28: "arm",
}

# PE Machine values
_PE_ARCH = {
    0x014C: "x86",
    0x8664: "x86_64",
    0xAA64: "arm64",
}

# Mach-O cputype values
_MACHO_ARCH = {
    7: "x86",         # CPU_TYPE_X86
    0x01000007: "x86_64",  # CPU_TYPE_X86_64
    12: "arm",         # CPU_TYPE_ARM
    0x0100000C: "arm64",   # CPU_TYPE_ARM64
}


def detect_binary_arch(path: str) -> str | None:
    """
    Detect the architecture of a binary by reading its file header.

    Returns an architecture string ("x86_64", "arm64", "x86", "arm")
    or None if the file is not a recognized executable format.
    """
    try:
        with open(path, 'rb') as f:
            header = f.read(64)
            if len(header) < 4:
                return None

            # ELF: \x7fELF
            if header[:4] == b'\x7fELF':
                if len(header) >= 20:
                    e_machine = struct.unpack_from('<H', header, 18)[0]
                    return _ELF_ARCH.get(e_machine)

            # PE: MZ header
            if header[:2] == b'MZ':
                if len(header) >= 64:
                    e_lfanew = struct.unpack_from('<I', header, 60)[0]
                    f.seek(e_lfanew)
                    pe_sig = f.read(4)
                    if pe_sig == b'PE\x00\x00':
                        machine_bytes = f.read(2)
                        if len(machine_bytes) == 2:
                            machine = struct.unpack('<H', machine_bytes)[0]
                            return _PE_ARCH.get(machine)

            # Mach-O: magic bytes (32-bit, 64-bit, and fat/universal)
            magic = struct.unpack_from('<I', header, 0)[0]
            if magic in (0xFEEDFACE, 0xFEEDFACF):  # 32-bit, 64-bit LE
                if len(header) >= 8:
                    cputype = struct.unpack_from('<I', header, 4)[0]
                    return _MACHO_ARCH.get(cputype)
            magic_be = struct.unpack_from('>I', header, 0)[0]
            if magic_be in (0xFEEDFACE, 0xFEEDFACF):  # 32-bit, 64-bit BE
                if len(header) >= 8:
                    cputype = struct.unpack_from('>I', header, 4)[0]
                    return _MACHO_ARCH.get(cputype)

    except (OSError, struct.error):
        pass

    return None


def is_executable(path: str) -> bool:
    """
    Check if a file is a recognized executable (ELF, PE, or Mach-O).

    Uses header magic bytes — does not rely on file permissions or
    system commands.
    """
    try:
        with open(path, 'rb') as f:
            magic = f.read(4)
            if len(magic) < 2:
                return False

            # ELF
            if magic == b'\x7fELF':
                return True

            # PE (MZ header)
            if magic[:2] == b'MZ':
                return True

            # Mach-O
            if len(magic) >= 4:
                val = struct.unpack_from('<I', magic, 0)[0]
                if val in (0xFEEDFACE, 0xFEEDFACF, 0xCAFEBABE):
                    return True
                val_be = struct.unpack_from('>I', magic, 0)[0]
                if val_be in (0xFEEDFACE, 0xFEEDFACF, 0xCAFEBABE):
                    return True

    except OSError:
        pass

    return False
