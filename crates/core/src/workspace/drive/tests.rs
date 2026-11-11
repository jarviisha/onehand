// A test that only reads the tree after a change lets its effects go.
#![allow(unused_must_use)]

use super::StartsOn;
use super::*;
use crate::config::AgentSpec;
use std::path::{Path, PathBuf};

fn spec() -> AgentSpec {
    AgentSpec {
        name: "agent".into(),
        command: "x".into(),
        ..Default::default()
    }
}

/// The tree itself, for a test to set up what it needs.
fn tree(drive: &mut WorkspaceDrive) -> &mut Workspace {
    &mut drive.workspace
}

/// A drive on a workspace of `/a`, bound to a folder of its own.
fn bound() -> WorkspaceDrive {
    let mut ws = Workspace::seeded("/a");
    ws.storage_dir = Some(PathBuf::from("/store"));
    WorkspaceDrive::new(ws)
}

/// Effects as names, for asserting their order.
fn names(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|effect| match effect {
            Effect::Close(uid) => format!("Close({uid})"),
            Effect::Forgot(gone) => format!("Forgot({})", gone.path.display()),
            Effect::ForgotOffload(path) => format!("ForgotOffload({})", path.display()),
            Effect::ForgotHere(path) => format!("ForgotHere({})", path.display()),
            Effect::Show => "Show".into(),
            Effect::Changed => "Changed".into(),
            Effect::Put(value) => format!("Put({value:?})"),
            Effect::Write(_) => "Write".into(),
            Effect::WriteOnClose(_) => "WriteOnClose".into(),
            Effect::Arm { .. } => "Arm".into(),
            Effect::Remember { name, .. } => format!("Remember({name})"),
            Effect::Sweep { roots, .. } => format!("Sweep({})", roots.len()),
            Effect::Resweep => "Resweep".into(),
            Effect::GitRead => "GitRead".into(),
            Effect::LookForRuns(path) => format!("LookForRuns({})", path.display()),
            Effect::EndOnHost { session, .. } => format!("EndOnHost({})", session.0),
            Effect::ListHostSessions => "ListHostSessions".into(),
            Effect::Signal(signal) => format!("{signal:?}"),
        })
        .collect()
}

fn armed(fx: &[Effect]) -> u64 {
    fx.iter()
        .find_map(|effect| match effect {
            Effect::Arm { token, .. } => Some(*token),
            _ => None,
        })
        .expect("a wait armed")
}

fn written(fx: Vec<Effect>) -> Pending<Thing, Value> {
    fx.into_iter()
        .find_map(|effect| match effect {
            Effect::Write(write) => Some(write.pending),
            _ => None,
        })
        .expect("a write")
}

// Sessions.

#[test]
fn a_session_is_added_on_the_project_selected_and_shown() {
    let mut drive = bound();
    let (added, fx) = drive.add_session(Some(spec()), 7, None);
    assert_eq!(added, Ok(7));
    assert_eq!(names(&fx), ["Show", "Changed"]);
    assert_eq!(drive.active_uid(), Some(7));
}

#[test]
fn a_session_without_an_agent_or_a_project_is_refused_and_said() {
    let mut drive = bound();
    let (added, fx) = drive.add_session(None, 1, None);
    assert_eq!(added, Err(Refusal::NoAgent));
    assert_eq!(names(&fx), ["Refused(NoAgent)"]);
    drive.forget(0);
    let (added, _) = drive.add_session(Some(spec()), 1, None);
    assert_eq!(added, Err(Refusal::NoProject));
}

/// Beside a worker, a new session goes to its lead's project.
#[test]
fn a_new_session_beside_a_worker_goes_home() {
    let mut drive = bound();
    tree(&mut drive).add_root("/worker");
    drive.add_session(Some(spec()), 1, Some(0));
    assert_eq!(drive.active_root, 0);
    assert_eq!(drive.roots[0].sessions.len(), 1);
}

/// A session asked for on a named project moves the selection there even
/// when it is refused, so the panels follow the rail.
#[test]
fn a_session_asked_for_on_a_project_selects_it_even_when_refused() {
    let mut drive = bound();
    tree(&mut drive).add_root("/b");
    tree(&mut drive).select_root(0);
    let (added, fx) = drive.add_session_in(1, None, 1);
    assert!(added.is_err());
    assert_eq!(drive.active_root, 1);
    assert_eq!(names(&fx), ["Show", "Refused(NoAgent)"]);
}

