//! An offloaded project on a branch, as this machine moves it: no key and no
//! lock; the host's sessions work on a branch of their own in a mirror of
//! its own, and the code travels by git alone over the person's ssh.
//!
//! **Up, only a commit:** `HEAD` is pushed into the mirror, never the
//! folder, so what is not committed here, and every other branch, stays.
//! **Back, only a remote-tracking ref:** the host's branch is fetched into
//! `refs/remotes/<branch>`, which no branch here is, so a fetch never moves
//! or refuses on what the person has checked out. Merging it is theirs.
//!
//! Blocking throughout; a front end runs these off its UI.

use std::path::Path;
use std::process::Command;

use super::{git, git_out, listed_blocking};
use crate::host::client::{HostAddr, SSH_OPTS};
use crate::workspace::HostName;
use crate::worktree::git_message;

/// Where the laptop's commit lands in the mirror, out of every branch's way.
const PUSHED: &str = "refs/onehand/laptop";

/// The branch the host's sessions work on: `onehand/` and the host, made a
/// name git takes whatever the person typed for it.
pub fn branch_for(host: &HostName) -> String {
    let mut name = String::new();
    for c in host.0.chars() {
        let c = match c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            true => c,
            false => '-',
        };
        let last = name.chars().last();
        let doubled = (c == '-' && last == Some('-')) || (c == '.' && last == Some('.'));
        if !doubled {
            name.push(c);
        }
    }
    let mut name = name.trim_matches(['-', '.']).to_string();
    if let Some(stem) = name.strip_suffix(".lock") {
        name = format!("{stem}-lock");
    }
    if name.is_empty() {
        name = "host".into();
    }
    format!("onehand/{name}")
}

/// The ref the host's `branch` is fetched into here.
pub fn tracking(branch: &str) -> String {
    format!("refs/remotes/{branch}")
}

/// The mirror, as git reaches a repository on the host: over ssh, or as a
/// path for the host `local`. A bare IPv6 host is bracketed, as a URL needs.
fn git_url(addr: &HostAddr, mirror: &Path) -> String {
    if addr.local {
        return mirror.display().to_string();
    }
    let (user, machine) = match addr.ssh.rsplit_once('@') {
        Some((user, machine)) => (format!("{user}@"), machine),
        None => (String::new(), addr.ssh.as_str()),
    };
    let machine = match machine.contains(':') && !machine.starts_with('[') {
        true => format!("[{machine}]"),
        false => machine.to_string(),
    };
    format!("ssh://{user}{machine}{}", mirror.display())
}

/// Git in `folder` reaching the host over the attach's ssh options.
fn git_to_host(folder: &Path) -> Command {
    let mut cmd = git(folder);
    cmd.env("GIT_SSH_COMMAND", format!("ssh {}", SSH_OPTS.join(" ")))
        .stdin(std::process::Stdio::null());
    cmd
}

fn said(out: std::io::Result<std::process::Output>) -> Result<(), String> {
    let out = out.map_err(|e| format!("git: {e}"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(git_message(&out.stderr)),
    }
}

/// Push `folder`'s `HEAD` into `mirror` for the host to start its branch
/// from or merge: the commit pushed. Refused for a folder the offload
/// refuses, and for one with no commit yet.
pub fn push_head_blocking(addr: &HostAddr, folder: &Path, mirror: &Path) -> Result<String, String> {
    listed_blocking(folder)?;
    let commit = git_out(folder, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])
        .ok()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            format!(
                "{} has no commit to start the host's branch from",
                folder.display()
            )
        })?;
    let refspec = format!("+{commit}:{PUSHED}");
    said(
        git_to_host(folder)
            .args(["push", "-q", "--no-verify"])
            .arg(git_url(addr, mirror))
            .arg(refspec)
            .output(),
    )?;
    Ok(commit)
}

/// Fetch the host's `branch` from `mirror` into its tracking ref here,
/// forced: the host's branch is the host's to rewrite.
pub fn fetch_blocking(
    addr: &HostAddr,
    folder: &Path,
    mirror: &Path,
    branch: &str,
) -> Result<(), String> {
    let refspec = format!("+refs/heads/{branch}:{}", tracking(branch));
    said(
        git_to_host(folder)
            .args(["fetch", "-q", "--no-tags", "--no-write-fetch-head"])
            .arg(git_url(addr, mirror))
            .arg(refspec)
            .output(),
    )
}

/// The commit the host's `branch` was at when last fetched here.
pub fn local_head_blocking(folder: &Path, branch: &str) -> Option<String> {
    let at = format!("{}^{{commit}}", tracking(branch));
    git_out(folder, &["rev-parse", "-q", "--verify", &at])
        .ok()
        .filter(|id| !id.is_empty())
}

/// How many commits the host's `branch`, as fetched, has that `HEAD` does
/// not: none before the first fetch.
pub fn ahead_blocking(folder: &Path, branch: &str) -> Result<usize, String> {
    if local_head_blocking(folder, branch).is_none() {
        return Ok(0);
    }
    let range = format!("HEAD..{}", tracking(branch));
    git_out(folder, &["rev-list", "--count", &range])?
        .parse()
        .map_err(|e| format!("git rev-list: {e}"))
}

#[cfg(test)]
mod tests;
