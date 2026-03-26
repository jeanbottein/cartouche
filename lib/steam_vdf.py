"""
Binary VDF reader/writer for Steam's shortcuts.vdf format.

Implemented with the standard library only (struct), following the format
used by Steam's shortcuts.vdf files.

Reference: GameSync by Maikeru86 (MIT) - adapted for stdlib-only usage.
"""

import struct


# Binary VDF type bytes
_TYPE_MAP    = 0x00  # nested map
_TYPE_STR    = 0x01  # null-terminated string
_TYPE_INT32  = 0x02  # 4-byte signed int (little-endian)
_TYPE_END    = 0x08  # end of current map


# ── Reader ───────────────────────────────────────────────────────────────

def _read_string(data, pos):
    """Read a null-terminated UTF-8 string starting at *pos*."""
    end = data.index(b'\x00', pos)
    return data[pos:end].decode('utf-8', errors='replace'), end + 1


def _read_int32(data, pos):
    """Read a 4-byte little-endian signed integer."""
    val = struct.unpack_from('<i', data, pos)[0]
    return val, pos + 4


def _read_map(data, pos):
    """Recursively read a binary VDF map and return a dict."""
    result = {}
    while pos < len(data):
        type_byte = data[pos]
        pos += 1

        if type_byte == _TYPE_END:
            return result, pos

        key, pos = _read_string(data, pos)

        if type_byte == _TYPE_MAP:
            val, pos = _read_map(data, pos)
        elif type_byte == _TYPE_STR:
            val, pos = _read_string(data, pos)
        elif type_byte == _TYPE_INT32:
            val, pos = _read_int32(data, pos)
        else:
            raise ValueError(f"Unknown VDF type byte 0x{type_byte:02x} at offset {pos - 1}")

        result[key] = val

    return result, pos


def binary_vdf_load(f):
    """Load a binary VDF file and return its contents as nested dicts."""
    data = f.read()
    if not data:
        return {}
    result, _ = _read_map(data, 0)
    return result


# ── Writer ───────────────────────────────────────────────────────────────

def _write_string(f, key, value):
    f.write(struct.pack('B', _TYPE_STR))
    f.write(key.encode('utf-8') + b'\x00')
    f.write(value.encode('utf-8') + b'\x00')


def _write_int32(f, key, value):
    f.write(struct.pack('B', _TYPE_INT32))
    f.write(key.encode('utf-8') + b'\x00')
    f.write(struct.pack('<i', value))


def _write_map(f, key, value):
    f.write(struct.pack('B', _TYPE_MAP))
    f.write(key.encode('utf-8') + b'\x00')
    _write_map_contents(f, value)
    f.write(struct.pack('B', _TYPE_END))


def _write_map_contents(f, d):
    for k, v in d.items():
        if isinstance(v, dict):
            _write_map(f, k, v)
        elif isinstance(v, int):
            _write_int32(f, k, v)
        elif isinstance(v, str):
            _write_string(f, k, v)
        else:
            _write_string(f, k, str(v))


def binary_vdf_dump(obj, f):
    """Write nested dicts as a binary VDF file."""
    for k, v in obj.items():
        if isinstance(v, dict):
            _write_map(f, k, v)
        elif isinstance(v, int):
            _write_int32(f, k, v)
        elif isinstance(v, str):
            _write_string(f, k, v)
    f.write(struct.pack('B', _TYPE_END))
