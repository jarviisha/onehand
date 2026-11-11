use super::*;
use crate::task::work::Act;
use crate::task::{queue, Group, Origin, Source};
use crate::unattended::IssueSource;
use crate::workflow::{Brief, Failure, Outcome, Setup, Stop};
use std::path::Path;

/// A window, as a front end names it.
type Win = u8;

const WINDOW: Win = 1;

fn setup(dir: &str) -> Setup {
    Setup {
        repo: PathBuf::from(dir),
        dir: PathBuf::from(dir),
        branch: None,
        agent: None,
        check: None,
        mode: None,
        forge: None,
    }
}

fn check(id: &str, dir: &str) -> Task {
    Task::check(id.into(), "true".into(), setup(dir))
}

fn titled(id: &str, title: &str, dir: &str) -> Task {
    let brief = Brief {
        title: title.into(),
        body: String::new(),
        instructions: None,
    };
    Task::new(
        id.into(),
        crate::workflow::builtin::all().remove(0),
        brief,
        setup(dir),
    )
}

fn has<W>(fx: &[Effect<W>], wanted: impl Fn(&Effect<W>) -> bool) -> bool {
    fx.iter().any(wanted)
}

fn saves<W>(fx: &[Effect<W>]) -> usize {
    fx.iter().filter(|e| matches!(e, Effect::Save(_))).count()
}

/// Ask for task `id`'s place, and find it at `place`, from an open window.
fn ask(tasks: &mut TasksDrive<Win>, id: &str, place: &str) -> Vec<Effect<Win>> {
    let fx = tasks.request(id, WINDOW, None);
    assert!(
        has(
            &fx,
            |e| matches!(e, Effect::FindPlace { task, .. } if task == id)
        ),
        "{fx:?}"
    );
    tasks.handle(Input::PlaceFound {
        task: id.into(),
        place: PathBuf::from(place),
        window: WINDOW,
        window_open: true,
    })
}

fn drives<W>(fx: &[Effect<W>], id: &str) -> bool {
    has(
        fx,
        |e| matches!(e, Effect::Drive { task, .. } if task == id),
    )
}

/// Task `id` ran and ended done, its session's turn over.
fn ended_done(tasks: &mut TasksDrive<Win>, id: &str) -> Vec<Effect<Win>> {
    let mut run = tasks.task(id).unwrap().runs[0].clone();
    run.outcome = Some(Outcome::Done);
    let fx = tasks.run_ended(id, run, usize::MAX, false, None, None, WINDOW);
    assert!(
        has(&fx, |e| matches!(e, Effect::Defer(Input::Free { .. }))),
        "{fx:?}"
    );
    tasks.handle(Input::Free {
        task: id.into(),
        from: usize::MAX,
    })
}

#[test]
fn a_free_place_starts_the_task_at_once() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    let fx = ask(&mut tasks, "1", "/a");
    assert!(drives(&fx, "1"), "{fx:?}");
    assert!(has(&fx, |e| *e == Effect::Asked("1".into())), "{fx:?}");
    assert_eq!(tasks.working("1"), Some(Working::Running));
}

#[test]
fn a_taken_place_queues_the_task_and_says_behind_whom() {
    let mut tasks = TasksDrive::default();
    tasks.add(titled("1", "First", "/a"));
    tasks.add(titled("2", "Second", "/a"));
    ask(&mut tasks, "1", "/a");
    let fx = ask(&mut tasks, "2", "/a");
    assert!(!drives(&fx, "2"), "{fx:?}");
    let said = fx.iter().find_map(|e| match e {
        Effect::Told {
            placed: Placed::Queued(said),
            ..
        } => Some(said.clone()),
        _ => None,
    });
    assert_eq!(said, Some(queue::queued_said("Second", Some("First"), "a")));
    assert_eq!(tasks.working("2"), Some(Working::Queued));
    // The first ends: the second starts in the window it was asked from.
    let fx = ended_done(&mut tasks, "1");
    assert!(
        has(&fx, |e| *e
            == Effect::Drive {
                task: "2".into(),
                window: WINDOW
            }),
        "{fx:?}"
    );
    assert_eq!(tasks.working("1"), None);
    assert_eq!(tasks.working("2"), Some(Working::Running));
}

