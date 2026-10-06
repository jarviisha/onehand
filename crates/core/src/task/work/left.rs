//! What an issue's work left, read off git: whether its check still vouches
//! for the work, the files this run and the branch changed, and the commits
//! past where the task started. Read on demand and never in a render.

use super::Work;
use crate::task::marks::{self, Change};
use crate::worktree;

/// How a run's check stands against the work as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStands {
    /// No command passed in this run, or none was recorded.
    NotRecorded,
    /// It passed on the commit checked out, with nothing uncommitted beside
    /// it: it vouches for the work as it is.
    OnThis(String),
    /// It passed on a commit, and the work has moved since: another commit,
    /// or changes not committed.
    Moved(String),
}

impl CheckStands {
    /// In one line.
    pub fn said(&self) -> String {
        match self {
            Self::NotRecorded => "not recorded for this run".to_string(),
            Self::OnThis(at) => format!("passed on the work as it is ({})", short(at)),
            Self::Moved(at) => format!("passed on {}; the work has changed since", short(at)),
        }
    }
}

/// How the check that passed on `verified_at` stands against the work at
/// `head`, with `dirty` saying whether anything is not committed there.
pub(crate) fn check_stands(verified_at: Option<&str>, head: &str, dirty: bool) -> CheckStands {
    match verified_at {
        None => CheckStands::NotRecorded,
        Some(at) if at == head && !dirty => CheckStands::OnThis(at.to_string()),
        Some(at) => CheckStands::Moved(at.to_string()),
    }
}

/// A commit as a person reads it: its first ten characters, as a run's own
/// lines name one.
fn short(commit: &str) -> &str {
    commit.get(..10).unwrap_or(commit)
}

/// What the work left, as one read found it. Each part fails on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    pub check: Result<CheckStands, String>,
    /// The files this run changed, from its first mark to its last.
    pub run_files: Option<Result<Vec<Change>, String>>,
    /// The files the branch changed, from where the task started to this
    /// run's last mark.
    pub branch_files: Option<Result<Vec<Change>, String>>,
    /// The commits past where the task started.
    pub commits: Option<Result<u64, String>>,
}

/// Read what `work` left, in its folder. Blocking: it runs git several times.
pub fn left_blocking(work: &Work) -> Left {
    let dir = &work.dir;
    let check = match &work.verified_at {
        None => Ok(CheckStands::NotRecorded),
        Some(at) => worktree::head_blocking(dir).and_then(|head| {
            let dirty = worktree::dirty_blocking(dir)?;
            Ok(check_stands(Some(at), &head, dirty))
        }),
    };
    Left {
        check,
        run_files: work
            .span
            .as_ref()
            .map(|(first, last)| marks::changes_blocking(dir, first, last)),
        branch_files: work
            .branch_span()
            .map(|(base, last)| marks::changes_blocking(dir, &base, &last)),
        commits: work
            .started_from()
            .map(|base| worktree::commits_since_blocking(dir, &base)),
    }
}

impl Work {
    /// The commit the task started from: the parent of its first mark, which
    /// is the work as the task found it.
    pub fn started_from(&self) -> Option<String> {
        self.base.as_ref().map(|base| format!("{base}^"))
    }

    /// The branch's work as two marks: where the task started from, and
    /// where this run last left the work.
    pub fn branch_span(&self) -> Option<(String, String)> {
        let last = self.span.as_ref().map(|(_, last)| last.clone());
        self.started_from().zip(last)
    }
}
