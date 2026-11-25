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

/// A project of the folder `path` here, its id taken from the path.
fn at(path: &str) -> crate::project::Project {
    crate::project::Project::new(
        crate::project::ProjectId::mint(u64::from(crate::workspace::fnv1a(path.as_bytes()))),
        None,
        path.into(),
    )
}

/// [`at`] for a folder on `host`.
fn at_on(host: &str, path: &str) -> crate::project::Project {
    let mut project = at(&format!("{host}:{path}"));
    project.host = Some(HostName(host.into()));
    project.path = path.into();
    project
}

/// The tree itself, for a test to set up what it needs.
fn tree(drive: &mut WorkspaceDrive) -> &mut Workspace {
    &mut drive.workspace
}

/// A drive on a workspace of `/a`, kept in a folder of its own.
fn bound() -> WorkspaceDrive {
    let mut ws = Workspace::seeded("/a");
    ws.storage_dir = PathBuf::from("/store");
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
            Effect::EndOnHost { session, .. } => format!("EndOnHost({})", session.0),
            Effect::ListHostSessions => "ListHostSessions".into(),
            Effect::ReadConversation(id) => format!("ReadConversation({id})"),
            Effect::Unplaced(path) => format!("Unplaced({})", path.display()),
            Effect::Deleted(dir) => format!("Deleted({})", dir.display()),
            Effect::Signal(signal) => format!("{signal:?}"),
        })
        .collect()
}

fn armed(fx: &[Effect]) -> u64 {
    fx.iter()
        .find_map(|effect| match effect {
            Effect::Arm { token, .. } => Some(*token),
            Effect::Close(_)
            | Effect::Forgot(_)
            | Effect::ForgotOffload(_)
            | Effect::ForgotHere(_)
            | Effect::Show
            | Effect::Changed
            | Effect::Put(_)
            | Effect::Write(_)
            | Effect::WriteOnClose(_)
            | Effect::Remember { .. }
            | Effect::Deleted(_)
            | Effect::Sweep { .. }
            | Effect::Resweep
            | Effect::GitRead
            | Effect::EndOnHost { .. }
            | Effect::ListHostSessions
            | Effect::ReadConversation(_)
            | Effect::Unplaced(_)
            | Effect::Signal(_) => None,
        })
        .expect("a wait armed")
}

fn written(fx: Vec<Effect>) -> Pending<Thing, Value> {
    fx.into_iter()
        .find_map(|effect| match effect {
            Effect::Write(write) => Some(write.pending),
            Effect::Close(_)
            | Effect::Forgot(_)
            | Effect::ForgotOffload(_)
            | Effect::ForgotHere(_)
            | Effect::Show
            | Effect::Changed
            | Effect::Put(_)
            | Effect::WriteOnClose(_)
            | Effect::Arm { .. }
            | Effect::Remember { .. }
            | Effect::Sweep { .. }
            | Effect::Resweep
            | Effect::GitRead
            | Effect::EndOnHost { .. }
            | Effect::ListHostSessions
            | Effect::ReadConversation(_)
            | Effect::Unplaced(_)
            | Effect::Deleted(_)
            | Effect::Signal(_) => None,
        })
        .expect("a write")
}

/// The sweep `effect` asks for: its token and the projects it reads.
fn swept(effect: Effect) -> (u64, Vec<SweepProject>) {
    match effect {
        Effect::Sweep { token, roots } => (token, roots),
        e @ (Effect::Close(_)
        | Effect::Forgot(_)
        | Effect::ForgotOffload(_)
        | Effect::ForgotHere(_)
        | Effect::Show
        | Effect::Changed
        | Effect::Put(_)
        | Effect::Write(_)
        | Effect::WriteOnClose(_)
        | Effect::Arm { .. }
        | Effect::Remember { .. }
        | Effect::Resweep
        | Effect::GitRead
        | Effect::EndOnHost { .. }
        | Effect::ListHostSessions
        | Effect::ReadConversation(_)
        | Effect::Unplaced(_)
        | Effect::Deleted(_)
        | Effect::Signal(_)) => {
            panic!("not a sweep: {:?}", names(&[e]))
        }
    }
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
    let idx = drive
        .add_run_session(Path::new("/run"), spec(), 5, None)
        .unwrap();
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
    assert_eq!(
        drive.add_run_session(Path::new("/b"), spec(), 3, None),
        Some(1)
    );
    assert_eq!(drive.active_root, 0);
    assert!(
        !drive.roots[1].transient,
        "a project a person has stays theirs"
    );
}

