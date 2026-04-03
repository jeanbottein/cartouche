import json
import os
import re
import logging
import glob
from pathlib import Path

from .app import APP_NAME

logger = logging.getLogger(f"{APP_NAME}.configurer")


# ── Variable resolution ──────────────────────────────────────────────────

def resolve_variables(text, config_vars=None):
    """Resolve ${VAR} placeholders in text."""
    if not isinstance(text, str):
        return (text, [])

    unresolved_vars = []

    def check_replacer(match):
        var = match.group(1)
        value = (config_vars or {}).get(var) or os.getenv(var)
        if value is None:
            unresolved_vars.append(var)
            return match.group(0)
        return value

    result = re.sub(r'\$\{(\w+)\}', check_replacer, text)
    return (None, unresolved_vars) if unresolved_vars else (result, [])


# ── Config loading ───────────────────────────────────────────────────────

def _expand_paths(raw: str | list, config_vars: dict) -> list[str]:
    """Resolve variables and glob-expand a path or list of paths."""
    if isinstance(raw, str):
        raw = [raw]
    expanded = []
    for path in raw:
        resolved, _ = resolve_variables(path, config_vars)
        if resolved is None:
            continue
        if '*' in resolved or '?' in resolved:
            expanded.extend(glob.glob(resolved, recursive=True))
        else:
            expanded.append(resolved)
    return expanded


def _resolve_replacement(rep: dict, config_vars: dict) -> dict | None:
    """Resolve variables in a single replacement rule; return None if any are missing."""
    if not isinstance(rep, dict):
        return None
    resolved_pattern, pattern_unresolved = resolve_variables(rep.get('pattern', ''), config_vars)
    resolved_value,   value_unresolved   = resolve_variables(rep.get('value', ''),   config_vars)
    all_unresolved = pattern_unresolved + value_unresolved
    if resolved_pattern is None or resolved_value is None:
        logger.info(f"  {', '.join(all_unresolved)} ignored")
        return None
    return {
        'name':    rep.get('name', 'unnamed'),
        'type':    rep.get('type', 'text'),
        'pattern': resolved_pattern,
        'value':   resolved_value,
        'insert':  rep.get('insert', False),
        'after':   rep.get('after', None),
    }


def _process_file_config(file_config: dict, config_vars: dict) -> dict | None:
    """Resolve one file-config block; return None if nothing usable."""
    paths        = _expand_paths(file_config.get('paths', []), config_vars)
    replacements = [
        r for rep in file_config.get('replacements', [])
        if (r := _resolve_replacement(rep, config_vars)) is not None
    ]
    if not paths or not replacements:
        return None
    return {'paths': paths, 'replacements': replacements}


def load_apps_config(config_vars: dict) -> dict:
    """Load and process configuration from configurer.json."""
    json_path = Path(__file__).resolve().parent / 'configurer.json'
    with open(json_path, 'r') as f:
        apps_config = json.load(f)

    processed = {}
    for app, config in apps_config.items():
        logger.debug(f"Loading {app} configuration...")
        files = [
            fc for raw in config.get('files', [])
            if (fc := _process_file_config(raw, config_vars)) is not None
        ]
        if files:
            processed[app] = {'files': files}
    return processed


# ── Text replacements ────────────────────────────────────────────────────

def _insert_after_marker(content: str, value: str, after: str) -> tuple[str, bool]:
    lines = content.splitlines(True)
    for i, line in enumerate(lines):
        if after in line:
            lines.insert(i + 1, value + '\n')
            return ''.join(lines), True
    return content, False


def _append_to_file(content: str, value: str) -> str:
    if not content.endswith('\n'):
        content += '\n'
    return content + value + '\n'


def apply_text_replacements(content: str, replacements: list) -> tuple[str, bool]:
    """Apply text-based regex replacements, with optional insert-if-missing."""
    modified = False
    for rep in replacements:
        pattern, value = rep['pattern'], rep['value']
        if re.search(pattern, content):
            content = re.sub(pattern, value, content, count=1)
            logger.info(f"✅ {rep['name']} -> {value}")
            modified = True
        elif rep.get('insert', False):
            after = rep.get('after')
            if after is not None:
                content, inserted = _insert_after_marker(content, value, after)
                if inserted:
                    logger.info(f"✅ {rep['name']} -> inserted after '{after}'")
                    modified = True
                else:
                    logger.warning(f"⚠️  {rep['name']}: marker '{after}' not found in file, skipping insert")
            else:
                content = _append_to_file(content, value)
                logger.info(f"✅ {rep['name']} -> inserted at end of file")
                modified = True
    return content, modified


# ── Hex replacements ─────────────────────────────────────────────────────

def _encode_value_bytes(value: str) -> bytes:
    result = b''
    for char in value:
        result += bytes([int(char)]) if char.isdigit() else char.encode('ascii')
    return result


def _apply_wildcard_hex(content: bytes, rep: dict) -> tuple[bytes, bool]:
    pattern, value = rep['pattern'], rep['value']
    prefix, _, suffix = pattern.partition('?')
    prefix_b = prefix.encode('ascii') if prefix else b''
    suffix_b = suffix.encode('ascii') if suffix else b''
    total_len = len(prefix_b) + 1 + len(suffix_b)

    for i in range(len(content) - total_len + 1):
        prefix_end = i + len(prefix_b)
        suffix_start = prefix_end + 1
        if (content[i:prefix_end] == prefix_b
                and content[suffix_start:suffix_start + len(suffix_b)] == suffix_b):
            replacement = _encode_value_bytes(value)
            logger.info(f"✅ {rep['name']} -> {value}")
            return content[:i] + replacement + content[i + total_len:], True
    return content, False


def apply_hex_replacements(content: bytes, replacements: list) -> tuple[bytes, bool]:
    """Apply hexadecimal replacements for binary files."""
    modified = False
    for rep in replacements:
        pattern, value = rep['pattern'], rep['value']
        if '?' in pattern:
            if pattern.count('?') > 1:
                logger.warning(f"  Skipping hex replacement with unsupported multi-wildcard pattern: {rep['name']!r}")
                continue
            content, changed = _apply_wildcard_hex(content, rep)
            modified = modified or changed
        else:
            pattern_bytes = pattern.encode('ascii')
            if pattern_bytes in content:
                content = content.replace(pattern_bytes, value.encode('ascii'))
                logger.info(f"✅ {rep['name']} -> {value}")
                modified = True
    return content, modified


# ── File mutation ────────────────────────────────────────────────────────

def modify_file(file_path: str, replacements: list) -> None:
    if not os.path.exists(file_path) or not replacements:
        return

    logger.info(f"🤖 {file_path} found")
    text_reps = [r for r in replacements if r.get('type') != 'hexadecimal']
    hex_reps  = [r for r in replacements if r.get('type') == 'hexadecimal']

    if text_reps:
        with open(file_path, 'r') as f:
            content = f.read()
        content, modified = apply_text_replacements(content, text_reps)
        if modified:
            with open(file_path, 'w') as f:
                f.write(content)

    if hex_reps:
        with open(file_path, 'rb') as f:
            content = f.read()
        content, modified = apply_hex_replacements(content, hex_reps)
        if modified:
            with open(file_path, 'wb') as f:
                f.write(content)


def run(config_vars: dict) -> None:
    """Main configuration runner."""
    apps_config = load_apps_config(config_vars)
    logger.info("🤖 Configuring apps...")
    for app_name, config in apps_config.items():
        for file_config in config.get('files', []):
            for path in file_config.get('paths', []):
                modify_file(path, file_config.get('replacements', []))
