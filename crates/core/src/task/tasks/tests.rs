use super::*;
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
    let (made, _) = tasks.retry("1", "2".into(), template, None, None);
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
    assert!(!tasks.retry("1", "2".into(), template, None, None).0);
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

#[test]
fn a_check_is_retried_at_once_from_its_one_step_and_others_are_not() {
    let mut tasks = TasksDrive::<Win>::default();
    tasks.add(check("1", "/a"));
    tasks.add(titled("2", "Mine", "/b"));
    let (made, fx) = tasks.retry_at_once("1", "1b".into()).unwrap();
    assert!(made);
    assert_eq!(saves(&fx), 1);
    assert_eq!(tasks.task("1").unwrap().runs.len(), 2);
    assert!(tasks.retry_at_once("2", "2b".into()).is_none());
}
