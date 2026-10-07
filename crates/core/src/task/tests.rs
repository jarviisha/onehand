use super::queue::{place_blocking, queued_said, Queue};
use super::*;
use super::{Group, Source, Working};
use crate::workflow::builtin;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A command that passed on `commit`.
fn passed(commit: Option<String>) -> crate::workflow::CommandResult {
    crate::workflow::CommandResult {
        passed: true,
        exit: Some(0),
        tail: String::new(),
        commit,
        digest: None,
    }
}

/// A command that failed, printing `tail`.
fn failed(tail: String) -> crate::workflow::CommandResult {
    crate::workflow::CommandResult {
        passed: false,
        exit: Some(1),
        tail,
        commit: None,
        digest: None,
    }
}

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
            mode: None,
            forge: None,
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
    assert_eq!(queue.holder_of("3"), Some("1"));
    assert_eq!(queue.holder_of("1"), None, "a holder waits behind nobody");
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

#[test]
fn a_task_is_listed_by_what_it_does_and_how_it_ended() {
    use crate::workflow::Outcome as O;
    let t = task("1");
    for (working, group) in [
        (Working::Queued, Group::Queued),
        (Working::Running, Group::Running),
        (Working::Waiting, Group::Waiting),
    ] {
        assert_eq!(t.group(Some(working)), group);
    }
    assert_eq!(t.group(None), Group::Ended, "cut off");
    for (outcome, group) in [
        (O::Done, Group::Finished),
        (O::Stopped(Stop::ByPerson), Group::Finished),
        (O::Stopped(Stop::TakenOver), Group::Finished),
        (O::Stopped(Stop::TimedOut), Group::Ended),
        (O::Stopped(Stop::LinkLost), Group::Ended),
        (O::Stopped(Stop::Closed), Group::Ended),
        (O::Exhausted { step: "x".into() }, Group::Ended),
        (O::Failed("x".into()), Group::Ended),
    ] {
        let mut t = task("1");
        t.runs[0].outcome = Some(outcome.clone());
        assert_eq!(t.group(None), group, "{outcome:?}");
        t.dismissed = true;
        assert_eq!(t.group(None), Group::Finished, "dismissed {outcome:?}");
    }
    // Called off before its run started.
    let mut t = task("1");
    t.runs.pop();
    assert_eq!(t.group(None), Group::Finished);
}

#[test]
fn a_file_from_before_checks_reads_as_a_workflows_task() {
    let mut json = serde_json::to_value(task("1")).unwrap();
    json.as_object_mut().unwrap().remove("source");
    let t: Task = serde_json::from_value(json).unwrap();
    assert_eq!(t.source, Source::Workflow);
}

#[test]
fn a_check_task_passes_or_fails_on_its_command() {
    let setup = task("1").setup;
    for (ran, outcome) in [
        (passed(Some("abc".to_string())), Outcome::Done),
        (
            failed("boom".to_string()),
            Outcome::Failed("the command failed".into()),
        ),
    ] {
        let mut t = Task::check("1".into(), "make check".into(), setup.clone());
        assert_eq!(t.source, Source::Check);
        let run = &mut t.runs[0];
        assert_eq!(
            run.resume(),
            crate::workflow::Action::RunCommand("make check".into())
        );
        assert_eq!(
            run.command_finished(ran),
            crate::workflow::Action::Finish(outcome)
        );
    }
}

#[test]
fn a_retry_pushes_a_new_run_and_keeps_the_old_one() {
    let mut t = task("1");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::TimedOut);
    let template = t.runs[0].template.clone();
    assert_eq!(
        t.retry("2".into(), template, None, None).map(|r| r.step),
        Some(0)
    );
    assert_eq!(t.runs.len(), 2);
    assert_eq!(t.group(None), Group::Ended, "cut off until it runs");
}

/// A finished task of project `repo`, last moved at `at`.
fn finished(id: &str, repo: &str, at: u64) -> Task {
    let mut t = task(id);
    t.setup.repo = PathBuf::from(repo);
    t.runs[0].resume();
    t.runs[0].stopped(Stop::ByPerson);
    for move_ in &mut t.runs[0].history {
        move_.at = at;
    }
    t
}