#[test]
fn an_opened_session_that_never_opened_takes_only_its_own_project() {
    let mut drive = bound();
    drive.add_run_session(Path::new("/w"), spec(), 3, None);
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
    assert_eq!(names(drive.flush().as_slice()), ["WriteOnClose"]);
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

// Projects.

#[test]
fn a_folder_opened_is_selected_shown_swept_and_written() {
    let mut drive = bound();
    let fx = drive.open_project("/b".into(), Some(&at("/b")));
    assert_eq!(names(&fx), ["Show", "Resweep", "Write", "Changed"]);
    assert_eq!(drive.active_root, 1);
    drive.open_project("/b".into(), Some(&at("/b")));
    assert_eq!(drive.roots.len(), 2, "an open project is not doubled");
}

#[test]
fn a_folder_kept_for_work_is_added_once() {
    let mut drive = bound();
    let (idx, fx) = drive.keep_project("/wt".into(), Some(&at("/wt")));
    assert_eq!(names(&fx), ["Resweep", "Write", "Changed"]);
    let (again, fx) = drive.keep_project("/wt".into(), Some(&at("/wt")));
    assert_eq!((idx, fx.len()), (again, 0));
    tree(&mut drive).select_root(0);
    drive.keep_project_quietly("/task".into(), Some(&at("/task")));
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
    drive.open_project("/b".into(), Some(&at("/b")));
    drive.add_session(Some(spec()), 1, None);
    drive.open_host_project(&at_on("box", "/b"));
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
    drive.open_host_project(&at_on("box", "/srv"));
    drive.open_host_project(&at_on("off", "/x"));
    drive.open_host_project(&at_on("box", "/a"));
    let (old, roots) = swept(drive.sweep(|host| host.0 == "box"));
    let read: Vec<_> = roots.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(read, ["/a", "/srv"]);
    let (new, _) = swept(drive.sweep(|_| true));
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
    drive.open_project("/b".into(), Some(&at("/b")));
    let (token, _) = swept(drive.sweep(|_| false));
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
    drive.open_host_project(&at_on("box", "/a"));
    let (token, roots) = swept(drive.sweep(|_| true));
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

/// What goes with a project depends on where it is: a host's folder leaves
/// nothing filed under its path here, an offloaded one stops its code being
/// checked for.
#[test]
fn what_goes_with_a_project_is_said_by_where_it_is() {
    let mut drive = bound();
    drive.open_host_project(&at_on("box", "/srv"));
    let fx = drive.forget(1);
    assert_eq!(names(&fx), ["Forgot(/srv)", "Changed"]);
    drive.open_project("/b".into(), Some(&at("/b")));
    tree(&mut drive).roots[1].offload = Some(crate::workspace::Offloaded {
        host: HostName("box".into()),
        mirror: "/m".into(),
        branch: None,
    });
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
    let failed = Signal::EndOnHostFailed {
        host: host.clone(),
        label: "Fix".into(),
        why: "away".into(),
    };
    assert_eq!(names(&fx), [format!("{failed:?}")]);
}

/// A session on a host leaving its conversation for a fresh one ends the
/// host session it watched, keeping the session here, and watches nothing
/// until its host names the new one; a session of this machine ends nothing.
#[test]
fn a_new_conversation_on_a_host_ends_the_one_it_left() {
    use crate::host::wire::HostSession;
    let mut drive = bound();
    let host = HostName("box".into());
    let _ = drive.open_host_project(&at_on(&host.0, "/srv"));
    let (watching, _) = drive.watch(HostSession("h-1".into()), Some(spec()), 1);
    assert_eq!(watching, Ok(1));
    let fx = drive.left_on_host(1, "Fix".into());
    assert_eq!(names(&fx), ["EndOnHost(h-1)"]);
    assert!(drive.locate(1).is_some(), "the session stays");
    assert_eq!(drive.watched_on(1), None);
    assert!(drive.left_on_host(1, "Fix".into()).is_empty(), "once");
    let _ = drive.add_session(Some(spec()), 2, None);
    assert!(drive.left_on_host(2, "Here".into()).is_empty());
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
    let _ = drive.open_host_project(&at_on(&host.0, "/srv"));
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

// A conversation asked for again.

fn saved(root: &str, there: bool, agent: &str) -> Option<Saved> {
    Some(Saved {
        root: PathBuf::from(root),
        there,
        agent: agent.into(),
        resume: PathBuf::from("/conv/c1"),
    })
}

fn read(drive: &mut WorkspaceDrive, found: Option<Saved>) -> Vec<Effect> {
    drive.handle(Input::ConversationRead {
        found,
        agents: vec![spec()],
        uid: 9,
        project: None,
    })
}

#[test]
fn a_live_conversation_is_shown_where_it_is_else_read() {
    let mut drive = bound();
    let (_, _) = drive.add_session(Some(spec()), 7, None);
    match drive.open_conversation::<u8>("c1", Live::Here(7)) {
        Reach::Here(fx) => assert!(names(&fx).contains(&"Show".to_string())),
        Reach::There(..) => panic!("here"),
    }
    match drive.open_conversation("c1", Live::There(8, 'w')) {
        Reach::There(uid, window) => assert_eq!((uid, window), (8, 'w')),
        Reach::Here(fx) => panic!("there: {:?}", names(&fx)),
    }
    match drive.open_conversation::<u8>("c1", Live::Nowhere) {
        Reach::Here(fx) => assert_eq!(names(&fx), ["ReadConversation(c1)"]),
        Reach::There(..) => panic!("read"),
    }
}

#[test]
fn a_saved_conversation_resumes_on_its_folder_kept_again() {
    let mut drive = bound();
    let fx = read(&mut drive, saved("/run", true, "agent"));
    assert!(
        names(&fx).contains(&"Unplaced(/run)".to_string()),
        "a folder of no project known is said to be placed: {:?}",
        names(&fx)
    );
    let idx = drive.local_index(Path::new("/run")).expect("kept again");
    assert_eq!(drive.active_root, idx);
    assert_eq!(drive.locate(9).map(|(root, _)| root), Some(idx));
    assert!(names(&fx).contains(&"Show".to_string()));
    assert_eq!(
        drive.take_start(9),
        StartsOn::Resume(PathBuf::from("/conv/c1"))
    );
    let roots = drive.roots.len();
    read(&mut drive, saved("/run", true, "agent"));
    assert_eq!(
        drive.roots.len(),
        roots,
        "a project already here is not added twice"
    );
}

#[test]
fn a_conversation_nothing_can_resume_is_said_and_keeps_nothing() {
    let mut drive = bound();
    assert_eq!(names(&read(&mut drive, None)), ["ConversationNotSaved"]);
    assert_eq!(
        names(&read(&mut drive, saved("/run", true, "gone"))),
        [r#"AgentGone("gone")"#]
    );
    assert_eq!(drive.local_index(Path::new("/run")), None, "no folder kept");
    assert_eq!(
        names(&read(&mut drive, saved("/run", false, "agent"))),
        [r#"FolderGone("/run")"#]
    );
    assert_eq!(drive.locate(9), None);
}

// What a start finds to start on.

#[test]
fn a_project_gone_is_refused_and_said() {
    let drive = bound();
    let (found, fx) = drive.find_project(Path::new("/a"));
    assert_eq!((found, fx.len()), (Ok(0), 0));
    let (found, fx) = drive.find_project(Path::new("/gone"));
    assert_eq!(found, Err(Refusal::ProjectGone));
    assert_eq!(names(&fx), ["Refused(ProjectGone)"]);
    assert_eq!(drive.local_project(0).0, Ok(PathBuf::from("/a")));
    assert_eq!(drive.local_project(5).0, Err(Refusal::ProjectGone));
    let mut drive = drive;
    let host = HostName("far".into());
    let idx = tree(&mut drive).add_host_root(host, PathBuf::from("/srv/b"));
    let (on_host, fx) = drive.local_project(idx);
    assert_eq!(on_host, Err(Refusal::NotHere), "still a project, on a host");
    assert_eq!(names(&fx), ["Refused(NotHere)"]);
}

#[test]
fn a_run_check_needs_a_project_here_with_a_check_command() {
    let mut drive = bound();
    let (target, fx) = drive.check_target();
    assert_eq!(target, Err(Refusal::NoCheck));
    assert_eq!(names(&fx), ["Refused(NoCheck)"]);
    tree(&mut drive).roots[0].check = Some("make test".into());
    assert_eq!(
        drive.check_target().0,
        Ok((PathBuf::from("/a"), "make test".to_string()))
    );
    let host = HostName("far".into());
    let idx = tree(&mut drive).add_host_root(host, PathBuf::from("/srv/b"));
    tree(&mut drive).select_root(idx);
    assert_eq!(drive.check_target().0, Err(Refusal::NotHere));
    tree(&mut drive).roots.clear();
    assert_eq!(drive.check_target().0, Err(Refusal::NoProject));
}

/// From the moment a workspace is deleted, nothing writes its file again:
/// not a burst waiting, not the write a closing window makes, not a change.
#[test]
fn a_deleted_workspace_writes_nothing() {
    let mut drive = bound();
    let token = armed(&drive.rename("one"));
    assert_eq!(names(&drive.delete()), ["Deleted(/store)"]);
    assert!(drive.handle(Input::Elapsed { token }).is_empty());
    assert!(drive.flush().is_none());
    assert!(drive.save().is_empty());
    assert!(drive.save_soon().is_empty());
    assert!(names(&drive.rename("two"))
        .iter()
        .all(|n| n != "Write" && n != "Arm"));
}

// What a project tells its rows.

#[test]
fn told_a_project_relabels_its_row_and_tells_its_folders_again() {
    let mut p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.open_project("/code/a-x".into(), Some(&p));
    assert_eq!(drive.roots.len(), 2, "one row for the project");
    p.name = "Alpha".into();
    p.check = Some("make".into());
    p.worktrees.push(crate::project::Worktree {
        path: "/code/a-y".into(),
        branch: "y".into(),
        from: None,
        made_by: crate::project::MadeBy::Split,
        offload: None,
    });
    let fx = drive.told(&p);
    assert_eq!(names(&fx), ["Changed"], "nothing is written");
    assert_eq!(drive.roots[1].label, "Alpha");
    assert_eq!(drive.roots[1].check.as_deref(), Some("make"));
    assert_eq!(drive.roots[1].branch_of(Path::new("/code/a-y")), Some("y"));
    assert_eq!(
        drive.roots[0].label, "a",
        "a row of no project is untouched"
    );
    assert!(drive.told(&p).is_empty(), "told again, nothing changed");
}

#[test]
fn told_an_offload_drops_the_sessions_of_that_folder_only() {
    let mut p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session(Some(spec()), 1, None);
    drive.add_session_at(Some(spec()), 2, PathBuf::from("/code/a-x"));
    p.worktrees[0].offload = Some(crate::workspace::Offloaded {
        host: HostName("box".into()),
        mirror: "/m".into(),
        branch: None,
    });
    let fx = drive.told(&p);
    assert!(
        names(&fx).contains(&"Close(2)".to_string()),
        "{:?}",
        names(&fx)
    );
    assert!(!names(&fx).contains(&"Close(1)".to_string()));
    assert!(drive.roots[1].worktrees[0].offload.is_some());
    assert!(drive.roots[1].offload.is_none());
}

#[test]
fn a_row_of_no_project_is_placed_written_and_drawn_from_it() {
    let mut drive = bound();
    let mut p = at("/a");
    p.name = "Alpha".into();
    p.check = Some("make".into());
    let fx = drive.placed(None, Path::new("/a"), &p);
    assert_eq!(names(&fx), ["Write", "Changed"]);
    assert_eq!(drive.roots[0].project.as_ref(), Some(&p.id));
    assert_eq!(drive.roots[0].label, "Alpha");
    assert_eq!(drive.roots[0].check.as_deref(), Some("make"));
    assert!(
        drive.placed(None, Path::new("/a"), &p).is_empty(),
        "placed once"
    );
    assert!(
        drive
            .placed(None, Path::new("/a"), &at("/other"))
            .is_empty(),
        "a project not holding the folder places nothing"
    );
}

#[test]
fn a_folder_of_no_known_project_comes_in_at_once_and_is_said_unplaced() {
    let mut drive = bound();
    let fx = drive.open_project("/b".into(), None);
    assert_eq!(
        names(&fx),
        ["Unplaced(/b)", "Show", "Resweep", "Write", "Changed"]
    );
    assert_eq!(drive.roots[1].project, None);
    assert!(
        !names(&drive.open_project("/b".into(), None)).contains(&"Unplaced(/b)".to_string()),
        "a row already there is not placed again"
    );
    let (_, fx) = drive.keep_project("/c".into(), Some(&at("/c")));
    assert!(
        !names(&fx).iter().any(|n| n.starts_with("Unplaced")),
        "a known project's folder"
    );
}

// The folder a session runs in.

#[test]
fn a_session_runs_in_the_folder_it_was_added_in() {
    let mut drive = bound();
    assert_eq!(
        drive.roots[0].folder_on_screen(),
        Path::new("/a"),
        "no session: the row's"
    );
    let (added, _) = drive.add_session(Some(spec()), 1, None);
    assert_eq!(added, Ok(1));
    assert_eq!(
        drive.folder_of(1),
        Some(PathBuf::from("/a")),
        "the row's own folder"
    );
    tree(&mut drive).add_session_at(spec(), 2, PathBuf::from("/a-x"));
    assert_eq!(drive.folder_of(2), Some(PathBuf::from("/a-x")));
    assert_eq!(
        drive.roots[0].folder_on_screen(),
        Path::new("/a-x"),
        "the session on screen's"
    );
    assert_eq!(drive.folder_of(1), Some(PathBuf::from("/a")));
}

#[test]
fn dropping_the_sessions_of_a_folder_leaves_the_rows_others() {
    let mut drive = bound();
    drive.add_session(Some(spec()), 1, None);
    tree(&mut drive).add_session_at(spec(), 2, PathBuf::from("/a-x"));
    let fx = drive.drop_sessions_in(Path::new("/a-x"));
    assert!(
        names(&fx).contains(&"Close(2)".to_string()),
        "{:?}",
        names(&fx)
    );
    assert!(!names(&fx).contains(&"Close(1)".to_string()));
    assert_eq!(drive.locate(1), Some((0, 0)));
}

#[test]
fn a_sweep_reads_each_folder_a_session_runs_in() {
    let mut drive = bound();
    tree(&mut drive).add_session_at(spec(), 2, PathBuf::from("/a-x"));
    let (token, roots) = swept(drive.sweep(|_| true));
    let read: Vec<_> = roots.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(read, ["/a", "/a-x"]);
    let status = crate::gitstat::GitStatus {
        branch: "feat".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    drive.handle(Input::Swept {
        token,
        found: vec![("/a-x".into(), status)],
    });
    assert_eq!(drive.git().get(Path::new("/a-x")).unwrap().branch, "feat");
    let shown = drive.git_on_screen(drive.active_root().unwrap());
    assert_eq!(
        shown.map(|s| s.branch.as_str()),
        Some("feat"),
        "the session's folder"
    );
}

/// A session forked in a worktree's folder runs there, on the project
/// holding it.
#[test]
fn a_fork_runs_in_the_folder_it_was_forked_from() {
    let mut drive = bound();
    let (added, _) = drive.add_session_at(Some(spec()), 2, PathBuf::from("/a-x"));
    assert_eq!(added, Ok(2));
    let (added, _) = drive.fork(Path::new("/a-x"), spec(), 3);
    assert_eq!(added, Ok(3));
    assert_eq!(drive.folder_of(3), Some(PathBuf::from("/a-x")));
    assert_eq!(
        drive.locate(3).map(|(root, _)| root),
        drive.locate(2).map(|(root, _)| root)
    );
    assert!(matches!(drive.take_start(3), StartsOn::Fork));
}

// One row per project.

/// [`at`] with a worktree `<path>-x` on branch `x`.
fn with_worktree(path: &str) -> crate::project::Project {
    let mut p = at(path);
    p.worktrees.push(crate::project::Worktree {
        path: format!("{path}-x").into(),
        branch: "x".into(),
        from: None,
        made_by: crate::project::MadeBy::Split,
        offload: None,
    });
    p
}

#[test]
fn a_worktree_folder_opens_on_its_projects_row() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a-x".into(), Some(&p));
    assert_eq!(drive.roots.len(), 2);
    assert_eq!(drive.roots[1].path, PathBuf::from("/code/a"));
    assert_eq!(drive.active_root, 1);
    drive.open_project("/code/a".into(), Some(&p));
    assert_eq!(
        drive.roots.len(),
        2,
        "the project's own folder: the same row"
    );
    let (idx, _) = drive.keep_project("/code/a-x".into(), Some(&p));
    assert_eq!(idx, 1);
    let (added, _) = drive.add_session_at(Some(spec()), 1, PathBuf::from("/code/a-x"));
    assert_eq!(added, Ok(1));
    assert_eq!(drive.locate(1), Some((1, 0)));
    assert_eq!(drive.folder_of(1), Some(PathBuf::from("/code/a-x")));
    assert_eq!(
        drive.add_run_session(Path::new("/code/a-x"), spec(), 2, None),
        Some(1),
        "a run in a worktree sits on its project's row"
    );
    assert_eq!(drive.folder_of(2), Some(PathBuf::from("/code/a-x")));
}

#[test]
fn a_worktree_row_an_unplaced_folder_was_folds_into_its_project() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a-x".into(), None);
    tree(&mut drive).add_session_at(spec(), 3, PathBuf::from("/code/a-x"));
    drive.open_project("/code/a".into(), Some(&p));
    drive.select_project(1);
    assert_eq!(drive.roots.len(), 3);
    drive.placed(None, Path::new("/code/a-x"), &p);
    assert_eq!(drive.roots.len(), 2, "folded into the project's row");
    assert_eq!(drive.locate(3), Some((1, 0)));
    assert_eq!(drive.folder_of(3), Some(PathBuf::from("/code/a-x")));
    assert_eq!(drive.active_root, 1, "the selection follows the fold");
}

#[test]
fn removing_a_row_forgets_every_folder_it_held() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session(Some(spec()), 1, None);
    drive.add_session_at(Some(spec()), 2, PathBuf::from("/code/a-x"));
    let fx = names(&drive.remove(None, Path::new("/code/a")));
    for want in [
        "Close(1)",
        "Close(2)",
        "ForgotHere(/code/a)",
        "ForgotHere(/code/a-x)",
        "Forgot(/code/a)",
    ] {
        assert!(fx.contains(&want.to_string()), "{want} in {fx:?}");
    }
}

#[test]
fn a_conversation_that_ran_in_a_worktree_resumes_there() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.handle(Input::ConversationRead {
        found: saved("/code/a-x", true, "agent"),
        agents: vec![spec()],
        uid: 9,
        project: Some(Box::new(p)),
    });
    assert_eq!(drive.roots[1].path, PathBuf::from("/code/a"));
    assert_eq!(drive.locate(9), Some((1, 0)));
    assert_eq!(drive.folder_of(9), Some(PathBuf::from("/code/a-x")));
    assert_eq!(
        drive.take_start(9),
        StartsOn::Resume(PathBuf::from("/conv/c1"))
    );
}

#[test]
fn a_new_session_beside_one_in_a_worktree_starts_in_its_folder() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session_at(Some(spec()), 1, PathBuf::from("/code/a-x"));
    drive.add_session(Some(spec()), 2, None);
    assert_eq!(
        drive.folder_of(2),
        Some(PathBuf::from("/code/a-x")),
        "beside the session on screen"
    );
    drive.add_session_in(1, Some(spec()), 3);
    assert_eq!(
        drive.folder_of(3),
        Some(PathBuf::from("/code/a")),
        "asked of the project: its own folder"
    );
}

#[test]
fn a_sweep_reads_every_folder_of_a_row() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    let (_, roots) = swept(drive.sweep(|_| true));
    let read: Vec<_> = roots.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(read, ["/a", "/code/a", "/code/a-x"]);
}