#[test]
fn a_task_already_at_work_is_refused_a_second_start() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    ask(&mut tasks, "1", "/a");
    let fx = ask(&mut tasks, "1", "/a");
    assert!(!drives(&fx, "1"));
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Told {
            placed: Placed::Refused(_),
            ..
        }
    )));
}

#[test]
fn a_place_found_after_its_window_closed_asks_for_nothing() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    tasks.request("1", WINDOW, None);
    let fx = tasks.handle(Input::PlaceFound {
        task: "1".into(),
        place: PathBuf::from("/a"),
        window: WINDOW,
        window_open: false,
    });
    assert!(!drives(&fx, "1"));
    assert_eq!(tasks.working("1"), None);
}

/// A retry the cap refuses never began: its new run is dropped, and the
/// task is as it was.
#[test]
fn a_start_over_the_cap_is_refused_and_an_unbegun_retry_dropped() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    let template = tasks.task("1").unwrap().runs[0].template.clone();
    let (made, _) = tasks.retry("1", "2".into(), template, None, None, WINDOW);
    assert!(made);
    assert_eq!(tasks.task("1").unwrap().runs.len(), 2);
    let fx = tasks.request("1", WINDOW, Some("two runs already work".into()));
    assert!(!has(&fx, |e| matches!(e, Effect::FindPlace { .. })));
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::Refused { why, .. }) if why == "two runs already work"
    )));
    assert_eq!(tasks.task("1").unwrap().runs.len(), 1);
}

/// A queued task whose window has gone is passed over, reads cut off, and
/// the place passes on to the next.
#[test]
fn a_queued_task_that_cannot_start_where_it_was_asked_is_passed_over() {
    let mut tasks = TasksDrive::default();
    for id in ["1", "2", "3"] {
        tasks.add(check(id, "/a"));
        ask(&mut tasks, id, "/a");
    }
    let fx = ended_done(&mut tasks, "1");
    assert!(drives(&fx, "2"), "{fx:?}");
    let fx = tasks.handle(Input::NotDriven("2".into()));
    assert!(drives(&fx, "3"), "{fx:?}");
    assert_eq!(tasks.working("2"), None);
    assert_eq!(tasks.task("2").unwrap().group(None), Group::Ended);
}

#[test]
fn stop_on_a_queued_task_that_never_ran_keeps_no_empty_run() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    tasks.add(check("2", "/a"));
    ask(&mut tasks, "1", "/a");
    ask(&mut tasks, "2", "/a");
    let (queued, fx) = tasks.stop_queued("2");
    assert!(queued);
    assert_eq!(saves(&fx), 1);
    let task = tasks.task("2").unwrap();
    assert!(task.runs.is_empty());
    assert_eq!(task.outcome(), Some(Outcome::Stopped(Stop::ByPerson)));
    assert_eq!(tasks.working("1"), Some(Working::Running));
    // Not queued: nothing to call off.
    assert!(!tasks.stop_queued("1").0);
}

#[test]
fn a_retry_is_refused_while_the_task_is_at_work() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    ask(&mut tasks, "1", "/a");
    let template = tasks.task("1").unwrap().runs[0].template.clone();
    assert!(!tasks.retry("1", "2".into(), template, None, None, WINDOW).0);
}

