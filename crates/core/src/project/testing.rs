//! What the project tests share: scratch folders and real repositories.

use std::path::{Path, PathBuf};

/// A fresh folder of its own for one test, named for `case`.
pub(crate) fn scratch(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "onehand-projects-{case}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run git in `dir`, failing the test when it fails.
pub(crate) fn git(dir: &Path, args: &[&str]) {
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
}

/// A scratch repository named for `case`, one commit on `main` holding
/// `sub/a.txt`, and a linked worktree of it on branch `w` beside it:
/// `(main, worktree)`, both canonical.
pub(crate) fn repo_with_worktree(case: &str) -> (PathBuf, PathBuf) {
    let base = scratch(case);
    let main = base.join("main");
    std::fs::create_dir_all(main.join("sub")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.com"]);
    git(&main, &["config", "user.name", "T"]);
    std::fs::write(main.join("sub").join("a.txt"), "x").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "one"]);
    let wt = base.join("wt");
    git(&main, &["worktree", "add", "-q", "-b", "w", wt.to_str().unwrap()]);
    (
        std::fs::canonicalize(&main).unwrap(),
        std::fs::canonicalize(&wt).unwrap(),
    )
}