#[test]
fn new_session_in_a_worktree_starts_in_its_folder() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.select_project(0);
    let (added, fx) = drive.add_session_in_folder(
        1,
        PathBuf::from("/code/a-x"),
        Some(spec()),
        4,
        StartsOn::Resume("/conv/c1".into()),
    );
    assert_eq!(added, Ok(4));
    assert_eq!(drive.active_root, 1, "its project selected");
    assert_eq!(drive.folder_of(4), Some(PathBuf::from("/code/a-x")));
    assert!(names(&fx).contains(&"Show".to_string()));
    assert_eq!(drive.take_start(4), StartsOn::Resume("/conv/c1".into()));
}

#[test]
fn a_worktree_just_recorded_opens_on_its_row_at_once() {
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&at("/code/a")));
    let p = with_worktree("/code/a");
    drive.open_project("/code/a-x".into(), Some(&p));
    assert_eq!(drive.roots.len(), 2);
    assert_eq!(drive.local_index(Path::new("/code/a-x")), Some(1));
    let mut q = with_worktree("/code/a");
    q.worktrees[0].path = "/code/a-y".into();
    let (idx, _) = drive.keep_project("/code/a-y".into(), Some(&q));
    assert_eq!(idx, 1);
    assert_eq!(drive.local_index(Path::new("/code/a-y")), Some(1));
}

