import json
import os
import re
import logging
import glob
from pathlib import Path

logger = logging.getLogger("configurer")

def resolve_variables(text, config_vars=None):
    """Resolve ${VAR} placeholders in text"""
    if not isinstance(text, str):
        return text
    
    # Find all unresolved variables
    unresolved_vars = []
    def check_replacer(match):
        var = match.group(1)
        value = (config_vars or {}).get(var) or os.getenv(var)
        if value is None:
            unresolved_vars.append(var)
            return match.group(0)  # Keep original placeholder
        return value
    
    result = re.sub(r'\$\{(\w+)\}', check_replacer, text)
    return (None, unresolved_vars) if unresolved_vars else (result, [])

def load_apps_config(config_vars):
    """Load and process configuration from JSON file"""
    json_path = Path(__file__).resolve().parent / 'configurer.json'
    with open(json_path, 'r') as f:
        apps_config = json.load(f)

    processed = {}
    for app, config in apps_config.items():
        logger.debug(f"Loading {app} configuration...")
        files = []
        for file_config in config.get('files', []):
            # Resolve paths
            raw_paths = file_config.get('paths', [])
            if isinstance(raw_paths, str):
                raw_paths = [raw_paths]
            
            resolved_paths = []
            for path in raw_paths:
                resolved, unresolved_vars = resolve_variables(path, config_vars)
                if resolved is not None:  # Only add if variables were resolved
                    # Expand wildcards if present
                    if '*' in resolved or '?' in resolved:
                        expanded_paths = glob.glob(resolved, recursive=True)
                        resolved_paths.extend(expanded_paths)
                    else:
                        resolved_paths.append(resolved)
            
            if not resolved_paths:
                continue
                
            # Process replacements
            replacements = []
            for rep in file_config.get('replacements', []):
                if isinstance(rep, dict):
                    resolved_pattern, pattern_unresolved = resolve_variables(rep.get('pattern', ''), config_vars)
                    resolved_value, value_unresolved = resolve_variables(rep.get('value', ''), config_vars)
                    
                    # Skip replacement if any variables are unresolved
                    all_unresolved = pattern_unresolved + value_unresolved
                    if resolved_pattern is None or resolved_value is None:
                        logger.info(f"⚠️  Skipping {rep.get('name', 'unnamed')} - undefined variables: {', '.join(all_unresolved)}")
                        continue
                    
                    processed_rep = {
                        'name': rep.get('name', 'unnamed'),
                        'type': rep.get('type', 'text'),
                        'pattern': resolved_pattern,
                        'value': resolved_value,
                        'insert': rep.get('insert', False),
                        'after': rep.get('after', None)
                    }
                    replacements.append(processed_rep)
            
            if replacements:
                files.append({'paths': resolved_paths, 'replacements': replacements})
        
        if files:
            processed[app] = {'files': files}
    
    return processed

def apply_text_replacements(content, replacements):
    """Apply text-based regex replacements, with optional insert-if-missing"""
    modified = False
    for rep in replacements:
        pattern, value = rep['pattern'], rep['value']
        if re.search(pattern, content):
            content = re.sub(pattern, value, content)
            logger.info(f"✅ {rep['name']} -> {value}")
            modified = True
        elif rep.get('insert', False):
            after = rep.get('after')
            if after is not None:
                # Insert after the marker line
                lines = content.splitlines(True)
                inserted = False
                for i, line in enumerate(lines):
                    if after in line:
                        # Insert value right after this line
                        newline = value + '\n'
                        lines.insert(i + 1, newline)
                        inserted = True
                        break
                if inserted:
                    content = ''.join(lines)
                    logger.info(f"✅ {rep['name']} -> inserted after '{after}'")
                    modified = True
                else:
                    logger.warning(f"⚠️  {rep['name']}: marker '{after}' not found in file, skipping insert")
            else:
                # Append at end of file
                if not content.endswith('\n'):
                    content += '\n'
                content += value + '\n'
                logger.info(f"✅ {rep['name']} -> inserted at end of file")
                modified = True
    return content, modified

def apply_hex_replacements(content, replacements):
    """Apply hexadecimal replacements for binary files"""
    modified = False
    for rep in replacements:
        pattern, value = rep['pattern'], rep['value']
        
        # Convert ASCII pattern to hex bytes
        if '?' in pattern:
            # Wildcard pattern - find and replace
            prefix = pattern.split('?')[0]
            suffix = pattern.split('?')[-1] if '?' in pattern else ''
            
            prefix_bytes = prefix.encode('ascii') if prefix else b''
            suffix_bytes = suffix.encode('ascii') if suffix else b''
            
            # Simple search for pattern
            pattern_len = len(prefix_bytes) + 1 + len(suffix_bytes)
            for i in range(len(content) - pattern_len + 1):
                if (content[i:i+len(prefix_bytes)] == prefix_bytes and 
                    content[i+len(prefix_bytes)+1:i+len(prefix_bytes)+1+len(suffix_bytes)] == suffix_bytes):
                    
                    # Convert value (handle decimal numbers)
                    value_bytes = b''
                    for char in value:
                        if char.isdigit():
                            value_bytes += bytes([int(char)])
                        else:
                            value_bytes += char.encode('ascii')
                    
                    content = content[:i] + value_bytes + content[i + pattern_len:]
                    logger.info(f"✅ {rep['name']} -> {value}")
                    modified = True
                    break
        else:
            # Exact pattern match
            pattern_bytes = pattern.encode('ascii')
            if pattern_bytes in content:
                value_bytes = value.encode('ascii')
                content = content.replace(pattern_bytes, value_bytes)
                logger.info(f"✅ {rep['name']} -> {value}")
                modified = True
    
    return content, modified

def modify_file(file_path, replacements):
    """Modify a single file with given replacements"""
    if not os.path.exists(file_path):
        #logger.info(f"ℹ️  {file_path} does not exist")
        return
    
    if not replacements:
        return
    
    logger.info(f"🤖 {file_path} found")

    # Separate by type
    text_reps = [r for r in replacements if r.get('type') != 'hexadecimal']
    hex_reps = [r for r in replacements if r.get('type') == 'hexadecimal']
    
    # Handle text files
    if text_reps:
        with open(file_path, 'r') as f:
            content = f.read()
        
        content, modified = apply_text_replacements(content, text_reps)
        
        if modified:
            with open(file_path, 'w') as f:
                f.write(content)
    
    # Handle binary files
    if hex_reps:
        with open(file_path, 'rb') as f:
            content = f.read()
        
        content, modified = apply_hex_replacements(content, hex_reps)
        
        if modified:
            with open(file_path, 'wb') as f:
                f.write(content)

def run(config_vars):
    """Main configuration runner"""
    apps_config = load_apps_config(config_vars)
    
    logger.info("🤖 Configuring apps...")
    for app_name, config in apps_config.items():
        for file_config in config.get('files', []):
            paths = file_config.get('paths', [])
            replacements = file_config.get('replacements', [])
            
            for path in paths:
                modify_file(path, replacements)
