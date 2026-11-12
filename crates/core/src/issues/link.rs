//! Where a session and an issues file meet: whether an issue can be worked
//! by hand here at all, and the note a session that took an issue up by hand
//! owes the issue, written once the session's agent has named its
//! conversation, which is what lets the issue open it again.

use super::drive::Change;
use crate::chat::Chat;
use std::path::PathBuf;

/// What the note says, as a person reading the issue's history sees it.
const TAKEN_UP_HERE: &str = "Taken up by a session on this checkout";

/// The note a session owes issue `number` of `file`, until it is told.
pub struct TakenUp {
    file: PathBuf,
    number: u64,
    told: bool,
}

impl TakenUp {
    pub fn new(file: PathBuf, number: u64) -> Self {
        Self {
            file,
            number,
            told: false,
        }
    }

    /// The session is as `chat` says: the file and the change to write in
    /// it, the first time its conversation is named, and never again.
    pub fn heard(&mut self, chat: &Chat) -> Option<(PathBuf, Change)> {
        let session = chat.session_id.clone().filter(|_| !self.told)?;
        self.told = true;
        let change = Change::TakenUp {
            number: self.number,
            text: TAKEN_UP_HERE.to_string(),
            session,
        };
        Some((self.file.clone(), change))
    }
}

/// Why an issue cannot be worked by hand where it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotHere {
    /// Its project is no longer in the workspace.
    ProjectGone,
    /// The workspace keeps no issues, so there is nowhere to note the session.
    NoIssuesFile,
}

/// Whether an issue of a project can be worked by hand: the project, as
/// the workspace holds it, and the file its note goes in, or why not.
/// `project` is the project if it is still in the workspace, and `file` the
/// issues file the workspace keeps for it, if it keeps one.
pub fn work_here<P>(project: Option<P>, file: Option<PathBuf>) -> Result<(P, PathBuf), NotHere> {
    match (project, file) {
        (None, _) => Err(NotHere::ProjectGone),
        (Some(_), None) => Err(NotHere::NoIssuesFile),
        (Some(project), Some(file)) => Ok((project, file)),
    }
}

#[cfg(test)]
mod tests;
