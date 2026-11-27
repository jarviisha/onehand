use super::super::*;
use crate::issues::Issues;
use crate::project::ProjectId;
use crate::task::rooms::Room;
use crate::workflow::{Outcome, Place, Stop, Template};

fn now() -> Now {
    Now {
        secs: 100,
        at: std::time::Instant::now(),
    }
}

fn id() -> ProjectId {
    ProjectId::mint(1)
}

fn flow(place: Place) -> Template {
    let mut template = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    template.place = place;
    template
}

/// A drive whose project's board, a team of `team`, holds `todo`, written.
fn drive(team: u8, todo: &[u64]) -> BoardDrive {
    let mut d = BoardDrive::new(HashMap::new(), HashMap::new(), "/data/projects".into());
    let fx = d.set_board(
        &id(),
        Some(Board::made("builtin:branch".into(), team, todo, 1)),
    );
    land(&mut d, fx);
    d
}

/// Land every write among `fx`, and answer the rest, what each landing
/// answered included.
fn land(d: &mut BoardDrive, fx: Vec<Effect>) -> Vec<Effect> {
    answer(d, fx, Ok(()))
}

/// Answer every write among `fx` with `result`, and give back the rest.
fn answer(d: &mut BoardDrive, fx: Vec<Effect>, result: Result<(), String>) -> Vec<Effect> {
    let mut rest = Vec::new();
    for e in fx {
        match e {
            Effect::Write(w) => {
                let more = d.handle(
                    Input::Written {
                        id: w.id,
                        pending: w.pending,
                        seq: w.seq,
                        result: result.clone(),
                    },
                    now(),
                );
                rest.extend(answer(d, more, result.clone()));
            }
            other @ (Effect::Arm { .. }
            | Effect::Changed(_)
            | Effect::Signal(_)
            | Effect::ReadIssues { .. }
            | Effect::GivePlace { .. }
            | Effect::Start { .. }
            | Effect::NextRun { .. }
            | Effect::CloseIssue { .. }
            | Effect::Held { .. }) => rest.push(other),
        }
    }
    rest
}