/// A run that ends while its session's turn still goes holds its place until
/// the turn is over, at most for the drain limit, then pins its last marks
/// and lets the place go.
#[test]
fn an_ended_run_holds_its_place_until_its_turn_is_over() {
    let mut tasks = TasksDrive::default();
    tasks.add(titled("1", "One", "/a"));
    tasks.add(titled("2", "Two", "/a"));
    ask(&mut tasks, "1", "/a");
    ask(&mut tasks, "2", "/a");
    let mut run = tasks.task("1").unwrap().runs[0].clone();
    run.resume();
    run.stopped(Stop::ByPerson);
    let fx = tasks.run_ended("1", run, 0, true, None, Some(7), WINDOW);
    assert!(
        has(&fx, |e| *e
            == Effect::WatchDrain {
                task: "1".into(),
                limit: DRAIN_LIMIT
            }),
        "{fx:?}"
    );
    assert_eq!(saves(&fx), 1);
    assert_eq!(tasks.working("1"), Some(Working::Running));
    let fx = tasks.handle(Input::Drained("1".into()));
    let pin = fx.iter().find_map(|e| match e {
        Effect::Pin { task, from, .. } => Some((task.clone(), *from)),
        _ => None,
    });
    assert_eq!(pin, Some(("1".to_string(), 0)), "{fx:?}");
    assert!(!drives(&fx, "2"), "the place waits for the pin");
    // A second word of the turn's end finds nothing left to drain.
    assert!(tasks.handle(Input::Drained("1".into())).is_empty());
    let fx = tasks.handle(Input::Pinned {
        task: "1".into(),
        from: 0,
        pinned: Ok("c1".into()),
    });
    assert_eq!(saves(&fx), 1);
    assert!(drives(&fx, "2"), "{fx:?}");
}

#[test]
fn a_last_mark_not_pinned_is_told_and_the_place_still_goes() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    ask(&mut tasks, "1", "/a");
    let fx = tasks.handle(Input::Pinned {
        task: "1".into(),
        from: 0,
        pinned: Err("no git".into()),
    });
    assert!(has(&fx, |e| *e
        == Effect::Signal(Signal::PinFailed("no git".into()))));
    assert_eq!(tasks.working("1"), None);
}

#[test]
fn a_check_says_how_it_came_out_and_a_stopped_one_says_nothing() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/work/app"));
    let mut run = tasks.task("1").unwrap().runs[0].clone();
    run.outcome = Some(Outcome::Done);
    let fx = tasks.run_ended("1", run.clone(), 0, false, None, None, WINDOW);
    assert!(has(&fx, |e| *e
        == Effect::Signal(Signal::CheckEnded {
            window: WINDOW,
            project: "app".into(),
            passed: true
        })));
    run.outcome = Some(Outcome::Stopped(Stop::ByPerson));
    let fx = tasks.run_ended("1", run, 0, false, None, None, WINDOW);
    assert!(!has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::CheckEnded { .. })
    )));
}

fn scheduled(id: &str) -> Task {
    Task {
        origin: Some(Origin::Schedule {
            id: "s".into(),
            name: "Nightly".into(),
        }),
        ..titled(id, "Review", "/a")
    }
}

/// A schedule's task that is done closes its session, and says on the
/// desktop how it ended once its place is let go.
#[test]
fn a_schedules_finished_task_closes_its_session_and_says_so() {
    let mut tasks = TasksDrive::default();
    tasks.add(scheduled("1"));
    ask(&mut tasks, "1", "/a");
    let mut run = tasks.task("1").unwrap().runs[0].clone();
    run.outcome = Some(Outcome::Done);
    let fx = tasks.run_ended("1", run, usize::MAX, false, None, Some(7), WINDOW);
    assert!(has(&fx, |e| *e
        == Effect::CloseSession {
            uid: 7,
            window: WINDOW
        }));
    let fx = tasks.handle(Input::Free {
        task: "1".into(),
        from: usize::MAX,
    });
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::ScheduleEnded { name, .. }) if name == "Nightly"
    )));
}

fn of_issue(id: &str) -> Task {
    Task {
        source: Source::Issue(IssueSource {
            tracker: crate::unattended::TrackerRef::Local {
                file: PathBuf::from("/a/issues.md"),
            },
            number: 3,
            forge_ref: None,
            forge: None,
            base: String::new(),
            picked: false,
            unsent: Vec::new(),
            notes: Vec::new(),
        }),
        ..titled(id, "Fix", "/a")
    }
}

