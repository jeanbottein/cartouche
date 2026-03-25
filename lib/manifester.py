import os
import json
import subprocess
import difflib
import logging
import sys
import platform

logger = logging.getLogger("manifester")

relative_path = True

manifest_filename= "launch_manifest.json"
manifests_filename= "manifests.json"

if sys.platform.startswith("linux") or sys.platform == "darwin":
    _machine = platform.machine().lower()
    if "arm" in _machine or "aarch64" in _machine:
        EXEC_FILTERS = ["arm64", "aarch64", "x86-64", "x86", ""]
    elif "64" in _machine or "x86_64" in _machine or "amd64" in _machine:
        EXEC_FILTERS = ["x86-64", "x86", "arm64", "aarch64", ""]
    else:
        EXEC_FILTERS = ["x86", "x86-64", "arm64", "aarch64", ""]
else:
    EXEC_FILTERS = [""]

if sys.platform.startswith("win"):
    _win_machine = platform.machine().lower()
    if "arm" in _win_machine:
        WIN_ARCH_GROUPS = [
            ["arm64", "aarch64"],
            ["x64", "amd64", "win64", "64"],
            ["x86", "win32", "32"],
        ]
    elif "64" in _win_machine or "amd64" in _win_machine or "x86_64" in _win_machine:
        WIN_ARCH_GROUPS = [
            ["x64", "amd64", "win64", "64"],
            ["x86", "win32", "32"],
            ["arm64", "aarch64"],
        ]
    else:
        WIN_ARCH_GROUPS = [
            ["x86", "win32", "32"],
            ["x64", "amd64", "win64", "64"],
            ["arm64", "aarch64"],
        ]
else:
    WIN_ARCH_GROUPS = []

def run_find_exe(cmd):
    try:
        executables = subprocess.check_output(cmd, shell=True, text=True).splitlines()
    except subprocess.CalledProcessError:
        return None
    if not executables:
        return None
    return executables

def find_best(game_dir, files):
    if files is None:
        return None
    if len(files) == 1:
        return files[0]

    folder_base = os.path.basename(game_dir)
    best_match = None
    highest_score = 0.0

    for file in files:
        file_base = os.path.splitext(os.path.basename(file))[0]
        score = difflib.SequenceMatcher(None, folder_base, file_base).ratio()
        if score > highest_score:
            highest_score = score
            best_match = file

    return best_match

def _get_bin_posix(game_dir, maxdepth, arch_filter=""):
    if arch_filter == "x86":
        cmd = f"""find "{game_dir}" -maxdepth {maxdepth} -type f -executable | \
            grep -E "\\.x86$" | grep -v "x86_64" """
    else:
        cmd = f"""find "{game_dir}" -maxdepth {maxdepth} -type f -executable -exec file {{}} + | \
            grep executable | grep "{arch_filter}"  | sed "s#:.*##" | \
            grep -v "/java/" | grep -v "/jre/" | grep -v "/lib/" """
    return find_best(game_dir, run_find_exe(cmd))


def _get_bin_windows(game_dir, maxdepth):
    root_depth = game_dir.rstrip(os.sep).count(os.sep)
    candidates = []
    for dirpath, dirnames, filenames in os.walk(game_dir):
        depth = dirpath.rstrip(os.sep).count(os.sep) - root_depth
        if depth >= maxdepth:
            dirnames[:] = []
        for name in filenames:
            if not name.lower().endswith(".exe"):
                continue
            full = os.path.join(dirpath, name)
            candidates.append(full)
    if not candidates:
        return None

    for group in WIN_ARCH_GROUPS or [[]]:
        if group:
            group_candidates = []
            for path in candidates:
                base = os.path.basename(path).lower()
                for token in group:
                    if token in base:
                        group_candidates.append(path)
                        break
            if group_candidates:
                return find_best(game_dir, group_candidates)

    return find_best(game_dir, candidates)


def get_bin(game_dir, maxdepth, arch_filter=""):
    if sys.platform.startswith("win"):
        return _get_bin_windows(game_dir, maxdepth)
    return _get_bin_posix(game_dir, maxdepth, arch_filter)


