//! Game packages: one `Foo.cartridge.tar` holding a game folder and its
//! metadata, to copy a game between machines and unpack it later.
//!
//! Layout, every path under `Foo/`:
//!   .cartridge/package.json    first: what the package is and its total size
//!   .cartridge/...             game.json and artwork, readable without the rest
//!   <game files>               sorted; Unix modes and symlinks kept
//!   .cartridge/checksums.json  last: crc32 per file, so packing reads files once
//!
//! It is a plain tar: `tar xf` works without •cartridge. Packing and unpacking
//! write to hidden `.…part` names and rename at the end, so a half-written
//! package or folder never shows up as a game.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::app::data_dir_name;
use super::models::{to_pretty_json, Game, GAME_JSON};
use super::paths;

pub const EXTENSION: &str = ".cartridge.tar";
const FORMAT: u32 = 1;
const PACKAGE_JSON: &str = "package.json";
const CHECKSUMS_JSON: &str = "checksums.json";
const UNPACKED_FROM_JSON: &str = "unpacked_from.json";
/// Files we write ourselves; never copied from a game folder into a package.
const GENERATED: [&str; 3] = [PACKAGE_JSON, CHECKSUMS_JSON, UNPACKED_FROM_JSON];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Identifies this package build; recorded on unpack to recognise the folder.
    pub id: String,
    pub folder_name: String,
    pub title: String,
    /// Bytes of every file in the package.
    pub size: u64,
    pub files: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UnpackedFrom {
    id: String,
}

/// Byte progress of a pack/unpack, shared between the worker and the UI.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    // Wired to the UI's cancel action with downloads; tests use it already.
    #[allow(dead_code)]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn start(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
    }

    fn advance(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// A reader that feeds a crc and `progress`, and fails once cancelled.
struct Tracked<'a, R> {
    inner: R,
    progress: &'a Progress,
    crc: crc32fast::Hasher,
    bytes: u64,
}

impl<'a, R: Read> Tracked<'a, R> {
    fn new(inner: R, progress: &'a Progress) -> Self {
        Tracked {
            inner,
            progress,
            crc: crc32fast::Hasher::new(),
            bytes: 0,
        }
    }

    fn checksum(self) -> String {
        format!("{:08x}", self.crc.finalize())
    }
}

impl<R: Read> Read for Tracked<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.progress.is_cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.crc.update(&buf[..n]);
        self.bytes += n as u64;
        self.progress.advance(n as u64);
        Ok(n)
    }
}

// ── Walking a game folder ────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File(u64),
    Symlink(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Relative to the walked folder.
    pub rel: PathBuf,
    pub kind: Kind,
    mode: u32,
    mtime: u64,
}

impl Item {
    pub fn file_size(&self) -> u64 {
        match self.kind {
            Kind::File(size) => size,
            _ => 0,
        }
    }
}

#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(meta: &fs::Metadata) -> u32 {
    if meta.is_dir() {
        0o755
    } else {
        0o644
    }
}

fn unix_secs(time: io::Result<SystemTime>) -> u64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Every dir, regular file and symlink under `root`, sorted, parents before
/// children. Symlinks are not followed; other file types are skipped.
pub fn walk(root: &Path) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    visit(root, Path::new(""), &mut items)?;
    Ok(items)
}

// A loop with recursion: a pipeline can't propagate io errors from nested walks readably.
fn visit(root: &Path, rel: &Path, items: &mut Vec<Item>) -> io::Result<()> {
    let mut names = fs::read_dir(root.join(rel))?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        let rel = rel.join(name);
        let path = root.join(&rel);
        let meta = fs::symlink_metadata(&path)?;
        let kind = match meta.file_type() {
            t if t.is_symlink() => Kind::Symlink(fs::read_link(&path)?),
            t if t.is_dir() => Kind::Dir,
            t if t.is_file() => Kind::File(meta.len()),
            _ => continue,
        };
        let is_dir = kind == Kind::Dir;
        items.push(Item {
            rel: rel.clone(),
            kind,
            mode: mode_of(&meta),
            mtime: unix_secs(meta.modified()),
        });
        if is_dir {
            visit(root, &rel, items)?;
        }
    }
    Ok(())
}