#[test]
fn each_project_keeps_its_newest_finished_tasks() {
    let n = history::KEPT + 2;
    let mut all: Vec<Task> = (0..n)
        .map(|i| finished(&i.to_string(), "/a", i as u64))
        .collect();
    all.push(finished("b", "/b", 0));
    let mut ended = task("e");
    ended.setup.repo = PathBuf::from("/a");
    all.push(ended);
    let listed: Vec<(&Task, Group)> = all.iter().map(|t| (t, t.group(None))).collect();
    let mut gone = history::over_cap(&listed);
    gone.sort();
    assert_eq!(
        gone,
        ["0", "1"],
        "the two oldest of /a, never /b's or one ended"
    );
}

/// An issue task of `/a` whose issue still waits for `unsent` reports.
fn issue_task(id: &str, at: u64, unsent: &[&str]) -> Task {
    use crate::unattended::{IssueSource, TrackerRef};
    let mut t = finished(id, "/a", at);
    t.source = Source::Issue(IssueSource {
        tracker: TrackerRef::Forge {
            connector: "Forge".into(),
        },
        number: 3,
        forge_ref: None,
        forge: Some("Forge".into()),
        base: "origin/main".into(),
        picked: false,
        unsent: unsent
            .iter()
            .map(|run| crate::unattended::PendingReport {
                run: run.to_string(),
                outcome: Some(Outcome::Done),
                started: true,
                ended_on: None,
                asked: None,
                notes: Vec::new(),
            })
            .collect(),
        notes: Vec::new(),
    });
    t
}

#[test]
fn a_task_whose_issue_was_not_told_is_never_let_go() {
    let mut all: Vec<Task> = (0..history::KEPT)
        .map(|i| finished(&(i + 10).to_string(), "/a", 100 + i as u64))
        .collect();
    all.push(issue_task("waits", 0, &["1"]));
    all.push(issue_task("told", 1, &[]));
    let listed: Vec<(&Task, Group)> = all.iter().map(|t| (t, t.group(None))).collect();
    assert_eq!(history::over_cap(&listed), ["told"]);
}

#[test]
fn an_issue_task_survives_its_file() {
    let t = issue_task("1", 5, &["1"]);
    let json = serde_json::to_string(&t).unwrap();
    let back: Task = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.issue().map(|i| i.shown()), Some("#3".to_string()));
}

#[test]
fn a_removal_lands_after_the_saves_before_it() {
    let dir = temp_dir("writer-remove");
    let writer = files::Writer::spawn(dir.clone());
    writer.save(task("7"));
    writer.save(task("8"));
    writer.remove("7".into());
    assert!(writer.flush(std::time::Duration::from_secs(10)));
    let loaded = files::load_all_blocking(&dir);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].1.as_ref().unwrap().id, "8");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A repository with one commit on `main`, as onehand's tests' own git.
fn repo_with_commit(name: &str) -> PathBuf {
    let repo = temp_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    repo
}

