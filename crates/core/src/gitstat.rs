//! Per-root git status: the rail meta line (branch + changed-file count) and
//! the per-path change map behind the file tree's VSCode-style badges. The
//! porcelain parser is pure (unit-testable); [`read`] shells out to `git`
//! asynchronously so it never runs on the UI loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How one path differs from HEAD — drives the tree row's badge and tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflicted,
}

impl FileChange {
    /// The one-letter badge on a tree file row (VSCode-style).
    pub fn badge(self) -> &'static str {
        match self {
            FileChange::Modified => "M",
            FileChange::Added => "A",
            FileChange::Untracked => "U",
            FileChange::Deleted => "D",
            FileChange::Renamed => "R",
            FileChange::Conflicted => "!",
        }
    }

    /// Ordering for folding a directory's contents into one indicator —
    /// the most severe change wins.
    fn severity(self) -> u8 {
        match self {
            FileChange::Conflicted => 4,
            FileChange::Deleted => 3,
            FileChange::Modified => 2,
            FileChange::Renamed | FileChange::Added => 1,
            FileChange::Untracked => 0,
        }
    }
}

/// One root's working-tree summary: the rail meta line plus the per-path
/// change map (paths are repo-relative, as porcelain prints them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    /// `branch.head` from porcelain v2 — `"(detached)"` when headless.
    pub branch: String,
    /// Changed entries: tracked changes, renames, unmerged, and untracked.
    pub changed: usize,
    /// Per-path change state, keyed by repo-relative path.
    pub entries: HashMap<PathBuf, FileChange>,
}

impl GitStatus {
    /// The rail meta line, e.g. `main` or `main · 3 changed`.
    pub fn label(&self) -> String {
        if self.changed > 0 {
            format!("{} · {} changed", self.branch, self.changed)
        } else {
            self.branch.clone()
        }
    }

    /// The change state of the file at repo-relative `rel`.
    pub fn file_change(&self, rel: &Path) -> Option<FileChange> {
        self.entries.get(rel).copied()
    }

    /// The most severe change *under* the directory at repo-relative `rel`
    /// (the tree's folder dot). `None` when nothing under it changed.
    pub fn dir_change(&self, rel: &Path) -> Option<FileChange> {
        self.entries
            .iter()
            .filter(|(p, _)| p.starts_with(rel))
            .map(|(_, c)| *c)
            .max_by_key(|c| c.severity())
    }
}

/// Fold porcelain's two status letters (`XY`, staged/unstaged) into one state.
fn classify(xy: &str) -> FileChange {
    if xy.contains('D') {
        FileChange::Deleted
    } else if xy.contains('R') {
        FileChange::Renamed
    } else if xy.contains('A') {
        FileChange::Added
    } else {
        FileChange::Modified
    }
}

/// The path tail of a porcelain line whose first `fields` space-separated
/// fields are metadata (paths may contain spaces, so only the tail is safe
/// to take whole).
fn path_tail(line: &str, fields: usize) -> Option<&str> {
    line.splitn(fields + 1, ' ').nth(fields)
}

/// Parse `git status --porcelain=v2 --branch -z` output (NUL-terminated
/// records). `-z` matters: without it git C-quotes pathnames holding
/// non-ASCII bytes (`"t\341\273\207p.txt"` for `tệp.txt` under the default
/// `core.quotepath`), so the stored path would never match the real file and
/// its badge silently vanished. With `-z` paths are always raw.
pub fn parse_porcelain(out: &str) -> GitStatus {
    let mut branch = String::new();
    let mut changed = 0;
    let mut entries = HashMap::new();
    let mut records = out.split('\0');
    while let Some(line) = records.next() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = head.trim().to_string();
            continue;
        }
        // Entry lines: `1` ordinary change, `2` rename/copy, `u` unmerged,
        // `?` untracked. Field counts per the git-status(1) v2 format.
        let xy = line.get(2..4).unwrap_or("");
        let parsed: Option<(&str, FileChange)> = if line.starts_with("1 ") {
            path_tail(line, 8).map(|p| (p, classify(xy)))
        } else if line.starts_with("2 ") {
            // Under `-z` a rename's origPath is the *next* NUL record —
            // consume it so it can't be misread as an entry; badge the new path.
            let p = path_tail(line, 9).map(|p| (p, classify(xy)));
            records.next();
            p
        } else if line.starts_with("u ") {
            path_tail(line, 10).map(|p| (p, FileChange::Conflicted))
        } else if line.starts_with("? ") {
            path_tail(line, 1).map(|p| (p, FileChange::Untracked))
        } else {
            None
        };
        if let Some((path, change)) = parsed {
            changed += 1;
            entries.insert(PathBuf::from(path), change);
        }
    }
    GitStatus {
        branch,
        changed,
        entries,
    }
}