/// An issue's run that ends keeps its report, the start's notes and a
/// waiting card's question in it, in the very write that keeps the run, so
/// no later write of the task can lose it before it is sent.
#[test]
fn an_issue_runs_end_is_kept_in_the_write_that_keeps_the_run() {
    let mut tasks = TasksDrive::default();
    let mut task = of_issue("1");
    if let Source::Issue(issue) = &mut task.source {
        issue.notes = vec!["No acceptance written".into()];
    }
    tasks.add(task);
    ask(&mut tasks, "1", "/a");
    let mut run = tasks.task("1").unwrap().runs[0].clone();
    run.outcome = Some(Outcome::Done);
    let fx = tasks.run_ended(
        "1",
        run,
        usize::MAX,
        false,
        Some("Which?".into()),
        None,
        WINDOW,
    );
    let saves: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Save(task) => task.issue().cloned(),
            _ => None,
        })
        .collect();
    let [issue] = saves.as_slice() else {
        panic!("one write: {fx:?}");
    };
    let [report] = issue.unsent.as_slice() else {
        panic!("one report: {issue:?}");
    };
    assert_eq!(report.outcome, Some(Outcome::Done));
    assert_eq!(report.asked.as_deref(), Some("Which?"));
    assert_eq!(report.notes, vec!["No acceptance written".to_string()]);
    assert!(issue.notes.is_empty(), "handed to the first report only");
}

/// A run that cannot start ends failed when nobody is watching it, an
/// issue's or a schedule's, and is told; a person's only gives its place on.
#[test]
fn a_task_that_cannot_start_fails_only_when_nobody_watches_it() {
    let mut tasks = TasksDrive::default();
    tasks.add(of_issue("1"));
    tasks.add(titled("2", "Mine", "/b"));
    ask(&mut tasks, "1", "/a");
    ask(&mut tasks, "2", "/b");
    let fx = tasks.not_started("1", "no agent".into(), Failure::Other);
    // Its report is in the task that is written, before the issue is told.
    let saved = fx.iter().position(|e| {
        matches!(e, Effect::Save(task) if task.id == "1"
            && task.issue().is_some_and(|issue| issue.unsent.len() == 1))
    });
    let told = fx.iter().position(|e| *e == Effect::IssueEnded("1".into()));
    assert!(saved.is_some() && saved < told, "{fx:?}");
    assert!(matches!(
        tasks.task("1").unwrap().outcome(),
        Some(Outcome::Failed(_))
    ));
    tasks.not_started("2", "no agent".into(), Failure::Other);
    assert_eq!(tasks.task("2").unwrap().outcome(), None);
    assert_eq!(tasks.working("2"), None);
}

#[test]
fn a_dismiss_is_kept_once_written_and_put_back_when_not() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    tasks.add(check("2", "/a"));
    let fx = tasks.dismiss("1", WINDOW);
    assert!(has(
        &fx,
        |e| matches!(e, Effect::SaveDismissed(task) if task.dismissed)
    ));
    tasks.handle(Input::Dismissed {
        task: "1".into(),
        written: Ok(()),
    });
    assert!(tasks.task("1").unwrap().dismissed);
    tasks.dismiss("2", WINDOW);
    let fx = tasks.handle(Input::Dismissed {
        task: "2".into(),
        written: Err("disk full".into()),
    });
    assert!(!tasks.task("2").unwrap().dismissed);
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::DismissFailed { why, .. }) if why == "disk full"
    )));
}

#[test]
fn a_task_at_work_is_not_dismissed() {
    let mut tasks = TasksDrive::default();
    tasks.add(check("1", "/a"));
    ask(&mut tasks, "1", "/a");
    assert!(tasks.dismiss("1", WINDOW).is_empty());
    assert!(!tasks.task("1").unwrap().dismissed);
}

