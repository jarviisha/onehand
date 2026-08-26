//! The Workbench's editor model: which files a project root has open, and the
//! rules for reading and writing them.
//!
//! A *quick* editor for reviewing and touching up what the agent did, not an
//! IDE: it highlights, and there is no language server behind it. Scope is
//! **per project root** —
//! switching roots swaps the whole tab set.
//!
//! The buffer itself is not here -- it is framework-shaped (an `InputState` in
//! the GPUI front end) and derived from the same text on disk. What lives here
//! is everything that decides correctness: the
//! size bound, the tab set, and the mtime guard.

use std::path::{Path, PathBuf};

/// Refuse to open files larger than this. The editor holds the whole buffer, so
/// an unbounded read would freeze the UI (bounded rendering).
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// One open file in a root's editor panel — everything about it except the
/// buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFile {
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Tab label — the file name.
    pub label: String,
    /// Unsaved edits (drives the amber tab dot; cleared on save).
    pub dirty: bool,
    /// The file's mtime as of open, or of our last save.
    ///
    /// This is what makes Ctrl+S safe here specifically: in this app the
    /// *agent* edits files by design, so a stale buffer overwriting its work is
    /// the expected failure, not an exotic one.
    pub disk_mtime: Option<std::time::SystemTime>,
    /// Process-wide-unique widget-id salt for this tab's buffer.
    ///
    /// Widget operations run against every window, so a path-keyed id would
    /// reach another window's copy of the same file.
    pub uid: u64,
}

/// The set of files one project root has open (tabs), plus the active tab.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RootEditors {
    pub files: Vec<EditorFile>,
    pub active: usize,
}

impl RootEditors {
    pub fn active_file(&self) -> Option<&EditorFile> {
        self.files.get(self.active)
    }

    pub fn active_file_mut(&mut self) -> Option<&mut EditorFile> {
        self.files.get_mut(self.active)
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|f| f.path == path)
    }

    /// Open `path`, or focus its existing tab.
    ///
    /// An already-open file is **never reloaded**: a second click on a path in
    /// a tool card must not discard what the user has typed since the first.
    /// Returns the tab's index, and whether it is new (the caller only has to
    /// build a buffer when it is).
    pub fn open(
        &mut self,
        path: PathBuf,
        mtime: Option<std::time::SystemTime>,
        uid: u64,
    ) -> (usize, bool) {
        if let Some(i) = self.index_of(&path) {
            self.active = i;
            return (i, false);
        }
        self.files.push(EditorFile {
            label: tab_label(&path),
            path,
            dirty: false,
            disk_mtime: mtime,
            uid,
        });
        self.active = self.files.len() - 1;
        (self.active, true)
    }

    /// Close tab `idx`, keeping the active index in bounds.
    pub fn close(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.files.remove(idx);
            if self.active >= idx && self.active > 0 {
                self.active -= 1;
            }
        }
    }

    /// Close every tab at once (discarding unsaved edits) — the one-touch
    /// escape that hands the whole well back to the chat.
    pub fn close_all(&mut self) {
        self.files.clear();
        self.active = 0;
    }
}

/// Outcome of a save.
#[derive(Debug, Clone)]
pub enum SaveOutcome {
    /// Written. `text` is the exact snapshot that landed on disk — the dirty
    /// dot only clears if the buffer still matches it, so keystrokes that
    /// arrived while the write was in flight are not silently marked saved.
    Saved {
        mtime: Option<std::time::SystemTime>,
        text: std::sync::Arc<String>,
    },
    /// The file changed on disk since open or last save (the agent is a
    /// concurrent writer by design) — **nothing was written**. `disk_mtime` is
    /// the current on-disk time; recording it arms the *next* save as an
    /// explicit overwrite.
    Conflict {
        disk_mtime: Option<std::time::SystemTime>,
    },
    Failed(String),
}

/// Read a file for the editor, refusing anything oversized or non-UTF-8.
///
/// Blocking: call it on a background thread, never on a UI loop. Core does not
/// impose a runtime, so the async wrapper belongs to whichever front end has
/// one.
pub fn read_blocking(path: &Path) -> Result<(String, Option<std::time::SystemTime>), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("file too large ({} KB)", meta.len() / 1024));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok((text, meta.modified().ok()))
}

