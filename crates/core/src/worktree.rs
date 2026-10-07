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

use crate::process::output_within;
use crate::workspace::label_for;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
pub(crate) fn slug(branch: &str) -> String {
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
pub(crate) fn branch_exists_blocking(root: &Path, branch: &str) -> bool {
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
    if branch_exists_blocking(root, branch) {
        worktree_add(root, dir, &[dir.as_os_str(), branch.as_ref()])
    } else {
        worktree_add(
            root,
            dir,
            &["-b".as_ref(), branch.as_ref(), dir.as_os_str()],
        )
    }
}

/// How long `git worktree add` may take. It is a local checkout, but a checkout
/// can run hooks and fetch large files, and either can wait on a network.
const ADD_LIMIT: Duration = Duration::from_secs(300);

/// How long a fetch may take. A large repository over a slow link is a
/// legitimate few minutes; an hour is a network that has gone away.
pub const FETCH_LIMIT: Duration = Duration::from_secs(300);

/// `git -C <root> worktree add <args>`, answering with the directory made.
fn worktree_add(root: &Path, dir: &Path, args: &[&std::ffi::OsStr]) -> Result<PathBuf, String> {
    let out = output_within(git(root).args(["worktree", "add"]).args(args), ADD_LIMIT)
        .map_err(|err| format!("git worktree add {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()))
}

/// Create a worktree of `root` at `dir` on a **new** branch cut from `start`.
///
/// The other half of [`add_blocking`], for a caller that knows what the branch
/// must start from rather than inheriting whatever the checkout has at HEAD —
/// an unattended run, whose pull request would otherwise carry every commit of
/// the branch the user happened to be on. Always `-b`: a name that already
/// exists is git's refusal, since reusing it would start from someone else's
/// work.
pub fn branch_off_blocking(
    root: &Path,
    branch: &str,
    dir: &Path,
    start: &str,
) -> Result<PathBuf, String> {
    worktree_add(
        root,
        dir,
        &[
            "-b".as_ref(),
            branch.as_ref(),
            dir.as_os_str(),
            start.as_ref(),
        ],
    )
}

/// Bring `origin/<branch>` up to date in the repository at `root`.
///
/// The remote is `origin` by assumption, which is what a forge's own tools
/// assume of a clone they did not make. It never waits on a person: a fetch that needs a
/// password, a passphrase or a host key nobody is there to accept fails rather
/// than asks.
pub fn fetch_blocking(root: &Path, branch: &str) -> Result<(), String> {
    with_origin_blocking(root, &["fetch", "--quiet", "origin", branch], "fetch")
}

/// Put `commit` on `origin` as `branch`, the way [`fetch_blocking`] reaches
/// it, and never by force: a branch that moved on the forge is refused rather
/// than written over.
pub fn push_blocking(root: &Path, commit: &str, branch: &str) -> Result<(), String> {
    let refspec = format!("{commit}:refs/heads/{branch}");
    with_origin_blocking(root, &["push", "--quiet", "origin", &refspec], "push")
}

/// Run git's `args` against `origin` at `root`, as `git <verb>` says when it
/// cannot be run at all.
fn with_origin_blocking(root: &Path, args: &[&str], verb: &str) -> Result<(), String> {
    let mut cmd = git(root);
    cmd.args(args);
    // Only when nothing chose an ssh command already: overriding one would
    // throw away whatever the user configured it to do.
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let out = output_within(&mut cmd, FETCH_LIMIT).map_err(|err| format!("git {verb} {err}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(git_message(&out.stderr))
    }
}

/// Bring the branch checked out at `dir` up to `to` without a merge commit:
/// refused when the two went their own ways, as git says.
pub fn fast_forward_blocking(dir: &Path, to: &str) -> Result<(), String> {
    let out = output_within(
        git(dir).args(["merge", "--ff-only", "--quiet", to]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git merge {err}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(git_message(&out.stderr))
    }
}

/// Whether the branch checked out at `dir` and `to` went their own ways:
/// neither holds the other. One behind is brought up by a fast-forward, and
/// one ahead, with commits not pushed yet, is up to date already.
pub fn went_its_own_way_blocking(dir: &Path, to: &str) -> Result<bool, String> {
    Ok(!ancestor_blocking(dir, "HEAD", to)? && !ancestor_blocking(dir, to, "HEAD")?)
}

/// Whether commit `old` is `new` or one of its ancestors.
fn ancestor_blocking(dir: &Path, old: &str, new: &str) -> Result<bool, String> {
    let out = output_within(
        git(dir).args(["merge-base", "--is-ancestor", old, new]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git merge-base {err}"))?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_message(&out.stderr)),
    }
}

/// How long a question git answers from the repository alone may take.
pub(crate) const LOCAL_LIMIT: Duration = Duration::from_secs(30);

/// The branch checked out at `root`: what a run on a project with no forge
/// starts from, since there is no remote default branch to ask for. A detached
/// HEAD is refused rather than started from, because the run's commits would
/// then be measured against a commit nobody named.
pub fn current_branch_blocking(root: &Path) -> Result<String, String> {
    let out = output_within(
        git(root).args(["symbolic-ref", "--short", "-q", "HEAD"]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git {err}"))?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() && !name.is_empty() {
        Ok(name)
    } else {
        Err("it is on a detached HEAD, so there is no branch to start from".to_string())
    }
}

/// How many commits the checkout at `dir` has that `base` does not — what a run
/// with no pull request to open left behind.
pub fn commits_since_blocking(dir: &Path, base: &str) -> Result<u64, String> {
    let out = output_within(
        git(dir).args(["rev-list", "--count", &format!("{base}..HEAD")]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .map_err(|err| format!("git printed an unreadable count: {err}"))
}

/// `git -C <dir> <args>`, answering with what it printed, trimmed.
fn read_blocking(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out =
        output_within(git(dir).args(args), LOCAL_LIMIT).map_err(|err| format!("git {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The commit checked out at `dir`.
pub fn head_blocking(dir: &Path) -> Result<String, String> {
    read_blocking(dir, &["rev-parse", "HEAD"])
}

/// Whether the checkout at `dir` holds anything not committed, a new file
/// nobody added included: work that would be lost to everyone but this
/// folder.
pub(crate) fn dirty_blocking(dir: &Path) -> Result<bool, String> {
    read_blocking(dir, &["status", "--porcelain"]).map(|out| !out.is_empty())
}

/// A fingerprint of the work in the checkout at `dir` that no commit holds:
/// equal before and after a turn exactly when the turn left it as it was, so
/// a checkout that was already dirty is measured by what the turn changed.
///
/// **An untracked file counts by its contents**, not only its name, which is
/// all the status and the diff carry of it: the step after a failed check
/// often fixes the very file the change before it created, and by name alone
/// that fix reads as no change at all.
// ponytail: every untracked file is read whole at each measure; a checkout
// with a large tree nobody ignores pays for it. Hash by size and mtime first
// if that is ever felt.
///
/// **FNV-1a, not std's hasher**: the digest is kept in a run's file and
/// compared after a restart, and std makes no promise that its hasher gives
/// the same answer from one Rust release to the next.
pub(crate) fn work_digest_blocking(dir: &Path) -> Result<String, String> {
    let mut work = Vec::new();
    for args in [
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"][..],
        &["diff", "HEAD", "--binary"][..],
    ] {
        let out =
            output_within(git(dir).args(args), LOCAL_LIMIT).map_err(|err| format!("git {err}"))?;
        if !out.status.success() {
            return Err(git_message(&out.stderr));
        }
        // The length first, so where one output ends is part of the digest.
        work.extend_from_slice(&(out.stdout.len() as u64).to_le_bytes());
        work.extend_from_slice(&out.stdout);
    }
    let untracked = output_within(
        git(dir).args(["ls-files", "--others", "--exclude-standard", "-z"]),
        LOCAL_LIMIT,
    )
    .map_err(|err| format!("git {err}"))?;
    if !untracked.status.success() {
        return Err(git_message(&untracked.stderr));
    }
    for name in untracked
        .stdout
        .split(|b| *b == 0)
        .filter(|n| !n.is_empty())
    {
        // A file gone since it was listed reads as empty; the next measure
        // sees it gone from the status.
        let contents =
            std::fs::read(dir.join(String::from_utf8_lossy(name).as_ref())).unwrap_or_default();
        work.extend_from_slice(&crate::chat::store::fnv1a(&contents).to_le_bytes());
    }
    Ok(format!("{:016x}", crate::chat::store::fnv1a(&work)))
}

/// `git -C <root>`, with every way git has of asking a person for something
/// switched off.
pub(crate) fn git(root: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(root).env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// Rename the branch that is checked out at `root`, in place.
///
/// **`-m` and never `-M`.** The forced form overwrites a branch that already
/// carries the new name, which throws away whatever was on it -- and the whole
/// of what the user asked for is that this branch be called something else. git
/// refuses on the collision and its sentence says so, which is the answer.
///
/// The name goes through the same rule a new worktree's does: a branch is a
/// branch whichever gesture made it, and a second spelling of what git will
/// accept is a second place for the two to disagree.
///
/// Blocking, like every other call here, and git's own words are passed through
/// for the same reason -- "a branch named x already exists" and "cannot rename
/// a detached HEAD" are sentences worth showing, and nothing here could write
/// them better.
pub fn rename_branch_blocking(root: &Path, name: &str) -> Result<(), String> {
    validate_branch(name).map_err(str::to_string)?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["branch", "-m", name])
        .output()
        .map_err(|err| format!("git could not be run: {err}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(())
}

/// git's complaint, as a line to put in front of someone.
///
/// `fatal:` and `error:` are stripped: they are the severity of a process that
/// has already been and gone, and the line is being shown in a place that is
/// already saying something failed. `hint:` lines are dropped whole — they
/// advise a shell user about their next command, which is not what the reader
/// of a dialog has in front of them.
pub(crate) fn git_message(stderr: &[u8]) -> String {
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
        "git refused, and said nothing about why.".to_string()
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
        // Two callers now, so the fallback cannot name one of the two things
        // git was asked to do.
        assert_eq!(git_message(b""), "git refused, and said nothing about why.");
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

        // Branching off a named start ignores what the checkout has at HEAD:
        // a commit made on another branch does not come along.
        git(&repo, &["checkout", "-qb", "feature"]);
        std::fs::write(repo.join("b.txt"), "y").unwrap();
        git(&repo, &["add", "b.txt"]);
        git(&repo, &["commit", "-qm", "two"]);
        let run = branch_off_blocking(&repo, "run", &worktree_dir(&repo, "run"), "main").unwrap();
        assert!(run.join("a.txt").exists());
        assert!(
            !run.join("b.txt").exists(),
            "HEAD's commit must not come along"
        );
        // And a name that exists is refused rather than reused.
        let twice = branch_off_blocking(&repo, "run", &repo.parent().unwrap().join("x"), "main");
        assert!(twice.is_err());

        // A run on a project with no forge starts from the branch checked out
        // and is judged by what it committed past that start.
        assert_eq!(current_branch_blocking(&repo), Ok("feature".to_string()));
        assert_eq!(commits_since_blocking(&run, "main"), Ok(0));
        std::fs::write(run.join("c.txt"), "z").unwrap();
        git(&run, &["add", "c.txt"]);
        git(&run, &["commit", "-qm", "three"]);
        assert_eq!(commits_since_blocking(&run, "main"), Ok(1));
        assert!(commits_since_blocking(&run, "no-such-branch").is_err());
        git(&repo, &["checkout", "-q", "--detach"]);
        assert!(
            current_branch_blocking(&repo).is_err(),
            "a detached HEAD has no branch"
        );

        for dir in [&repo, &made, &spare, &run] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_fast_forward_catches_up_and_refuses_a_branch_that_went_its_own_way() {
        let git = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git must be installed to run this test")
        };
        let commit = |dir: &Path, file: &str| {
            std::fs::write(dir.join(file), file).unwrap();
            git(dir, &["add", file]);
            git(dir, &["commit", "-qm", file]);
        };
        let repo =
            std::env::temp_dir().join(format!("onehand-fast-forwards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "T"]);
        commit(&repo, "a");
        git(&repo, &["branch", "theirs"]);
        git(&repo, &["checkout", "-q", "theirs"]);
        commit(&repo, "b");
        git(&repo, &["checkout", "-q", "main"]);

        assert_eq!(
            went_its_own_way_blocking(&repo, "theirs"),
            Ok(false),
            "behind"
        );
        fast_forward_blocking(&repo, "theirs").unwrap();
        assert!(repo.join("b").exists(), "main caught up with theirs");

        // Ahead of it, with a commit never pushed: up to date, not apart.
        commit(&repo, "c");
        assert_eq!(
            went_its_own_way_blocking(&repo, "theirs"),
            Ok(false),
            "ahead"
        );
        fast_forward_blocking(&repo, "theirs").unwrap();

        git(&repo, &["checkout", "-q", "theirs"]);
        commit(&repo, "d");
        assert_eq!(went_its_own_way_blocking(&repo, "main"), Ok(true));
        assert!(
            fast_forward_blocking(&repo, "main").is_err(),
            "two branches that went their own ways are never merged"
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// The rename, against a real repository for the same reason: what it does
    /// is one argument order, and the two failures worth having are both git's
    /// rather than ours.
    #[test]
    fn rename_moves_the_checked_out_branch_and_refuses_a_collision() {
        let git = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git must be installed to run this test")
        };
        let head = |dir: &Path| {
            let out = git(dir, &["branch", "--show-current"]);
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        let repo = std::env::temp_dir().join(format!("onehand-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-qm", "one"]);

        rename_branch_blocking(&repo, "trunk").unwrap();
        assert_eq!(head(&repo), "trunk");

        // A name the branch rule refuses never reaches git.
        assert!(rename_branch_blocking(&repo, "  ").is_err());
        assert_eq!(head(&repo), "trunk", "a refused name changes nothing");

        // And `-m` rather than `-M`, so a name already taken is git's refusal
        // and not a branch quietly destroyed.
        git(&repo, &["branch", "keep"]);
        let clash = rename_branch_blocking(&repo, "keep");
        assert!(
            clash.is_err(),
            "renaming onto an existing branch must refuse"
        );
        assert_eq!(head(&repo), "trunk");
        assert!(
            branch_exists_blocking(&repo, "keep"),
            "and must not destroy it"
        );

        let _ = std::fs::remove_dir_all(&repo);
    }

    /// The digest moves with any change no commit holds, a new file included,
    /// and comes back when the change is undone.
    #[test]
    fn the_work_digest_follows_uncommitted_changes() {
        let git = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git must be installed to run this test")
        };
        let repo = std::env::temp_dir().join(format!("onehand-digest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-qm", "one"]);

        let clean = work_digest_blocking(&repo).unwrap();
        std::fs::write(repo.join("a.txt"), "y").unwrap();
        let edited = work_digest_blocking(&repo).unwrap();
        assert_ne!(clean, edited);
        std::fs::write(repo.join("a.txt"), "z").unwrap();
        assert_ne!(
            edited,
            work_digest_blocking(&repo).unwrap(),
            "a second edit"
        );
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        assert_eq!(clean, work_digest_blocking(&repo).unwrap());
        std::fs::write(repo.join("new.txt"), "n").unwrap();
        let created = work_digest_blocking(&repo).unwrap();
        assert_ne!(clean, created, "a new file");
        // The fix a failed check asks for, made to the file the change created.
        std::fs::write(repo.join("new.txt"), "fixed").unwrap();
        assert_ne!(
            created,
            work_digest_blocking(&repo).unwrap(),
            "an edit inside an untracked file"
        );

        let _ = std::fs::remove_dir_all(&repo);
    }
}