#[test]
fn a_session_in_a_folder_no_sweep_reads_asks_for_one() {
    let mut drive = bound();
    let (_, fx) = drive.add_session(Some(spec()), 1, None);
    assert!(
        !names(&fx).contains(&"Resweep".to_string()),
        "its row's own"
    );
    let (_, fx) = drive.add_session_at(Some(spec()), 2, PathBuf::from("/a-gone"));
    assert!(
        names(&fx).contains(&"Resweep".to_string()),
        "{:?}",
        names(&fx)
    );
}

#[test]
fn a_session_in_an_offloaded_worktree_watches_its_host() {
    use crate::host::wire::HostSession;
    let mut p = with_worktree("/code/a");
    p.worktrees[0].offload = Some(crate::workspace::Offloaded {
        host: HostName("vps".into()),
        mirror: "/m".into(),
        branch: None,
    });
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session_at(Some(spec()), 5, PathBuf::from("/code/a-x"));
    let id = HostSession("h-5".into());
    drive.watching(5, id.clone());
    assert_eq!(
        drive.watched_on(5),
        Some((HostName("vps".into()), id.clone()))
    );
    assert_eq!(drive.watcher(&HostName("vps".into()), &id, None), Some(5));
}

#[test]
fn opening_a_worktree_of_a_row_adds_no_second_row() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.select_project(0);
    let fx = drive.open_project("/code/a-x".into(), None);
    assert_eq!(drive.roots.len(), 2, "{:?}", names(&fx));
    assert_eq!(drive.active_root, 1);
}

