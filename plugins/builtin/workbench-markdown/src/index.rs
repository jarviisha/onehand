//! Which markdown documents a project root holds, and how they flatten into
//! rows.
//!
//! GUI-free and unit-tested, but it lives with the mode that draws it rather
//! than in the shared core: nothing outside this mode has a use for a list of a
//! project's `.md` files, and a rule kept where its only caller is, is a rule
//! that leaves with the caller. What is in core is what more than one half of
//! the app has to agree about.
//!
//! Not a filter over the Workbench's file tree, either, and that is the point.
//! The tree is *lazy* — it knows only the directories somebody has unfolded — so
//! filtering it to markdown draws a project as a column of empty folders with
//! every document still one click each away, which is the opposite of what a
//! document list is for. This walks the root once and keeps only what it found,
//! so a directory appears **iff** a document was found under it.
//!
//! Everything here is bounded and blocking: run the scan on a background pool,
//! never on a UI loop.

use std::collections::{HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};

/// Cap on documents one index holds (bounded render).
pub const MAX_DOCUMENTS: usize = 400;

/// Cap on directories one scan opens.
///
/// The walk is breadth-first, so this and [`MAX_DOCUMENTS`] both cut at the
/// deepest level reached rather than at an arbitrary subtree: the documents
/// lying beside the project and one folder in are found before anything buried,
/// which is the order somebody would have looked in anyway.
pub const MAX_DIRECTORIES: usize = 2_000;

/// Directory names the walk never descends into.
///
/// `.git` is skipped for the same reason the file tree skips it. The rest are
/// where *other people's* documentation lands: a dependency tree or a build
/// output holds one introductory document per package, hundreds of them, none
/// written by anybody here and none being looked for — and on a JavaScript
/// project they would fill the document cap before the project's own docs were
/// reached. By name, because the name is the only thing available before the
/// directory is opened, and opening it is the cost being avoided.
pub const GENERATED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".cache",
];

/// Whether `path` names a markdown document.
///
/// By extension, case-insensitively: there is no other evidence available
/// before the file is read, and an upper-cased extension written on a
/// case-insensitive filesystem names the same kind of document.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .is_some_and(|ext| ext == "md" || ext == "markdown")
}

/// Every markdown document under one project root, in drawing order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DocIndex {
    /// Absolute paths, sorted the way the rows are drawn.
    pub docs: Vec<PathBuf>,
    /// Whether a cap cut the walk short, so the view can say so.
    pub truncated: bool,
}

/// One row of the document list: a directory holding documents, or a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRow {
    /// Absolute path — the directory to fold, or the document to open.
    pub path: PathBuf,
    /// The last component, which is what the row prints.
    pub name: String,
    /// Indent level, counted from the root.
    pub depth: u16,
    pub is_dir: bool,
}

/// Walk `root` for markdown documents. Blocking — run it off the UI loop.
pub fn scan_blocking(root: &Path) -> DocIndex {
    let mut docs = Vec::new();
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut opened = 0usize;
    let mut truncated = false;

    while let Some(dir) = queue.pop_front() {
        if opened >= MAX_DIRECTORIES || docs.len() >= MAX_DOCUMENTS {
            truncated = true;
            break;
        }
        opened += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `file_type` does not follow symlinks, and here that is the
            // behaviour wanted rather than a limitation: a link pointing back
            // at an ancestor turns the walk into one that never ends, and what
            // not following costs is a linked folder going unindexed.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !GENERATED_DIRS.contains(&name.as_str()) {
                    queue.push_back(entry.path());
                }
            } else if kind.is_file() && is_markdown(Path::new(&name)) {
                if docs.len() >= MAX_DOCUMENTS {
                    truncated = true;
                    break;
                }
                docs.push(entry.path());
            }
        }
    }

    docs.sort_by(|a, b| cmp_in_tree(root, a, b));
    DocIndex { docs, truncated }
}

impl DocIndex {
    /// Flatten the index into rows: each directory once, above the documents it
    /// holds, and nothing under a directory that has been folded away.
    ///
    /// The set is what is *closed*, unlike the file tree's set of what is open,
    /// because the whole index is known by the time this is asked: a document
    /// list that opened folded would be a list of folders hiding the one thing
    /// it exists to show.
    pub fn rows(&self, root: &Path, folded: &HashSet<PathBuf>) -> Vec<DocRow> {
        let mut rows = Vec::new();
        // The directory chain the last document sat in, so a directory shared
        // by a run of documents is drawn once instead of above each of them.
        let mut chain: Vec<PathBuf> = Vec::new();

        for doc in &self.docs {
            let parts = relative_parts(root, doc);
            let Some((_, dirs)) = parts.split_last() else {
                continue;
            };

            let mut path = root.to_path_buf();
            let mut hidden = false;
            for (depth, part) in dirs.iter().enumerate() {
                path.push(part);
                if chain.get(depth) != Some(&path) {
                    chain.truncate(depth);
                    chain.push(path.clone());
                    // A folded directory still draws its own row — it is the
                    // only thing left to click to get its documents back.
                    if !hidden {
                        rows.push(DocRow {
                            path: path.clone(),
                            name: part.clone(),
                            depth: depth as u16,
                            is_dir: true,
                        });
                    }
                }
                hidden |= folded.contains(&path);
            }
            chain.truncate(dirs.len());

            if !hidden {
                rows.push(DocRow {
                    path: doc.clone(),
                    name: file_name(doc),
                    depth: dirs.len() as u16,
                    is_dir: false,
                });
            }
        }
        rows
    }
}

