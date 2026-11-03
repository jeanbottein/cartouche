//! Path and string helpers mirroring the Python `os.path` behaviour the data
//! formats depend on (`normpath`, `expanduser`, `expandvars`, `relpath`).
//! Steam AppIDs are derived from path strings, so these must stay lexical and
//! exact.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Lexical cleanup like `os.path.normpath`: drops `.`, folds `..`, never
/// climbs above the root.
pub fn normpath(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(comp),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        PathBuf::from(".")
    } else {
        out.iter().collect()
    }
}

/// Absolute + normalized, without touching the filesystem (`os.path.abspath`).
pub fn abspath(path: &Path) -> PathBuf {
    normpath(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// Absolute with symlinks resolved when the path exists (`Path.resolve()`).
pub fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| abspath(path))
}

/// Replace a leading `~` with the home directory.
pub fn expand_user(text: &str) -> String {
    match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{}{rest}", home_dir().display())
        }
        _ => text.to_string(),
    }
}

/// Substitute `${name}` and `$name` using `lookup`; unknown names stay as-is.
pub fn substitute(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let (name, consumed) = if let Some(braced) = after.strip_prefix('{') {
            match braced.find('}') {
                Some(end) => (&braced[..end], end + 2),
                None => ("", 0),
            }
        } else {
            let end = after
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            (&after[..end], end)
        };
        match lookup(name).filter(|_| !name.is_empty()) {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[pos..pos + 1 + consumed]),
        }
        rest = &rest[pos + 1 + consumed..];
    }
    out.push_str(rest);
    out
}

/// Environment-variable expansion like `os.path.expandvars` (`%VAR%` too on Windows).
pub fn expand_vars(text: &str) -> String {
    let expanded = substitute(text, |name| std::env::var(name).ok());
    if cfg!(windows) {
        expand_percent_vars(&expanded)
    } else {
        expanded
    }
}

fn expand_percent_vars(text: &str) -> String {
    let parts: Vec<&str> = text.split('%').collect();
    let mut out = String::from(parts[0]);
    let mut i = 1;
    while i < parts.len() {
        match std::env::var(parts[i]).ok().filter(|_| i + 1 < parts.len()) {
            Some(value) => {
                out.push_str(&value);
                out.push_str(parts[i + 1]);
                i += 2;
            }
            None => {
                out.push('%');
                out.push_str(parts[i]);
                i += 1;
            }
        }
    }
    out
}

/// `expanduser` then `expandvars`, as a path.
pub fn expand(text: &str) -> PathBuf {
    PathBuf::from(expand_vars(&expand_user(text)))
}

/// `os.path.relpath(path, base)`.
pub fn relpath(path: &Path, base: &Path) -> PathBuf {
    let path = abspath(path);
    let base = abspath(base);
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let ups = (common..base_parts.len()).map(|_| Component::ParentDir);
    let rel: PathBuf = ups.chain(path_parts[common..].iter().copied()).collect();
    if rel.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        rel
    }
}

/// ASCII case-insensitive replace of every occurrence of `from`.
pub fn replace_ignore_case(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let haystack = text.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, _) in haystack.match_indices(&needle) {
        out.push_str(&text[last..start]);
        out.push_str(to);
        last = start + from.len();
    }
    out.push_str(&text[last..]);
    out
}

/// Expand `*` / `?` wildcards component by component (like `glob.glob`, hidden
/// entries excluded). Non-wildcard patterns are returned unchanged.
pub fn glob(pattern: &str) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::new()];
    for comp in Path::new(pattern).components() {
        let part = comp.as_os_str().to_string_lossy();
        if !part.contains(['*', '?']) {
            paths.iter_mut().for_each(|p| p.push(comp));
            continue;
        }
        paths = paths
            .iter()
            .flat_map(|base| {
                sorted_entries(base)
                    .into_iter()
                    .filter(|name| !name.starts_with('.') && wildcard_match(&part, name))
                    .map(move |name| base.join(name))
            })
            .collect();
    }
    paths
}

fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Entry names of `dir`, sorted (empty when unreadable).
pub fn sorted_entries(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Sorted visible subdirectories of `dir` (like the Python `listdir` + `isdir` loops).
pub fn visible_subdirs(dir: &Path) -> Vec<PathBuf> {
    sorted_entries(dir)
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .map(|name| dir.join(name))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Copy a file keeping its modification time (`shutil.copy2`).
pub fn copy_with_mtime(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst)?;
    let mtime: SystemTime = std::fs::metadata(src)?.modified()?;
    std::fs::File::options()
        .write(true)
        .open(dst)?
        .set_modified(mtime)
}

pub fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normpath_folds_dots_and_stays_under_root() {
        assert_eq!(
            normpath(Path::new("/g/Game/./bin/../run.sh")),
            PathBuf::from("/g/Game/run.sh")
        );
        assert_eq!(normpath(Path::new("/../a")), PathBuf::from("/a"));
        assert_eq!(normpath(Path::new("../a/./b")), PathBuf::from("../a/b"));
        assert_eq!(normpath(Path::new("a/..")), PathBuf::from("."));
    }

    #[test]
    fn relpath_walks_up_and_down() {
        assert_eq!(
            relpath(Path::new("/g/A/bin/x"), Path::new("/g/A")),
            PathBuf::from("bin/x")
        );
        assert_eq!(
            relpath(Path::new("/g/A"), Path::new("/g/A")),
            PathBuf::from(".")
        );
        assert_eq!(
            relpath(Path::new("/g/B"), Path::new("/g/A/c")),
            PathBuf::from("../../B")
        );
    }

    #[test]
    fn substitute_replaces_known_names_only() {
        let lookup = |n: &str| (n == "steamappid").then(|| "42".to_string());
        assert_eq!(
            substitute("${steamappid}/$steamappid/$other/${x}", lookup),
            "42/42/$other/${x}"
        );
        assert_eq!(
            substitute("cost $5 and ${unclosed", lookup),
            "cost $5 and ${unclosed"
        );
    }

    #[test]
    fn replace_ignore_case_matches_any_case() {
        assert_eq!(
            replace_ignore_case("%appdata%/X/%APPDATA%", "%APPDATA%", "R"),
            "R/X/R"
        );
    }

    #[test]
    fn wildcard_matching() {
        assert!(wildcard_match("*", "sdcard"));
        assert!(wildcard_match("ab*cd?", "abXXcde"));
        assert!(!wildcard_match("ab*cd", "abXXce"));
    }

    #[test]
    fn expand_user_only_touches_leading_tilde() {
        assert_eq!(expand_user("a/~/b"), "a/~/b");
        assert!(!expand_user("~/x").starts_with('~'));
    }
}
