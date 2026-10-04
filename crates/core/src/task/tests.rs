use super::queue::{place_blocking, Queue};
use super::*;
use crate::workflow::builtin;
use std::path::{Path, PathBuf};
use std::process::Command;

fn task(id: &str) -> Task {
    Task::new(
        id.into(),
        builtin::all().remove(0),
        Brief {
            title: "Fix the thing".into(),
            body: String::new(),
            instructions: None,
        },
        Setup {
            repo: PathBuf::from("/repo"),
            dir: PathBuf::from("/repo"),
            branch: None,
            agent: None,
            check: None,
        },
    )
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-task-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn one_task_works_in_a_place_and_the_rest_wait_in_order() {
    let mut queue = Queue::default();
    let (a, b) = (PathBuf::from("/a"), PathBuf::from("/b"));
    assert!(queue.ask(a.clone(), "1".into()));
    assert!(queue.ask(b.clone(), "2".into()), "another place is free");
    assert!(!queue.ask(a.clone(), "3".into()));
    assert!(!queue.ask(a.clone(), "4".into()));
    assert!(queue.queued("3") && queue.holds("1") && !queue.queued("1"));
    // Only the holder frees a place.
    assert_eq!(queue.release("4"), None);
    assert_eq!(queue.release("1").as_deref(), Some("3"));
    assert!(queue.holds("3") && !queue.queued("3"));
    // One called off is passed over.
    assert!(!queue.ask(a.clone(), "5".into()));
    assert!(queue.call_off("4"));
    assert!(!queue.call_off("4"));
    assert_eq!(queue.release("3").as_deref(), Some("5"));
    assert_eq!(queue.release("5"), None);
    assert!(queue.ask(a, "6".into()), "free once nobody waits");
    assert_eq!(queue.release("2"), None);
}

#[test]
fn a_subfolder_and_a_symlink_of_one_checkout_are_one_place() {
    let root = temp_dir("place");
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    git(&repo, &["init", "-q"]);
    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo, &link).unwrap();
    let top = place_blocking(&repo);
    assert_eq!(place_blocking(&repo.join("sub")), top);
    #[cfg(unix)]
    assert_eq!(place_blocking(&link.join("sub")), top);
    // Outside git, a folder is its own place.
    let plain = root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        place_blocking(&plain),
        std::fs::canonicalize(&plain).unwrap()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_task_reads_as_its_last_run() {
    let mut t = task("1");
    assert_eq!(t.outcome(), None);
    assert!(t.resumable(), "a run cut off before it started");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::LinkLost);
    assert!(t.resumable());
    t.dismissed = true;
    assert!(!t.resumable(), "a dismissed task is history");
    let mut t = task("2");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::TimedOut);
    assert!(!t.resumable());
    // Called off before its run started: it reads as stopped by a person.
    let mut t = task("3");
    t.runs.pop();
    assert_eq!(t.outcome(), Some(Outcome::Stopped(Stop::ByPerson)));
    assert!(!t.resumable());
}

