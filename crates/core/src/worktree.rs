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
    // A branch of nothing but punctuation slugs away to nothing, and git will
    // take such a name: `+++` is a branch. Both folder names that would follow
    // are wrong, and the second is dangerous — a trailing dash, or, when the
    // project has no name to prefix with either, the *parent itself*, which is
    // then what `git worktree add` is pointed at. A stand-in keeps the target
    // inside the folder it was supposed to go in; two such branches want the
    // same folder, and git refusing the second is the right way to find that
    // out, since the alternative is a folder name nobody can read.
    let branch = if branch.is_empty() {
        "branch".to_string()
    } else {
        branch
    };
    if repo.is_empty() {
        parent.join(branch)
    } else {
        parent.join(format!("{repo}-{branch}"))
    }
}

/// The top level of the repository `root` sits in, if it sits in one.
///
/// A project root does not have to *be* a repository — a folder inside one is a
/// project the app supports everywhere else, git status included — and a
/// worktree cannot be taken of a folder: git checks out repositories. So the
/// question every part of this has to be asked about is the repository, not the
/// root: put the second checkout beside *that*, name it after *that*, and the
/// worktree lands next to the repository instead of inside it, which is what the
/// whole beside-rather-than-inside rule was for.
///
/// Blocking, and runtime-agnostic like every other process call in this crate.
pub fn repo_top_blocking(root: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    let top = PathBuf::from(text);
    Some(std::fs::canonicalize(&top).unwrap_or(top))
}

/// The folder to adopt inside a fresh checkout, when the project split was a
/// folder inside the repository rather than the repository itself.
///
/// The new root is the *same subtree* of the new checkout. Handing back the
/// repository top instead would quietly change which files the project's agent,
/// file tree and terminal are pointed at — the user split one part of a
/// monorepo and would find themselves standing in all of it.
///
/// Pure: whether that subtree exists on the branch being checked out is a
/// question for whoever has just run git, not for the rule.
pub fn subtree_in(made: &Path, top: &Path, root: &Path) -> PathBuf {
    match root.strip_prefix(top) {
        Ok(rel) if !rel.as_os_str().is_empty() => made.join(rel),
        _ => made.to_path_buf(),
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

    /// A branch git accepts but a slug cannot represent still has to land
    /// somewhere, and the somewhere must be a new folder inside the parent.
    #[test]
    fn a_branch_that_slugs_to_nothing_still_names_a_folder() {
        assert_eq!(slug("+++"), "", "this is the name that has no slug");
        assert_eq!(validate_branch("+++"), Ok(()), "and git would take it");

        // Not `/code/onehand-`, which reads as a typo…
        assert_eq!(
            worktree_dir(Path::new("/code/onehand"), "+++"),
            PathBuf::from("/code/onehand-branch")
        );
        // …and above all not the parent itself, which is what would then be
        // handed to `git worktree add` as the directory to create.
        assert_eq!(
            worktree_dir(Path::new("/"), "+++"),
            PathBuf::from("/branch")
        );
    }

    /// Splitting a folder *inside* a repository puts the checkout beside the
    /// repository, and adopts the same folder inside it.
    #[test]
    fn a_subtree_keeps_its_place_in_the_new_checkout() {
        let (top, root) = (Path::new("/code/mono"), Path::new("/code/mono/apps/web"));
        // Named and placed after the repository, since that is what git checks
        // out -- not after the folder that was split.
        assert_eq!(
            worktree_dir(top, "fix"),
            PathBuf::from("/code/mono-fix"),
            "beside the repository, not inside it"
        );
        assert_eq!(
            subtree_in(Path::new("/code/mono-fix"), top, root),
            PathBuf::from("/code/mono-fix/apps/web")
        );
        // A root that is the repository adopts the checkout itself.
        assert_eq!(
            subtree_in(Path::new("/code/mono-fix"), top, top),
            PathBuf::from("/code/mono-fix")
        );
        // And a root that is not under the top at all is not forced under it.
        assert_eq!(
            subtree_in(Path::new("/code/mono-fix"), top, Path::new("/elsewhere")),
            PathBuf::from("/code/mono-fix")
        );
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

        // A folder inside the repository answers with the repository, which is
        // what decides where its worktree goes and what it is called.
        let inner = repo.join("apps").join("web");
        std::fs::create_dir_all(&inner).unwrap();
        let top = std::fs::canonicalize(&repo).unwrap();
        assert_eq!(repo_top_blocking(&inner), Some(top.clone()));
        assert_eq!(repo_top_blocking(&repo), Some(top));
        // Somewhere that is no repository at all has no top to answer with.
        assert_eq!(repo_top_blocking(Path::new("/")), None);

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
