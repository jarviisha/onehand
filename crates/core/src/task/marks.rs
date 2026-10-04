//! The work pinned as a commit at each step visit's start and end, so what a
//! step did can be shown later as a diff, untracked files included.
//!
//! A commit made from a temporary index, never the person's own, and pointed
//! at by a ref under `refs/onehand/`, so `git gc` keeps it. Not `git stash
//! create`, which leaves untracked files out and makes a commit nothing points
//! at.

use crate::process::output_within;
use crate::workflow::Run;
use crate::worktree::{git, git_message, LOCAL_LIMIT};
use std::path::Path;

/// The ref a visit's mark is kept under.
pub(crate) fn ref_name(task: &str, run: &str, visit: u32, end: bool) -> String {
    let at = if end { "end" } else { "start" };
    format!("refs/onehand/tasks/{task}/{run}/{visit}/{at}")
}

/// The refs of every boundary of `run`, a run of task `task`, from the
/// `from`th on: the ones a driver that pinned `from` of them has still to pin.
pub fn refs_from(task: &str, run: &Run, from: usize) -> Vec<String> {
    run.boundaries()
        .into_iter()
        .skip(from)
        .map(|(visit, end)| ref_name(task, &run.id, visit, end))
        .collect()
}

/// Commit the work at `dir` as it stands, tracked or not and ignored files
/// left out, and point each of `refs` at it. The commit, or why not.
/// Blocking.
pub fn pin_blocking(dir: &Path, refs: &[String]) -> Result<String, String> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let index =
        std::env::temp_dir().join(format!("onehand-mark-{}-{seq}.index", std::process::id()));
    let made = commit(dir, &index);
    let _ = std::fs::remove_file(&index);
    let commit = made?;
    for name in refs {
        run(dir, None, &["update-ref", name, &commit])?;
    }
    Ok(commit)
}

fn commit(dir: &Path, index: &Path) -> Result<String, String> {
    let head = run(dir, None, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]).ok();
    // ponytail: an index read from a tree has no file stats, so `add -A`
    // hashes every file again; seed from a copy of the real index if marks
    // grow slow on a large checkout.
    match &head {
        Some(head) => run(dir, Some(index), &["read-tree", head])?,
        None => run(dir, Some(index), &["read-tree", "--empty"])?,
    };
    run(dir, Some(index), &["add", "-A"])?;
    let tree = run(dir, Some(index), &["write-tree"])?;
    let mut args = vec!["commit-tree", "--no-gpg-sign", tree.as_str()];
    if let Some(head) = &head {
        args.extend(["-p", head]);
    }
    args.extend(["-m", "onehand mark"]);
    run(dir, None, &args)
}

/// `git <args>` in `dir`, on `index` when given, as onehand: a mark needs no
/// identity of the person's. What it printed, trimmed.
fn run(dir: &Path, index: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = git(dir);
    for who in ["AUTHOR", "COMMITTER"] {
        cmd.env(format!("GIT_{who}_NAME"), "onehand")
            .env(format!("GIT_{who}_EMAIL"), "onehand@localhost");
    }
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let out = output_within(cmd.args(args), LOCAL_LIMIT).map_err(|err| format!("git {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