/// Write `text` to `path` unless somebody else wrote it since `expected_mtime`.
///
/// Blocking, for the same reason as [`read_blocking`].
pub fn save_blocking(
    path: &Path,
    text: String,
    expected_mtime: Option<std::time::SystemTime>,
) -> SaveOutcome {
    let on_disk = match std::fs::metadata(path) {
        Ok(meta) => meta.modified().ok(),
        // The file we opened is gone. Falling through to `write` here would
        // silently *recreate* it, which in an app where the agent deletes and
        // renames files by design is the same class of mistake the mtime guard
        // exists to prevent -- so it is a conflict, and the second save is the
        // user saying "yes, put it back".
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if expected_mtime.is_some() {
                return SaveOutcome::Conflict { disk_mtime: None };
            }
            None
        }
        Err(e) => return SaveOutcome::Failed(e.to_string()),
    };
    if let (Some(expected), Some(disk)) = (expected_mtime, on_disk) {
        if disk != expected {
            return SaveOutcome::Conflict {
                disk_mtime: on_disk,
            };
        }
    }
    match std::fs::write(path, text.as_bytes()) {
        Ok(()) => SaveOutcome::Saved {
            mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()),
            text: std::sync::Arc::new(text),
        },
        Err(e) => SaveOutcome::Failed(e.to_string()),
    }
}

/// The syntax token for a file — its extension. Highlighters fall back to plain
/// text for tokens they do not know, which is the right failure here.
pub fn syntax_token(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt")
        .to_string()
}

/// A tab label — the file name (the path tail).
pub fn tab_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_token_is_the_extension() {
        assert_eq!(syntax_token(Path::new("src/main.rs")), "rs");
        assert_eq!(syntax_token(Path::new("a/b.tar.gz")), "gz");
    }

    #[test]
    fn extensionless_files_fall_back_to_txt() {
        assert_eq!(syntax_token(Path::new("Makefile")), "txt");
        assert_eq!(syntax_token(Path::new(".gitignore")), "txt");
    }

    #[test]
    fn tab_label_is_the_file_name() {
        assert_eq!(tab_label(Path::new("/abs/dir/mod.rs")), "mod.rs");
    }

    #[test]
    fn reopening_focuses_the_tab_instead_of_reloading_it() {
        // The second open must report "not new": rebuilding the buffer would
        // throw away edits the user made after the first click.
        let mut ed = RootEditors::default();
        assert_eq!(ed.open(PathBuf::from("/a.rs"), None, 1), (0, true));
        assert_eq!(ed.open(PathBuf::from("/b.rs"), None, 2), (1, true));
        assert_eq!(ed.open(PathBuf::from("/a.rs"), None, 3), (0, false));
        assert_eq!(ed.files.len(), 2);
        assert_eq!(ed.active, 0);
    }

    #[test]
    fn close_all_empties_the_panel() {
        let mut ed = RootEditors::default();
        ed.open(PathBuf::from("/a.rs"), None, 1);
        ed.open(PathBuf::from("/b.rs"), None, 2);
        ed.close_all();
        assert!(ed.files.is_empty());
        assert_eq!(ed.active, 0);
        assert!(ed.active_file().is_none());
    }

    #[test]
    fn a_concurrent_writer_blocks_the_save() {
        let dir = std::env::temp_dir().join("onehand-editor-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("guard.txt");
        std::fs::write(&path, "original").unwrap();

        // An mtime that does not match what is on disk stands for "the agent
        // wrote this file while we were editing it".
        let stale = Some(std::time::SystemTime::UNIX_EPOCH);
        assert!(matches!(
            save_blocking(&path, "ours".into(), stale),
            SaveOutcome::Conflict { .. }
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");

        let fresh = std::fs::metadata(&path).unwrap().modified().ok();
        assert!(matches!(
            save_blocking(&path, "ours".into(), fresh),
            SaveOutcome::Saved { .. }
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ours");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_deleted_file_is_a_conflict_not_a_silent_recreate() {
        // The agent deleting or renaming a file under an open buffer is normal
        // here, so a save must not put it back without being asked twice.
        let dir = std::env::temp_dir().join("onehand-editor-test-deleted");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gone.txt");
        let _ = std::fs::remove_file(&path);

        let opened_at = Some(std::time::SystemTime::UNIX_EPOCH);
        assert!(matches!(
            save_blocking(&path, "ours".into(), opened_at),
            SaveOutcome::Conflict { disk_mtime: None }
        ));
        assert!(
            !path.exists(),
            "the conflict must not have written anything"
        );

        // `Conflict { disk_mtime: None }` is what the panel records, so the
        // next save arrives with no expectation and goes through -- the
        // deliberate second press that recreates the file.
        assert!(matches!(
            save_blocking(&path, "ours".into(), None),
            SaveOutcome::Saved { .. }
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ours");
        let _ = std::fs::remove_file(&path);
    }
}
