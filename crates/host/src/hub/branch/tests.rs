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

const BRANCH: &str = "onehand/box";

/// A laptop's repository with one commit, and an inited mirror beside it.
fn laptop_and_mirror(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let laptop = dir.join("laptop");
    fs::create_dir_all(&laptop).unwrap();
    run_git(&laptop, &["init", "-q", "-b", "main"]);
    fs::write(laptop.join("a.txt"), "one\n").unwrap();
    fs::write(laptop.join(".gitignore"), "obj/\n").unwrap();
    run_git(&laptop, &["add", "."]);
    run_git(&laptop, &["commit", "-q", "-m", "one"]);
    let mirror = dir.join("mirror");
    fs::create_dir_all(&mirror).unwrap();
    init_blocking(&mirror).unwrap();
    (laptop, mirror)
}

/// Push the laptop's HEAD into the mirror as a laptop would; its commit.
fn push(laptop: &Path, mirror: &Path) -> String {
    let to = mirror.to_str().unwrap();
    run_git(
        laptop,
        &["push", "-q", "-f", to, "HEAD:refs/onehand/laptop"],
    );
    run_git(laptop, &["rev-parse", "HEAD"])
}

fn commit_here(dir: &Path, file: &str, text: &str) {
    fs::write(dir.join(file), text).unwrap();
    run_git(dir, &["commit", "-q", "-am", "here"]);
}

#[test]
fn a_branch_starts_on_the_laptops_commit() {
    let (laptop, mirror) = laptop_and_mirror("start");
    assert_eq!(head_blocking(&mirror, BRANCH), None);
    let commit = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &commit).unwrap();
    assert_eq!(head_blocking(&mirror, BRANCH), Some(commit));
    assert_eq!(
        run_git(&mirror, &["symbolic-ref", "HEAD"]),
        format!("refs/heads/{BRANCH}")
    );
    assert_eq!(fs::read_to_string(mirror.join("a.txt")).unwrap(), "one\n");
}

#[test]
fn an_update_merges_and_a_conflict_leaves_the_branch_as_it_was() {
    let (laptop, mirror) = laptop_and_mirror("merge");
    let first = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &first).unwrap();
    // The host and the laptop each change another file: a clean merge.
    fs::write(mirror.join("host.txt"), "host\n").unwrap();
    run_git(&mirror, &["add", "."]);
    run_git(&mirror, &["commit", "-q", "-m", "host"]);
    fs::write(laptop.join("b.txt"), "b\n").unwrap();
    run_git(&laptop, &["add", "."]);
    run_git(&laptop, &["commit", "-q", "-m", "b"]);
    let second = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &second).unwrap();
    assert!(mirror.join("b.txt").is_file() && mirror.join("host.txt").is_file());
    // Both change one line: a conflict, and the branch as it was.
    commit_here(&mirror, "a.txt", "host's\n");
    let before = head_blocking(&mirror, BRANCH).unwrap();
    commit_here(&laptop, "a.txt", "laptop's\n");
    let third = push(&laptop, &mirror);
    let why = update_blocking(&mirror, BRANCH, &third).unwrap_err();
    assert!(why.contains("conflict") && why.contains("a.txt"), "{why}");
    assert_eq!(head_blocking(&mirror, BRANCH), Some(before));
    assert!(!mirror.join(".git/MERGE_HEAD").exists());
    assert_eq!(run_git(&mirror, &["status", "--porcelain"]), "");
}

#[test]
fn an_update_commits_what_was_left_first() {
    let (laptop, mirror) = laptop_and_mirror("left");
    let first = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &first).unwrap();
    fs::write(mirror.join("left.txt"), "left\n").unwrap();
    fs::write(laptop.join("b.txt"), "b\n").unwrap();
    run_git(&laptop, &["add", "."]);
    run_git(&laptop, &["commit", "-q", "-m", "b"]);
    let second = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &second).unwrap();
    let log = run_git(&mirror, &["log", "--format=%an %s"]);
    assert!(log.contains(&format!("onehand {LEFT_MESSAGE}")), "{log}");
    assert_eq!(run_git(&mirror, &["status", "--porcelain"]), "");
}

#[test]
fn a_turn_end_commits_on_the_branch_only() {
    let (laptop, mirror) = laptop_and_mirror("turn");
    let first = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &first).unwrap();
    fs::write(mirror.join("new.txt"), "new\n").unwrap();
    fs::create_dir_all(mirror.join("obj")).unwrap();
    fs::write(mirror.join("obj/out.dll"), "built").unwrap();
    assert_eq!(commit_left_blocking(&mirror, BRANCH), Ok(true));
    let tracked = run_git(&mirror, &["ls-files"]);
    assert!(tracked.contains("new.txt") && !tracked.contains("obj/"));
    assert_eq!(commit_left_blocking(&mirror, BRANCH), Ok(false));
    run_git(&mirror, &["checkout", "-q", "-b", "elsewhere"]);
    fs::write(mirror.join("other.txt"), "x").unwrap();
    assert_eq!(commit_left_blocking(&mirror, BRANCH), Ok(false));
    assert!(run_git(&mirror, &["status", "--porcelain"]).contains("other.txt"));
}

#[test]
fn two_commits_at_once_both_land() {
    let (laptop, mirror) = laptop_and_mirror("race");
    let first = push(&laptop, &mirror);
    update_blocking(&mirror, BRANCH, &first).unwrap();
    let threads: Vec<_> = (0..2)
        .map(|n| {
            let mirror = mirror.clone();
            std::thread::spawn(move || {
                fs::write(mirror.join(format!("{n}.txt")), "x").unwrap();
                commit_left_blocking(&mirror, BRANCH)
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap().unwrap();
    }
    assert_eq!(run_git(&mirror, &["status", "--porcelain"]), "");
    let tracked = run_git(&mirror, &["ls-files"]);
    assert!(tracked.contains("0.txt") && tracked.contains("1.txt"));
}