#[test]
fn ending_a_session_closes_it_and_shows_what_is_left() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    drive.add_session(Some(spec()), 2, None);
    let fx = drive.end(2, None);
    assert_eq!(names(&fx), ["Close(2)", "Changed", "Show"]);
    assert_eq!(drive.active_uid(), Some(1));
    assert!(drive.end(9, None).is_empty(), "nothing of a stranger");
}

/// A run's project left empty goes with its last session.
#[test]
fn a_run_project_goes_with_its_last_session() {
    let mut drive = bound();
    let idx = drive.add_run_session(Path::new("/run"), spec(), 5).unwrap();
    assert!(drive.roots[idx].transient);
    assert!(!drive.left_behind(idx), "a session is still on it");
    assert!(!drive.left_behind(0), "the person's own project stays");
    let fx = drive.end(5, None);
    assert_eq!(
        names(&fx),
        [
            "Close(5)",
            "Changed",
            "ForgotHere(/run)",
            "Forgot(/run)",
            "Changed",
            "Show"
        ]
    );
    assert_eq!(drive.roots.len(), 1);
}

/// A worker on screen closing hands the screen to its lead; one not on
/// screen moves nothing.
#[test]
fn a_worker_on_screen_closing_shows_its_lead() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    let worker = tree(&mut drive).add_root("/worker");
    drive.add_session_in(worker, Some(spec()), 2);
    drive.end(2, Some(1));
    assert_eq!(drive.active_uid(), Some(1));

    let worker = tree(&mut drive).add_root("/worker");
    drive.add_session_in(worker, Some(spec()), 3);
    drive.add_session_in(worker, Some(spec()), 4);
    drive.end(3, Some(1));
    assert_eq!(drive.active_uid(), Some(4), "not on screen: stays put");
}

#[test]
fn a_run_session_goes_on_an_open_project_and_selects_nothing() {
    let mut drive = bound();
    tree(&mut drive).add_root("/b");
    tree(&mut drive).select_root(0);
    assert_eq!(drive.add_run_session(Path::new("/b"), spec(), 3), Some(1));
    assert_eq!(drive.active_root, 0);
    assert!(
        !drive.roots[1].transient,
        "a project a person has stays theirs"
    );
}

#[test]
fn a_run_project_ends_quietly_unless_it_was_selected() {
    let mut drive = bound();
    drive.add_run_session(Path::new("/run"), spec(), 3);
    assert_eq!(
        names(&drive.end_run(Path::new("/run"))),
        ["Close(3)", "ForgotHere(/run)", "Forgot(/run)", "Changed"]
    );
    let idx = drive.add_run_session(Path::new("/run"), spec(), 4).unwrap();
    drive.select_project(idx);
    assert_eq!(
        names(&drive.end_run(Path::new("/run"))).last().unwrap(),
        "Show"
    );
    assert!(
        drive.end_run(Path::new("/a")).is_empty(),
        "a person's project stays"
    );
}

#[test]
fn a_run_project_taken_over_is_written() {
    let mut drive = bound();
    drive.add_run_session(Path::new("/run"), spec(), 3);
    let fx = drive.take_over_run(Path::new("/run"));
    assert_eq!(names(&fx), ["Write"]);
    assert!(!drive.roots[1].transient);
}

#[test]
fn an_opened_session_that_never_opened_takes_only_its_own_project() {
    let mut drive = bound();
    drive.add_run_session(Path::new("/w"), spec(), 3);
    let fx = drive.drop_opened(3, Path::new("/w"), true, false);
    assert_eq!(
        names(&fx),
        [
            "Close(3)",
            "Changed",
            "ForgotHere(/w)",
            "Forgot(/w)",
            "Changed"
        ]
    );

    drive.add_session(Some(spec()), 4, None);
    let fx = drive.drop_opened(4, Path::new("/a"), false, true);
    assert_eq!(
        names(&fx),
        ["Close(4)", "Changed", "Show"],
        "the person's project stays"
    );
}

#[test]
fn a_finished_session_being_watched_stays() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    assert!(drive.close_finished(1, true).is_empty());
    assert_eq!(
        names(&drive.close_finished(1, false)),
        ["Close(1)", "Changed"]
    );
}

#[test]
fn a_session_out_of_range_selects_nothing() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    assert!(drive.select_session(3).is_empty());
    assert_eq!(names(&drive.select(1)), ["Show", "Changed"]);
}

