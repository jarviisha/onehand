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
/// left out, and point each of `refs` at it. The commit, or why not. Its
/// message names the branch checked out, so a retry can tell the work has
/// moved to another. Blocking.
pub fn pin_blocking(dir: &Path, refs: &[String]) -> Result<String, String> {
    let (tree, head) = with_index(|index| tree(dir, index))?;
    let message = match branch(dir) {
        Some(branch) => format!("onehand mark\n\nBranch: {branch}"),
        None => "onehand mark".to_string(),
    };
    let mut args = vec!["commit-tree", "--no-gpg-sign", tree.as_str()];
    if let Some(head) = &head {
        args.extend(["-p", head]);
    }
    args.extend(["-m", &message]);
    let commit = run(dir, None, &args)?;
    for name in refs {
        run(dir, None, &["update-ref", name, &commit])?;
    }
    Ok(commit)
}

/// How the work at `dir` stands against the mark `commit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Against {
    /// As the mark pinned it.
    Same,
    /// On the same branch, but the commit or the files moved since.
    Changed,
    /// Another branch is checked out than the one the mark was pinned on.
    OtherBranch(String),
}

/// How the work at `dir` stands against the mark `commit`: the branch it
/// was pinned on, then the commit under it and its files. A mark that names
/// no branch skips the first. Blocking.
pub fn against_blocking(dir: &Path, commit: &str) -> Result<Against, String> {
    let message = run(dir, None, &["log", "-1", "--format=%B", commit])?;
    let pinned_on = message
        .lines()
        .find_map(|line| line.strip_prefix("Branch: "))
        .map(str::to_string);
    if let Some(pinned_on) = pinned_on.filter(|on| branch(dir).as_ref() != Some(on)) {
        return Ok(Against::OtherBranch(pinned_on));
    }
    let parent = run(
        dir,
        None,
        &["rev-parse", "-q", "--verify", &format!("{commit}^")],
    )
    .ok();
    let mark_tree = run(dir, None, &["rev-parse", &format!("{commit}^{{tree}}")])?;
    let (tree, head) = with_index(|index| tree(dir, index))?;
    Ok(match parent == head && tree == mark_tree {
        true => Against::Same,
        false => Against::Changed,
    })
}

/// Delete every mark task `task` pinned in the repository at `dir`.
/// Blocking.
pub fn drop_blocking(dir: &Path, task: &str) -> Result<(), String> {
    let prefix = format!("refs/onehand/tasks/{task}/");
    let refs = run(dir, None, &["for-each-ref", "--format=%(refname)", &prefix])?;
    for name in refs.lines().filter(|name| !name.is_empty()) {
        run(dir, None, &["update-ref", "-d", name])?;
    }
    Ok(())
}

/// A file one mark changed against another, with the lines it added and
/// removed; `None` for a binary file, which has no lines to count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub lines: Option<(u32, u32)>,
}

/// Every file that differs between the marks `from` and `to` in the
/// repository at `dir`. Blocking.
pub fn changes_blocking(dir: &Path, from: &str, to: &str) -> Result<Vec<Change>, String> {
    let out = run(
        dir,
        None,
        &["diff", "--numstat", "-z", "--no-renames", from, to],
    )?;
    Ok(out
        .split('\0')
        .filter_map(|entry| {
            let mut parts = entry.trim_start_matches('\n').splitn(3, '\t');
            let (added, removed, path) = (parts.next()?, parts.next()?, parts.next()?);
            Some(Change {
                path: path.to_string(),
                lines: added.parse().ok().zip(removed.parse().ok()),
            })
        })
        .collect())
}

/// The line diff of `path` between the marks `from` and `to`; a side that
/// has no such file reads as empty. Blocking.
pub fn file_diff_blocking(
    dir: &Path,
    from: &str,
    to: &str,
    path: &str,
) -> Result<Vec<crate::diff::Row>, String> {
    let blob = |at: &str| {
        // The mark itself must be there: only a file missing from it reads
        // as empty, never a mark git cannot find.
        run(
            dir,
            None,
            &["rev-parse", "--verify", "-q", &format!("{at}^{{commit}}")],
        )
        .map_err(|_| format!("the mark {at} is not in the repository"))?;
        let spec = format!("{at}:{path}");
        match run(dir, None, &["cat-file", "-e", &spec]) {
            Ok(_) => run_raw(dir, &["show", &spec]),
            Err(_) => Ok(String::new()),
        }
    };
    Ok(crate::diff::rows(&blob(from)?, &blob(to)?))
}

/// The branch checked out at `dir`, or `None` on a detached HEAD.
fn branch(dir: &Path) -> Option<String> {
    run(dir, None, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|name| !name.is_empty())
}

/// Do `then` with a temporary index of its own, removed after.
fn with_index<R>(then: impl FnOnce(&Path) -> R) -> R {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let index =
        std::env::temp_dir().join(format!("onehand-mark-{}-{seq}.index", std::process::id()));
    let made = then(&index);
    let _ = std::fs::remove_file(&index);
    made
}

/// The tree of the work at `dir` as it stands, built on `index`, with the
/// commit checked out, if any.
fn tree(dir: &Path, index: &Path) -> Result<(String, Option<String>), String> {
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
    Ok((tree, head))
}

/// What `git <args>` in `dir` printed, untrimmed: a file's own text.
fn run_raw(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out =
        output_within(git(dir).args(args), LOCAL_LIMIT).map_err(|err| format!("git {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
