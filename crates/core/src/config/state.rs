//! `state.toml`: what onehand remembers between launches.

use super::{config_dir, write_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Global, cross-launch app state at `<config_dir>/onehand/state.toml`:
/// which workspaces were opened, most recent first, by the name of the folder
/// each is kept in under `<config_dir>/workspaces/`. The next launch opens the
/// first of them that loads.
///
/// Only the order is kept here. Which workspaces there are is whatever folders
/// that directory holds, and what each is called is in its own file, so
/// nothing here can disagree with them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Workspace ids, most recently opened first, each once.
    pub(crate) workspaces: Vec<String>,
    /// What an earlier build kept: the folders its workspaces were in, and
    /// their names. Read once, to bring those workspaces in; kept as read
    /// until that has landed, so a copy that failed is tried again at the
    /// next launch, and written no more once it is cleared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) recent_workspaces: Vec<PathBuf>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) names: BTreeMap<PathBuf, String>,
}

impl AppState {
    pub(crate) fn path() -> PathBuf {
        config_dir().join("state.toml")
    }

    /// Load state, falling back to the default (nothing remembered) on any error.
    pub(crate) fn load() -> Self {
        Self::load_from(&Self::path())
    }

    /// The state at `path`. One that will not parse is copied to
    /// `state.toml.bad` first: the next save writes the file whole, and
    /// without the copy what it held would be gone for good.
    pub(crate) fn load_from(path: &Path) -> Self {
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

    /// Move `id` to the front, adding it if absent.
    pub(crate) fn touch(&mut self, id: &str) {
        self.workspaces.retain(|d| d != id);
        self.workspaces.insert(0, id.to_string());
    }

    /// Drop `id`.
    pub(crate) fn forget(&mut self, id: &str) {
        self.workspaces.retain(|d| d != id);
    }

    /// Write the state to `path`, blocking: the front end runs it off its
    /// UI thread, in order with every other write of it.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, &text)
    }
}

#[cfg(test)]
mod tests;