#[test]
fn a_worker_on_a_project_not_in_the_window_brings_its_row_in_and_takes_it_out() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    let idx = drive.add_run_session(Path::new("/code/a-x"), spec(), 1, Some(&p));
    assert_eq!(idx, Some(1));
    assert_eq!(
        drive.roots[1].path,
        PathBuf::from("/code/a"),
        "the project's row"
    );
    assert!(drive.roots[1].transient);
    assert_eq!(drive.folder_of(1), Some(PathBuf::from("/code/a-x")));
    assert_eq!(drive.active_root, 0, "nothing on screen moves");
    assert_eq!(drive.to_config().projects.len(), 1, "never written");
    drive.end(1, None);
    assert_eq!(drive.roots.len(), 1, "it leaves with its last session");
}

#[test]
fn a_project_opened_while_only_a_worker_brought_it_is_kept() {
    let p = at("/code/a");
    let mut drive = bound();
    drive.add_run_session(Path::new("/code/a"), spec(), 1, Some(&p));
    drive.open_project("/code/a".into(), Some(&p));
    assert!(!drive.roots[1].transient, "the person's now");
    drive.end(1, None);
    assert_eq!(drive.roots.len(), 2);
}

#[test]
fn a_worker_on_a_worktree_just_recorded_joins_its_row() {
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&at("/code/a")));
    let p = with_worktree("/code/a");
    assert_eq!(
        drive.add_run_session(Path::new("/code/a-x"), spec(), 1, Some(&p)),
        Some(1)
    );
    assert_eq!(drive.roots.len(), 2, "no second row of the project");
    assert!(!drive.roots[1].transient);
}