#[test]
fn a_failed_write_is_said_once_until_one_lands() {
    let mut tasks = TasksDrive::<Win>::default();
    let failed =
        |fx: &[Effect<Win>]| has(fx, |e| matches!(e, Effect::Signal(Signal::SaveFailed(_))));
    assert!(failed(&tasks.handle(Input::Saved(Err("full".into())))));
    assert!(!failed(&tasks.handle(Input::Saved(Err("full".into())))));
    tasks.handle(Input::Saved(Ok(())));
    assert!(failed(&tasks.handle(Input::Saved(Err("full".into())))));
}

/// The history keeps the newest finished tasks of each project; one past
/// them has its file removed and its marks dropped when a place is let go.
#[test]
fn a_finished_task_past_the_history_is_let_go() {
    let mut tasks = TasksDrive::<Win>::default();
    let mut read = Vec::new();
    for n in 0..=history::KEPT {
        let mut task = check(&format!("{}", 1000 + n), "/a");
        task.runs[0].outcome = Some(Outcome::Done);
        read.push(task);
    }
    let fx = tasks.loaded(read);
    assert_eq!(
        fx.iter().filter(|e| matches!(e, Effect::Remove(_))).count(),
        1
    );
    assert!(has(
        &fx,
        |e| matches!(e, Effect::DropMarks { id, .. } if id == "1000")
    ));
    assert!(has(&fx, |e| *e == Effect::DeliverAll));
    assert_eq!(tasks.tasks().len(), history::KEPT);
    assert_eq!(tasks.removed(Path::new("/a")), 1);
}

#[test]
fn tasks_read_at_boot_come_before_those_added_meanwhile() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(check("2", "/a"));
    tasks.loaded(vec![check("1", "/a"), check("2", "/a")]);
    let ids: Vec<&str> = tasks.tasks().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["1", "2"]);
}

#[test]
fn only_a_first_drive_is_off_screen() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.drive_off_screen("1");
    assert!(tasks.take_off_screen("1"));
    assert!(!tasks.take_off_screen("1"));
}

#[test]
fn a_task_begins_as_its_kind_and_a_tools_first_drive_is_off_screen() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(check("1", "/a"));
    tasks.add(titled("2", "Mine", "/b"));
    tasks.add(titled("3", "A tool's", "/c"));
    tasks.add(of_issue("4"));
    tasks.drive_off_screen("3");
    assert!(matches!(tasks.begin("1", true), Begin::Check(_)));
    let off = |begun| match begun {
        Begin::Session { off_screen, .. } => off_screen,
        other => panic!("{other:?}"),
    };
    assert!(!off(tasks.begin("2", true)));
    assert!(off(tasks.begin("3", true)));
    // Only its first drive: a Resume a person presses later shows.
    assert!(!off(tasks.begin("3", true)));
    assert!(off(tasks.begin("4", true)));
}

#[test]
fn a_task_whose_folder_is_gone_or_with_nothing_to_run_is_refused() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(titled("1", "Mine", "/b"));
    assert!(matches!(
        tasks.begin("1", false),
        Begin::Refused { why, .. } if why.starts_with("The folder that task works in is gone")
    ));
    assert!(matches!(tasks.begin("9", true), Begin::Refused { .. }));
}

/// A session that did not start because its agent is no longer configured
/// is what the preflight would have blocked, found late.
#[test]
fn a_session_not_started_for_a_gone_agent_fails_as_found_late() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(of_issue("1"));
    ask(&mut tasks, "1", "/a");
    tasks.session_not_started("1", true);
    let run = tasks.task("1").unwrap().runs.last().unwrap().clone();
    assert_eq!(
        run.failure,
        Some(crate::preflight::found_late(crate::preflight::About::Agent))
    );
}