/// Open issues `open` and closed ones `closed`, of the project's own
/// tracker.
fn issues(open: &[u64], closed: &[u64]) -> Issues {
    let one = |n: &u64, open: bool| format!(r#"{{"number":{n},"title":"t{n}","open":{open}}}"#);
    let all: Vec<String> = (open.iter().map(|n| one(n, true)))
        .chain(closed.iter().map(|n| one(n, false)))
        .collect();
    serde_json::from_str(&format!(r#"{{"next":99,"issues":[{}]}}"#, all.join(","))).unwrap()
}

fn facts(running: usize) -> Facts {
    Facts {
        templates: [("builtin:branch".to_string(), Ok(flow(Place::Worktree)))]
            .into_iter()
            .collect(),
        running,
        extends_waiting: Vec::new(),
        first: HashMap::new(),
        window: true,
        ids: (0..8).map(|n| format!("id{n}")).collect(),
    }
}

fn read(d: &mut BoardDrive, issues: Issues, facts: Facts) -> Vec<Effect> {
    d.handle(
        Input::IssuesRead {
            id: id(),
            read: Ok((issues, None)),
            facts: Box::new(facts),
        },
        now(),
    )
}

/// What `fx` says, in order, as words a test can compare.
fn said(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|e| match e {
            Effect::Write(w) => format!(
                "write {:?}",
                w.board.as_ref().map(Board::todo).unwrap_or_default()
            ),
            Effect::Arm { .. } => "arm".into(),
            Effect::Changed(_) => "changed".into(),
            Effect::Signal(Signal::SaveFailed { .. }) => "save failed".into(),
            Effect::Signal(Signal::NotStarted { issue, why, .. }) => {
                format!("not started {issue}: {why}")
            }
            Effect::ReadIssues { .. } => "read".into(),
            Effect::GivePlace { task } => format!("give {task}"),
            Effect::Start { issue, task, .. } => format!("start {issue} as {task}"),
            Effect::NextRun {
                issue, task, run, ..
            } => format!("next {issue} of {task} as {run}"),
            Effect::CloseIssue { issue, .. } => format!("close {issue}"),
            Effect::Held { .. } => "held".into(),
        })
        .collect()
}

#[test]
fn a_look_reads_the_issues_first() {
    let mut d = drive(2, &[4]);
    assert_eq!(said(&d.look(&id())), ["read"]);
    let mut none = BoardDrive::new(HashMap::new(), HashMap::new(), "/x".into());
    assert!(none.look(&id()).is_empty());
}

#[test]
fn issues_read_starts_in_order_in_one_write() {
    let mut d = drive(2, &[4, 9, 11]);
    let fx = read(&mut d, issues(&[4, 9, 11], &[]), facts(0));
    assert_eq!(
        said(&fx),
        ["write [11]", "changed", "held"],
        "nothing starts before its write"
    );
    assert_eq!(
        said(&land(&mut d, fx)),
        ["start 4 as id0", "start 9 as id1", "changed", "held"]
    );
    let work: Vec<u64> = d
        .board(&id())
        .unwrap()
        .work()
        .iter()
        .map(|e| e.issue)
        .collect();
    assert_eq!(work, [4, 9]);
    assert_eq!(d.starting(&id()).collect::<Vec<_>>(), [4, 9]);
}

#[test]
fn gone_issues_and_started_ones_leave_to_do_in_one_write() {
    let mut d = drive(1, &[3, 4, 9]);
    let fx = read(&mut d, issues(&[4, 9], &[3]), facts(0));
    assert_eq!(said(&fx), ["write [9]", "changed", "held"]);
    assert_eq!(
        said(&land(&mut d, fx)),
        ["start 4 as id0", "changed", "held"]
    );
}

#[test]
fn places_go_to_extends_first() {
    let mut d = drive(2, &[4]);
    let mut f = facts(1);
    f.extends_waiting = vec!["a".into(), "b".into()];
    let fx = read(&mut d, issues(&[4], &[]), f);
    assert_eq!(said(&fx), ["give a"], "one place, the first extend's");
}

#[test]
fn no_window_starts_nothing_but_still_gives_places() {
    let mut d = drive(2, &[4]);
    let mut f = facts(0);
    f.window = false;
    f.extends_waiting = vec!["a".into()];
    assert_eq!(said(&read(&mut d, issues(&[4], &[]), f)), ["give a"]);
    assert_eq!(d.board(&id()).unwrap().todo(), [4]);
}

#[test]
fn an_unreadable_read_only_gives_places() {
    let mut d = drive(2, &[4]);
    let mut f = facts(0);
    f.extends_waiting = vec!["a".into()];
    let fx = d.handle(
        Input::IssuesRead {
            id: id(),
            read: Err("gone".into()),
            facts: Box::new(f),
        },
        now(),
    );
    assert_eq!(said(&fx), ["give a"]);
}

#[test]
fn a_board_that_cannot_run_its_workflow_starts_nothing() {
    let mut d = drive(2, &[4]);
    let mut f = facts(0);
    f.templates
        .insert("builtin:branch".into(), Ok(flow(Place::Checkout)));
    assert!(read(&mut d, issues(&[4], &[]), f).is_empty());
    let mut f = facts(0);
    f.templates
        .insert("builtin:branch".into(), Err("gone".into()));
    assert!(read(&mut d, issues(&[4], &[]), f).is_empty());
}

#[test]
fn a_start_on_its_way_counts_toward_the_team() {
    let mut d = drive(1, &[4, 9]);
    let fx = read(&mut d, issues(&[4, 9], &[]), facts(0));
    let _ = land(&mut d, fx);
    assert!(read(&mut d, issues(&[4, 9], &[]), facts(0)).is_empty());
    let mut d = drive(3, &[4, 9, 2]);
    let fx = read(&mut d, issues(&[4, 9, 2], &[]), facts(1));
    let _ = land(&mut d, fx);
    let room = Room {
        limit: 2,
        holding: 0,
        waiting: 0,
    };
    let mut f = facts(0);
    f.first.insert("builtin:branch".into(), Some(room));
    assert!(
        read(&mut d, issues(&[2], &[]), f).is_empty(),
        "the 2 on their way fill a first step of 2"
    );
}

#[test]
fn the_first_steps_room_narrows_a_pull() {
    let mut d = drive(3, &[2, 7]);
    let mut f = facts(0);
    f.first.insert(
        "builtin:branch".into(),
        Some(Room {
            limit: 3,
            holding: 1,
            waiting: 1,
        }),
    );
    let fx = read(&mut d, issues(&[2, 7], &[]), f);
    assert_eq!(
        said(&land(&mut d, fx)),
        ["start 2 as id0", "changed", "held"]
    );
}

fn landed(d: &mut BoardDrive, issue: u64, kept: bool, result: Result<(), String>) -> Vec<Effect> {
    d.handle(
        Input::Landed {
            id: id(),
            issue,
            task: "id0".into(),
            kept,
            result,
        },
        now(),
    )
}

#[test]
fn landed_ok_looks_again_and_is_no_longer_starting() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let _ = land(&mut d, fx);
    assert_eq!(said(&landed(&mut d, 4, true, Ok(()))), ["read"]);
    assert_eq!(d.starting(&id()).count(), 0);
}

#[test]
fn refused_before_kept_says_it_once() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let _ = land(&mut d, fx);
    assert_eq!(
        said(&landed(&mut d, 4, false, Err("no brief".into()))),
        ["not started 4: no brief"]
    );
    assert_eq!(d.starting(&id()).count(), 0);
}

