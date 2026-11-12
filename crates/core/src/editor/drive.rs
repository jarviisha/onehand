//! What happens next for the files one Workbench has open: a read landing as
//! a tab, a save and the one asked for while it ran, a file written by
//! somebody else since it was read, a folder that may not be written, and a
//! close that would throw edits away.
//!
//! **One per Workbench**, every project's tabs in it, as the Editor mode holds
//! them. Which project is on screen is the view's, so every command names its
//! project.
//!
//! **The text is the front end's.** A buffer is framework-shaped, so the drive
//! never holds the words in it: it says when a buffer is to be built from what
//! was read, and a write takes the buffer's text as it stands when the write
//! is carried out, which is what lets a save asked for while one ran pick up
//! the keystrokes that prompted it. Whether the buffer still holds what
//! landed comes back with the write's answer (`still`), as a view fact the
//! drive depends on.

use super::{Editors, Standing};
use crate::disk::Disk;
use crate::editor::SaveOutcome;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What is said once, where the Workbench is.
#[derive(Debug, PartialEq, Eq)]
pub enum Signal {
    /// `path` could not be read to open it: why.
    OpenFailed { path: PathBuf, why: String },
    /// The file `label` could not be written: why. A conflict is not this,
    /// but a [`Standing`].
    SaveFailed { label: String, why: String },
}

/// What the front end carries out.
pub enum Effect {
    /// Read `path` through `disk`, off the UI loop, and report
    /// [`Input::Read`]. A late answer is **dropped** when its project has left
    /// the Workbench, and otherwise opens the file: a read leaves nothing
    /// behind.
    Read {
        root: PathBuf,
        path: PathBuf,
        disk: Disk,
    },
    /// Build the buffer of tab `uid` in `root`, holding `text`.
    Buffer {
        root: PathBuf,
        uid: u64,
        path: PathBuf,
        text: String,
    },
    /// Write the buffer of tab `uid` as its text stands now to `path`
    /// through `disk`, unless somebody wrote it since `mtime`
    /// ([`Disk::save_file`]), off the UI loop, and report [`Input::Saved`]. A
    /// late answer is **recorded**: the file is written or it is not,
    /// whatever changed since.
    Write {
        root: PathBuf,
        uid: u64,
        path: PathBuf,
        mtime: Option<SystemTime>,
        disk: Disk,
    },
    /// Drop the buffer of tab `uid`.
    Forget {
        uid: u64,
    },
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// What reading `path` in `root` found, and the id its tab takes if it
    /// opens one. The front end hands the id in: it must not repeat in
    /// another window's copy of the same file, and a replay of the same
    /// inputs gives the same tabs.
    Read {
        root: PathBuf,
        path: PathBuf,
        read: Result<(String, Option<SystemTime>), String>,
        uid: u64,
    },
    /// What writing tab `uid` in `root` came to, and whether its buffer
    /// still holds what landed (`false` when it was typed into meanwhile, or
    /// is gone).
    Saved {
        root: PathBuf,
        uid: u64,
        outcome: SaveOutcome,
        still: bool,
    },
}

/// What a close asked for comes to.
pub enum Closing {
    /// Closed, with these effects.
    Closed(Vec<Effect>),
    /// It would throw away edits: ask first, about this many files (the tab
    /// named by `label` when it is one). [`EditorDrive::discard`] with
    /// `close` closes once the person agrees.
    Asks {
        close: Close,
        unsaved: usize,
        label: Option<String>,
    },
}

/// What a close is about: one tab of a project, or all of its tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Close {
    Tab(PathBuf, u64),
    All(PathBuf),
}

/// A write on its way: whether another save was asked for while it ran, and
/// the file's label.
struct Saving {
    again: bool,
    label: String,
}

/// The files one Workbench has open. See the module header.
#[derive(Default)]
pub struct EditorDrive {
    editors: Editors,
    /// Tabs with a write in flight, and whether another save was asked for
    /// while it ran. Two writes of one tab must not overlap: each carries the
    /// time the file was last seen, so the second would carry the time from
    /// before the first wrote and come back as a conflict against our own
    /// write. The second is not dropped either: it runs once the first lands.
    /// Each with the file's label, so a write that failed after its tab
    /// closed is still said by name.
    saving: HashMap<u64, Saving>,
}

impl Deref for EditorDrive {
    type Target = Editors;
    fn deref(&self) -> &Editors {
        &self.editors
    }
}

impl EditorDrive {
    /// `root`'s files are on `disk`. Told again whenever the disk may have
    /// changed: a host's attach can be replaced while its project stays.
    pub fn set_root(&mut self, root: &Path, disk: &Disk) {
        self.editors.disks.insert(root.to_path_buf(), disk.clone());
    }