/// A worker in its worktree stands under its lead's project; one whose lead
/// is in another window stands under its own; one gone, nowhere.
#[test]
fn a_worker_stands_under_its_lead_s_project_when_the_lead_is_here() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    let worker = tree(&mut drive).add_root("/worker");
    drive.add_session_in(worker, Some(spec()), 2);
    assert_eq!(drive.home_of(2, Some(1)), Some(0));
    assert_eq!(drive.home_of(2, Some(99)), Some(worker));
    assert_eq!(drive.home_of(9, None), None);
}

// The workspace file.

#[test]
fn an_unbound_workspace_writes_nothing() {
    let mut drive = WorkspaceDrive::new(Workspace::seeded("/a"));
    assert!(drive.save().is_empty());
    assert!(drive.save_soon().is_empty());
    assert!(drive.flush().is_none());
}

/// A burst is one write, once the last change's wait runs out; an earlier
/// wait running out writes nothing.
#[test]
fn a_burst_of_changes_is_written_once() {
    let mut drive = bound();
    let first = armed(&drive.rename("one"));
    let last = armed(&drive.rename("two"));
    assert!(drive.handle(Input::Elapsed { token: first }).is_empty());
    assert_eq!(
        names(&drive.handle(Input::Elapsed { token: last })),
        ["Write"]
    );
    assert!(
        drive.handle(Input::Elapsed { token: last }).is_empty(),
        "once"
    );
}

#[test]
fn a_window_closing_writes_the_burst_still_waiting_and_only_that() {
    let mut drive = bound();
    assert!(drive.flush().is_none());
    drive.rename("one");
    assert!(matches!(drive.flush(), Some(Effect::WriteOnClose(_))));
    assert!(drive.flush().is_none());
}

#[test]
fn a_blank_or_unchanged_name_changes_nothing() {
    let mut drive = bound();
    assert!(drive.rename("  ").is_empty());
    assert!(drive.rename(" Workspace ").is_empty());
}

/// A name whose write fails goes back to the one last saved, and is said
/// once; a name that lands is remembered.
#[test]
fn a_name_whose_write_fails_is_put_back_and_one_that_lands_remembered() {
    let mut drive = bound();
    let token = armed(&drive.rename("Mine"));
    let pending = written(drive.handle(Input::Elapsed { token }));
    let fx = drive.handle(Input::Written {
        pending,
        result: Err("disk full".into()),
    });
    assert_eq!(
        names(&fx),
        [
            "Put(Name(\"Workspace\"))",
            "SaveFailed(\"disk full\")",
            "Changed"
        ]
    );
    assert_eq!(drive.name, "Workspace");

    drive.rename("Mine");
    let pending = written(drive.save());
    let fx = drive.handle(Input::Written {
        pending,
        result: Ok(()),
    });
    assert_eq!(names(&fx), ["Remember(Mine)"]);
}

/// A pin whose write fails goes back alone, and a later pin's write that
/// lands keeps it.
#[test]
fn a_pin_whose_write_fails_is_put_back() {
    let mut drive = bound();
    let pending = written(drive.toggle_pin(0));
    assert!(drive.roots[0].pinned);
    let fx = drive.handle(Input::Written {
        pending,
        result: Err("no".into()),
    });
    assert!(names(&fx)[0].starts_with("Put(Pin"));
    assert!(!drive.roots[0].pinned);
}

#[test]
fn an_order_whose_write_fails_goes_back_to_the_one_saved() {
    let mut drive = bound();
    tree(&mut drive).add_root("/b");
    let pending = written(drive.move_root(1, 0));
    assert_eq!(drive.roots[0].path, PathBuf::from("/b"));
    drive.handle(Input::Written {
        pending,
        result: Err("no".into()),
    });
    assert_eq!(drive.roots[0].path, PathBuf::from("/a"));
    assert!(
        drive.move_root(0, 0).is_empty(),
        "nothing moved, nothing written"
    );
}

/// Unattended runs are switched only on a project a person chose that runs
/// here, and a project switched on is looked at at once.
#[test]
fn unattended_runs_are_switched_only_where_they_can_run() {
    let mut drive = bound();
    let fx = drive.toggle_unattended(0);
    assert_eq!(names(&fx), ["LookForRuns(/a)", "Write", "Changed"]);
    assert_eq!(names(&drive.toggle_unattended(0)), ["Write", "Changed"]);
    let run = drive.add_run_session(Path::new("/run"), spec(), 1).unwrap();
    assert!(drive.toggle_unattended(run).is_empty());
    let host = tree(&mut drive).add_host_root(HostName("box".into()), PathBuf::from("/p"));
    assert!(drive.toggle_unattended(host).is_empty());
}