/// A check is retried at once and asks for its place; anything else is
/// asked about first.
#[test]
fn a_check_is_retried_at_once_from_its_one_step_and_others_are_not() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(check("1", "/a"));
    tasks.add(titled("2", "Mine", "/b"));
    let (retry, fx) = tasks.retry_asked("1", "1b".into(), WINDOW);
    assert_eq!(retry, Retry::AtOnce);
    assert_eq!(saves(&fx), 1);
    assert!(has(
        &fx,
        |e| matches!(e, Effect::Ask { task, .. } if task == "1")
    ));
    assert_eq!(tasks.task("1").unwrap().runs.len(), 2);
    let (retry, fx) = tasks.retry_asked("2", "2b".into(), WINDOW);
    assert_eq!(retry, Retry::Confirm);
    assert!(fx.is_empty());
}

/// A Resume its preflight refuses is said where it was pressed; one it
/// lets through asks for its place.
#[test]
fn a_resume_refused_is_said_and_one_let_through_asks() {
    let mut tasks = TasksDrive::<Win>::default();
    let fx = tasks.resume("1", Some("no agent".into()), WINDOW);
    assert!(matches!(
        fx.as_slice(),
        [Effect::Signal(Signal::NotResumed { why, .. })] if why == "no agent"
    ));
    let fx = tasks.resume("1", None, WINDOW);
    assert!(matches!(fx.as_slice(), [Effect::Ask { task, .. }] if task == "1"));
}

fn starting(place: crate::workflow::Place) -> crate::task::Starting {
    let template = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.place == place)
        .expect("a workflow working there");
    crate::task::Starting {
        project: PathBuf::from("/p"),
        template,
        brief: Brief {
            title: "Tidy".into(),
            body: String::new(),
            instructions: None,
        },
        origin: Origin::Tool,
        grant: Default::default(),
        agent: None,
        mode: None,
    }
}

fn launch(place: crate::workflow::Place) -> Launch {
    Launch::off_screen(&[], starting(place), None, Some("a".into())).unwrap()
}

fn told<W>(fx: &[Effect<W>]) -> Option<&Placed> {
    let Some(Effect::Told { placed, .. }) = fx.iter().find(|e| matches!(e, Effect::Told { .. }))
    else {
        return None;
    };
    Some(placed)
}

/// A start refused by what blocks it is told so under its id, and keeps
/// nothing.
#[test]
fn a_start_off_screen_refused_is_told_and_keeps_nothing() {
    let mut tasks = TasksDrive::<Win>::default();
    let fx = tasks.start_off_screen("t".into(), Err("no agent".into()), WINDOW);
    assert_eq!(told(&fx), Some(&Placed::Refused("no agent".into())));
    assert!(tasks.tasks().is_empty());
}

/// On the checkout, its project is kept first; with the window still open
/// it is kept as a task with its origin, asks for its place, and its first
/// drive is off screen.
#[test]
fn a_start_off_screen_keeps_its_project_then_its_task_and_asks() {
    let mut tasks = TasksDrive::<Win>::default();
    let fx = tasks.start_off_screen(
        "t".into(),
        Ok(launch(crate::workflow::Place::Checkout)),
        WINDOW,
    );
    let Some(Effect::Keep {
        task,
        launch,
        window,
    }) = fx.into_iter().next()
    else {
        panic!("the project kept first");
    };
    assert_eq!(launch.dir(), Path::new("/p"));
    assert!(tasks.tasks().is_empty(), "nothing kept before its project");
    let mut workspace =
        crate::workspace::WorkspaceDrive::new(crate::workspace::Workspace::seeded("/other"));
    let (fx, _) = link::keep(&mut tasks, Some(&mut workspace), task, *launch, window);
    assert!(
        workspace
            .roots
            .iter()
            .any(|root| root.path == Path::new("/p")),
        "its project kept in the window's workspace"
    );
    assert_eq!(workspace.active_root, 0, "and not put on screen");
    assert_eq!(saves(&fx), 1);
    assert!(has(
        &fx,
        |e| matches!(e, Effect::Ask { task, .. } if task == "t")
    ));
    assert_eq!(tasks.task("t").unwrap().origin, Some(Origin::Tool));
    let fx = ask(&mut tasks, "t", "/p");
    assert!(drives(&fx, "t"));
    match tasks.begin("t", true) {
        Begin::Session { off_screen, .. } => assert!(off_screen),
        Begin::Check(_) | Begin::Refused { .. } => panic!("a session's run"),
    }
}

