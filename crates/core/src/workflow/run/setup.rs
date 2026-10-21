//! What a run is asked to do, where it works, and the points it measures
//! itself from.

use crate::workflow::Mark;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What a run is asked to do: a title, a body, and what the person starting
/// it asked of every step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    pub title: String,
    pub body: String,
    pub instructions: Option<String>,
}

/// Where a run works and with what, as chosen when it started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    /// The project it was started from.
    pub repo: PathBuf,
    /// Where its work is: the project itself, or the worktree made for it.
    pub dir: PathBuf,
    /// The branch made for it, on a worktree.
    pub branch: Option<String>,
    /// The configured agent it runs, by name; the default one when unset.
    pub agent: Option<String>,
    /// The project's check command, for a command step that names none.
    pub check: Option<String>,
    /// The session mode the agent is put in before the first prompt, by the
    /// adapter's own id; left as the agent starts when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The connector the branch goes to, by name; `None` where no forge
    /// serves the project, and then a step on the forge passes at once, the
    /// branch being the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<String>,
}

/// The points in the work a run measures from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// Where the current agent step's work is measured from.
    pub step_from: Option<Mark>,
    /// The commit a command last passed on.
    pub verified_at: Option<String>,
}