fn is_generated(rel: &Path) -> bool {
    rel.parent() == Some(Path::new(&data_dir_name()))
        && GENERATED
            .iter()
            .any(|g| rel.file_name() == Some(g.as_ref()))
}

/// `a/b/c` on every OS (checksum keys).
fn key_of(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

// ── Locations ────────────────────────────────────────────────────────────

pub fn package_path(games_dir: &Path, folder_name: &str) -> PathBuf {
    games_dir.join(format!("{folder_name}{EXTENSION}"))
}

/// Visible `*.cartridge.tar` files in `games_dir`, sorted.
pub fn list(games_dir: &Path) -> Vec<PathBuf> {
    paths::sorted_entries(games_dir)
        .into_iter()
        .filter(|name| !name.starts_with('.') && name.ends_with(EXTENSION))
        .map(|name| games_dir.join(name))
        .filter(|p| p.is_file())
        .collect()
}

fn staging_name(name: &str) -> String {
    format!(".{name}.part")
}

/// A single plain folder name (no separators, `..` or hidden name).
fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !name.starts_with('.')
}

/// The package id a game folder was unpacked from.
pub fn unpacked_from(game_dir: &Path) -> Option<String> {
    let path = game_dir.join(data_dir_name()).join(UNPACKED_FROM_JSON);
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<UnpackedFrom>(&text)
        .ok()
        .map(|u| u.id)
}

// ── Packing ──────────────────────────────────────────────────────────────

fn new_id(folder_name: &str, size: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let mut crc = crc32fast::Hasher::new();
    crc.update(folder_name.as_bytes());
    crc.update(&size.to_le_bytes());
    crc.update(&now.subsec_nanos().to_le_bytes());
    format!("{:x}-{:08x}", now.as_secs(), crc.finalize())
}

fn now_secs() -> u64 {
    unix_secs(Ok(SystemTime::now()))
}

fn append_bytes<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(now_secs());
    builder.append_data(&mut header, path, bytes)
}

/// Append one walked item; returns its checksum for regular files.
fn append_item<W: Write>(
    builder: &mut tar::Builder<W>,
    game_dir: &Path,
    path: &Path,
    item: &Item,
    progress: &Progress,
) -> Result<Option<String>, String> {
    let fail = |e: io::Error| format!("{}: {e}", item.rel.display());
    let mut header = tar::Header::new_gnu();
    header.set_mode(item.mode);
    header.set_mtime(item.mtime);
    match &item.kind {
        Kind::Dir => {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            builder
                .append_data(&mut header, path, io::empty())
                .map_err(fail)?;
            Ok(None)
        }
        Kind::Symlink(target) => {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            builder
                .append_link(&mut header, path, target)
                .map_err(fail)?;
            Ok(None)
        }
        Kind::File(size) => {
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(*size);
            let file = File::open(game_dir.join(&item.rel)).map_err(fail)?;
            let mut reader = Tracked::new(file.take(*size), progress);
            builder
                .append_data(&mut header, path, &mut reader)
                .map_err(fail)?;
            if reader.bytes != *size {
                return Err(format!("{}: changed while packing", item.rel.display()));
            }
            Ok(Some(reader.checksum()))
        }
    }
}

/// Stream `game` as a package into `out`. The total size is known before the
/// first byte, so `progress` is exact from the start.
pub fn pack_to(game: &Game, out: impl Write, progress: &Progress) -> Result<Manifest, String> {
    let data = PathBuf::from(data_dir_name());
    let items: Vec<Item> = walk(&game.dir)
        .map_err(|e| format!("{}: {e}", game.dir.display()))?
        .into_iter()
        .filter(|i| !is_generated(&i.rel))
        .collect();
    // Metadata first so a reader can stop early.
    let (meta_items, game_items): (Vec<&Item>, Vec<&Item>) =
        items.iter().partition(|i| i.rel.starts_with(&data));
    let size = items.iter().map(Item::file_size).sum();
    let manifest = Manifest {
        format: FORMAT,
        id: new_id(&game.folder_name, size),
        folder_name: game.folder_name.clone(),
        title: game.title.clone(),
        size,
        files: items
            .iter()
            .filter(|i| matches!(i.kind, Kind::File(_)))
            .count(),
    };
    progress.start(size);

    let prefix = Path::new(&game.folder_name);
    let io_fail = |e: io::Error| e.to_string();
    let mut builder = tar::Builder::new(out);
    append_bytes(
        &mut builder,
        &prefix.join(&data).join(PACKAGE_JSON),
        to_pretty_json(&manifest).as_bytes(),
    )
    .map_err(io_fail)?;
    let mut checksums = BTreeMap::new();
    for item in meta_items.into_iter().chain(game_items) {
        let path = prefix.join(&item.rel);
        if let Some(crc) = append_item(&mut builder, &game.dir, &path, item, progress)? {
            checksums.insert(key_of(&item.rel), crc);
        }
    }
    append_bytes(
        &mut builder,
        &prefix.join(&data).join(CHECKSUMS_JSON),
        to_pretty_json(&checksums).as_bytes(),
    )
    .map_err(io_fail)?;
    builder
        .into_inner()
        .and_then(|mut w| w.flush())
        .map_err(io_fail)?;
    Ok(manifest)
}