def get_target(game_dir):
    real_game_dir=get_real_first_path(game_dir)
    for depth in range(1, 4):
        for arch in EXEC_FILTERS:
            exe = get_bin(real_game_dir, depth, arch)
            if exe is not None:
                return exe

    return None

def format_path(full_path, base_path):
    if relative_path:
        return os.path.relpath(full_path, base_path)
    return full_path

def get_real_first_path(game_dir):
    game_dir = os.path.normpath(game_dir)
    entries = [entry for entry in os.listdir(game_dir) if not entry.startswith('.') and entry != manifest_filename]

    # Filter out directories and files
    directories = [entry for entry in entries if os.path.isdir(os.path.join(game_dir, entry))]
    files = [entry for entry in entries if os.path.isfile(os.path.join(game_dir, entry))]

    if len(directories) == 1 and len(files) == 0:
        return get_real_first_path(os.path.join(game_dir, directories[0]))

    return game_dir

def get_title(game_dir):
    return os.path.basename(get_real_first_path(game_dir))


def write_manifest(manifest_path, manifest):
    try:
        with open(manifest_path, 'w') as f:
            json.dump(manifest, f, indent=4)
        logger.info(f"✅ {os.path.basename(os.path.dirname(manifest_path))} manifest created")
    except Exception as e:
        logger.error(f"❌ Error creating manifest in {os.path.basename(os.path.dirname(manifest_path))}: {str(e)}")


def _os_tag():
    if sys.platform.startswith("linux"):
        return "linux"
    if sys.platform.startswith("win"):
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    return "other"


def _arch_tag():
    m = platform.machine().lower()
    if "arm" in m or "aarch64" in m:
        return "arm64"
    if "64" in m or "x86_64" in m or "amd64" in m:
        return "x86_64"
    if "86" in m or "i386" in m or "i686" in m:
        return "x86"
    return "other"


def _arch_from_filter(arch_filter, exe_path):
    f = (arch_filter or "").lower()
    if f in ("arm64", "aarch64"):
        return "arm64"
    if f in ("x86-64", "x86_64"):
        return "x86_64"
    if f == "x86":
        return "x86"
    base = os.path.basename(exe_path).lower()
    if "arm64" in base or "aarch64" in base:
        return "arm64"
    if "x86_64" in base or "amd64" in base or "64" in base:
        return "x86_64"
    if "x86" in base or "32" in base:
        return "x86"
    return _arch_tag()


def _pick_save_path(spec):
    if isinstance(spec, str):
        return spec
    if isinstance(spec, list):
        os_tag = _os_tag()
        same_os = [s for s in spec if (s.get("os") or "").lower() == os_tag]
        if not same_os:
            same_os = [s for s in spec if not (s.get("os") or "").strip() or (s.get("os") or "").lower() == "any"]
        pool = same_os or spec
        for entry in pool:
            path = entry.get("path") or entry.get("savePath") or entry.get("value") or ""
            if path:
                return path
    return ""


def _pick_target_entry(manifest):
    targets = manifest.get("targets") or []
    if not targets:
        return None
    os_tag = _os_tag()
    arch_tag = _arch_tag()

    same_os = [t for t in targets if (t.get("os") or "").lower() == os_tag]
    if not same_os:
        same_os = [t for t in targets if not (t.get("os") or "").strip() or (t.get("os") or "").lower() == "any"]
    pool = same_os or targets

    same_arch = [t for t in pool if (t.get("arch") or "").lower() == arch_tag]
    if not same_arch:
        same_arch = [t for t in pool if not (t.get("arch") or "").strip() or (t.get("arch") or "").lower() == "any"]
    pool = same_arch or pool

    return pool[0]


