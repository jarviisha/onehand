//! `state.toml`: what onehand remembers between launches.

use super::{config_dir, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Global, cross-launch app state at `<config_dir>/onehand/state.toml`.
/// Remembers which workspace storage directories were used, most-recent-first;
/// the next launch reopens `recent_workspaces[0]` (taking precedence over the
/// CLI root).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Legacy single remembered dir (pre-recents). Folded into
    /// `recent_workspaces` on load; mirrors `recent_workspaces[0]` on save so
    /// an older binary reading this file still reopens the right workspace.
    pub(crate) workspace_dir: Option<PathBuf>,
    /// Bound workspace storage dirs, most-recent-first, deduped, capped.
    pub recent_workspaces: Vec<PathBuf>,
    /// Each remembered workspace's name, as last opened or renamed, so a list
    /// of them reads as the rail does without opening every one's own file.
    /// A dir with none, remembered before names were, reads as its folder.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    names: BTreeMap<PathBuf, String>,
}

impl AppState {
    /// Cap on `recent_workspaces`.
    pub const MAX_RECENTS: usize = 8;

    pub(crate) fn path() -> PathBuf {
        config_dir().join("state.toml")
    }

    /// Load state, falling back to the default (nothing remembered) on any error.
    pub fn load() -> Self {
        let mut state = Self::load_from(&Self::path());
        state.migrate();
        state
    }

    /// The state at `path`. One that will not parse is copied to
    /// `state.toml.bad` first: the next save writes the file whole, and
    /// without the copy the recents it held would be gone for good.
    fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            let kept = path.with_extension("toml.bad");
            eprintln!(
                "onehand: bad state {}: {e}; kept as {}",
                path.display(),
                kept.display()
            );
            let _ = std::fs::copy(path, &kept);
            Self::default()
        })
    }

    /// Fold the legacy `workspace_dir` into `recent_workspaces`, dedup, cap.
    fn migrate(&mut self) {
        if let Some(dir) = self.workspace_dir.clone() {
            if !self.recent_workspaces.contains(&dir) {
                self.recent_workspaces.insert(0, dir);
            }
        }
        let mut seen = Vec::new();
        self.recent_workspaces.retain(|d| {
            let fresh = !seen.contains(d);
            if fresh {
                seen.push(d.clone());
            }
            fresh
        });
        self.recent_workspaces.truncate(Self::MAX_RECENTS);
        self.prune_names();
    }

    /// Drop the names of workspaces no longer remembered.
    fn prune_names(&mut self) {
        let recents = &self.recent_workspaces;
        self.names.retain(|dir, _| recents.contains(dir));
    }

    /// Remember `dir`'s name, if `dir` is remembered at all; whether that
    /// changed anything, so a caller writes only what is new.
    pub fn set_name(&mut self, dir: &Path, name: &str) -> bool {
        if !self.recent_workspaces.iter().any(|d| d == dir) || self.name_of(dir) == Some(name) {
            return false;
        }
        self.names.insert(dir.to_path_buf(), name.to_string());
        true
    }

    /// What a list of workspaces calls `dir`: its remembered name, else its
    /// folder's, else the whole path.
    pub fn label_of(&self, dir: &Path) -> String {
        self.name_of(dir).map_or_else(
            || {
                dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string())
            },
            str::to_string,
        )
    }

    /// The name `dir` was last opened or renamed under, if one was remembered.
    pub fn name_of(&self, dir: &Path) -> Option<&str> {
        self.names.get(dir).map(String::as_str)
    }

    /// Move `dir` to the front of the recents (inserting if absent), keep the
    /// list capped, and mirror the legacy field. Pure — callers canonicalize.
    pub fn touch(&mut self, dir: PathBuf) {
        self.recent_workspaces.retain(|d| *d != dir);
        self.recent_workspaces.insert(0, dir);
        self.recent_workspaces.truncate(Self::MAX_RECENTS);
        self.workspace_dir = self.recent_workspaces.first().cloned();
        self.prune_names();
    }

    /// Drop `dir` from the recents and re-mirror the legacy field (used by
    /// Unbind: an unbound workspace must not be reopened at boot).
    pub fn forget(&mut self, dir: &Path) {
        self.recent_workspaces.retain(|d| d != dir);
        self.workspace_dir = self.recent_workspaces.first().cloned();
        self.prune_names();
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&Self::path(), &text)
    }
}

#[cfg(test)]
mod tests;