#[test]
fn refused_after_kept_says_nothing() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let _ = land(&mut d, fx);
    assert!(landed(&mut d, 4, true, Err("no agent".into())).is_empty());
}

fn ended(d: &mut BoardDrive, issue: u64, task: &str, outcome: Outcome) -> Vec<Effect> {
    d.handle(
        Input::Ended {
            id: id(),
            issue,
            task: task.into(),
            run: Some(task.into()),
            outcome: Some(outcome),
        },
        now(),
    )
}

/// A drive whose board has started `issue` as task `id0`, landed.
fn started(issue: u64) -> BoardDrive {
    let mut d = drive(2, &[issue]);
    let fx = read(&mut d, issues(&[issue], &[]), facts(0));
    let _ = land(&mut d, fx);
    let _ = landed(&mut d, issue, true, Ok(()));
    d
}

#[test]
fn a_start_waits_for_its_write() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    assert!(said(&fx).iter().all(|e| !e.starts_with("start")));
    assert_eq!(
        d.starting(&id()).collect::<Vec<_>>(),
        [4],
        "counted meanwhile"
    );
}

#[test]
fn a_failed_start_write_puts_the_issue_back_and_starts_nothing() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let rest = answer(&mut d, fx, Err("disk full".into()));
    assert!(
        said(&rest).iter().all(|e| !e.starts_with("start")),
        "{:?}",
        said(&rest)
    );
    assert_eq!(d.board(&id()).unwrap().todo(), [4]);
    assert!(d.board(&id()).unwrap().work().is_empty());
    assert_eq!(d.starting(&id()).count(), 0);
}

#[test]
fn a_start_carried_by_a_later_write_is_asked_once() {
    let mut d = drive(2, &[4]);
    let first = read(&mut d, issues(&[4], &[]), facts(0));
    let later = d.add_todo(&id(), 9, now());
    // The first write fails, the later one, which carries the start, lands.
    let rest = answer(&mut d, first, Err("busy".into()));
    assert!(said(&rest).iter().all(|e| !e.starts_with("start")));
    let rest = land(&mut d, later);
    let starts = said(&rest)
        .iter()
        .filter(|e| e.starts_with("start"))
        .count();
    assert_eq!(starts, 1, "{:?}", said(&rest));
    assert_eq!(d.board(&id()).unwrap().work().len(), 1);
}

/// A done end marks the entry done in its column, and the look its read
/// brings moves it on: here, past the last column, into Done, closing it.
#[test]
fn a_done_end_moves_the_entry_to_done_and_closes_once() {
    let mut d = started(4);
    let fx = {
        let fx = ended(&mut d, 4, "id0", Outcome::Done);
        land(&mut d, fx)
    };
    assert!(said(&fx).contains(&"read".to_string()), "{:?}", said(&fx));
    assert!(d.board(&id()).unwrap().work()[0].done, "done in its column");
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    assert!(
        said(&fx).contains(&"close 4".to_string()),
        "{:?}",
        said(&fx)
    );
    let _ = land(&mut d, fx);
    let board = d.board(&id()).unwrap();
    assert!(board.work().is_empty());
    assert_eq!(board.done()[0].issue, 4);
    assert!(read(&mut d, issues(&[4], &[]), facts(0)).is_empty());
}

/// Task `id0`'s run on issue 4 ended done and the look moved it to Done,
/// over issues `open` and `closed`; what that look's writes answered.
fn done_and_moved(d: &mut BoardDrive, open: &[u64], closed: &[u64]) -> Vec<Effect> {
    let fx = ended(d, 4, "id0", Outcome::Done);
    let _ = land(d, fx);
    let fx = read(d, issues(open, closed), facts(0));
    land(d, fx)
}

#[test]
fn a_closed_issue_is_not_closed_again() {
    let mut d = started(4);
    let fx = done_and_moved(&mut d, &[], &[4]);
    assert!(
        said(&fx).iter().all(|e| !e.starts_with("close")),
        "{:?}",
        said(&fx)
    );
}

