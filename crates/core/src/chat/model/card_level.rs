//! How much of what a session's agent asks it answers by itself: *Cards*.
//!
//! **A level, never a guess about the agent's own modes.** Each agent names
//! its modes its own way, so whether one already approves cannot be told;
//! this decides only the cards the agent does send, whatever its mode.

use super::items::ChatItem;
use super::Chat;
use crate::acp::types::BYPASS_ANSWER;
use crate::acp::{AcpRequest, PermissionRequest, ToolKind};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

/// What a session answers by itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cards {
    /// Every card waits for the person.
    #[default]
    Manual,
    /// A permission to read or edit files is allowed; the rest waits.
    AllowEdits,
    /// Every card is answered, as a host answers them while nobody watches.
    Bypass,
}

impl Cards {
    pub const ALL: [Self; 3] = [Self::Manual, Self::AllowEdits, Self::Bypass];

    /// What a host offers: its switch is on or off for a folder, so the
    /// folder's cards all wait or are all answered.
    pub const ON_A_HOST: [Self; 2] = [Self::Manual, Self::Bypass];

    /// A host's switch for a folder, `on`, as a level.
    pub fn of_host_switch(on: bool) -> Self {
        match on {
            true => Self::Bypass,
            false => Self::Manual,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::AllowEdits => "Allow edits",
            Self::Bypass => "Bypass",
        }
    }

    /// What the level does, in a line.
    pub fn about(self) -> &'static str {
        match self {
            Self::Manual => "Every card waits for you",
            Self::AllowEdits => "Reading and editing files is allowed; the rest waits for you",
            Self::Bypass => "Every card is answered without you",
        }
    }

    /// What the level costs, said wherever it is in force.
    pub fn cost(self) -> Option<&'static str> {
        match self {
            Self::Manual => None,
            Self::AllowEdits => Some(
                "The agent can read and change files in this folder without asking, its git and agent settings aside",
            ),
            Self::Bypass => Some(
                "The agent can do anything your user can on this machine, git push and deleting files included",
            ),
        }
    }

    /// The grant this level gives `req`, asked in a session working in
    /// `root`, by itself: its narrowest, once before always; none where the
    /// level leaves it to the person.
    fn grants(self, req: &PermissionRequest, root: &Path) -> Option<String> {
        let answers = match self {
            Self::Manual => false,
            Self::AllowEdits => {
                matches!(req.kind, ToolKind::Read | ToolKind::Edit) && inside(&req.locations, root)
            }
            Self::Bypass => true,
        };
        match req.bypass_answer().filter(|_| answers)? {
            AcpRequest::PermissionResponse { option_id, .. } => option_id,
            _ => None,
        }
    }
}

/// Folders whose files decide what runs or what an agent may do: git's
/// hooks and settings, and the agents' own settings. Work in one is never
/// allowed by a level that allows edits.
const GUARDED: [&str; 4] = [".git", ".claude", ".codex", ".gemini"];

/// Whether `paths` are all work a level that allows edits may allow: named,
/// absolute, inside `root` with no step out of it, and in none of the
/// guarded folders. Nothing named is nothing allowed.
fn inside(paths: &[std::path::PathBuf], root: &Path) -> bool {
    // Where the folder really is, links resolved: a path spelled inside it
    // through a link that leads out lands out of it.
    let Ok(real_root) = root.canonicalize() else {
        return false;
    };
    !paths.is_empty()
        && paths.iter().all(|path| {
            path.is_absolute()
                && within(path, root)
                && landing(path).is_some_and(|real| within(&real, &real_root))
        })
}

/// Whether `path` is under `root` with no step out of it and in none of the
/// guarded folders, as it is spelled.
fn within(path: &Path, root: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(root) else {
        return false;
    };
    rest.components().all(|part| match part {
        Component::Normal(name) => !GUARDED
            .iter()
            .any(|guarded| name == std::ffi::OsStr::new(guarded)),
        Component::CurDir => true,
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => false,
    })
}

/// Where work at `path` lands: its nearest folder that is there, links
/// resolved, then the rest of it as written, which is yet to be made.
/// Blocking: a stat per step, a handful for one card.
fn landing(path: &Path) -> Option<std::path::PathBuf> {
    let mut there = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = there.canonicalize() {
            return Some(rest.iter().rev().fold(real, |at, part| at.join(part)));
        }
        rest.push(there.file_name()?.to_os_string());
        there = there.parent()?;
    }
}

impl Chat {
    /// The level this session answers its cards at.
    pub fn cards(&self) -> Cards {
        self.cards
    }

    /// Set the level, and answer what already waits that it answers.
    pub(crate) fn set_cards(&mut self, level: Cards) {
        self.cards = level;
        for idx in 0..self.items.len() {
            self.answer_alone(idx);
        }
    }

    /// Answer the card at `idx` if the level answers it and the agent is up
    /// to hear it: whether it did.
    pub(crate) fn answer_alone(&mut self, idx: usize) -> bool {
        if self.tx.is_none() {
            return false;
        }
        match self.items.get(idx) {
            Some(ChatItem::Permission(p)) if p.resolved.is_none() => {
                let Some(option) = self.cards.grants(&p.req, &self.root) else {
                    return false;
                };
                self.answer_permission(idx, &option);
                true
            }
            Some(ChatItem::Ask(_)) if self.cards == Cards::Bypass => {
                // Written into every box that takes words, as a host writes
                // it; a choice is never picked for the agent.
                let Some(ask) = self.ask_at_mut(idx) else {
                    return false;
                };
                for field in 0..ask.req.fields.len() {
                    if ask.has_custom(field) {
                        ask.set_custom(field, BYPASS_ANSWER.to_string());
                    }
                }
                let answered = ask.has_answer();
                self.answer_ask(idx, !answered);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
