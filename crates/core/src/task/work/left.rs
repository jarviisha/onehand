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
    /// It passed on the work as it is: the same commit, and the same
    /// uncommitted work beside it.
    OnThis(String),
    /// It passed on another commit, `commits` before the one checked out.
    Behind { at: String, commits: u64 },
    /// It passed on the commit checked out, and the uncommitted work beside
    /// it, tracked or not, changed since.
    Changed(String),
    /// It passed on the commit checked out, kept from before the uncommitted
    /// work was fingerprinted, and there is uncommitted work now.
    CannotTell(String),
}

impl CheckStands {
    /// In one line.
    pub fn said(&self) -> String {
        match self {
            Self::NotRecorded => "not recorded for this run".to_string(),
            Self::OnThis(at) => format!("passed on the work as it is ({})", short(at)),
            Self::Behind { at, commits: 0 } => {
                format!("passed on {}; the work is on another commit now", short(at))
            }
            Self::Behind { at, commits: 1 } => {
                format!("passed on {}, 1 commit before the work now", short(at))
            }
            Self::Behind { at, commits } => {
                format!(
                    "passed on {}, {commits} commits before the work now",
                    short(at)
                )
            }
            Self::Changed(at) => {
                format!(
                    "passed on {}; the work changed since the check passed",
                    short(at)
                )
            }
            Self::CannotTell(at) => format!(
                "passed on {}; cannot tell whether the check covers the work now",
                short(at)
            ),
        }
    }
}

/// What a passed command vouches for: the commit it ran on, the fingerprint
/// of the uncommitted work beside it when that was kept, and how its output
/// ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vouched {
    pub commit: String,
    pub digest: Option<String>,
    pub tail: Option<String>,
}

/// The work as it is now, as a check is judged against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkNow {
    pub(crate) head: String,
    pub(crate) dirty: bool,
    pub(crate) digest: String,
    /// The commits from the check's to `head`.
    pub(crate) behind: u64,
}

/// How the check that vouched for `vouched` stands against the work `now`.
pub(crate) fn check_stands(vouched: Option<&Vouched>, now: &WorkNow) -> CheckStands {
    let Some(vouched) = vouched else {
        return CheckStands::NotRecorded;
    };
    let at = vouched.commit.clone();
    if at != now.head {
        return CheckStands::Behind {
            at,
            commits: now.behind,
        };
    }
    match &vouched.digest {
        Some(digest) if *digest == now.digest => CheckStands::OnThis(at),
        Some(_) => CheckStands::Changed(at),
        None if now.dirty => CheckStands::CannotTell(at),
        None => CheckStands::OnThis(at),
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
    let check = match &work.vouched {
        None => Ok(CheckStands::NotRecorded),
        Some(vouched) => worktree::head_blocking(dir).and_then(|head| {
            let behind = match head == vouched.commit {
                true => 0,
                false => worktree::commits_since_blocking(dir, &vouched.commit)?,
            };
            let now = WorkNow {
                dirty: worktree::dirty_blocking(dir)?,
                digest: worktree::work_digest_blocking(dir)?,
                head,
                behind,
            };
            Ok(check_stands(Some(vouched), &now))
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