#[test]
fn forgetting_a_missing_worktree_keeps_its_sessions() {
    use crate::project::{Projects, ProjectsDrive};
    let p = with_worktree("/code/a");
    let mut projects = ProjectsDrive::new(Projects::from_iter([p.clone()]), "/data".into());
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session_at(Some(spec()), 1, PathBuf::from("/code/a-x"));
    let fx = projects.forget_worktree(&p.id, Path::new("/code/a-x"));
    assert!(!fx.is_empty(), "written");
    let now = projects.get(&p.id).unwrap().clone();
    assert!(now.worktrees.is_empty());
    let fx = drive.told(&now);
    assert!(!names(&fx).contains(&"Close(1)".to_string()));
    assert!(drive.roots[1].worktrees.is_empty());
    assert_eq!(drive.folder_of(1), Some(PathBuf::from("/code/a-x")));
}

#[test]
fn a_worktree_the_record_drops_is_let_go_of_unless_a_session_runs_there() {
    let mut p = with_worktree("/code/a");
    p.worktrees.push(crate::project::Worktree {
        path: "/code/a-y".into(),
        branch: "y".into(),
        from: None,
        made_by: crate::project::MadeBy::Split,
        offload: None,
    });
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    drive.add_session_at(Some(spec()), 1, PathBuf::from("/code/a-y"));
    p.worktrees.clear();
    let fx = names(&drive.told(&p));
    assert!(fx.contains(&"ForgotHere(/code/a-x)".to_string()), "{fx:?}");
    assert!(fx.contains(&"Forgot(/code/a-x)".to_string()));
    assert!(
        !fx.iter().any(|n| n.contains("/code/a-y")),
        "a session runs there"
    );
}