/// A window gone before its project is kept refuses the start, keeping no
/// task.
#[test]
fn a_start_off_screen_whose_window_went_is_refused() {
    let mut tasks = TasksDrive::<Win>::default();
    let (fx, _) = link::keep(
        &mut tasks,
        None,
        "t".into(),
        launch(crate::workflow::Place::Checkout),
        WINDOW,
    );
    assert_eq!(
        told(&fx),
        Some(&Placed::Refused("the window closed".into()))
    );
    assert!(tasks.tasks().is_empty());
}

/// On a worktree, the cut goes first: a failed cut is told, one that lands
/// keeps the project on the worktree.
#[test]
fn a_start_off_screen_on_a_worktree_is_cut_first() {
    let mut tasks = TasksDrive::<Win>::default();
    let fx = tasks.start_off_screen(
        "t".into(),
        Ok(launch(crate::workflow::Place::Worktree)),
        WINDOW,
    );
    let Some(Effect::Cut { launch, .. }) = fx.into_iter().next() else {
        panic!("cut first");
    };
    let cut = |made: Cut| Input::Cut {
        task: "t".into(),
        launch: launch.clone(),
        window: WINDOW,
        made,
    };
    let fx = tasks.handle(cut(Err("not a repository".into())));
    assert_eq!(told(&fx), Some(&Placed::Refused("not a repository".into())));
    let fx = tasks.handle(cut(Ok(crate::task::Worktree {
        dir: "/w".into(),
        branch: "onehand/tidy".into(),
        forge: None,
    })));
    assert!(
        matches!(fx.as_slice(), [Effect::Keep { launch, .. }] if launch.dir() == Path::new("/w")),
        "{fx:?}"
    );
}

/// A session's run begins with what its task was given and, an issue's,
/// with the lines its transcript opens with.
#[test]
fn a_sessions_run_begins_with_its_grant_and_an_issues_opening() {
    let mut tasks = TasksDrive::<Win>::default();
    let mut mine = titled("1", "Mine", "/b");
    mine.grant.tools = vec!["start_task".into()];
    tasks.add(mine);
    tasks.add(of_issue("2"));
    let begun = |tasks: &mut TasksDrive<Win>, id| match tasks.begin(id, true) {
        Begin::Session { grant, opening, .. } => (grant, opening),
        Begin::Check(_) | Begin::Refused { .. } => panic!("a session's run"),
    };
    let (grant, opening) = begun(&mut tasks, "1");
    assert_eq!(grant.tools, ["start_task"]);
    assert!(opening.is_empty());
    let (_, opening) = begun(&mut tasks, "2");
    assert!(
        opening[0].starts_with("Unattended run on issue"),
        "{opening:?}"
    );
}

/// Where a driven task landed is told to whoever asked: begun, refused
/// for its reason, or its window gone, which also lets its place go.
#[test]
fn a_drive_tells_where_its_task_landed() {
    let mut tasks = TasksDrive::<Win>::default();
    let driven = |tasks: &mut TasksDrive<Win>, started| {
        let fx = tasks.handle(Input::Driven {
            task: "1".into(),
            started,
        });
        told(&fx).cloned()
    };
    assert_eq!(driven(&mut tasks, Ok(())), Some(Placed::Started));
    assert_eq!(
        driven(&mut tasks, Err("no".into())),
        Some(Placed::Refused("no".into()))
    );
    tasks.add(titled("2", "Mine", "/b"));
    let fx = ask(&mut tasks, "2", "/b");
    assert!(drives(&fx, "2"));
    let fx = tasks.handle(Input::NotDriven("2".into()));
    assert_eq!(
        told(&fx),
        Some(&Placed::Refused("the window closed".into()))
    );
    assert!(!tasks.busy("2"), "its place let go");
}