    /// `root` has left the workspace: its tabs go, unsaved edits and all,
    /// since there is nowhere left to save them to.
    #[must_use = "its effects are to be carried"]
    pub fn forget_root(&mut self, root: &Path) -> Vec<Effect> {
        let editors = &mut self.editors;
        editors.disks.remove(root);
        editors.standing.remove(root);
        let Some(tabs) = editors.roots.remove(root) else {
            return Vec::new();
        };
        tabs.files
            .iter()
            .map(|file| {
                editors.read_through.remove(&file.uid);
                Effect::Forget { uid: file.uid }
            })
            .collect()
    }

    /// Which folders may not be written now, and why, whole.
    pub fn set_locked(&mut self, locks: &HashMap<PathBuf, String>) {
        self.editors.locked = locks.clone();
    }

    /// Open `path` in `root`, and say whether this mode took it: focused when
    /// it is open already, never read again (a second click on a path must
    /// not throw away what was typed since the first), and read otherwise.
    #[must_use = "its effects are to be carried"]
    pub fn open(&mut self, root: &Path, path: &Path) -> (bool, Vec<Effect>) {
        let Some(disk) = self.editors.disks.get(root).cloned() else {
            return (false, Vec::new());
        };
        let tabs = self.editors.roots.get_mut(root);
        if let Some((tabs, i)) = tabs.and_then(|t| t.index_of(path).map(|i| (t, i))) {
            tabs.active = i;
            return (true, Vec::new());
        }
        self.editors.opening = Some(path.to_path_buf());
        let read = Effect::Read {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            disk,
        };
        (true, vec![read])
    }

    /// Show tab `idx` of `root`.
    pub fn select(&mut self, root: &Path, idx: usize) {
        if let Some(tabs) = self.editors.roots.get_mut(root) {
            if idx < tabs.files.len() {
                tabs.active = idx;
            }
        }
    }

    /// Tab `uid` in `root` was typed into.
    pub fn typed(&mut self, root: &Path, uid: u64) {
        if let Some(file) = self.file_mut(root, uid) {
            file.dirty = true;
        }
    }

    /// Save the tab on screen in `root`.
    #[must_use = "its effects are to be carried"]
    pub fn save_active(&mut self, root: &Path) -> Vec<Effect> {
        let Some(uid) = self
            .editors
            .roots
            .get(root)
            .and_then(|tabs| tabs.active_file())
            .map(|file| file.uid)
        else {
            return Vec::new();
        };
        self.save(root, uid)
    }

    /// Write tab `uid` of `root`, unless somebody wrote its file since it was
    /// last seen. Asked while its last write is on its way, it waits for that
    /// one; asked of a folder that may not be written, it writes nothing (the
    /// line under the body says why).
    #[must_use = "its effects are to be carried"]
    pub fn save(&mut self, root: &Path, uid: u64) -> Vec<Effect> {
        if let Some(saving) = self.saving.get_mut(&uid) {
            saving.again = true;
            return Vec::new();
        }
        if self.editors.locked.contains_key(root) {
            return Vec::new();
        }
        let Some(disk) = self.editors.save_disk(root, uid) else {
            return Vec::new();
        };
        let Some(file) = self.file_mut(root, uid) else {
            return Vec::new();
        };
        let write = Effect::Write {
            root: root.to_path_buf(),
            uid,
            path: file.path.clone(),
            mtime: file.disk_mtime,
            disk,
        };
        let label = file.label.clone();
        self.saving.insert(
            uid,
            Saving {
                again: false,
                label,
            },
        );
        vec![write]
    }

    /// Ask to close tab `idx` of `root`: closed at once unless it holds edits.
    #[must_use = "its effects are to be carried"]
    pub fn close_tab(&mut self, root: &Path, idx: usize) -> Closing {
        let Some(file) = self.editors.roots.get(root).and_then(|t| t.files.get(idx)) else {
            return Closing::Closed(Vec::new());
        };
        let close = Close::Tab(root.to_path_buf(), file.uid);
        if file.dirty {
            return Closing::Asks {
                close,
                unsaved: 1,
                label: Some(file.label.clone()),
            };
        }
        Closing::Closed(self.discard(&close))
    }

    /// Ask to close every tab of `root`: closed at once unless one holds edits.
    #[must_use = "its effects are to be carried"]
    pub fn close_all(&mut self, root: &Path) -> Closing {
        let close = Close::All(root.to_path_buf());
        match self.editors.unsaved(root) {
            0 => Closing::Closed(self.discard(&close)),
            unsaved => Closing::Asks {
                close,
                unsaved,
                label: None,
            },
        }
    }