#[test]
fn a_synced_issue_is_left_open() {
    let mut d = started(4);
    let _ = {
        let fx = ended(&mut d, 4, "id0", Outcome::Done);
        land(&mut d, fx)
    };
    let mut kept = issues(&[4], &[]);
    kept.sync_with(Some("Forge".into()));
    let forge: &'static dyn crate::connector::Connector =
        Box::leak(Box::new(crate::connector::memory::Forge::default()));
    let fx = d.handle(
        Input::IssuesRead {
            id: id(),
            read: Ok((kept, Some(forge))),
            facts: Box::new(facts(0)),
        },
        now(),
    );
    assert!(
        said(&fx).iter().all(|e| !e.starts_with("close")),
        "{:?}",
        said(&fx)
    );
}

#[test]
fn a_stop_keeps_the_entry_and_looks_again() {
    let mut d = started(4);
    let fx = ended(&mut d, 4, "id0", Outcome::Stopped(Stop::ByPerson));
    assert_eq!(said(&fx), ["read"]);
    assert_eq!(d.board(&id()).unwrap().work().len(), 1);
}

#[test]
fn a_failed_end_keeps_the_entry_and_pulls_nothing() {
    let mut d = started(4);
    assert!(ended(&mut d, 4, "id0", Outcome::Failed("no".into())).is_empty());
    assert_eq!(d.board(&id()).unwrap().work().len(), 1);
}

#[test]
fn an_end_of_a_task_no_entry_names_does_nothing() {
    let mut d = started(4);
    let _ = done_and_moved(&mut d, &[4], &[]);
    // Retried from Done by a person: its end is theirs.
    assert!(ended(&mut d, 4, "id0", Outcome::Done).is_empty());
    assert!(ended(&mut d, 8, "elsewhere", Outcome::Stopped(Stop::ByPerson)).is_empty());
}

#[test]
fn remove_refuses_while_it_works() {
    let mut d = started(4);
    assert!(d
        .remove(&id(), 4, true)
        .is_err_and(|why| why.contains("Stop")));
    assert_eq!(d.board(&id()).unwrap().work().len(), 1);
}

#[test]
fn remove_gives_the_issue_back_and_says_held() {
    let mut d = started(4);
    let fx = d.remove(&id(), 4, false).unwrap();
    assert!(said(&fx).contains(&"held".to_string()));
    let board = d.board(&id()).unwrap();
    assert!(board.work().is_empty() && board.todo().is_empty());
}

#[test]
fn unbind_says_held() {
    let mut d = started(4);
    assert!(said(&d.unbind(&id())).contains(&"held".to_string()));
}

/// A board not yet adopting its tasks starts nothing: its tasks are not
/// known yet, and adoption would write over what it started.
#[test]
fn a_board_before_its_tasks_are_read_starts_nothing() {
    let mut boards = HashMap::new();
    let board = Board::made("builtin:branch".into(), 2, &[4], 1).unadopted();
    boards.insert(id(), board);
    let mut d = BoardDrive::new(boards, HashMap::new(), "/data/projects".into());
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    assert!(said(&fx)
        .iter()
        .all(|e| !e.starts_with("start") && !e.starts_with("write")));
    assert_eq!(d.board(&id()).unwrap().todo(), [4]);
}

/// A start whose write failed while a change of the team waited to be
/// written: the board still names it, so the team's write carries it, and
/// it is asked once that lands.
#[test]
fn a_start_left_on_the_board_by_a_failed_write_waits_for_the_next() {
    let mut d = drive(2, &[4]);
    let first = read(&mut d, issues(&[4], &[]), facts(0));
    let waits = d.set_team(&id(), 3);
    let rest = answer(&mut d, first, Err("busy".into()));
    assert!(said(&rest).iter().all(|e| !e.starts_with("start")));
    assert_eq!(d.board(&id()).unwrap().work().len(), 1, "still shown");
    let token = waits
        .iter()
        .find_map(|e| match e {
            Effect::Arm { token, .. } => Some(*token),
            Effect::Write(_)
            | Effect::Changed(_)
            | Effect::Signal(_)
            | Effect::ReadIssues { .. }
            | Effect::GivePlace { .. }
            | Effect::Start { .. }
            | Effect::NextRun { .. }
            | Effect::CloseIssue { .. }
            | Effect::Held { .. } => None,
        })
        .unwrap();
    let written = d.handle(Input::Elapsed { token }, now());
    let rest = land(&mut d, written);
    let starts = said(&rest)
        .iter()
        .filter(|e| e.starts_with("start"))
        .count();
    assert_eq!(starts, 1, "{:?}", said(&rest));
}