/// Pack an installed game into `package` (via a hidden `.part` file).
pub fn pack(game: &Game, package: &Path, progress: &Progress) -> Result<Manifest, String> {
    if !game.is_installed() {
        return Err(format!("{} is not installed", game.title));
    }
    if package.exists() {
        return Err(format!("{} already exists", package.display()));
    }
    let name = package.file_name().unwrap_or_default().to_string_lossy();
    let staging = package.with_file_name(staging_name(&name));
    let result = File::create(&staging)
        .map_err(|e| format!("{}: {e}", staging.display()))
        .and_then(|file| pack_to(game, BufWriter::new(file), progress))
        .and_then(|manifest| {
            fs::rename(&staging, package)
                .map(|()| manifest)
                .map_err(|e| format!("{}: {e}", package.display()))
        });
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

// ── Reading ──────────────────────────────────────────────────────────────

type Entries<'a, R> = tar::Entries<'a, R>;

/// The manifest, which must be the first entry.
fn read_manifest<R: Read>(entries: &mut Entries<'_, R>) -> Result<Manifest, String> {
    let not_a_package = || "Not a •cartridge package (no package.json first)".to_string();
    let mut entry = entries
        .next()
        .ok_or_else(not_a_package)?
        .map_err(|e| e.to_string())?;
    let path = entry.path().map_err(|e| e.to_string())?.into_owned();
    if !path.ends_with(Path::new(&data_dir_name()).join(PACKAGE_JSON)) {
        return Err(not_a_package());
    }
    let manifest: Manifest = serde_json::from_reader(&mut entry).map_err(|e| e.to_string())?;
    if manifest.format > FORMAT {
        return Err(format!("Package format {} is too new", manifest.format));
    }
    let expected = Path::new(&manifest.folder_name)
        .join(data_dir_name())
        .join(PACKAGE_JSON);
    if !is_plain_name(&manifest.folder_name) || path != expected {
        return Err(format!(
            "Bad folder name in package: {:?}",
            manifest.folder_name
        ));
    }
    Ok(manifest)
}

/// Manifest and `game.json` text of a package, reading only its first entries.
pub fn read_meta(package: &Path) -> Result<(Manifest, Option<String>), String> {
    let file = File::open(package).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(BufReader::new(file));
    let mut entries = archive.entries().map_err(|e| e.to_string())?;
    let manifest = read_manifest(&mut entries)?;
    let data = Path::new(&manifest.folder_name).join(data_dir_name());
    let game_json = data.join(GAME_JSON);
    for entry in entries {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path == game_json {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
            return Ok((manifest, Some(text)));
        }
        if !path.starts_with(&data) {
            break;
        }
    }
    Ok((manifest, None))
}

/// A file of the package's `.cartridge/` (artwork), reading only the first entries.
pub fn read_data_file(package: &Path, file_name: &str) -> Option<Vec<u8>> {
    let file = File::open(package).ok()?;
    let mut archive = tar::Archive::new(BufReader::new(file));
    let mut entries = archive.entries().ok()?;
    let manifest = read_manifest(&mut entries).ok()?;
    let data = Path::new(&manifest.folder_name).join(data_dir_name());
    let wanted = data.join(file_name);
    for entry in entries {
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.into_owned();
        if path == wanted {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).ok()?;
            return Some(bytes);
        }
        if !path.starts_with(&data) {
            return None;
        }
    }
    None
}

