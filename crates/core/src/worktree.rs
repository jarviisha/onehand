//! Splitting a project root onto its own branch, as a git worktree.
//!
//! A worktree is a second checkout of one repository: another directory, on
//! another branch, sharing the same history and the same object store. That is
//! exactly the shape the workspace tree already has a slot for — the new
//! directory is just another project root, with its own file tree, its own
//! terminal and its own sessions, because every per-root map in the app is
//! keyed by path.
//!
//! The rules live here rather than at the call site for the usual reason: the
//! name check has to answer *before* anything is created, so the dialog can say
//! what is wrong with a name while it is being typed, and the same rule has to
//! be the one `git worktree add` is finally handed.

use crate::workspace::label_for;
use std::path::{Path, PathBuf};

/// Why a branch name cannot be used, written for the person typing it.
///
/// A subset of `git check-ref-format`, kept to the mistakes a person actually
/// makes: git's full rule set includes cases (a trailing `.lock`, a component
/// beginning with a dot) nobody types by hand but which still have to be
/// refused, so they are checked and simply share one message. Anything this
/// misses is caught by git itself and surfaces as the command's own error —
/// this exists to answer *early*, not to be the only gate.
pub fn validate_branch(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Give the branch a name.");
    }
    if name == "@" {
        return Err("`@` on its own is git's name for HEAD, so a branch cannot take it.");
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err("Branch names cannot hold spaces.");
    }
    if name
        .chars()
        .any(|c| c.is_control() || "~^:?*[\\".contains(c))
    {
        return Err("Branch names cannot hold ~ ^ : ? * [ or \\.");
    }
    if name.contains("..") || name.contains("@{") {
        return Err("Branch names cannot hold `..` or `@{`.");
    }
    if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return Err(
            "A `/` in a branch name separates two parts, so it cannot start, end or double.",
        );
    }
    if name.starts_with('-') || name.ends_with('.') {
        return Err("Branch names cannot start with `-` or end with `.`.");
    }
    if name
        .split('/')
        .any(|part| part.starts_with('.') || part.ends_with(".lock"))
    {
        return Err("No part of a branch name may start with `.` or end with `.lock`.");
    }
    Ok(())
}

