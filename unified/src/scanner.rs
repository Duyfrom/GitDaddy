use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::util::natural_cmp;

pub const MAX_DEPTH: usize = 5;

const IGNORED_DIRECTORY_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    ".build",
    "DerivedData",
    "Pods",
    "Carthage",
    "venv",
    ".venv",
    "env",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    "target",
    "dist",
    "build",
    "out",
    "obj",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".gradle",
    ".idea",
    ".vs",
    ".vscode",
    "vendor",
    "bower_components",
    "AppData",
    "$Recycle.Bin",
    "System Volume Information",
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
];

#[derive(Debug, Clone)]
pub struct DiscoveredRepo {
    pub repo_path: String,
}

/// Walks the watched folders and returns every Git repository found, sorted by path.
pub fn discover_repositories(root_folders: &[String], max_depth: usize, scan_nested: bool) -> Vec<DiscoveredRepo> {
    let mut discovered = Vec::new();
    let mut seen = HashSet::new();

    for raw_root in root_folders {
        let root = PathBuf::from(raw_root);
        if !root.is_dir() {
            continue;
        }
        if is_git_repository(&root) {
            if seen.insert(key(&root)) {
                discovered.push(DiscoveredRepo { repo_path: display(&root) });
            }
            if !scan_nested {
                continue;
            }
        }
        walk(&root, 1, max_depth, scan_nested, &mut seen, &mut discovered);
    }

    discovered.sort_by(|a, b| natural_cmp(&a.repo_path, &b.repo_path));
    discovered
}

fn walk(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    scan_nested: bool,
    seen: &mut HashSet<String>,
    out: &mut Vec<DiscoveredRepo>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        // `file_type` does not follow links, so symlinks and junctions report as links and are skipped.
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || IGNORED_DIRECTORY_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            continue;
        }
        if is_hidden_or_system(&entry) {
            continue;
        }

        let path = entry.path();
        if is_git_repository(&path) {
            if seen.insert(key(&path)) {
                out.push(DiscoveredRepo { repo_path: display(&path) });
            }
            if !scan_nested {
                continue;
            }
        }
        walk(&path, depth + 1, max_depth, scan_nested, seen, out);
    }
}

#[cfg(windows)]
fn is_hidden_or_system(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    entry.metadata().is_ok_and(|m| m.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0)
}

#[cfg(not(windows))]
fn is_hidden_or_system(_entry: &fs::DirEntry) -> bool {
    false
}

fn is_git_repository(dir: &Path) -> bool {
    // `.git` is a directory for normal clones and a file for worktrees and submodules.
    dir.join(".git").exists()
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Windows paths are case-insensitive, so dedupe on a lowercase key.
fn key(path: &Path) -> String {
    display(path).trim_end_matches(['\\', '/']).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nested_and_skips_ignored() {
        let base = std::env::temp_dir().join(format!("gitdaddy-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for dir in ["a/.git", "group/b/.git", "group/b/inner/.git", "node_modules/c/.git"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let roots = vec![display(&base)];

        let flat: Vec<String> =
            discover_repositories(&roots, MAX_DEPTH, false).into_iter().map(|r| folder(&r.repo_path)).collect();
        assert_eq!(flat, ["a", "b"]);

        let nested: Vec<String> =
            discover_repositories(&roots, MAX_DEPTH, true).into_iter().map(|r| folder(&r.repo_path)).collect();
        assert_eq!(nested, ["a", "b", "inner"]);

        let _ = fs::remove_dir_all(&base);
    }

    fn folder(path: &str) -> String {
        Path::new(path).file_name().unwrap().to_string_lossy().into_owned()
    }
}