/// Writes land in the order sent, so a snapshot sent before a newer one
/// never lands over it.
#[test]
fn the_writer_lands_the_last_snapshot_sent() {
    let dir = temp_dir("writer");
    let mut t = task("7");
    let writer = files::Writer::spawn(dir.clone());
    for n in 0..50 {
        t.runs[0].spent_secs = n;
        writer.save(t.clone());
    }
    assert!(writer.flush(std::time::Duration::from_secs(10)));
    let loaded = files::load_all_blocking(&dir);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].1.as_ref().unwrap(), &t);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_old_run_becomes_a_task_of_the_same_id_and_the_old_folder_goes() {
    let root = temp_dir("migrate-runs");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    let mut run = task("5").runs.remove(0);
    run.resume();
    // As a build from before visits and outcomes wrote it.
    let mut json = serde_json::to_value(&run).unwrap();
    json.as_object_mut().unwrap().remove("visits");
    json.as_object_mut().unwrap().remove("outcome");
    std::fs::write(old.join("5.json"), json.to_string()).unwrap();
    assert!(files::migrate_blocking(&old, &new).is_empty());
    let loaded = files::load_all_blocking(&new);
    let moved = loaded[0].1.as_ref().unwrap();
    assert_eq!((moved.id.as_str(), moved.runs.len()), ("5", 1));
    assert!(moved.resumable(), "it comes back cut off");
    assert_eq!(moved.brief, run.brief);
    assert!(!old.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_task_already_moved_wins_and_an_unreadable_run_stays() {
    let root = temp_dir("migrate-both");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let mut t = task("5");
    std::fs::write(
        old.join("5.json"),
        serde_json::to_string(&t.runs[0]).unwrap(),
    )
    .unwrap();
    // The moved task went on after a crash left the old file behind.
    t.runs[0].resume();
    let ahead = serde_json::to_string_pretty(&t).unwrap();
    std::fs::write(new.join("5.json"), &ahead).unwrap();
    std::fs::write(old.join("6.json"), "{ not a run").unwrap();
    let problems = files::migrate_blocking(&old, &new);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(std::fs::read_to_string(new.join("5.json")).unwrap(), ahead);
    assert!(!old.join("5.json").exists());
    assert!(old.join("6.json").exists());
    assert!(!new.join("6.json").exists());
    let _ = std::fs::remove_dir_all(&root);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        // The test's own git calls read no identity of anybody's.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_mark_holds_untracked_work_and_leaves_the_persons_index_alone() {
    let repo = temp_dir("mark");
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("first.txt"), "one").unwrap();
    // On an unborn HEAD, in a repository that sets no identity: the mark
    // commits as onehand, whatever the person's own config says.
    let refs = [marks::ref_name("t", "r", 1, false)];
    let unborn = marks::pin_blocking(&repo, &refs).unwrap();
    assert_eq!(git(&repo, &["rev-parse", &refs[0]]), unborn);
    assert_eq!(git(&repo, &["show", &format!("{unborn}:first.txt")]), "one");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? first.txt");

    let mut commit = Command::new("git");
    commit
        .arg("-C")
        .arg(&repo)
        .args(["-c", "user.name=T", "-c", "user.email=t@t"])
        .args(["commit", "-q", "--allow-empty", "-m", "base"]);
    assert!(commit.status().unwrap().success());
    std::fs::write(repo.join("staged.txt"), "staged").unwrap();
    git(&repo, &["add", "staged.txt"]);
    let staged_before = git(&repo, &["diff", "--cached", "--name-only"]);
    let both = [
        marks::ref_name("t", "r", 1, true),
        marks::ref_name("t", "r", 2, false),
    ];
    let mark = marks::pin_blocking(&repo, &both).unwrap();
    assert_eq!(git(&repo, &["rev-parse", &both[1]]), mark);
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{mark}^")]),
        git(&repo, &["rev-parse", "HEAD"])
    );
    let files = git(&repo, &["ls-tree", "--name-only", &mark]);
    assert_eq!(files, "first.txt\nstaged.txt");
    assert_eq!(
        git(&repo, &["diff", "--cached", "--name-only"]),
        staged_before
    );
    assert_eq!(
        git(&repo, &["for-each-ref", "--count=9", "refs/onehand"])
            .lines()
            .count(),
        3
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn the_refs_still_to_pin_name_every_boundary_from_the_count_pinned() {
    let mut t = task("9");
    let run = &mut t.runs[0];
    run.resume();
    assert_eq!(
        marks::refs_from("9", run, 0),
        ["refs/onehand/tasks/9/9/1/start"]
    );
    assert!(marks::refs_from("9", run, 1).is_empty());
    run.stopped(Stop::ByPerson);
    assert_eq!(
        marks::refs_from("9", run, 1),
        ["refs/onehand/tasks/9/9/1/end"]
    );
}
