use super::*;
use std::path::{Path, PathBuf};

/// A scratch repository named for `case`, one commit on `main`, and a linked
/// worktree of it beside it: `(main, worktree)`, both canonical.
fn repo_with_worktree(case: &str) -> (PathBuf, PathBuf) {
    let git = |dir: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git must be installed to run this test");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let base = std::env::temp_dir().join(format!("onehand-worker-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let main = base.join("main");
    std::fs::create_dir_all(main.join("sub")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.com"]);
    git(&main, &["config", "user.name", "T"]);
    std::fs::write(main.join("sub").join("a.txt"), "x").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "one"]);
    let wt = base.join("wt");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "w", wt.to_str().unwrap()],
    );
    (
        std::fs::canonicalize(&main).unwrap(),
        std::fs::canonicalize(&wt).unwrap(),
    )
}

#[test]
fn a_linked_worktree_of_a_project_is_accepted() {
    let (main, wt) = repo_with_worktree("linked");
    let repo = worktree_of_blocking(&wt, &[main.clone()]).unwrap();
    assert_eq!(repo.top, wt);
    assert_eq!(repo.common, main.join(".git"));
}

#[test]
fn the_worktree_named_through_a_symlink_is_the_same() {
    let (main, wt) = repo_with_worktree("symlink");
    let link = wt.parent().unwrap().join("link");
    std::os::unix::fs::symlink(&wt, &link).unwrap();
    let repo = worktree_of_blocking(&link, &[main]).unwrap();
    assert_eq!(repo.top, wt);
}

#[test]
fn a_folder_inside_a_worktree_is_refused_naming_its_top() {
    let (main, wt) = repo_with_worktree("inside");
    let why = worktree_of_blocking(&wt.join("sub"), &[main]).unwrap_err();
    assert!(why.contains(&wt.display().to_string()), "{why}");
}

#[test]
fn a_project_inside_the_main_checkout_still_matches() {
    let (main, wt) = repo_with_worktree("subproject");
    assert!(worktree_of_blocking(&wt, &[main.join("sub")]).is_ok());
}

#[test]
fn a_repository_no_project_belongs_to_is_refused() {
    let (main, _) = repo_with_worktree("mine");
    let (_, other) = repo_with_worktree("other");
    let why = worktree_of_blocking(&other, &[main]).unwrap_err();
    assert!(why.contains("no project of this window"), "{why}");
}

#[test]
fn a_relative_path_is_refused() {
    let (main, _) = repo_with_worktree("relative");
    assert!(worktree_of_blocking(Path::new("wt"), &[main]).is_err());
}

#[test]
fn writable_refuses_the_root_home_and_the_worktree() {
    let (main, wt) = repo_with_worktree("refuse");
    let repo = repo_of_blocking(&wt).unwrap();
    let base = wt.parent().unwrap().to_path_buf();
    let home = std::env::temp_dir().join(format!("onehand-worker-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    for bad in [
        PathBuf::from("/"),
        home.clone(),
        wt.clone(),
        wt.join("sub"),
        base.clone(),
        main.clone(),
        PathBuf::from("relative"),
        base.join("missing"),
    ] {
        assert!(
            writable_blocking(&[bad.clone()], &repo, Some(&home)).is_err(),
            "{} should be refused",
            bad.display()
        );
    }
}

#[test]
fn writable_refuses_through_a_symlink() {
    let (_, wt) = repo_with_worktree("alias");
    let repo = repo_of_blocking(&wt).unwrap();
    let outside =
        std::env::temp_dir().join(format!("onehand-worker-alias-link-{}", std::process::id()));
    let _ = std::fs::remove_file(&outside);
    std::os::unix::fs::symlink(&wt, &outside).unwrap();
    let why = writable_blocking(&[outside.clone()], &repo, None).unwrap_err();
    assert!(why.contains(&outside.display().to_string()), "{why}");
    assert!(why.contains(&wt.display().to_string()), "{why}");
}

#[test]
fn writable_keeps_an_outside_folder_canonical() {
    let (_, wt) = repo_with_worktree("keep");
    let repo = repo_of_blocking(&wt).unwrap();
    let run = std::env::temp_dir().join(format!("onehand-worker-run-{}", std::process::id()));
    std::fs::create_dir_all(&run).unwrap();
    let kept = writable_blocking(&[run.join("."), run.clone()], &repo, None).unwrap();
    assert_eq!(kept, vec![std::fs::canonicalize(&run).unwrap()]);
}

#[test]
fn worker_dirs_put_the_common_git_first_for_a_linked_worktree() {
    let (main, wt) = repo_with_worktree("first");
    let repo = repo_of_blocking(&wt).unwrap();
    let run = PathBuf::from("/run-folder");
    assert_eq!(
        worker_dirs(&repo, vec![run.clone()]),
        vec![main.join(".git"), run]
    );
}

#[test]
fn worker_dirs_add_nothing_for_the_main_checkout() {
    let (main, _) = repo_with_worktree("plain");
    let repo = repo_of_blocking(&main).unwrap();
    assert!(worker_dirs(&repo, Vec::new()).is_empty());
}