#[test]
fn a_check_command_is_written_after_the_typing_and_blank_is_none() {
    let mut drive = bound();
    assert_eq!(
        names(&drive.set_check(Path::new("/a"), " make test ")),
        ["Arm", "Changed"]
    );
    assert_eq!(drive.roots[0].check.as_deref(), Some("make test"));
    assert!(drive.set_check(Path::new("/a"), "make test").is_empty());
    drive.set_check(Path::new("/a"), "  ");
    assert_eq!(drive.roots[0].check, None);
}

// Projects.

#[test]
fn a_folder_opened_is_selected_shown_swept_and_written() {
    let mut drive = bound();
    let fx = drive.open_project("/b".into());
    assert_eq!(names(&fx), ["Show", "Resweep", "Write", "Changed"]);
    assert_eq!(drive.active_root, 1);
    drive.open_project("/b".into());
    assert_eq!(drive.roots.len(), 2, "an open project is not doubled");
}

#[test]
fn a_folder_kept_for_work_is_added_once() {
    let mut drive = bound();
    let (idx, fx) = drive.keep_project("/wt".into());
    assert_eq!(names(&fx), ["Resweep", "Write", "Changed"]);
    let (again, fx) = drive.keep_project("/wt".into());
    assert_eq!((idx, fx.len()), (again, 0));
    tree(&mut drive).select_root(0);
    drive.keep_project_quietly("/task".into());
    assert_eq!(drive.active_root, 0, "nothing selected");
}

/// Removing asks first only when something would be lost.
#[test]
fn removing_asks_only_when_it_loses_something() {
    let mut drive = bound();
    assert_eq!(drive.losses(0, 0), None);
    assert_eq!(
        drive.losses(0, 2),
        Some(Losses {
            sessions: 0,
            unsaved: 2
        })
    );
    drive.add_session(Some(spec()), 1, None);
    assert_eq!(
        drive.losses(0, 0),
        Some(Losses {
            sessions: 1,
            unsaved: 0
        })
    );
}

/// A removal lands on the project it was asked about, by host and path,
/// after a reorder; its sessions close first, then it is said and written.
#[test]
fn a_removal_lands_by_host_and_path() {
    let mut drive = bound();
    drive.open_project("/b".into());
    drive.add_session(Some(spec()), 1, None);
    drive.open_host_project(HostName("box".into()), "/b".into());
    let fx = drive.remove(None, Path::new("/b"));
    assert_eq!(
        names(&fx),
        [
            "Close(1)",
            "ForgotHere(/b)",
            "Forgot(/b)",
            "Changed",
            "Removed(\"b\")",
            "Show",
            "Write"
        ]
    );
    assert!(drive
        .roots
        .iter()
        .any(|r| r.host.is_some() && r.path == Path::new("/b")));
    assert!(
        drive.remove(None, Path::new("/b")).is_empty(),
        "gone already"
    );
}

// The git sweep.

