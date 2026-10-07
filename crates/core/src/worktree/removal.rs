//! Removing a run's worktree and its branch after its pull request merged,
//! on a person's request, and only when nothing would be lost.
//!
//! **Never automatic.** A half-done run has work in its worktree, and a
//! merged pull request is the first signal clear enough to act on; even then
//! it is a person's press, judged when its modal opens and again when it is
//! confirmed, since anything can change between the two.
//!
//! **The branch is judged by the forge, not by `git branch -d`.** After a
//! squash or rebase merge the branch's commits are reachable from nothing
//! upstream, so `-d` refuses a branch that is fully merged, and the forge's
//! own branch may already be deleted. What the forge says it merged is the
//! measure: a local branch holding nothing past that head goes with `-D`.

use super::{git, git_message, head_blocking, LOCAL_LIMIT};
use crate::process::output_within;
use std::path::{Path, PathBuf};

/// How many uncommitted files a refusal names before saying how many more.
const UNCOMMITTED_SHOWN: usize = 5;

/// The pull request as the forge says it merged: its number, and the head
/// commit it merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub number: u64,
    pub head: String,
}

/// What a removal is judged on, gathered off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub folder: PathBuf,
    /// The branch checked out in the worktree, `None` when it is detached.
    pub branch: Option<String>,
    /// Every path git says is not committed, untracked ones included, as its
    /// status prints it; `Err` when git could not say.
    pub uncommitted: Result<Vec<String>, String>,
    /// The pull request as the forge says it merged; `Ok(None)` when it is
    /// not merged, `Err` when the forge could not be read.
    pub merged: Result<Option<Merged>, String>,
    /// How many commits the worktree holds that the merged head does not;
    /// `Err` when it cannot be told.
    pub past_merge: Result<u64, String>,
    /// Everything of onehand's using the folder, in any window, as said.
    pub users: Vec<String>,
}

/// What the judgement came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judged {
    /// The worktree goes, and `branch` with `git branch -D`; `why` says why
    /// the branch may go.
    Remove { branch: Option<String>, why: String },
    /// Nothing is removed, for every reason listed.
    Refused(Vec<String>),
}

/// Whether the worktree `facts` describe may be removed, and why not.
pub fn judge(facts: &Facts) -> Judged {
    let mut refused = Vec::new();
    let merged = match &facts.merged {
        Ok(Some(merged)) => Some(merged),
        Ok(None) => {
            refused.push("Its pull request is not merged.".to_string());
            None
        }
        Err(why) => {
            refused.push(format!("The forge could not be read: {why}"));
            None
        }
    };
    if merged.is_some() {
        match &facts.past_merge {
            Ok(0) => {}
            Ok(n) => refused.push(format!(
                "The branch has {n} {} past the head the forge merged.",
                if *n == 1 { "commit" } else { "commits" }
            )),
            Err(why) => refused.push(format!(
                "Whether the branch holds work past the merge cannot be told: {why}"
            )),
        }
    }
    match &facts.uncommitted {
        Ok(files) if files.is_empty() => {}
        Ok(files) => {
            let shown: Vec<&str> = files
                .iter()
                .take(UNCOMMITTED_SHOWN)
                .map(|f| f.trim())
                .collect();
            let more = files.len().saturating_sub(UNCOMMITTED_SHOWN);
            let more = match more {
                0 => String::new(),
                n => format!(" and {n} more"),
            };
            refused.push(format!(
                "Uncommitted or untracked files: {}{more}.",
                shown.join(", ")
            ));
        }
        Err(why) => refused.push(format!("git could not say what is uncommitted: {why}")),
    }
    refused.extend(
        facts
            .users
            .iter()
            .map(|user| format!("{user} uses the folder.")),
    );
    match (refused.is_empty(), merged) {
        (true, Some(merged)) => Judged::Remove {
            branch: facts.branch.clone(),
            why: format!(
                "The forge merged #{} at {}, and the branch holds nothing past it, so it is \
                 deleted with `git branch -D`.",
                merged.number,
                short(&merged.head)
            ),
        },
        _ => Judged::Refused(refused),
    }
}

/// A commit as a person reads it.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// What git says of the worktree at `folder`, judged against `merged`; the
/// forge's answer and the folder's users are the caller's.
pub fn facts_blocking(
    folder: &Path,
    merged: Result<Option<Merged>, String>,
    users: Vec<String>,
) -> Facts {
    let branch = super::current_branch_blocking(folder).ok();
    let uncommitted = super::read_blocking(folder, &["status", "--porcelain"])
        .map(|out| out.lines().map(str::to_string).collect());
    let past_merge = match &merged {
        Ok(Some(merged)) => past_blocking(folder, &merged.head),
        Ok(None) | Err(_) => Ok(0),
    };
    Facts {
        folder: folder.to_path_buf(),
        branch,
        uncommitted,
        merged,
        past_merge,
        users,
    }
}

/// How many commits `HEAD` at `folder` holds that `head` does not. A head
/// this clone has never seen cannot be measured against.
fn past_blocking(folder: &Path, head: &str) -> Result<u64, String> {
    if super::read_blocking(folder, &["cat-file", "-e", &format!("{head}^{{commit}}")]).is_err() {
        return Err(format!(
            "the commit the forge merged, {}, is not in this clone",
            short(head)
        ));
    }
    // Nothing is past it when the worktree is at it already.
    if head_blocking(folder).is_ok_and(|at| at == head) {
        return Ok(0);
    }
    super::commits_since_blocking(folder, head)
}

/// What a removal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// Why the branch was kept when deleting it failed, the worktree being
    /// removed all the same.
    pub branch_kept: Option<String>,
}

/// Remove the worktree at `folder` from the repository at `repo`, then
/// `branch` with `-D`. git itself still refuses a worktree with changes in
/// it, since nothing here forces it.
pub fn remove_blocking(
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
        let out = output_within(git(repo).args(["branch", "-D", branch]), LOCAL_LIMIT);
        match out {
            Ok(out) if out.status.success() => None,
            Ok(out) => Some(git_message(&out.stderr)),
            Err(err) => Some(format!("git branch {err}")),
        }
    });
    Ok(Removed { branch_kept })
}

#[cfg(test)]
mod tests;