/// A branch name as a directory name: `/` is the one character a valid branch
/// may hold that a single folder name may not, and anything else the filesystem
/// or the eye would rather not carry goes the same way.
///
/// Runs of separators collapse and the ends are trimmed, so `feat//x-` cannot
/// produce a name with a doubled or dangling dash — this is a label, and a
/// label that reads as a typo reads as the app having made one.
pub fn slug(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for ch in branch.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Where a worktree of `root` on `branch` goes when nobody says otherwise:
/// beside the project it came from.
///
/// Beside rather than inside, deliberately. A worktree under the repository is
/// a second checkout sitting in the first one's file tree and in its
/// `git status` — the project would start reporting its own copy as untracked
/// work, and the file panel would offer to open it — and the only thing that
/// keeps it out is an ignore rule the app would be quietly asking every
/// repository it touches to add.
pub fn worktree_dir(root: &Path, branch: &str) -> PathBuf {
    worktree_dir_in(root.parent().unwrap_or(root), root, branch)
}

/// The same folder name under a parent the user chose instead.
///
/// The project's own name stays in it. A folder called after the branch alone
/// is unreadable the moment two projects are split onto branches called
/// `fix` — and a chosen parent is *where the worktrees live*, which is
/// precisely the case where that collision is waiting.
pub fn worktree_dir_in(parent: &Path, root: &Path, branch: &str) -> PathBuf {
    let (repo, branch) = (slug(&label_for(root)), slug(branch));
    if repo.is_empty() {
        parent.join(branch)
    } else {
        parent.join(format!("{repo}-{branch}"))
    }
}

/// Whether `branch` already names a local branch of the repository at `root`.
///
/// Blocking, and runtime-agnostic like every other process call in this crate.
pub fn branch_exists_blocking(root: &Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Create a worktree of `root` at `dir`, on `branch`. Returns the directory
/// git actually made, canonicalized so it compares equal to a root added any
/// other way.
///
/// An existing branch is **checked out**; a name nothing answers to is
/// **created** off the current HEAD. One entry point for both, because the
/// distinction is not one the person naming a branch should have to make first
/// — and getting it wrong is not a no-op either way round: `-b` on a name that
/// exists fails, and omitting it on a name that does not asks git to check out
/// a commit-ish that isn't there.
///
/// Blocking. Every failure git can have here is a sentence worth showing —
/// the branch is checked out in another worktree, the directory is not empty,
/// the root is not a repository — so its own words are passed through rather
/// than folded into one message of ours.
pub fn add_blocking(root: &Path, branch: &str, dir: &Path) -> Result<PathBuf, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(root).args(["worktree", "add"]);
    if branch_exists_blocking(root, branch) {
        cmd.arg(dir).arg(branch);
    } else {
        cmd.arg("-b").arg(branch).arg(dir);
    }
    let out = cmd
        .output()
        .map_err(|err| format!("git could not be run: {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()))
}

/// git's complaint, as a line to put in front of someone.
///
/// `fatal:` and `error:` are stripped: they are the severity of a process that
/// has already been and gone, and the line is being shown in a place that is
/// already saying something failed. `hint:` lines are dropped whole — they
/// advise a shell user about their next command, which is not what the reader
/// of a dialog has in front of them.
fn git_message(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let out = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("hint:"))
        .map(|line| {
            line.strip_prefix("fatal:")
                .or_else(|| line.strip_prefix("error:"))
                .unwrap_or(line)
                .trim()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if out.is_empty() {
        "git could not create the worktree.".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_accepted() {
        for name in ["fix", "feat/rail-menu", "v2.1", "user_x-1"] {
            assert_eq!(validate_branch(name), Ok(()), "{name} should be a branch");
        }
    }

    #[test]
    fn the_names_git_would_refuse_are_refused_first() {
        for name in [
            "",
            "@",
            "has space",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "a\\b",
            "a..b",
            "a@{b",
            "/a",
            "a/",
            "a//b",
            "-a",
            "a.",
            ".a",
            "a/.b",
            "a.lock",
            "a/b.lock",
        ] {
            assert!(
                validate_branch(name).is_err(),
                "{name:?} should not be a branch"
            );
        }
    }

    #[test]
    fn a_slug_is_one_folder_name() {
        assert_eq!(slug("feat/rail-menu"), "feat-rail-menu");
        assert_eq!(slug("v2.1"), "v2.1");
        // Runs collapse and the ends are trimmed, so no doubled or dangling
        // dash reaches a folder name.
        assert_eq!(slug("a//b--c"), "a-b-c");
        assert_eq!(slug("-a-"), "a");
        assert_eq!(slug("việt"), "việt"); // alphanumeric is not ASCII-only
    }

    #[test]
    fn the_default_folder_sits_beside_the_project() {
        assert_eq!(
            worktree_dir(Path::new("/code/onehand"), "feat/rail"),
            PathBuf::from("/code/onehand-feat-rail")
        );
    }

    #[test]
    fn a_chosen_parent_keeps_the_project_name() {
        // Two projects split onto a branch of the same name land in one folder
        // of worktrees, so the project's name is what tells them apart.
        assert_eq!(
            worktree_dir_in(Path::new("/wt"), Path::new("/code/onehand"), "fix"),
            PathBuf::from("/wt/onehand-fix")
        );
        assert_eq!(
            worktree_dir_in(Path::new("/wt"), Path::new("/code/other"), "fix"),
            PathBuf::from("/wt/other-fix")
        );
    }

    #[test]
    fn a_rootless_path_still_produces_a_name() {
        // `/` has no name of its own to prefix with, and a folder called `-fix`
        // is a folder every command-line tool reads as a flag.
        assert_eq!(worktree_dir(Path::new("/"), "fix"), PathBuf::from("/fix"));
    }

    #[test]
    fn git_complaints_are_trimmed_to_the_sentence() {
        assert_eq!(
            git_message(b"fatal: '/a/b' already exists\nhint: use --force\n"),
            "'/a/b' already exists"
        );
        assert_eq!(git_message(b""), "git could not create the worktree.");
    }

    /// The command itself, against a real repository: the argument order for a
    /// new branch and for an existing one differ, and no amount of unit-testing
    /// the surrounding rules would catch getting either of them wrong.
    #[test]
    fn add_creates_a_branch_and_checks_out_an_existing_one() {
        let git = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git must be installed to run this test")
        };

        let repo = std::env::temp_dir().join(format!("onehand-worktree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-qm", "one"]);

        // A name nothing answers to is created off HEAD.
        let made = add_blocking(&repo, "feat/x", &worktree_dir(&repo, "feat/x")).unwrap();
        assert!(made.join("a.txt").exists());
        assert!(branch_exists_blocking(&repo, "feat/x"));

        // A branch that already exists is checked out rather than re-created.
        git(&repo, &["branch", "spare"]);
        let spare = add_blocking(&repo, "spare", &worktree_dir(&repo, "spare")).unwrap();
        assert!(spare.join("a.txt").exists());

        // And a branch already checked out somewhere is git's refusal to pass
        // on, not a panic or a silent success.
        let again = add_blocking(&repo, "spare", &repo.parent().unwrap().join("dupe"));
        assert!(again.is_err(), "a checked-out branch cannot be split twice");

        for dir in [&repo, &made, &spare] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