/// A sweep reads the folders here and those on an attached host, never a
/// host's folder on a local one's path; an older sweep's answer is dropped.
#[test]
fn a_sweep_reads_what_it_can_and_keeps_only_the_newest_answer() {
    let mut drive = bound();
    drive.open_host_project(HostName("box".into()), "/srv".into());
    drive.open_host_project(HostName("off".into()), "/x".into());
    drive.open_host_project(HostName("box".into()), "/a".into());
    let Effect::Sweep { token: old, roots } = drive.sweep(|host| host.0 == "box") else {
        panic!("a sweep");
    };
    let read: Vec<_> = roots.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(read, ["/a", "/srv"]);
    let Effect::Sweep { token: new, .. } = drive.sweep(|_| true) else {
        panic!("a sweep");
    };
    let status = |branch: &str| crate::gitstat::GitStatus {
        branch: branch.into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    assert!(drive
        .handle(Input::Swept {
            token: old,
            found: vec![("/a".into(), status("old"))],
        })
        .is_empty());
    let fx = drive.handle(Input::Swept {
        token: new,
        found: vec![("/a".into(), status("main"))],
    });
    assert_eq!(names(&fx), ["GitRead"]);
    assert_eq!(drive.git_of(&drive.roots[0]).unwrap().branch, "main");
    assert!(
        drive.git_of(&drive.roots[3]).is_none(),
        "a host's folder on /a"
    );
}

/// A sweep under way when a project leaves would put back what was read of
/// it: that part of its answer is dropped, the rest kept.
#[test]
fn a_sweep_under_way_when_a_project_leaves_is_dropped() {
    let mut drive = bound();
    drive.open_project("/b".into());
    let Effect::Sweep { token, .. } = drive.sweep(|_| false) else {
        panic!("a sweep");
    };
    drive.forget(1);
    let status = crate::gitstat::GitStatus {
        branch: "main".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    drive.handle(Input::Swept {
        token,
        found: vec![("/b".into(), status.clone()), ("/a".into(), status)],
    });
    assert_eq!(drive.git().keys().collect::<Vec<_>>(), [Path::new("/a")]);
}

/// A late sweep read a local folder; once it leaves, a host's folder on the
/// same path is not given what was read of the local one.
#[test]
fn a_late_sweep_does_not_give_a_host_folder_the_local_one_s_status() {
    let mut drive = bound();
    drive.open_host_project(HostName("box".into()), "/a".into());
    let Effect::Sweep { token, roots } = drive.sweep(|_| true) else {
        panic!("a sweep");
    };
    assert_eq!(roots.len(), 1, "the host's folder on /a is not read");
    drive.forget(0);
    let status = crate::gitstat::GitStatus {
        branch: "main".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    drive.handle(Input::Swept {
        token,
        found: vec![("/a".into(), status)],
    });
    assert!(drive.git().is_empty());
}

/// A run is offered on a repository here that runs here and a person chose,
/// once unattended runs are set up.
#[test]
fn runs_are_offered_on_a_chosen_repository_here_once_set_up() {
    let mut drive = bound();
    drive.open_project("/b".into());
    drive.add_run_session(Path::new("/run"), spec(), 1);
    let Effect::Sweep { token, .. } = drive.sweep(|_| true) else {
        panic!("a sweep");
    };
    let status = crate::gitstat::GitStatus {
        branch: "main".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    drive.handle(Input::Swept {
        token,
        found: vec![("/a".into(), status.clone()), ("/run".into(), status)],
    });
    assert_eq!(drive.offered_for_runs(true), [PathBuf::from("/a")]);
    assert!(drive.offered_for_runs(false).is_empty());
}

/// What goes with a project depends on where it is: a host's folder leaves
/// nothing filed under its path here, an offloaded one stops its code being
/// checked for.
#[test]
fn what_goes_with_a_project_is_said_by_where_it_is() {
    let mut drive = bound();
    drive.open_host_project(HostName("box".into()), "/srv".into());
    let fx = drive.forget(1);
    assert_eq!(names(&fx), ["Forgot(/srv)", "Changed"]);
    drive.open_project("/b".into());
    drive.set_offload(
        Path::new("/b"),
        Some(crate::workspace::Offloaded {
            host: HostName("box".into()),
            mirror: "/m".into(),
            branch: None,
        }),
    );
    let fx = drive.forget(1);
    assert_eq!(
        names(&fx),
        [
            "ForgotOffload(/b)",
            "ForgotHere(/b)",
            "Forgot(/b)",
            "Changed"
        ]
    );
}

/// A fork opens on its project; one whose project left meanwhile is said
/// and adds nothing; one that failed ends its session and is said.
#[test]
fn a_fork_opens_on_its_project_or_says_why_not() {
    let mut drive = bound();
    let (added, _) = drive.fork(Path::new("/a"), spec(), 1);
    assert_eq!(added, Ok(1));
    assert_eq!(drive.active_uid(), Some(1));
    let (added, fx) = drive.fork(Path::new("/gone"), spec(), 2);
    assert!(added.is_err());
    assert_eq!(
        names(&fx),
        [r#"ForkFailed("its project left the workspace")"#]
    );
    assert!(drive.locate(2).is_none(), "nothing added");
    let fx = drive.fork_failed(Some(1), None, "unsaved".into());
    assert_eq!(names(&fx)[0], "Close(1)");
    assert_eq!(names(&fx).last().unwrap(), r#"ForkFailed("unsaved")"#);
    let fx = drive.fork_failed(None, None, "unsaved".into());
    assert_eq!(names(&fx), [r#"ForkFailed("unsaved")"#]);
}

/// *End on <host>* closes the session watching it first, then ends it
/// there; ended, the hosts' sessions are listed again, and a failure is
/// said.
#[test]
fn ending_on_a_host_closes_its_watcher_first_and_says_a_failure() {
    use crate::host::wire::HostSession;
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    let host = HostName("vps".into());
    let fx = drive.end_on_host(
        host.clone(),
        HostSession("h-1".into()),
        "Fix".into(),
        Some(1),
        None,
    );
    let said = names(&fx);
    assert_eq!(said.first().map(String::as_str), Some("Close(1)"));
    assert_eq!(said.last().map(String::as_str), Some("EndOnHost(h-1)"));
    let ended = |result| Input::EndedOnHost {
        host: host.clone(),
        label: "Fix".into(),
        result,
    };
    assert_eq!(names(&drive.handle(ended(Ok(())))), ["ListHostSessions"]);
    let fx = drive.handle(ended(Err("away".into())));
    assert!(matches!(
        fx.as_slice(),
        [Effect::Signal(Signal::EndOnHostFailed { why, .. })] if why == "away"
    ));
}

/// What a session is added on is what it starts on when first shown, once;
/// one added plainly starts fresh, and one closed takes what it was to start
/// on with it.
#[test]
fn a_session_starts_on_what_it_was_added_on_once() {
    let mut drive = bound();
    let resume = StartsOn::Resume(PathBuf::from("/store/c1"));
    let _ = drive.add_session_on(Some(spec()), 1, None, resume.clone());
    let _ = drive.add_session(Some(spec()), 2, None);
    assert_eq!(drive.take_start(1), resume);
    assert_eq!(drive.take_start(1), StartsOn::Fresh, "only the first show");
    assert_eq!(drive.take_start(2), StartsOn::Fresh);
    let _ = drive.add_session_on(Some(spec()), 3, None, resume);
    let _ = drive.end(3, None);
    assert_eq!(
        drive.take_start(3),
        StartsOn::Fresh,
        "gone with its session"
    );
    let (added, _) = drive.fork(Path::new("/a"), spec(), 4);
    assert_eq!(added, Ok(4));
    assert_eq!(drive.take_start(4), StartsOn::Fork);
    let resume = StartsOn::Resume(PathBuf::from("/store/c5"));
    let _ = drive.add_session_on(Some(spec()), 5, None, resume);
    let idx = drive.local_index(Path::new("/a")).expect("its project");
    let _ = drive.forget(idx);
    assert_eq!(
        drive.take_start(5),
        StartsOn::Fresh,
        "gone with its project"
    );
}

/// A host session already watched is shown again rather than opened twice;
/// one in another folder, or on another host, is not that one. What its
/// host says it watches counts as much as what it was opened on, and a
/// session closed watches nothing.
#[test]
fn a_host_session_watched_is_shown_again_not_opened_twice() {
    use crate::host::wire::HostSession;
    let mut drive = bound();
    let host = HostName("box".into());
    let _ = drive.open_host_project(host.clone(), "/srv".into());
    let id = HostSession("h-1".into());
    let (first, _) = drive.watch(id.clone(), Some(spec()), 1);
    assert_eq!(first, Ok(1));
    assert_eq!(drive.take_start(1), StartsOn::Watch(id.clone()));
    let (again, _) = drive.watch(id.clone(), None, 2);
    assert_eq!(again, Ok(1), "the one already on it, its agent gone or not");
    assert!(drive.locate(2).is_none(), "nothing added");
    assert_eq!(drive.watched_on(1), Some((host.clone(), id.clone())));
    assert_eq!(drive.watcher(&HostName("off".into()), &id, None), None);
    assert_eq!(
        drive.watcher(&host, &id, Some(Path::new("/elsewhere"))),
        None
    );
    let _ = drive.add_session(Some(spec()), 3, None);
    let later = HostSession("h-2".into());
    drive.watching(3, later.clone());
    assert_eq!(drive.watcher(&host, &later, None), Some(3));
    assert_eq!(drive.watched_on(3), Some((host.clone(), later.clone())));
    let _ = drive.end(3, None);
    assert_eq!(drive.watcher(&host, &later, None), None);
    let (none, _) = drive.watch(HostSession("h-3".into()), None, 4);
    assert_eq!(none, Err(Refusal::NoAgent), "a new one needs its agent");
}