/// Re-base toplevel-relative entries onto a root that sits *below* the repo's
/// toplevel (a monorepo-subdir root): keep only entries under `prefix` (the
/// root's repo-relative path, as `git rev-parse --show-prefix` prints it,
/// trailing slash included) and strip it — consumers look paths up relative
/// to the *root*, and would otherwise never match. A toplevel root
/// (empty prefix) passes through untouched.
pub fn rebase_to_root(status: GitStatus, prefix: &str) -> GitStatus {
    if prefix.is_empty() {
        return status;
    }
    let prefix = Path::new(prefix.trim_end_matches('/'));
    let entries: HashMap<PathBuf, FileChange> = status
        .entries
        .into_iter()
        .filter_map(|(p, c)| p.strip_prefix(prefix).ok().map(|r| (r.to_path_buf(), c)))
        .collect();
    GitStatus {
        branch: status.branch,
        // Scope the rail count to this root's subtree too — a count that
        // includes changes the panel can't show would read as a bug.
        changed: entries.len(),
        entries,
    }
}

/// Run `git status` in `root`. `None` when the root isn't a git repo (or git
/// isn't installed).
///
/// Blocking, and deliberately runtime-agnostic: core must not dictate which
/// async runtime its callers use -- the GPUI shell drives it through
/// `cx.background_executor()` on smol, and a caller on tokio would reach for
/// `spawn_blocking`. Either way it must never run on a UI loop.
pub fn read_blocking(root: &Path) -> Option<GitStatus> {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
    };

    let out = git(&["status", "--porcelain=v2", "--branch", "-z"])?;
    let status = parse_porcelain(&String::from_utf8_lossy(&out.stdout));
    // Porcelain paths are relative to the repo *toplevel*; the root may be a
    // subdir of it. Fetch the offset and re-base.
    let prefix = git(&["rev-parse", "--show-prefix"])
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();
    Some(rebase_to_root(status, &prefix))
}