#[test]
fn dropping_a_tasks_marks_leaves_every_other_tasks() {
    let repo = repo_with_commit("drop");
    let mine = [
        marks::ref_name("t1", "r", 1, false),
        marks::ref_name("t1", "r2", 1, true),
    ];
    let theirs = [marks::ref_name("t10", "r", 1, false)];
    marks::pin_blocking(&repo, &mine).unwrap();
    marks::pin_blocking(&repo, &theirs).unwrap();
    marks::drop_blocking(&repo, "t1").unwrap();
    assert_eq!(
        git(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/onehand"]
        ),
        theirs[0]
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn the_work_is_read_against_a_mark_and_its_branch() {
    use marks::Against;
    let repo = repo_with_commit("against");
    let mark = marks::pin_blocking(&repo, &[]).unwrap();
    assert_eq!(marks::against_blocking(&repo, &mark), Ok(Against::Same));
    std::fs::write(repo.join("b.txt"), "b").unwrap();
    assert_eq!(marks::against_blocking(&repo, &mark), Ok(Against::Changed));
    std::fs::remove_file(repo.join("b.txt")).unwrap();
    git(&repo, &["checkout", "-q", "-b", "other"]);
    assert_eq!(
        marks::against_blocking(&repo, &mark),
        Ok(Against::OtherBranch("main".into()))
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn a_visits_changes_and_a_files_diff_read_between_two_marks() {
    use crate::diff::Row as D;
    let repo = repo_with_commit("changes");
    std::fs::write(repo.join("gone.txt"), "x").unwrap();
    git(&repo, &["add", "gone.txt"]);
    let from = marks::pin_blocking(&repo, &[]).unwrap();
    std::fs::write(repo.join("a.txt"), "a\nb").unwrap();
    std::fs::write(repo.join("new.txt"), "n").unwrap();
    std::fs::remove_file(repo.join("gone.txt")).unwrap();
    let to = marks::pin_blocking(&repo, &[]).unwrap();
    let changes = marks::changes_blocking(&repo, &from, &to).unwrap();
    let said: Vec<(&str, Option<(u32, u32)>)> =
        changes.iter().map(|c| (c.path.as_str(), c.lines)).collect();
    assert_eq!(
        said,
        [
            ("a.txt", Some((2, 1))),
            ("gone.txt", Some((0, 1))),
            ("new.txt", Some((1, 0))),
        ]
    );
    assert_eq!(
        marks::file_diff_blocking(&repo, &from, &to, "new.txt").unwrap(),
        [D::Added("n".into())]
    );
    assert_eq!(
        marks::file_diff_blocking(&repo, &from, &to, "gone.txt").unwrap(),
        [D::Removed("x".into())]
    );
    assert!(
        marks::file_diff_blocking(&repo, "0000000", &to, "a.txt").is_err(),
        "a mark that is not there is not an empty file"
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn retrying_a_task_let_go_makes_it_live_again() {
    let mut t = task("1");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::TimedOut);
    t.dismissed = true;
    assert_eq!(t.group(None), Group::Finished);
    let template = t.runs[0].template.clone();
    t.retry("2".into(), template, None, None).unwrap();
    assert!(!t.dismissed);
    assert!(t.resumable(), "its new run may start");
}

#[test]
fn needs_attention_is_waiting_and_ended_only() {
    for (group, needs) in [
        (Group::Waiting, true),
        (Group::Ended, true),
        (Group::Running, false),
        (Group::Queued, false),
        (Group::Finished, false),
    ] {
        assert_eq!(group.needs_attention(), needs, "{group:?}");
    }
}

/// Groups in page order; finished newest first, every other group oldest
/// first, and the same answer whatever order the rows came in.
#[test]
fn a_listing_reads_by_group_then_age() {
    let (f1, f3) = (finished("1", "/a", 30), finished("3", "/a", 10));
    let (r2, r4) = (task("2"), task("4"));
    let rows = [
        (Group::Finished, Some(&f1)),
        (Group::Running, Some(&r4)),
        (Group::Finished, Some(&f3)),
        (Group::Running, Some(&r2)),
        (Group::Waiting, None),
    ];
    for turn in 0..rows.len() {
        let mut listed = rows.to_vec();
        listed.rotate_left(turn);
        sort_listed(&mut listed, |row| *row);
        let ids: Vec<&str> = listed
            .iter()
            .map(|(_, task)| task.map_or("-", |t| t.id.as_str()))
            .collect();
        // f1 moved last, so it is the newer finished one.
        assert_eq!(ids, ["-", "2", "4", "1", "3"]);
    }
}

#[test]
fn a_task_stopped_before_it_ran_is_as_new_as_when_it_was_made() {
    let (f1, f3) = (finished("1", "/a", 30), finished("3", "/a", 10));
    let mut never = task(&(20 * 1_000_000_000u128).to_string());
    never.runs.clear();
    let mut listed = [Some(&f3), Some(&never), Some(&f1)];
    sort_listed(&mut listed, |task| (Group::Finished, *task));
    let ids: Vec<&str> = listed.iter().map(|t| t.unwrap().id.as_str()).collect();
    assert_eq!(ids, ["1", never.id.as_str(), "3"]);
}

#[test]
fn a_check_says_whether_it_passed() {
    let mut t = Task::check("1".into(), "true".into(), task("1").setup);
    assert_eq!(t.ended_said(), None);
    t.runs[0].resume();
    t.runs[0].command_finished(passed(None));
    assert_eq!(t.ended_said().as_deref(), Some("Check passed"));
    t.runs[0].outcome = Some(Outcome::Failed("exit 1".into()));
    assert_eq!(t.ended_said().as_deref(), Some("Check failed: exit 1"));
    t.runs[0].outcome = Some(Outcome::Stopped(Stop::ByPerson));
    assert_eq!(t.ended_said().as_deref(), Some("Stopped by hand"));
    let mut w = task("2");
    w.runs[0].outcome = Some(Outcome::Done);
    assert_eq!(w.ended_said(), Some(Outcome::Done.said()));
}

#[test]
fn a_retry_with_a_note_tells_its_first_step_what_to_change() {
    let mut t = task("1");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::TimedOut);
    let template = t.runs[0].template.clone();
    let run = t
        .retry(
            "2".into(),
            template,
            None,
            Some("Address the review.".into()),
        )
        .unwrap();
    assert_eq!(run.revise.as_deref(), Some("Address the review."));
}

/// A task file written before runs kept their failure's kind, their pull
/// request and their commands' results reads, saves and reads again
/// unchanged, its unsent report with it; what it lacks reads as not kept.
#[test]
fn a_task_file_from_before_part_b_reads_and_saves_unchanged() {
    let mut t = issue_task("1", 5, &["1"]);
    if let Source::Issue(issue) = &mut t.source {
        issue.unsent[0].outcome = Some(Outcome::Failed("boom".into()));
    }
    let run = &mut t.runs[0];
    run.outcome = Some(Outcome::Failed("boom".into()));
    run.failure = Some(crate::workflow::Failure::Configuration);
    let mut old = serde_json::to_value(&t).unwrap();
    let kept = old["runs"][0].as_object_mut().unwrap();
    assert!(kept.remove("failure").is_some(), "a newer file names it");
    assert!(!kept.contains_key("pull_request"), "absent is not written");
    let read: Task = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(
        read.runs[0].failed_on(),
        Some(crate::workflow::Failure::Other)
    );
    assert_eq!(read.runs[0].pull_request, None);
    assert!(read.runs[0].visits().iter().all(|v| v.command.is_none()));
    assert_eq!(serde_json::to_value(&read).unwrap(), old, "saved unchanged");
    let again: Task = serde_json::from_value(serde_json::to_value(&read).unwrap()).unwrap();
    assert_eq!(again, read);

    // Every new field round-trips.
    let mut t = issue_task("2", 5, &[]);
    let run = &mut t.runs[0];
    run.failure = Some(crate::workflow::Failure::Forge);
    run.pull_request = Some(crate::workflow::PrOpened {
        number: 7,
        url: "u".into(),
    });
    let back: Task = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(back, t);
}

/// A retry with current settings is a new run of the same task with the
/// setup a new task takes now, from where its plan starts.
#[test]
fn a_retry_with_current_settings_runs_the_new_setup_from_its_start() {
    let mut t = task("1");
    t.runs[0].resume();
    t.runs[0].stopped(Stop::TimedOut);
    let mut setup = t.runs[0].setup.clone();
    setup.mode = Some("auto".into());
    let plan = crate::workflow::WithCurrent {
        template: t.runs[0].template.clone(),
        setup: setup.clone(),
        changes: Vec::new(),
        start: 0,
        why: crate::workflow::StartWhy::CarriedOver,
    };
    let next = t.retry_with("2".into(), plan).unwrap();
    assert_eq!(next.setup, setup);
    assert_eq!(next.step, 0);
    assert_eq!(t.runs.len(), 2, "the old run is kept");
    assert_eq!(t.runs[0].setup.mode, None);
}

/// A task opened a pull request when a run kept it, or, from before runs
/// kept it, when a pull request step's visit ended done on the forge.
#[test]
fn a_task_knows_it_opened_a_pull_request() {
    let template = crate::workflow::builtin::all().remove(2);
    let pr_step = template
        .steps
        .iter()
        .find(|step| step.kind == StepKind::PullRequest)
        .map(|step| step.id.clone())
        .unwrap();
    let mut t = task("1");
    t.runs[0].template = template;
    assert!(!t.opened_pull_request());
    t.runs[0].visits.push(crate::workflow::Visit {
        id: 1,
        step: pr_step,
        started_at: 1,
        ended_at: Some(2),
        start: None,
        end: None,
        output: None,
        why: Some("done on the forge".into()),
        command: None,
    });
    assert!(t.opened_pull_request(), "from before runs kept it");
    t.runs[0].visits.clear();
    t.runs[0].pull_request = Some(crate::workflow::PrOpened {
        number: 7,
        url: "u".into(),
    });
    assert!(t.opened_pull_request());
}

#[test]
fn a_queued_task_is_told_which_task_holds_its_place() {
    assert_eq!(
        queued_said("Fix b", Some("Fix a"), "repo"),
        "Fix b is queued behind Fix a, working in repo"
    );
    assert_eq!(
        queued_said("Fix b", None, "repo"),
        "Fix b is queued behind the task working in repo"
    );
}
