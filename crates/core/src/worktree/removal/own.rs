//! Removing a worktree that is not a task's: one a person split off, a
//! worker's, or one taken in from git's list.
//!
//! **Nothing a person committed is lost.** It goes only when git reports
//! nothing uncommitted in the checkout and nothing of onehand's, nor any
//! process, uses it; and never while another project records a folder inside
//! the same checkout. Its branch goes only by git's safe delete
//! (`git branch -d`), which takes a merged branch alone: an unmerged one is
//! kept, and the confirmation says so beforehand.

use super::{git, git_message, uncommitted_said, users_said, Judged, Removed, LOCAL_LIMIT};
use crate::process::output_within;
use std::path::{Path, PathBuf};

/// What removing such a worktree is judged on, gathered off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnFacts {
    pub folder: PathBuf,
    /// The branch checked out there; none when detached.
    pub branch: Option<String>,
    /// Every path git says is not committed; `Err` when git could not say.
    pub uncommitted: Result<Vec<String>, String>,
    /// Everything of onehand's, and every process, using the folder, as said.
    pub users: Vec<String>,
    /// The other projects recording a folder inside this checkout, by name.
    pub shared_with: Vec<String>,
}

/// Whether the worktree `facts` describe may be removed, and why not.
pub fn judge_own(facts: &OwnFacts) -> Judged {
    let mut refused: Vec<String> = uncommitted_said(&facts.uncommitted).into_iter().collect();
    refused.extend(users_said(&facts.users));
    refused.extend(
        (facts.shared_with.iter())
            .map(|name| format!("The project {name} has a folder in this checkout.")),
    );
    match refused.is_empty() {
        true => Judged::Remove {
            branch: facts.branch.clone(),
            why: String::new(),
        },
        false => Judged::Refused(refused),
    }
}

/// What git says of the worktree at `folder`; its users and the projects
/// sharing it are the caller's. Blocking.
pub fn own_facts_blocking(folder: &Path, users: Vec<String>, shared_with: Vec<String>) -> OwnFacts {
    OwnFacts {
        folder: folder.to_path_buf(),
        branch: crate::worktree::current_branch_blocking(folder).ok(),
        uncommitted: crate::worktree::read_blocking(folder, &["status", "--porcelain"])
            .map(|out| out.lines().map(str::to_string).collect()),
        users,
        shared_with,
    }
}

/// Whether `branch` is merged into what is checked out at `repo`: what git's
/// safe delete takes. Blocking; git failing reads as not merged.
pub fn branch_merged_blocking(repo: &Path, branch: &str) -> bool {
    crate::worktree::read_blocking(repo, &["branch", "--merged", "HEAD", "--list", branch])
        .is_ok_and(|out| !out.is_empty())
}

/// Remove the worktree at `folder` from the repository at `repo`, then
/// `branch` with `git branch -d`, kept, and said why, when git refuses it.
/// git itself still refuses a worktree with changes in it. Blocking.
pub fn remove_own_blocking(
    repo: &Path,
    folder: &Path,
    branch: Option<&str>,
) -> Result<Removed, String> {
    let out = output_within(
        git(repo).arg("worktree").arg("remove").arg(folder),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git worktree {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    let branch_kept = branch.and_then(|branch| {
        match output_within(git(repo).args(["branch", "-d", branch]), LOCAL_LIMIT) {
            Ok(out) if out.status.success() => None,
            Ok(out) => Some(git_message(&out.stderr)),
            Err(err) => Some(format!("git branch {err}")),
        }
    });
    Ok(Removed { branch_kept })
}

#[cfg(test)]
mod tests;