/// A board of two workflow columns with a person's review between.
fn two_columns(d: &mut BoardDrive) {
    let mut board = d.board(&id()).unwrap().clone();
    let column = |id: &str, work: crate::board::Work| crate::board::model::Column {
        id: id.into(),
        name: id.into(),
        work,
        starts: crate::board::Starts::OnEntry,
        moves_on: crate::board::MovesOn::ByItself,
        limit: None,
        entries: Vec::new(),
    };
    board.columns[1].moves_on = crate::board::MovesOn::ByItself;
    board
        .columns
        .push(column("review", crate::board::Work::Person));
    board.columns[2].moves_on = crate::board::MovesOn::ByPerson;
    board.columns.push(column(
        "ship",
        crate::board::Work::Workflow {
            id: "builtin:branch".into(),
        },
    ));
    let fx = d.set_board(&id(), Some(board));
    let _ = land(d, fx);
}

#[test]
fn next_run_is_asked_for_a_task_entering_a_workflow_column() {
    let mut d = started(4);
    two_columns(&mut d);
    let _ = done_and_moved(&mut d, &[4], &[]);
    assert_eq!(
        d.board(&id()).unwrap().at(4).map(|(at, _)| at),
        Some(2),
        "in review"
    );
    let fx = d.place(&id(), 4, Some("ship"), None, false, now()).unwrap();
    let _ = land(&mut d, fx);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let rest = land(&mut d, fx);
    assert!(
        said(&rest)
            .iter()
            .any(|e| e.starts_with("next 4 of id0 as id")),
        "{:?}",
        said(&rest)
    );
}

#[test]
fn an_end_in_a_column_the_task_left_changes_nothing() {
    let mut d = started(4);
    two_columns(&mut d);
    let fx = d
        .place(&id(), 4, Some("review"), None, false, now())
        .unwrap();
    let _ = land(&mut d, fx);
    assert!(ended(&mut d, 4, "id0", Outcome::Done).is_empty());
    assert!(!d.board(&id()).unwrap().columns[2].entries[0].done);
}

#[test]
fn a_lost_start_is_kept_and_not_started_again() {
    let mut d = started(4);
    let _ = d.handle(Input::TasksRead { tasks: Vec::new() }, now());
    assert_eq!(d.board(&id()).unwrap().work().len(), 1, "kept");
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    let rest = land(&mut d, fx);
    assert!(said(&rest)
        .iter()
        .all(|e| !e.starts_with("start") && !e.starts_with("next")));
}

/// An end from a workflow column the task has left, retried there by a
/// person, does not mark it done in the workflow column it now waits in.
#[test]
fn an_end_from_a_workflow_column_left_for_another_changes_nothing() {
    let mut d = started(4);
    two_columns(&mut d);
    let mut board = d.board(&id()).unwrap().clone();
    board.columns[3].starts = crate::board::Starts::OnPress;
    let fx = d.set_board(&id(), Some(board));
    let _ = land(&mut d, fx);
    let fx = d.place(&id(), 4, Some("ship"), None, false, now()).unwrap();
    let _ = land(&mut d, fx);
    assert!(ended(&mut d, 4, "id0", Outcome::Done).is_empty());
    assert!(!d.board(&id()).unwrap().columns[3].entries[0].done);
}

/// What the board holds is every column's tasks, not the first's alone.
#[test]
fn every_columns_tasks_are_held() {
    let mut d = started(4);
    two_columns(&mut d);
    let fx = d
        .place(&id(), 4, Some("review"), None, false, now())
        .unwrap();
    assert!(d.held().contains("id0"), "in review, still held");
    let _ = fx;
}

/// An issue whose start is on its way does not move.
#[test]
fn place_refuses_an_issue_starting() {
    let mut d = drive(2, &[4]);
    let fx = read(&mut d, issues(&[4], &[]), facts(0));
    assert!(d
        .place(&id(), 4, Some("todo"), None, false, now())
        .is_err_and(|why| why.contains("starting")));
    let _ = fx;
}

/// A done end moves to Done and closes with no window holding the project:
/// only the starts need one.
#[test]
fn a_done_end_closes_with_no_window() {
    let mut d = started(4);
    let fx = ended(&mut d, 4, "id0", Outcome::Done);
    let _ = land(&mut d, fx);
    let mut f = facts(0);
    f.window = false;
    let fx = read(&mut d, issues(&[4], &[]), f);
    assert!(
        said(&fx).contains(&"close 4".to_string()),
        "{:?}",
        said(&fx)
    );
}
