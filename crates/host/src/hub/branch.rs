//! Git in a branch mirror: the mirror of an offloaded project that works on
//! a branch of its own rather than hold a key. The laptop pushes its commit
//! in; the branch starts from it or merges it, and what a turn leaves behind
//! is committed onto the branch, so nothing the agent did is lost before
//! the laptop fetches it.
//!
//! Blocking throughout; the hub calls these off its lock.

use std::path::Path;
use std::process::{Command, Output};
use std::sync::Mutex;

use onehand_core::worktree::git_message;

/// Every write to a branch mirror's repository, one at a time: two turns
/// ending together, or a turn's end and an update, would race on its index.
// ponytail: one lock for every mirror; per mirror if many offloaded projects
// end turns at once.
static GIT_WRITES: Mutex<()> = Mutex::new(());

/// The message of what a turn's end commits.
pub(super) const LEFT_MESSAGE: &str = "Work left at a turn's end";

/// How many conflicting files a refused merge names.
const CONFLICTS_NAMED: usize = 10;

/// Git in `mirror`, committing as onehand whatever the host's own git says,
/// so a host with no identity set commits all the same.
fn git(mirror: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args([
        "-c",
        "user.name=onehand",
        "-c",
        "user.email=onehand@onehand.invalid",
    ])
    .args(["-c", "commit.gpgsign=false", "-C"])
    .arg(mirror)
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(std::process::Stdio::null());
    cmd
}

fn run(mirror: &Path, args: &[&str]) -> Result<Output, String> {
    git(mirror)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))
}

fn ok(mirror: &Path, args: &[&str]) -> Result<String, String> {
    let out = run(mirror, args)?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
        false => Err(git_message(&out.stderr)),
    }
}

/// Make `mirror` an empty repository the laptop can push into.
pub(super) fn init_blocking(mirror: &Path) -> Result<(), String> {
    ok(mirror, &["init", "-q"]).map(drop)
}

/// The commit `branch` is at in `mirror`, if it has started.
pub(super) fn head_blocking(mirror: &Path, branch: &str) -> Option<String> {
    ok(
        mirror,
        &[
            "rev-parse",
            "-q",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
    .ok()
    .filter(|id| !id.is_empty())
}

/// Bring `commit`, pushed into `mirror`, onto `branch`: the branch starts
/// from it, checked out, or it is merged into the branch once what was left
/// there is committed. A merge that conflicts is undone and named.
pub(crate) fn update_blocking(mirror: &Path, branch: &str, commit: &str) -> Result<(), String> {
    // The laptop sends what `rev-parse` gave it; anything else could read
    // as one of git's options.
    let id = matches!(commit.len(), 40 | 64) && commit.chars().all(|c| c.is_ascii_hexdigit());
    if !id {
        return Err(format!("{commit} is not a commit"));
    }
    let _writing = GIT_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    if head_blocking(mirror, branch).is_none() {
        return ok(mirror, &["checkout", "-q", "-b", branch, commit]).map(drop);
    }
    if !on_branch(mirror, branch) {
        return Err(format!(
            "The host's copy is not on {branch}; it was left as it is"
        ));
    }
    if mid_operation(mirror) {
        return Err(format!(
            "The host's copy is in the middle of a merge or a cherry-pick on {branch}; it was \
             left as it is"
        ));
    }
    commit_left(mirror)?;
    let merged = run(mirror, &["merge", "-q", "--no-edit", commit])?;
    if merged.status.success() {
        return Ok(());
    }
    let conflicts = ok(mirror, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
    // Undone whatever it was, so the branch and its files stay as they were.
    let _ = run(mirror, &["merge", "--abort"]);
    let files: Vec<&str> = conflicts.lines().filter(|l| !l.is_empty()).collect();
    if files.is_empty() {
        return Err(git_message(&merged.stderr));
    }
    let mut named = files
        .iter()
        .take(CONFLICTS_NAMED)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if files.len() > CONFLICTS_NAMED {
        named.push_str(&format!(" and {} more", files.len() - CONFLICTS_NAMED));
    }
    Err(format!(
        "Merging this machine's commit into {branch} stopped on a conflict in: {named}; the \
         host's branch is as it was"
    ))
}

/// Commit what is left in `mirror` onto `branch`, at a turn's end: whether
/// anything was. Nothing while the mirror is on another branch, or in the
/// middle of a merge, a cherry-pick or a rebase: the agent left it so, and
/// it is its to finish.
pub(super) fn commit_left_blocking(mirror: &Path, branch: &str) -> Result<bool, String> {
    let _writing = GIT_WRITES.lock().unwrap_or_else(|e| e.into_inner());
    if !on_branch(mirror, branch) || mid_operation(mirror) {
        return Ok(false);
    }
    commit_left(mirror)
}

/// Whether the agent left `mirror` in the middle of something git would
/// conclude with whatever is there, conflict markers and all: a merge, a
/// cherry-pick, a revert or a rebase, or files still unmerged.
pub(super) fn mid_operation(mirror: &Path) -> bool {
    let git_dir = mirror.join(".git");
    let pending = [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ];
    pending.iter().any(|name| git_dir.join(name).exists())
        || ok(mirror, &["ls-files", "-u"]).is_ok_and(|unmerged| !unmerged.is_empty())
}

/// Whether `mirror`'s HEAD is `branch`.
pub(super) fn on_branch(mirror: &Path, branch: &str) -> bool {
    ok(mirror, &["symbolic-ref", "-q", "HEAD"]).is_ok_and(|r| r == format!("refs/heads/{branch}"))
}

/// Under [`GIT_WRITES`]: everything git does not ignore, committed.
fn commit_left(mirror: &Path) -> Result<bool, String> {
    ok(mirror, &["add", "-A"])?;
    let staged = run(mirror, &["diff", "--cached", "--quiet"])?;
    if staged.status.success() {
        return Ok(false);
    }
    ok(mirror, &["commit", "-q", "-m", LEFT_MESSAGE]).map(|_| true)
}

#[cfg(test)]
mod tests;
