//! What happens next for the list of workspaces: every workspace onehand
//! keeps, most recently opened first, and the order written after a change.
//!
//! **Every folder is a workspace.** The list is read once, at launch, from
//! the folders under `<config_dir>/workspaces/` ([`list_blocking`]), and kept
//! in step as workspaces are opened, renamed, made and dropped. Only the
//! order is written (`state.toml`); each workspace's name is its own file's.
//!
//! **Written whole, in order.** Every change of the order answers one write
//! of it as it stands ([`Effect::Write`]), and the front end runs its writes
//! one after another, so an older copy never lands over a newer one. Nothing
//! is put back when a write fails: the order is a convenience, and the next
//! change writes it again. A failure is only logged ([`Signal::WriteFailed`]).

use super::{AppState, WorkspaceConfig, WorkspaceLoad};
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// One workspace in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The folder it is kept in.
    pub dir: PathBuf,
    /// What it is called; its folder's name when its file cannot be read.
    pub name: String,
    /// Why its file cannot be read, when it cannot. It is listed rather than
    /// dropped, and never written over.
    pub unreadable: Option<String>,
}

/// What a person is told, or the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A write of the order failed: why. Only logged.
    WriteFailed(String),
}

/// What the front end carries out.
pub enum Effect {
    /// Write `state` to `path` after every write before it, and report
    /// [`Input::Written`] with `pending`.
    Write {
        pending: u64,
        state: AppState,
        path: PathBuf,
    },
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// The write `pending` landed, or failed.
    Written {
        pending: u64,
        result: Result<(), String>,
    },
}

/// The list's drive. See the module header.
pub struct RecentsDrive {
    state: AppState,
    path: PathBuf,
    listed: Vec<Listed>,
    minted: u64,
}

impl Deref for RecentsDrive {
    type Target = AppState;

    fn deref(&self) -> &AppState {
        &self.state
    }
}

/// A workspace's id: the name of the folder it is kept in.
pub(crate) fn id_of(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every workspace kept under `root`, in no order. Blocking. A folder whose
/// name starts with `.` is one being brought in, and is passed over.
pub(crate) fn list_blocking(root: &Path) -> Vec<Listed> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir() && !id_of(dir).starts_with('.'))
        .filter_map(|dir| match WorkspaceConfig::load_from(&dir) {
            WorkspaceLoad::Found(cfg) => Some(Listed {
                name: cfg.name,
                unreadable: None,
                dir,
            }),
            WorkspaceLoad::Missing => None,
            WorkspaceLoad::Unreadable(why) => Some(Listed {
                name: id_of(&dir),
                unreadable: Some(why),
                dir,
            }),
        })
        .collect()
}

impl RecentsDrive {
    /// Every workspace onehand keeps, in the order last written.
    pub fn load() -> Self {
        Self::new(
            AppState::load(),
            AppState::path(),
            list_blocking(&super::workspaces_dir()),
        )
    }

    /// `listed`, ordered by `state` (written to `path`) first and then by
    /// name. An id in `state` with no folder listed is dropped from it.
    pub fn new(mut state: AppState, path: PathBuf, mut listed: Vec<Listed>) -> Self {
        state
            .workspaces
            .retain(|id| listed.iter().any(|l| id_of(&l.dir) == *id));
        let rank = |l: &Listed| {
            let id = id_of(&l.dir);
            state.workspaces.iter().position(|d| *d == id)
        };
        listed.sort_by(|a, b| match (rank(a), rank(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name).then_with(|| a.dir.cmp(&b.dir)),
        });
        Self {
            state,
            path,
            listed,
            minted: 0,
        }
    }

    /// Every workspace, most recently opened first.
    pub fn listed(&self) -> &[Listed] {
        &self.listed
    }

    /// The workspace in `dir`, called `name`, was opened in a window: it is
    /// the most recent, and listed if it was not yet.
    #[must_use = "its effects are to be carried"]
    pub fn opened(&mut self, dir: PathBuf, name: &str) -> Vec<Effect> {
        self.listed.retain(|l| l.dir != dir);
        self.state.touch(&id_of(&dir));
        self.listed.insert(
            0,
            Listed {
                dir,
                name: name.to_string(),
                unreadable: None,
            },
        );
        vec![self.write()]
    }

    /// The workspace in `dir` is now called `name`. Its own file holds the
    /// name, so nothing is written.
    #[must_use = "its effects are to be carried"]
    pub fn named(&mut self, dir: &Path, name: &str) -> Vec<Effect> {
        if let Some(listed) = self.listed.iter_mut().find(|l| l.dir == dir) {
            listed.name = name.to_string();
        }
        Vec::new()
    }

    /// The workspace in `dir` is gone: its folder was deleted, or is no more.
    #[must_use = "its effects are to be carried"]
    pub fn dropped(&mut self, dir: &Path) -> Vec<Effect> {
        self.listed.retain(|l| l.dir != dir);
        self.state.forget(&id_of(dir));
        vec![self.write()]
    }

    /// What the front end reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Written { result, .. } => match result {
                Ok(()) => Vec::new(),
                Err(why) => vec![Effect::Signal(Signal::WriteFailed(why))],
            },
        }
    }

    fn write(&mut self) -> Effect {
        self.minted += 1;
        Effect::Write {
            pending: self.minted,
            state: self.state.clone(),
            path: self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