/// Drawing order: directories before documents at each level, then
/// case-insensitive name — the same rule the file tree sorts by, applied to
/// whole paths so everything under one directory stays contiguous.
fn cmp_in_tree(root: &Path, a: &Path, b: &Path) -> std::cmp::Ordering {
    let (a, b) = (relative_parts(root, a), relative_parts(root, b));
    for i in 0..a.len().min(b.len()) {
        if a[i] == b[i] {
            continue;
        }
        // Components left past this one mean this level is a directory rather
        // than the document itself.
        let a_dir = a.len() > i + 1;
        let b_dir = b.len() > i + 1;
        return b_dir
            .cmp(&a_dir)
            .then_with(|| a[i].to_lowercase().cmp(&b[i].to_lowercase()))
            .then_with(|| a[i].cmp(&b[i]));
    }
    a.len().cmp(&b.len())
}

/// `path` relative to `root`, as its plain components. A path that is not under
/// `root` keeps its own, so it draws somewhere rather than vanishing.
fn relative_parts(root: &Path, path: &Path) -> Vec<String> {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(root: &str, docs: &[&str]) -> (PathBuf, DocIndex) {
        let root = PathBuf::from(root);
        let mut docs: Vec<PathBuf> = docs.iter().map(|d| root.join(d)).collect();
        docs.sort_by(|a, b| cmp_in_tree(&root, a, b));
        (
            root,
            DocIndex {
                docs,
                truncated: false,
            },
        )
    }

    fn drawn(rows: &[DocRow]) -> Vec<(String, u16, bool)> {
        rows.iter()
            .map(|r| (r.name.clone(), r.depth, r.is_dir))
            .collect()
    }

    #[test]
    fn matches_markdown_by_extension_case_insensitively() {
        assert!(is_markdown(Path::new("/r/README.md")));
        assert!(is_markdown(Path::new("/r/notes.MARKDOWN")));
        assert!(!is_markdown(Path::new("/r/main.rs")));
        // A file whose *name* merely ends in those letters is not one.
        assert!(!is_markdown(Path::new("/r/readme")));
        assert!(!is_markdown(Path::new("/r/md")));
    }

    #[test]
    fn sorts_directories_before_documents_then_by_name() {
        let (root, index) = index("/r", &["README.md", "docs/b.md", "docs/A.md", "CLAUDE.md"]);
        let rel: Vec<String> = index
            .docs
            .iter()
            .map(|d| d.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        assert_eq!(rel, ["docs/A.md", "docs/b.md", "CLAUDE.md", "README.md"]);
    }

    #[test]
    fn every_directory_is_drawn_once_above_what_it_holds() {
        let (root, index) = index(
            "/r",
            &["docs/a.md", "docs/deep/b.md", "docs/deep/c.md", "README.md"],
        );
        assert_eq!(
            drawn(&index.rows(&root, &HashSet::new())),
            [
                ("docs".to_string(), 0, true),
                ("deep".to_string(), 1, true),
                ("b.md".to_string(), 2, false),
                ("c.md".to_string(), 2, false),
                ("a.md".to_string(), 1, false),
                ("README.md".to_string(), 0, false),
            ]
        );
    }

    #[test]
    fn a_folded_directory_keeps_its_row_and_hides_the_rest() {
        let (root, index) = index("/r", &["docs/deep/b.md", "docs/a.md", "README.md"]);

        let folded = HashSet::from([root.join("docs/deep")]);
        assert_eq!(
            drawn(&index.rows(&root, &folded)),
            [
                ("docs".to_string(), 0, true),
                ("deep".to_string(), 1, true),
                ("a.md".to_string(), 1, false),
                ("README.md".to_string(), 0, false),
            ]
        );

        // Folding the top of the chain takes what is under it along.
        let folded = HashSet::from([root.join("docs")]);
        assert_eq!(
            drawn(&index.rows(&root, &folded)),
            [
                ("docs".to_string(), 0, true),
                ("README.md".to_string(), 0, false),
            ]
        );
    }

    #[test]
    fn a_scan_finds_documents_and_skips_generated_directories() {
        let root = std::env::temp_dir().join(format!("onehand-md-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/deep")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("README.md"), "# hi").unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("docs/deep/design.md"), "# design").unwrap();
        std::fs::write(root.join("node_modules/pkg/README.md"), "# theirs").unwrap();
        std::fs::write(root.join(".git/COMMIT_EDITMSG.md"), "# no").unwrap();

        let index = scan_blocking(&root);
        let rel: Vec<String> = index
            .docs
            .iter()
            .map(|d| d.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        assert_eq!(rel, ["docs/deep/design.md", "README.md"]);
        assert!(!index.truncated);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_scan_is_bounded_and_says_so() {
        let root = std::env::temp_dir().join(format!("onehand-md-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..MAX_DOCUMENTS + 20 {
            std::fs::write(root.join(format!("doc{i:04}.md")), "x").unwrap();
        }

        let index = scan_blocking(&root);
        assert_eq!(index.docs.len(), MAX_DOCUMENTS);
        assert!(index.truncated);

        let _ = std::fs::remove_dir_all(&root);
    }
}