/// Async wrapper over [`read_blocking`] for callers already on tokio.
pub async fn read(root: PathBuf) -> Option<GitStatus> {
    tokio::task::spawn_blocking(move || read_blocking(&root))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `-z` output: NUL-terminated records; a rename's origPath is its own
    // following record (no `\t`, no quoting).
    const SAMPLE: &str = "# branch.oid 1234567890abcdef\0\
# branch.head main\0\
# branch.upstream origin/main\0\
# branch.ab +1 -0\0\
1 .M N... 100644 100644 100644 abc def src/lib.rs\0\
2 R. N... 100644 100644 100644 abc def R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 abc def ghi conflict.rs\0\
? untracked.txt\0";

    #[test]
    fn parses_branch_and_counts_all_entry_kinds() {
        let st = parse_porcelain(SAMPLE);
        assert_eq!(st.branch, "main");
        assert_eq!(st.changed, 4);
        assert_eq!(st.label(), "main · 4 changed");
    }

    #[test]
    fn clean_tree_is_branch_only() {
        let out = "# branch.oid abc\0# branch.head trunk\0";
        let st = parse_porcelain(out);
        assert_eq!(st.branch, "trunk");
        assert_eq!(st.changed, 0);
        assert!(st.entries.is_empty());
        assert_eq!(st.label(), "trunk");
    }

    #[test]
    fn detached_head_passes_through() {
        let st = parse_porcelain("# branch.head (detached)\0? x\0");
        assert_eq!(st.branch, "(detached)");
        assert_eq!(st.label(), "(detached) · 1 changed");
    }

    #[test]
    fn entries_map_paths_to_change_kinds() {
        let st = parse_porcelain(SAMPLE);
        assert_eq!(
            st.file_change(Path::new("src/lib.rs")),
            Some(FileChange::Modified)
        );
        // A rename badges the *new* path, without the `\told.rs` tail.
        assert_eq!(
            st.file_change(Path::new("new.rs")),
            Some(FileChange::Renamed)
        );
        assert_eq!(
            st.file_change(Path::new("conflict.rs")),
            Some(FileChange::Conflicted)
        );
        assert_eq!(
            st.file_change(Path::new("untracked.txt")),
            Some(FileChange::Untracked)
        );
        assert_eq!(st.file_change(Path::new("absent.rs")), None);
    }

    #[test]
    fn paths_with_spaces_survive() {
        let st = parse_porcelain("1 .M N... 100644 100644 100644 abc def my dir/my file.rs\0");
        assert_eq!(
            st.file_change(Path::new("my dir/my file.rs")),
            Some(FileChange::Modified)
        );
    }

    #[test]
    fn non_ascii_paths_survive_raw() {
        // Under `-z` git never C-quotes, so a Vietnamese filename arrives raw.
        let st = parse_porcelain("1 .M N... 100644 100644 100644 abc def docs/tệp.txt\0");
        assert_eq!(
            st.file_change(Path::new("docs/tệp.txt")),
            Some(FileChange::Modified)
        );
    }

    #[test]
    fn staged_letters_classify_too() {
        let st = parse_porcelain("1 A. N... 000000 100644 100644 0000 abc staged.rs\0");
        assert_eq!(
            st.file_change(Path::new("staged.rs")),
            Some(FileChange::Added)
        );
        let st = parse_porcelain("1 .D N... 100644 100644 000000 abc 0000 gone.rs\0");
        assert_eq!(
            st.file_change(Path::new("gone.rs")),
            Some(FileChange::Deleted)
        );
    }

    #[test]
    fn rebase_scopes_entries_to_a_subdir_root() {
        // A root at `crates/foo` inside a monorepo: only its subtree survives,
        // keys become root-relative, and the count follows.
        let st = parse_porcelain(
            "# branch.head main\0\
             1 .M N... 100644 100644 100644 abc def crates/foo/src/lib.rs\0\
             ? crates/bar/x.txt\0",
        );
        let st = rebase_to_root(st, "crates/foo/");
        assert_eq!(st.changed, 1);
        assert_eq!(
            st.file_change(Path::new("src/lib.rs")),
            Some(FileChange::Modified)
        );
        assert_eq!(st.file_change(Path::new("x.txt")), None);
        // A toplevel root (empty prefix) is untouched.
        let st2 = parse_porcelain("# branch.head main\0? a.txt\0");
        let st2 = rebase_to_root(st2, "");
        assert_eq!(
            st2.file_change(Path::new("a.txt")),
            Some(FileChange::Untracked)
        );
    }

    #[test]
    fn dir_change_folds_to_the_most_severe() {
        let st = parse_porcelain(
            // `\u{0}` rather than `\0`: the next record starts with the digit
            // `1`, and `"\01"` reads like an octal escape in every other
            // language even though Rust has none.
            "? src/new.rs\u{0}1 .M N... 100644 100644 100644 abc def src/app/mod.rs\u{0}",
        );
        // Modified (in a subdir) outranks Untracked.
        assert_eq!(st.dir_change(Path::new("src")), Some(FileChange::Modified));
        assert_eq!(
            st.dir_change(Path::new("src/app")),
            Some(FileChange::Modified)
        );
        assert_eq!(st.dir_change(Path::new("tests")), None);
    }
}
