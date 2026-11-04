use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn clone(url: &Path, into: &Path) -> Result<Reply, String> {
    answer(Call::Clone {
        url: url.display().to_string(),
        into: into.to_path_buf(),
    })
}

#[test]
fn clone_makes_the_folder() {
    let tmp = scratch("clone-makes");
    let repo = tmp.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    fs::write(repo.join("a.txt"), "a").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "one"]);
    let out = tmp.join("out");
    assert!(matches!(clone(&repo, &out), Ok(Reply::Done)));
    assert!(out.join(".git").exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn clone_refuses_an_existing_folder() {
    let tmp = scratch("clone-exists");
    let err = clone(Path::new("/nonexistent"), &tmp).unwrap_err();
    assert_eq!(err, format!("there is already {}", tmp.display()));
    assert!(tmp.exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn clone_refuses_a_relative_folder() {
    let err = clone(Path::new("/nonexistent"), Path::new("out")).unwrap_err();
    assert_eq!(err, "the folder to clone into starts at /");
}

#[test]
fn clone_failure_says_gits_line() {
    let tmp = scratch("clone-fails");
    let out = tmp.join("out");
    let err = clone(Path::new("/nonexistent"), &out).unwrap_err();
    // git's own words, without its `fatal:`.
    assert!(err.contains("/nonexistent"), "{err}");
    assert!(!err.starts_with("fatal:"), "{err}");
    assert!(!out.exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn home_is_the_users_home() {
    let Ok(Reply::Home(home)) = answer(Call::Home) else {
        panic!("no home")
    };
    assert_eq!(Some(home), dirs::home_dir());
}