#[test]
fn a_bare_worktree_row_becoming_its_projects_shows_and_reads_its_folders() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a-x".into(), None);
    let fx = names(&drive.placed(None, Path::new("/code/a-x"), &p));
    assert_eq!(drive.roots[1].path, PathBuf::from("/code/a"));
    assert!(fx.contains(&"Show".to_string()), "{fx:?}");
    assert!(fx.contains(&"Resweep".to_string()), "{fx:?}");
}

#[test]
fn a_transient_row_is_never_a_second_row_of_its_project() {
    let p = with_worktree("/code/a");
    let mut ws = Workspace::seeded("/elsewhere");
    let mut row = ProjectRoot::of(&p);
    row.worktrees.clear();
    ws.roots.push(row);
    assert_eq!(
        ws.add_transient_root("/code/a-x", Some(&p), spec(), 1),
        None
    );
}

#[test]
fn a_rows_worktrees_on_disk_are_those_git_was_read_in() {
    let p = with_worktree("/code/a");
    let mut drive = bound();
    drive.open_project("/code/a".into(), Some(&p));
    assert!(
        drive.worktrees_on_disk(&drive.roots[1]).is_empty(),
        "not read yet"
    );
    let (token, _) = swept(drive.sweep(|_| true));
    let status = crate::gitstat::GitStatus {
        branch: "x".into(),
        changed: 0,
        ahead: 0,
        behind: 0,
        entries: Default::default(),
    };
    drive.handle(Input::Swept {
        token,
        found: vec![("/code/a-x".into(), status)],
    });
    let on: Vec<_> = drive
        .worktrees_on_disk(&drive.roots[1])
        .iter()
        .map(|w| w.branch.clone())
        .collect();
    assert_eq!(on, ["x"]);
}

