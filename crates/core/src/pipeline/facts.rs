//! What onehand reads of the work itself, never the agent's word for it: the
//! mark a step is measured from, what a turn left, whether a gate holds, and
//! the command a step runs.

use super::template::GateKind;
use crate::worktree;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Where a step's work is measured from: the commit, and the fingerprints of
/// the uncommitted work it may still be at — as the step found it, and after
/// any turn whose change may have been a person's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub head: String,
    pub digests: Vec<String>,
}

impl Mark {
    /// Read the mark for the work at `dir` as it stands. Blocking.
    pub fn read_blocking(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            head: worktree::head_blocking(dir)?,
            digests: vec![worktree::work_digest_blocking(dir)?],
        })
    }
}

/// What the work amounts to after a turn, as git says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The commit the work is at.
    pub head: String,
    /// Changes no commit holds.
    pub dirty: bool,
    /// Commits past the mark's.
    pub commits: u64,
    /// The fingerprint of the uncommitted work.
    pub digest: String,
}

impl Facts {
    /// Read them for the work at `dir`, counting from `from`. Blocking.
    pub fn read_blocking(dir: &Path, from: &Mark) -> Result<Self, String> {
        Ok(Self {
            head: worktree::head_blocking(dir)?,
            dirty: worktree::dirty_blocking(dir)?,
            commits: worktree::commits_since_blocking(dir, &from.head)?,
            digest: worktree::work_digest_blocking(dir)?,
        })
    }
}

/// Whether `gate` holds for a turn that left `facts`, measured from `from`,
/// and answered `answer`.
pub(crate) fn holds(gate: GateKind, facts: &Facts, from: &Mark, answer: &str) -> bool {
    let touched = facts.head != from.head || !from.digests.contains(&facts.digest);
    match gate {
        GateKind::Answered => !answer.trim().is_empty(),
        GateKind::CodeUnchanged => !touched,
        GateKind::CodeChanged => touched,
        GateKind::Committed => !facts.dirty && facts.commits > 0,
        GateKind::Uncommitted => facts.commits == 0,
    }
}

/// How long a step's command may run before it counts as failed.
const COMMAND_LIMIT: Duration = Duration::from_secs(15 * 60);

/// How many lines of a failed command's output are kept. The end is where a
/// build or a test run says what went wrong.
const COMMAND_LINES: usize = 200;

/// Run `command` in `dir`. `Err` holds how its output ended, or why it did
/// not finish. Blocking.
///
/// Through `sh`, because a check is often a chain (`make fmt && cargo test`),
/// with stderr folded into stdout first so the two stay in the order they
/// were written.
pub fn run_command_blocking(dir: &Path, command: &str) -> Result<(), String> {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(format!("exec 2>&1\n{command}"))
        .current_dir(dir);
    let out = crate::process::output_within(&mut cmd, COMMAND_LIMIT).map_err(|why| match why {
        crate::process::Failure::TimedOut(limit) => {
            format!("timed out after {}m", limit.as_secs() / 60)
        }
        why => format!("the command's shell {why}"),
    })?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(COMMAND_LINES)..].join("\n");
    Err(match tail.trim() {
        "" => format!("it exited with {} and printed nothing", out.status),
        _ => tail,
    })
}