def _collect_targets_for_manifest(game_dir):
    os_tag = _os_tag()
    real_game_dir = get_real_first_path(game_dir)
    targets = []

    if sys.platform.startswith("win"):
        exe = get_target(game_dir)
        if exe:
            targets.append(
                {
                    "os": os_tag,
                    "arch": _arch_tag(),
                    "target": format_path(exe, game_dir),
                    "startIn": format_path(os.path.dirname(exe), game_dir),
                    "launchOptions": "",
                }
            )
        return targets

    seen = set()
    for arch_filter in EXEC_FILTERS:
        exe = get_bin(real_game_dir, 3, arch_filter)
        if not exe or exe in seen:
            continue
        seen.add(exe)
        arch = _arch_from_filter(arch_filter, exe)
        targets.append(
            {
                "os": os_tag,
                "arch": arch,
                "target": format_path(exe, game_dir),
                "startIn": format_path(os.path.dirname(exe), game_dir),
                "launchOptions": "",
            }
        )
    return targets


def createManifest(game_dir):
    manifest_path = os.path.join(game_dir, manifest_filename)
    if(os.path.exists(manifest_path)):
        return
    targets = _collect_targets_for_manifest(game_dir)
    if not targets:
        logger.info(f"❌ {os.path.basename(game_dir)} no executable found")
        return None
    logger.info(f"✅ {os.path.basename(game_dir)} executable detected")
    manifest = {
        "title": get_title(game_dir),
        "targets": targets,
        "savePath": [
            {
                "os": _os_tag(),
                "path": "",
            }
        ],
    }

    write_manifest(manifest_path, manifest)


def find_manifests(game_dir):
    manifests = []
    for root, dirs, files in os.walk(game_dir):
        if manifest_filename in files:
            manifest_path = os.path.join(root, manifest_filename)
            manifests.append(manifest_path)
    return manifests

def load_and_ajust_manifest(manifest_path):
    subfolder = os.path.dirname(manifest_path)
    with open(manifest_path, 'r') as f:
        manifest = json.load(f)

    if 'targets' in manifest:
        entry = _pick_target_entry(manifest)
        if not entry:
            return None
        target_path = entry.get('target') or ''
        start_in_path = entry.get('startIn') or os.path.dirname(target_path)
        save_spec = manifest.get("savePath", "")
        result = {
            "title": manifest.get("title") or os.path.basename(os.path.dirname(manifest_path)),
            "target": os.path.normpath(os.path.join(subfolder, target_path)),
            "startIn": os.path.normpath(os.path.join(subfolder, start_in_path)),
            "launchOptions": entry.get("launchOptions", ""),
            "savePath": _pick_save_path(save_spec),
        }
        if "steamgriddb_id" in manifest:
            result["steamgriddb_id"] = manifest["steamgriddb_id"]
        return result

    if 'target' in manifest:
        manifest['target'] = os.path.normpath(os.path.join(subfolder, manifest['target']))
    if 'startIn' in manifest:
        manifest['startIn'] = os.path.normpath(os.path.join(subfolder, manifest['startIn']))
    save_spec = manifest.get("savePath", "")
    manifest["savePath"] = _pick_save_path(save_spec)
    return manifest

def create_main_manifest(games_dir):
    manifests = find_manifests(games_dir)
    main_manifest = []
    for manifest_path in manifests:
        manifest = load_and_ajust_manifest(manifest_path)
        if not manifest:
            continue
        main_manifest.append(manifest)
        logger.info(f"✅ {manifest['title']}")

    main_manifest_path = os.path.join(games_dir, manifests_filename)
    with open(main_manifest_path, 'w') as f:
        json.dump(main_manifest, f, indent=4)

    logger.info(f"✅ Manifests file created at: {main_manifest_path}")

def run(config: dict):
    games_dir = config.get('FREEGAMES_PATH')

    if not games_dir or not os.path.isdir(games_dir):
        logger.warning("🤖 FREEGAMES_PATH not configured or invalid")
        return

    logger.info(f"🤖 looking for games in {games_dir}")
    for item in os.listdir(games_dir):
        item_path = os.path.join(games_dir, item)
        if os.path.isdir(item_path):
            createManifest(item_path)

    create_main_manifest(games_dir)
