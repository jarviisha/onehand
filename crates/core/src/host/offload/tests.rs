use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-offload-{name}-{}", std::process::id()));
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
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository with one commit: `a.txt` tracked, `.gitignore` ignoring
/// `secret.env`, which is there, and `new.txt` untracked.
fn repo(name: &str) -> PathBuf {
    let dir = scratch(name).join("project");
    fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("a.txt"), "a").unwrap();
    fs::write(dir.join(".gitignore"), "secret.env\n").unwrap();
    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-q", "-m", "one"]);
    fs::write(dir.join("secret.env"), "TOKEN=1").unwrap();
    fs::write(dir.join("new.txt"), "new").unwrap();
    dir
}

fn mirror_of(project: &Path) -> PathBuf {
    let mirror = project.parent().unwrap().join("mirror");
    fs::create_dir_all(&mirror).unwrap();
    mirror
}

/// Bump a file's time past what the manifest saw, as a later edit would.
fn touch_later(path: &Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

#[test]
fn the_first_copy_sends_what_git_keeps_and_never_an_ignored_file() {
    let project = repo("first");
    let mirror = mirror_of(&project);
    send_up_blocking(&local(), &project, &mirror, &Manifest::default()).unwrap();
    assert_eq!(fs::read_to_string(mirror.join("a.txt")).unwrap(), "a");
    assert_eq!(fs::read_to_string(mirror.join("new.txt")).unwrap(), "new");
    assert!(mirror.join(".git/HEAD").exists());
    assert!(!mirror.join("secret.env").exists());
    assert_eq!(
        run_git(&mirror, &["rev-parse", "HEAD"]),
        run_git(&project, &["rev-parse", "HEAD"])
    );
}

#[test]
fn a_file_deleted_here_is_deleted_in_the_mirror() {
    let project = repo("delete");
    let mirror = mirror_of(&project);
    let sent = send_up_blocking(&local(), &project, &mirror, &Manifest::default()).unwrap();
    fs::remove_file(project.join("new.txt")).unwrap();
    send_up_blocking(&local(), &project, &mirror, &sent).unwrap();
    assert!(!mirror.join("new.txt").exists());
    assert!(mirror.join("a.txt").exists());
}

#[test]
fn nothing_changed_reads_as_no_change() {
    let project = repo("same");
    let was = Manifest::read_blocking(&project).unwrap();
    let now = Manifest::read_blocking(&project).unwrap();
    assert!(was.changes(&now).is_empty());
}

#[test]
fn a_file_changed_added_or_gone_outside_is_listed() {
    let project = repo("outside");
    let was = Manifest::read_blocking(&project).unwrap();
    fs::write(project.join("a.txt"), "changed").unwrap();
    touch_later(&project.join("a.txt"));
    fs::write(project.join("b.txt"), "b").unwrap();
    fs::remove_file(project.join("new.txt")).unwrap();
    let changes = was.changes(&Manifest::read_blocking(&project).unwrap());
    assert_eq!(
        changes.files,
        vec![
            PathBuf::from("a.txt"),
            PathBuf::from("b.txt"),
            PathBuf::from("new.txt")
        ]
    );
    assert!(changes.refs.is_empty());
}

#[test]
fn an_ignored_file_changing_is_not_a_change() {
    let project = repo("ignored");
    let was = Manifest::read_blocking(&project).unwrap();
    fs::write(project.join("secret.env"), "TOKEN=2").unwrap();
    assert!(was
        .changes(&Manifest::read_blocking(&project).unwrap())
        .is_empty());
}

#[test]
fn a_commit_or_a_branch_made_outside_is_listed_and_object_churn_is_not() {
    let project = repo("refs");
    let was = Manifest::read_blocking(&project).unwrap();
    run_git(&project, &["gc", "-q"]);
    assert!(was
        .changes(&Manifest::read_blocking(&project).unwrap())
        .is_empty());
    run_git(&project, &["branch", "side"]);
    let changes = was.changes(&Manifest::read_blocking(&project).unwrap());
    assert_eq!(changes.refs, vec!["refs/heads/side".to_string()]);
    run_git(&project, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let changes = was.changes(&Manifest::read_blocking(&project).unwrap());
    assert_eq!(
        changes.refs,
        vec![
            "HEAD".to_string(),
            "refs/heads/main".into(),
            "refs/heads/side".into()
        ]
    );
}

#[test]
fn coming_back_brings_the_hosts_work_and_keeps_a_commit_only_here() {
    let project = repo("back");
    let mirror = mirror_of(&project);
    let sent = send_up_blocking(&local(), &project, &mirror, &Manifest::default()).unwrap();
    // The host's turn: a file changed, one added, one deleted, a commit.
    fs::write(mirror.join("a.txt"), "host").unwrap();
    fs::write(mirror.join("host.txt"), "h").unwrap();
    fs::remove_file(mirror.join("new.txt")).unwrap();
    run_git(&mirror, &["add", "-A"]);
    run_git(&mirror, &["commit", "-q", "-m", "host"]);
    // A commit only this folder has, on a branch of its own.
    run_git(&project, &["branch", "here"]);
    let mine = run_git(&project, &["rev-parse", "here"]);
    let listed = listed_blocking(&mirror).unwrap();
    bring_back_blocking(&local(), &project, &mirror, &listed, &sent).unwrap();
    assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "host");
    assert!(project.join("host.txt").exists());
    assert!(!project.join("new.txt").exists());
    assert_eq!(
        run_git(&project, &["rev-parse", "HEAD"]),
        run_git(&mirror, &["rev-parse", "HEAD"])
    );
    assert!(
        project.join("secret.env").exists(),
        "an ignored file here stays"
    );
    assert_eq!(run_git(&project, &["cat-file", "-t", &mine]), "commit");
}

#[test]
fn a_path_sent_by_hand_goes_ignored_or_not() {
    let project = repo("send");
    let mirror = mirror_of(&project);
    fs::create_dir_all(project.join("conf")).unwrap();
    fs::write(project.join("conf/app.json"), "{}").unwrap();
    send_path_blocking(&local(), &project, &mirror, Path::new("secret.env")).unwrap();
    send_path_blocking(&local(), &project, &mirror, Path::new("conf")).unwrap();
    assert_eq!(
        fs::read_to_string(mirror.join("secret.env")).unwrap(),
        "TOKEN=1"
    );
    assert!(mirror.join("conf/app.json").exists());
}

#[test]
fn a_backup_holds_exactly_the_changed_files_and_the_refs() {
    let project = repo("backup");
    let into = project.parent().unwrap().join("backup");
    let changes = Changes {
        files: vec!["a.txt".into(), "gone.txt".into()],
        refs: vec!["HEAD".into()],
    };
    backup_blocking(&project, &changes, &into).unwrap();
    assert_eq!(fs::read_to_string(into.join("a.txt")).unwrap(), "a");
    assert!(!into.join("gone.txt").exists());
    assert!(!into.join("new.txt").exists());
    let refs = fs::read_to_string(into.join("refs.txt")).unwrap();
    assert!(refs.contains("refs/heads/main"));
}

#[test]
fn the_state_round_trips_and_none_kept_is_none() {
    let dir = scratch("state");
    assert_eq!(OffloadState::load(&dir).unwrap(), None);
    let state = OffloadState {
        key: Holder::Host,
        manifest: Manifest::read_blocking(&repo("state-repo")).unwrap(),
        moved: 7,
        stopped: Some(Changes {
            files: vec!["a.txt".into()],
            refs: vec![],
        }),
    };
    state.save(&dir).unwrap();
    assert_eq!(OffloadState::load(&dir).unwrap(), Some(state));
}

#[test]
fn rsync_is_found_here() {
    rsync_present_blocking(&local()).unwrap();
}