#[test]
fn showing_a_project_page_selects_the_root_and_no_session() {
    let mut drive = bound();
    drive.open_project("/code/a".into(), None);
    drive.open_project("/code/b".into(), None);
    tree(&mut drive).add_session_at(spec(), 7, PathBuf::from("/code/b"));
    assert_eq!(drive.active_uid(), Some(7));
    let (b, _) = drive.locate(7).unwrap();
    let a = drive.local_index(Path::new("/code/a")).unwrap();
    let fx = drive.show_project_page(a);
    assert!(matches!(fx.as_slice(), [Effect::Show, Effect::Changed]));
    assert_eq!(drive.active_root, a);
    let fx = drive.show_project_page(b);
    assert!(matches!(fx.as_slice(), [Effect::Show, Effect::Changed]));
    assert_eq!(drive.active_root, b);
    assert_eq!(drive.active_uid(), None);
    assert_eq!(drive.roots[b].sessions.len(), 1, "the session stays open");
}

/// A session added to a row by its place, a host's row included, selects
/// nothing and leaves the page on screen as it was.
#[test]
fn a_session_added_to_a_row_unshown_moves_nothing() {
    let mut drive = bound();
    drive.open_host_project(&at_on("box", "/srv/app"));
    let host = drive
        .roots
        .iter()
        .position(|row| row.host.is_some())
        .expect("the host's row");
    drive.show_project_page(host);
    let added = drive.add_session_unshown(host, spec(), 41);
    assert_eq!(added, Some(host));
    assert_eq!(drive.active_root, host);
    assert_eq!(drive.active_uid(), None, "its page stays");
    assert_eq!(drive.locate(41), Some((host, 0)));
}
