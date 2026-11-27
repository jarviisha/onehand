use super::super::*;
use crate::issues::Issues;
use crate::project::ProjectId;
use crate::task::rooms::Room;
use crate::workflow::{Outcome, Place, Stop, Template};
use std::collections::HashSet;

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

/// Land every write among `fx`, and answer the rest.
fn land(d: &mut BoardDrive, fx: Vec<Effect>) -> Vec<Effect> {
    let mut rest = Vec::new();
    for e in fx {
        match e {
            Effect::Write(w) => {
                let _ = d.handle(
                    Input::Written {
                        id: w.id,
                        pending: w.pending,
                        result: Ok(()),
                    },
                    now(),
                );
            }
            other @ (Effect::Arm { .. }
            | Effect::Changed(_)
            | Effect::Signal(_)
            | Effect::ReadIssues { .. }
            | Effect::GivePlace { .. }
            | Effect::Start { .. }
            | Effect::CloseIssue { .. }) => rest.push(other),
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
        template: Ok(flow(Place::Worktree)),
        running,
        extends_waiting: Vec::new(),
        first: None,
        taken: HashSet::new(),
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
            Effect::CloseIssue { issue, .. } => format!("close {issue}"),
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
        ["write [11]", "changed", "start 4 as id0", "start 9 as id1"]
    );
    assert_eq!(d.starting(&id()).collect::<Vec<_>>(), [4, 9]);
}

#[test]
fn gone_issues_and_started_ones_leave_to_do_in_one_write() {
    let mut d = drive(1, &[3, 4, 9]);
    let fx = read(&mut d, issues(&[4, 9], &[3]), facts(0));
    assert_eq!(said(&fx), ["write [9]", "changed", "start 4 as id0"]);
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
    f.template = Ok(flow(Place::Checkout));
    assert!(read(&mut d, issues(&[4], &[]), f).is_empty());
    let mut f = facts(0);
    f.template = Err("gone".into());
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
    f.first = Some(room);
    assert!(
        read(&mut d, issues(&[2], &[]), f).is_empty(),
        "the 2 on their way fill a first step of 2"
    );
}

#[test]
fn the_first_steps_room_narrows_a_pull() {
    let mut d = drive(3, &[2, 7]);
    let mut f = facts(0);
    f.first = Some(Room {
        limit: 3,
        holding: 1,
        waiting: 1,
    });
    let fx = read(&mut d, issues(&[2, 7], &[]), f);
    assert_eq!(said(&fx), ["write [7]", "changed", "start 2 as id0"]);
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

fn ended(d: &mut BoardDrive, issue: u64, outcome: Outcome) -> Vec<Effect> {
    d.handle(
        Input::Ended {
            id: id(),
            issue,
            outcome: Some(outcome),
        },
        now(),
    )
}

#[test]
fn a_done_end_closes_an_own_issue_once() {
    let mut d = drive(2, &[]);
    assert_eq!(said(&ended(&mut d, 4, Outcome::Done)), ["read"]);
    assert_eq!(
        said(&read(&mut d, issues(&[4], &[]), facts(0))),
        ["close 4"]
    );
    assert!(read(&mut d, issues(&[4], &[]), facts(0)).is_empty());
}

#[test]
fn a_closed_issue_is_not_closed_again() {
    let mut d = drive(2, &[]);
    let _ = ended(&mut d, 4, Outcome::Done);
    assert!(read(&mut d, issues(&[], &[4]), facts(0)).is_empty());
}

#[test]
fn a_synced_issue_is_left_open() {
    let mut d = drive(2, &[]);
    let _ = ended(&mut d, 4, Outcome::Done);
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
fn a_failed_end_pulls_nothing() {
    let mut d = drive(2, &[4]);
    assert!(ended(&mut d, 9, Outcome::Failed("no".into())).is_empty());
}

#[test]
fn a_stopped_end_looks_again() {
    let mut d = drive(2, &[4]);
    assert_eq!(
        said(&ended(&mut d, 9, Outcome::Stopped(Stop::ByPerson))),
        ["read"]
    );
}

#[test]
fn an_end_after_unbinding_still_closes_but_pulls_nothing() {
    let mut d = drive(2, &[]);
    let fx = d.unbind(&id());
    let _ = land(&mut d, fx);
    assert_eq!(said(&ended(&mut d, 4, Outcome::Done)), ["read"]);
    assert_eq!(
        said(&read(&mut d, issues(&[4], &[]), facts(0))),
        ["close 4"]
    );
    assert!(ended(&mut d, 5, Outcome::Stopped(Stop::ByPerson)).is_empty());
}