fn facts(folder: &str) -> Box<crate::worktree::removal::Facts> {
    Box::new(crate::worktree::removal::Facts {
        folder: PathBuf::from(folder),
        branch: Some("b".into()),
        expected: Some("b".into()),
        uncommitted: Ok(Vec::new()),
        merged: Ok(None),
        past_merge: Ok(0),
        users: Vec::new(),
    })
}

/// *Remove worktree…* is judged and asked first; once confirmed it is
/// judged again, removed, and kept with the task, its button waiting
/// meanwhile; a refusal as things stand is said instead.
#[test]
fn a_worktree_is_judged_asked_judged_again_then_removed() {
    use crate::worktree::removal::{Judged, Removed};
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(titled("1", "Mine", "/w"));
    let judged = |tasks: &mut TasksDrive<Win>, confirmed, judged| {
        tasks.handle(Input::Judged {
            task: "1".into(),
            confirmed,
            window: WINDOW,
            repo: PathBuf::from("/r"),
            judged,
        })
    };
    let fx = tasks.remove_worktree("1", false, WINDOW);
    assert!(has(&fx, |e| matches!(
        e,
        Effect::JudgeRemoval {
            confirmed: false,
            ..
        }
    )));
    assert_eq!(tasks.acting("1"), Some(Act::RemoveWorktree));
    let remove = Judged::Remove {
        branch: Some("b".into()),
        why: "merged".into(),
    };
    let fx = judged(&mut tasks, false, Some((facts("/w"), remove.clone())));
    assert!(has(&fx, |e| matches!(e, Effect::ConfirmRemoval { .. })));
    assert_eq!(tasks.acting("1"), None);
    // The window went before what uses the folder was read: nothing more.
    let _ = tasks.remove_worktree("1", true, WINDOW);
    let fx = judged(&mut tasks, true, None);
    assert!(!has(&fx, |e| matches!(e, Effect::RemoveWorktree { .. })));
    assert_eq!(tasks.acting("1"), None);
    let fx = judged(
        &mut tasks,
        true,
        Some((facts("/w"), Judged::Refused(vec!["no".into()]))),
    );
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::NotRemoved { .. })
    )));
    let fx = judged(&mut tasks, true, Some((facts("/w"), remove)));
    assert!(has(&fx, |e| matches!(
        e,
        Effect::RemoveWorktree { folder, branch: Some(b), .. } if folder == Path::new("/w") && b == "b"
    )));
    assert_eq!(tasks.acting("1"), Some(Act::RemoveWorktree));
    let fx = tasks.handle(Input::WorktreeRemoved {
        task: "1".into(),
        window: WINDOW,
        folder: PathBuf::from("/w"),
        branch: Some("b".into()),
        removed: Ok(Removed { branch_kept: None }),
    });
    assert_eq!(saves(&fx), 1);
    assert!(tasks.task("1").unwrap().worktree_removed);
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::Removed { .. })
    )));
    assert_eq!(tasks.acting("1"), None);
}

/// *Answer the pull request review* reads first, its button waiting; what
/// was read is asked of the person, and a read that failed is said.
#[test]
fn a_review_is_read_before_it_is_asked() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(titled("1", "Mine", "/w"));
    let fx = tasks.answer_review("1", WINDOW);
    assert!(has(&fx, |e| matches!(e, Effect::ReadReview { .. })));
    assert_eq!(tasks.acting("1"), Some(Act::AnswerReview));
    let read = |tasks: &mut TasksDrive<Win>, read| {
        tasks.handle(Input::ReviewRead {
            task: "1".into(),
            window: WINDOW,
            read,
        })
    };
    let fx = read(&mut tasks, Ok(()));
    assert!(has(&fx, |e| matches!(e, Effect::ConfirmAnswer { .. })));
    assert_eq!(tasks.acting("1"), None);
    let fx = read(&mut tasks, Err("gone".into()));
    assert!(has(&fx, |e| matches!(
        e,
        Effect::Signal(Signal::ReviewNotRead { .. })
    )));
}
