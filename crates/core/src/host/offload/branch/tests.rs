use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-branch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn local() -> HostAddr {
    HostAddr {
        ssh: "local".into(),
        local: true,
    }
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(["-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str) -> String {
    fs::write(dir.join(file), text).unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", file]);
    run_git(dir, &["rev-parse", "HEAD"])
}

/// A laptop repository with one commit and an empty mirror repository.
fn laptop_and_mirror(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let (laptop, mirror) = (dir.join("laptop"), dir.join("mirror"));
    fs::create_dir_all(&laptop).unwrap();
    fs::create_dir_all(&mirror).unwrap();
    run_git(&laptop, &["init", "-q", "-b", "main"]);
    commit(&laptop, "a.txt", "a\n");
    run_git(&mirror, &["init", "-q"]);
    (laptop, mirror)
}

const BRANCH: &str = "onehand/box";

#[test]
fn a_branch_is_named_for_its_host() {
    let named = |host: &str| branch_for(&HostName(host.into()));
    assert_eq!(named("box"), "onehand/box");
    assert_eq!(named("user@box.lan"), "onehand/user-box.lan");
    assert_eq!(named("fe80::1"), "onehand/fe80-1");
    assert_eq!(named("user@[fe80::1]"), "onehand/user-fe80-1");
    assert_eq!(named(".."), "onehand/host");
    assert_eq!(named("a..b.lock"), "onehand/a.b-lock");
    assert_eq!(tracking(BRANCH), "refs/remotes/onehand/box");
}

#[test]
fn a_url_names_the_host_as_ssh_does() {
    let url = |host: &str| {
        let addr = HostName(host.into()).addr();
        git_url(&addr, Path::new("/m"))
    };
    assert_eq!(url("box"), "ssh://box/m");
    assert_eq!(url("user@box"), "ssh://user@box/m");
    assert_eq!(url("user@[fe80::1]"), "ssh://user@[fe80::1]/m");
    assert_eq!(url("fe80::1"), "ssh://[fe80::1]/m");
    assert_eq!(url("user@fe80::1"), "ssh://user@[fe80::1]/m");
    assert_eq!(url("local"), "/m");
}

#[test]
fn a_head_with_no_commit_is_refused() {
    let dir = scratch("unborn");
    let (laptop, mirror) = (dir.join("laptop"), dir.join("mirror"));
    fs::create_dir_all(&laptop).unwrap();
    fs::create_dir_all(&mirror).unwrap();
    run_git(&laptop, &["init", "-q"]);
    run_git(&mirror, &["init", "-q"]);
    let why = push_head_blocking(&local(), &laptop, &mirror).unwrap_err();
    assert!(why.contains("no commit"), "{why}");
}

#[test]
fn a_push_carries_the_commit_and_nothing_uncommitted() {
    let (laptop, mirror) = laptop_and_mirror("push");
    fs::write(laptop.join("dirty.txt"), "not committed").unwrap();
    let pushed = push_head_blocking(&local(), &laptop, &mirror).unwrap();
    assert_eq!(pushed, run_git(&laptop, &["rev-parse", "HEAD"]));
    assert_eq!(
        run_git(&mirror, &["rev-parse", "refs/onehand/laptop"]),
        pushed
    );
    let carried = run_git(&mirror, &["ls-tree", "-r", "--name-only", &pushed]);
    assert!(carried.contains("a.txt") && !carried.contains("dirty.txt"));
    // Pushed again from an older commit, it moves back: the laptop's word.
    run_git(&laptop, &["checkout", "-q", "-b", "older", "HEAD"]);
    let newer = commit(&laptop, "b.txt", "b\n");
    push_head_blocking(&local(), &laptop, &mirror).unwrap();
    run_git(&laptop, &["checkout", "-q", "main"]);
    let again = push_head_blocking(&local(), &laptop, &mirror).unwrap();
    assert_ne!(again, newer);
    assert_eq!(
        run_git(&mirror, &["rev-parse", "refs/onehand/laptop"]),
        again
    );
}

#[test]
fn a_fetch_writes_only_the_tracking_ref() {
    let (laptop, mirror) = laptop_and_mirror("fetch");
    let start = push_head_blocking(&local(), &laptop, &mirror).unwrap();
    run_git(&mirror, &["checkout", "-q", "-b", BRANCH, &start]);
    // A branch here named as the host's, checked out: never touched.
    run_git(&laptop, &["checkout", "-q", "-b", BRANCH]);
    let heads = run_git(&laptop, &["for-each-ref", "refs/heads"]);
    assert_eq!(ahead_blocking(&laptop, BRANCH), Ok(0));
    assert_eq!(local_head_blocking(&laptop, BRANCH), None);
    commit(&mirror, "host1.txt", "1\n");
    let host2 = commit(&mirror, "host2.txt", "2\n");
    fetch_blocking(&local(), &laptop, &mirror, BRANCH).unwrap();
    assert_eq!(local_head_blocking(&laptop, BRANCH), Some(host2));
    assert_eq!(ahead_blocking(&laptop, BRANCH), Ok(2));
    assert_eq!(run_git(&laptop, &["for-each-ref", "refs/heads"]), heads);
    // The host's branch rewritten: fetched all the same.
    run_git(&mirror, &["reset", "-q", "--hard", &start]);
    let rewritten = commit(&mirror, "other.txt", "o\n");
    fetch_blocking(&local(), &laptop, &mirror, BRANCH).unwrap();
    assert_eq!(local_head_blocking(&laptop, BRANCH), Some(rewritten));
    assert_eq!(run_git(&laptop, &["for-each-ref", "refs/heads"]), heads);
    assert_eq!(ahead_blocking(&laptop, BRANCH), Ok(1));
    run_git(&laptop, &["merge", "-q", "--no-edit", &tracking(BRANCH)]);
    assert_eq!(ahead_blocking(&laptop, BRANCH), Ok(0));
}
