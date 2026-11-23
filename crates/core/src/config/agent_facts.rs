//! What each agent offered when a session of it last came up: its modes,
//! its config groups with the values then in force, and its `/` commands.
//! Kept across launches in `agents-seen.toml`, by the agent's name, so a
//! project's page can offer them before any session of the agent runs.
//!
//! **What the agent said, never a list of onehand's own.** Nothing here is
//! written by hand or shipped: an agent that changes its models is read
//! again the next time one of its sessions comes up.

use super::{config_dir, write_atomic};
use crate::acp::{ConfigOption, Mode, SlashCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What one agent last offered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentFacts {
    pub modes: Vec<Mode>,
    /// The mode then in force.
    pub current_mode: Option<String>,
    /// Its config groups, each with the value then in force.
    pub options: Vec<ConfigOption>,
    pub commands: Vec<SlashCommand>,
}

/// Every agent's facts, by its name, as the file keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactsFile {
    pub agents: BTreeMap<String, AgentFacts>,
}

impl FactsFile {
    pub fn path() -> PathBuf {
        config_dir().join("agents-seen.toml")
    }

    /// The facts at `path`; none when there is no file. One that will not
    /// parse is copied to `agents-seen.toml.bad` first, since the next write
    /// replaces it whole.
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            let kept = path.with_extension("toml.bad");
            eprintln!(
                "onehand: bad agent facts {}: {e}; kept as {}",
                path.display(),
                kept.display()
            );
            let _ = std::fs::copy(path, &kept);
            Self::default()
        })
    }

    /// Write the facts to `path` whole, blocking: the front end runs it off
    /// its UI thread, in order with its other writes.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, &text)
    }
}

#[cfg(test)]
mod tests;