// ── Unpacking ────────────────────────────────────────────────────────────

/// `root/rel`, refusing anything that could land outside `root`: non-plain
/// components, or a parent that is a symlink (written by an earlier entry).
fn safe_join(root: &Path, rel: &Path) -> Result<PathBuf, String> {
    let plain = rel.components().all(|c| matches!(c, Component::Normal(_)));
    let through_symlink = rel
        .ancestors()
        .skip(1)
        .filter(|a| !a.as_os_str().is_empty())
        .any(|a| paths::is_symlink(&root.join(a)));
    if !plain || through_symlink {
        return Err(format!("Unsafe path in package: {}", rel.display()));
    }
    Ok(root.join(rel))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn make_symlink(target: &Path, link: &Path) -> io::Result<()> {
    log::warn!(
        "Skipping symlink {} -> {}",
        link.display(),
        target.display()
    );
    Ok(())
}

/// Write one regular file entry; returns (bytes, checksum).
fn write_file<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    dest: &Path,
    progress: &Progress,
) -> io::Result<(u64, String)> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(dest)?;
    let header = entry.header().clone();
    let mut reader = Tracked::new(entry, progress);
    io::copy(&mut reader, &mut file)?;
    let mtime = UNIX_EPOCH + Duration::from_secs(header.mtime().unwrap_or(0));
    file.set_modified(mtime)?;
    set_mode(dest, header.mode().unwrap_or(0o644))?;
    Ok((reader.bytes, reader.checksum()))
}

/// Hand every entry after the manifest to `handle` (with its path relative to
/// the game folder), then check the regular files' checksums it returns
/// against the trailing `checksums.json`.
fn for_each_checked<R: Read>(
    entries: Entries<'_, R>,
    manifest: &Manifest,
    mut handle: impl FnMut(&Path, &mut tar::Entry<'_, R>) -> Result<Option<String>, String>,
) -> Result<(), String> {
    let prefix = Path::new(&manifest.folder_name);
    let checksums_rel = Path::new(&data_dir_name()).join(CHECKSUMS_JSON);
    let mut actual = BTreeMap::new();
    let mut expected: Option<BTreeMap<String, String>> = None;
    for entry in entries {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let rel = path
            .strip_prefix(prefix)
            .map_err(|_| format!("Entry outside {}: {}", prefix.display(), path.display()))?
            .to_path_buf();
        if rel == checksums_rel {
            expected = Some(serde_json::from_reader(&mut entry).map_err(|e| e.to_string())?);
            continue;
        }
        if let Some(crc) = handle(&rel, &mut entry)? {
            actual.insert(key_of(&rel), crc);
        }
    }
    if expected != Some(actual) {
        return Err("Package is incomplete or corrupted".into());
    }
    Ok(())
}

/// Extract every entry into `staging`, verified. Returns the bytes of the
/// game's own files.
fn extract<R: Read>(
    entries: Entries<'_, R>,
    manifest: &Manifest,
    staging: &Path,
    progress: &Progress,
) -> Result<u64, String> {
    let data = PathBuf::from(data_dir_name());
    let mut game_bytes = 0;
    for_each_checked(entries, manifest, |rel, entry| {
        let dest = safe_join(staging, rel)?;
        let fail = |e: io::Error| format!("{}: {e}", rel.display());
        match entry.header().entry_type() {
            tar::EntryType::Directory => fs::create_dir_all(&dest).map_err(fail).map(|()| None),
            tar::EntryType::Regular => {
                let (bytes, crc) = write_file(entry, &dest, progress).map_err(fail)?;
                if !rel.starts_with(&data) {
                    game_bytes += bytes;
                }
                Ok(Some(crc))
            }
            tar::EntryType::Symlink => {
                let target = entry
                    .link_name()
                    .map_err(fail)?
                    .ok_or_else(|| format!("{}: symlink without target", rel.display()))?;
                make_symlink(&target, &dest).map_err(fail).map(|()| None)
            }
            other => Err(format!("{}: unsupported entry {other:?}", rel.display())),
        }
    })?;
    Ok(game_bytes)
}

/// Read a whole package stream and check it, writing nothing.
pub fn verify_from(input: impl Read, progress: &Progress) -> Result<Manifest, String> {
    let mut archive = tar::Archive::new(input);
    let mut entries = archive.entries().map_err(|e| e.to_string())?;
    let manifest = read_manifest(&mut entries)?;
    progress.start(manifest.size);
    for_each_checked(entries, &manifest, |rel, entry| {
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("Unsafe path in package: {}", rel.display()));
        }
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Ok(None);
        }
        let mut reader = Tracked::new(entry, progress);
        io::copy(&mut reader, &mut io::sink()).map_err(|e| e.to_string())?;
        Ok(Some(reader.checksum()))
    })?;
    Ok(manifest)
}