    /// Close without asking; the question, if there was one, is answered.
    /// Named by project and a tab by `uid`, so it still means what it said
    /// if the strip changed while it was asked.
    #[must_use = "its effects are to be carried"]
    pub fn discard(&mut self, close: &Close) -> Vec<Effect> {
        let editors = &mut self.editors;
        let (Close::Tab(root, _) | Close::All(root)) = close;
        let Some(tabs) = editors.roots.get_mut(root) else {
            return Vec::new();
        };
        let gone: Vec<u64> = match close {
            Close::Tab(_, uid) => {
                let Some(idx) = tabs.files.iter().position(|f| f.uid == *uid) else {
                    return Vec::new();
                };
                tabs.close(idx);
                vec![*uid]
            }
            Close::All(_) => {
                let uids = tabs.files.iter().map(|f| f.uid).collect();
                tabs.close_all();
                uids
            }
        };
        editors.standing.remove(root);
        gone.into_iter()
            .map(|uid| {
                editors.read_through.remove(&uid);
                Effect::Forget { uid }
            })
            .collect()
    }

    /// What the world reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Read {
                root,
                path,
                read,
                uid,
            } => self.read(root, path, read, uid),
            Input::Saved {
                root,
                uid,
                outcome,
                still,
            } => self.saved(root, uid, outcome, still),
        }
    }

    fn read(
        &mut self,
        root: PathBuf,
        path: PathBuf,
        read: Result<(String, Option<SystemTime>), String>,
        uid: u64,
    ) -> Vec<Effect> {
        let editors = &mut self.editors;
        if editors.opening.as_ref() == Some(&path) {
            editors.opening = None;
        }
        // A read about a project that has left the Workbench is dropped,
        // whatever it found.
        let Some(disk) = editors.disks.get(&root).cloned() else {
            return Vec::new();
        };
        let (text, mtime) = match read {
            Ok(read) => read,
            Err(why) => return vec![Effect::Signal(Signal::OpenFailed { path, why })],
        };
        // A read that worked answers whatever the last save said here.
        editors.standing.remove(&root);
        let (_, new) =
            editors
                .roots
                .entry(root.clone())
                .or_default()
                .open(path.clone(), mtime, uid);
        if !new {
            return Vec::new();
        }
        editors.read_through.insert(uid, disk);
        vec![Effect::Buffer {
            root,
            uid,
            path,
            text,
        }]
    }

    fn saved(&mut self, root: PathBuf, uid: u64, outcome: SaveOutcome, still: bool) -> Vec<Effect> {
        let Some(Saving { again, label }) = self.saving.remove(&uid) else {
            return Vec::new();
        };
        let Some(file) = self.file_mut(&root, uid) else {
            // The tab closed while it was written: the write happened or it
            // did not, and a failure is still said.
            return match outcome {
                SaveOutcome::Failed(why) => vec![Effect::Signal(Signal::SaveFailed { label, why })],
                SaveOutcome::Saved { .. } | SaveOutcome::Conflict { .. } => Vec::new(),
            };
        };
        let (standing, failed) = match outcome {
            SaveOutcome::Saved { mtime, .. } => {
                file.disk_mtime = mtime;
                // Keystrokes that came in while the write was on its way
                // are still unsaved.
                file.dirty = !still;
                (None, None)
            }
            SaveOutcome::Conflict { disk_mtime } => {
                // The next save is an informed overwrite.
                file.disk_mtime = disk_mtime;
                let standing = match disk_mtime {
                    Some(_) => Standing::Changed { label },
                    None => Standing::Gone { label },
                };
                (Some(standing), None)
            }
            // Said once, rather than standing: a conflict stands until a
            // save answers it, and a write that failed does not.
            SaveOutcome::Failed(why) => (None, Some(Signal::SaveFailed { label, why })),
        };
        match standing {
            Some(standing) => self.editors.standing.insert(root.clone(), standing),
            None => self.editors.standing.remove(&root),
        };
        let mut effects: Vec<Effect> = failed.map(Effect::Signal).into_iter().collect();
        if again {
            effects.extend(self.save(&root, uid));
        }
        effects
    }

    fn file_mut(&mut self, root: &Path, uid: u64) -> Option<&mut super::EditorFile> {
        self.editors
            .roots
            .get_mut(root)?
            .files
            .iter_mut()
            .find(|f| f.uid == uid)
    }
}

#[cfg(test)]
mod tests;
