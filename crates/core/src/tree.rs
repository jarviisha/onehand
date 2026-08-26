//! The rail **file tree** (per project root): lazily-expanded directory
//! listings that feed the editor panel (click a file → `Message::OpenFile`).
//! The flatten/sort logic is pure and unit-testable; directory scans run off
//! the UI loop. Everything is bounded: entries per
//! directory and total visible rows are capped so the per-row render can't
//! freeze the UI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Cap on entries read per directory (bounded render).
pub const MAX_DIR_ENTRIES: usize = 300;
/// Cap on total rows the rail draws for the tree (bounded render).
pub const MAX_VISIBLE_ROWS: usize = 600;

/// One directory entry, pre-resolved at scan time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Absolute path.
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// One project root's tree state: which directories are unfolded, and the
/// cached listing per scanned directory.
#[derive(Default)]
pub struct FileTree {
    pub expanded: HashSet<PathBuf>,
    pub listings: HashMap<PathBuf, Vec<TreeEntry>>,
}

/// A renderable row: the entry plus its indent depth.
pub struct TreeRow<'a> {
    pub entry: &'a TreeEntry,
    pub depth: u16,
}

impl FileTree {
    /// Flatten the cached listings into visible rows, depth-first from `root`
    /// through expanded directories. The bool is `true` when the row cap cut
    /// the walk short.
    pub fn visible_rows(&self, root: &Path) -> (Vec<TreeRow<'_>>, bool) {
        let mut rows = Vec::new();
        let mut truncated = false;
        self.walk(root, 0, &mut rows, &mut truncated);
        (rows, truncated)
    }

    fn walk<'a>(
        &'a self,
        dir: &Path,
        depth: u16,
        rows: &mut Vec<TreeRow<'a>>,
        truncated: &mut bool,
    ) {
        let Some(list) = self.listings.get(dir) else {
            return;
        };
        for entry in list {
            if rows.len() >= MAX_VISIBLE_ROWS {
                *truncated = true;
                return;
            }
            rows.push(TreeRow { entry, depth });
            if entry.is_dir && self.expanded.contains(&entry.path) {
                self.walk(&entry.path, depth + 1, rows, truncated);
            }
        }
    }
}

/// Read one directory, sorted directories-first then case-insensitive by
/// name; `.git` is skipped; bounded at [`MAX_DIR_ENTRIES`]. Sync — run it on a
/// blocking pool, never the UI loop.
pub fn read_dir_sorted(dir: &Path) -> Vec<TreeEntry> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            // `DirEntry::file_type` doesn't follow symlinks, which would
            // render a symlink-to-directory as an unopenable file row; take
            // the target's type instead (a dangling link falls back to file).
            let ft = e.file_type();
            let is_dir = match ft {
                Ok(t) if t.is_symlink() => std::fs::metadata(e.path())
                    .map(|m| m.is_dir())
                    .unwrap_or(false),
                Ok(t) => t.is_dir(),
                Err(_) => false,
            };
            out.push(TreeEntry {
                path: e.path(),
                name,
                is_dir,
            });
        }
    }
    // Sort *before* bounding: truncating in filesystem (inode) order would
    // silently drop an arbitrary subset while the survivors still look like
    // a complete alphabetical listing.
    sort_entries(&mut out);
    out.truncate(MAX_DIR_ENTRIES);
    out
}

/// Directories before files, then case-insensitive name.
pub fn sort_entries(entries: &mut [TreeEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, is_dir: bool) -> TreeEntry {
        TreeEntry {
            path: PathBuf::from(format!("/r/{name}")),
            name: name.into(),
            is_dir,
        }
    }

    #[test]
    fn sorts_dirs_first_then_case_insensitive() {
        let mut v = vec![
            e("zeta.rs", false),
            e("Beta", true),
            e("alpha", true),
            e("Apple.rs", false),
        ];
        sort_entries(&mut v);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Beta", "Apple.rs", "zeta.rs"]);
    }

    #[test]
    fn visible_rows_walks_expanded_dirs_only() {
        let mut t = FileTree::default();
        let root = PathBuf::from("/r");
        t.listings
            .insert(root.clone(), vec![e("sub", true), e("main.rs", false)]);
        t.listings.insert(
            PathBuf::from("/r/sub"),
            vec![TreeEntry {
                path: "/r/sub/inner.rs".into(),
                name: "inner.rs".into(),
                is_dir: false,
            }],
        );

        // Collapsed: only the top level shows.
        let (rows, trunc) = t.visible_rows(&root);
        assert!(!trunc);
        assert_eq!(rows.len(), 2);

        // Expanded: the child appears, indented one level deeper.
        t.expanded.insert(PathBuf::from("/r/sub"));
        let (rows, _) = t.visible_rows(&root);
        let flat: Vec<(&str, u16)> = rows
            .iter()
            .map(|r| (r.entry.name.as_str(), r.depth))
            .collect();
        assert_eq!(flat, [("sub", 0), ("inner.rs", 1), ("main.rs", 0)]);
    }

    #[test]
    fn visible_rows_is_bounded() {
        let mut t = FileTree::default();
        let root = PathBuf::from("/r");
        let many: Vec<TreeEntry> = (0..MAX_VISIBLE_ROWS + 50)
            .map(|i| e(&format!("f{i}.rs"), false))
            .collect();
        t.listings.insert(root.clone(), many);
        let (rows, truncated) = t.visible_rows(&root);
        assert_eq!(rows.len(), MAX_VISIBLE_ROWS);
        assert!(truncated);
    }
}