/// Record the package id, and the measured size in `game.json` when present.
fn finish(staging: &Path, dest: &Path, manifest: &Manifest, game_bytes: u64) -> io::Result<()> {
    let data = staging.join(data_dir_name());
    fs::create_dir_all(&data)?;
    let record = UnpackedFrom {
        id: manifest.id.clone(),
    };
    fs::write(data.join(UNPACKED_FROM_JSON), to_pretty_json(&record))?;
    let game_json = data.join(GAME_JSON);
    let Ok(text) = fs::read_to_string(&game_json) else {
        return Ok(());
    };
    let Ok(mut game) = Game::from_json(dest, &text) else {
        return Ok(());
    };
    game.size = Some(game_bytes);
    fs::write(game_json, game.to_json())
}

/// Unpack a package stream into `games_dir/<folder>`. The folder appears only
/// once every file is written and verified.
pub fn unpack_from(
    input: impl Read,
    games_dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let mut archive = tar::Archive::new(input);
    let mut entries = archive.entries().map_err(|e| e.to_string())?;
    let manifest = read_manifest(&mut entries)?;
    let dest = games_dir.join(&manifest.folder_name);
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    let staging = games_dir.join(staging_name(&manifest.folder_name));
    let _ = fs::remove_dir_all(&staging); // leftover from a crash
    fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    progress.start(manifest.size);
    let result = extract(entries, &manifest, &staging, progress).and_then(|game_bytes| {
        finish(&staging, &dest, &manifest, game_bytes)
            .and_then(|()| fs::rename(&staging, &dest))
            .map_err(|e| format!("{}: {e}", dest.display()))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| dest)
}

pub fn unpack(package: &Path, games_dir: &Path, progress: &Progress) -> Result<PathBuf, String> {
    let file = File::open(package).map_err(|e| format!("{}: {e}", package.display()))?;
    unpack_from(BufReader::with_capacity(1 << 20, file), games_dir, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testutil::TempDir;

    fn sample_game(dir: &TempDir) -> Game {
        let root = dir.mkdir("games/Celeste");
        dir.write("games/Celeste/Celeste.bin", b"binary");
        dir.write("games/Celeste/Content/level.dat", &[7u8; 3000]);
        let mut game = Game::skeleton(&root);
        game.title = "Celeste".into();
        dir.write(
            &format!("games/Celeste/{}/{GAME_JSON}", data_dir_name()),
            game.to_json().as_bytes(),
        );
        game
    }

    fn packed(dir: &TempDir) -> (Game, PathBuf, Manifest) {
        let game = sample_game(dir);
        let package = dir.path().join("out").join(format!("Celeste{EXTENSION}"));
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        let manifest = pack(&game, &package, &Progress::default()).unwrap();
        (game, package, manifest)
    }

    #[test]
    fn round_trip_restores_files_and_records_origin() {
        let dir = TempDir::new();
        let (_, package, manifest) = packed(&dir);
        let target = dir.mkdir("target");
        let progress = Progress::default();
        let dest = unpack(&package, &target, &progress).unwrap();
        assert_eq!(
            fs::read(dest.join("Content/level.dat")).unwrap(),
            vec![7u8; 3000]
        );
        assert_eq!(unpacked_from(&dest), Some(manifest.id));
        assert_eq!(
            (progress.done(), progress.total()),
            (manifest.size, manifest.size)
        );
        let text = fs::read_to_string(dest.join(data_dir_name()).join(GAME_JSON)).unwrap();
        assert_eq!(Game::from_json(&dest, &text).unwrap().size, Some(3006));
        assert!(!dest.join(data_dir_name()).join(CHECKSUMS_JSON).exists());
    }

    #[test]
    fn read_meta_returns_manifest_and_game_json() {
        let dir = TempDir::new();
        let (_, package, manifest) = packed(&dir);
        let (read, game_json) = read_meta(&package).unwrap();
        assert_eq!(read, manifest);
        assert!(game_json.unwrap().contains("\"Celeste\""));
        assert!(read_data_file(&package, GAME_JSON).is_some());
        assert!(read_data_file(&package, "cover.png").is_none());
    }

    #[test]
    fn corrupted_package_is_rejected_and_leaves_nothing() {
        let dir = TempDir::new();
        let (_, package, _) = packed(&dir);
        let mut bytes = fs::read(&package).unwrap();
        let at = bytes.windows(4).position(|w| w == [7, 7, 7, 7]).unwrap();
        bytes[at] = 8;
        fs::write(&package, bytes).unwrap();
        let target = dir.mkdir("target");
        assert!(unpack(&package, &target, &Progress::default()).is_err());
        assert!(fs::read_dir(&target).unwrap().next().is_none());
    }

    #[test]
    fn truncated_stream_is_rejected() {
        let dir = TempDir::new();
        let (_, package, _) = packed(&dir);
        let bytes = fs::read(&package).unwrap();
        let target = dir.mkdir("target");
        let cut = &bytes[..bytes.len() / 2];
        assert!(unpack_from(cut, &target, &Progress::default()).is_err());
        assert!(!target.join("Celeste").exists());
    }

    #[test]
    fn existing_folder_is_never_overwritten() {
        let dir = TempDir::new();
        let (game, package, _) = packed(&dir);
        let err = unpack(&package, game.dir.parent().unwrap(), &Progress::default());
        assert!(err.unwrap_err().contains("already exists"));
    }

    #[test]
    fn cancel_stops_and_cleans_up() {
        let dir = TempDir::new();
        let (_, package, _) = packed(&dir);
        let target = dir.mkdir("target");
        let progress = Progress::default();
        progress.cancel();
        assert!(unpack(&package, &target, &progress).is_err());
        assert!(fs::read_dir(&target).unwrap().next().is_none());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        let dir = TempDir::new();
        assert!(safe_join(dir.path(), Path::new("../evil")).is_err());
        assert!(safe_join(dir.path(), Path::new("/etc/passwd")).is_err());
        assert!(safe_join(dir.path(), Path::new("a/b")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlinks_are_refused() {
        let dir = TempDir::new();
        std::os::unix::fs::symlink("/tmp", dir.path().join("link")).unwrap();
        assert!(safe_join(dir.path(), Path::new("link/file")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_modes_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let game = sample_game(&dir);
        set_mode(&game.dir.join("Celeste.bin"), 0o755).unwrap();
        make_symlink(Path::new("Celeste.bin"), &game.dir.join("run")).unwrap();
        let mut tar = Vec::new();
        pack_to(&game, &mut tar, &Progress::default()).unwrap();
        let target = dir.mkdir("target");
        let dest = unpack_from(tar.as_slice(), &target, &Progress::default()).unwrap();
        let mode = fs::metadata(dest.join("Celeste.bin"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            fs::read_link(dest.join("run")).unwrap(),
            Path::new("Celeste.bin")
        );
    }

    #[test]
    fn repacking_an_unpacked_game_skips_generated_files() {
        let dir = TempDir::new();
        let (_, package, _) = packed(&dir);
        let target = dir.mkdir("target");
        let dest = unpack(&package, &target, &Progress::default()).unwrap();
        let items = walk(&dest).unwrap();
        assert!(items.iter().any(|i| is_generated(&i.rel)));
        let mut game = Game::skeleton(&dest);
        game.title = "Celeste".into();
        let mut tar = Vec::new();
        let manifest = pack_to(&game, &mut tar, &Progress::default()).unwrap();
        assert_eq!(manifest.files, 3);
    }

    #[test]
    fn lists_only_visible_packages() {
        let dir = TempDir::new();
        dir.write(&format!("A{EXTENSION}"), b"");
        dir.write(&format!(".B{EXTENSION}.part"), b"");
        dir.write("C.tar", b"");
        let names: Vec<_> = list(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [format!("A{EXTENSION}")]);
    }
}
