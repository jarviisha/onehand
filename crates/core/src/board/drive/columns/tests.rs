use super::super::*;
use crate::board::model::Column;
use crate::board::{Entry, MovesOn, Starts, Work};
use crate::project::{ProjectId, Toward};

fn now() -> Now {
    Now {
        secs: 100,
        at: std::time::Instant::now(),
    }
}

fn id() -> ProjectId {
    ProjectId::mint(1)
}

fn column(id: &str, work: Work) -> Column {
    Column {
        id: id.into(),
        name: id.into(),
        work,
        starts: Starts::OnEntry,
        moves_on: MovesOn::ByItself,
        limit: None,
        entries: Vec::new(),
    }
}

/// A drive whose board holds `board`.
fn drive(board: Board) -> BoardDrive {
    let mut boards = HashMap::new();
    boards.insert(id(), board);
    BoardDrive::new(boards, HashMap::new(), "/data/projects".into())
}

fn ok(_: &Work) -> Result<(), String> {
    Ok(())
}

fn idle(_: &str) -> bool {
    false
}

fn starts(fx: &[Effect]) -> usize {
    fx.iter()
        .filter(|e| match e {
            Effect::Start { .. } | Effect::NextRun { .. } => true,
            Effect::Write(_)
            | Effect::Arm { .. }
            | Effect::Changed(_)
            | Effect::Signal(_)
            | Effect::ReadIssues { .. }
            | Effect::GivePlace { .. }
            | Effect::CloseIssue { .. }
            | Effect::Held { .. } => false,
        })
        .count()
}

/// Land every write among `fx`, and give back the rest and what landing
/// answered.
fn land(d: &mut BoardDrive, fx: Vec<Effect>) -> Vec<Effect> {
    let mut rest = Vec::new();
    for e in fx {
        match e {
            Effect::Write(w) => {
                let more = d.handle(
                    Input::Written {
                        id: w.id,
                        pending: w.pending,
                        seq: w.seq,
                        result: Ok(()),
                    },
                    now(),
                );
                rest.extend(land(d, more));
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

#[test]
fn add_column_refuses_what_valid_refuses() {
    let mut d = drive(Board::made("a".into(), 2, &[], 1));
    let refuse = |_: &Work| Err("not on offer".to_string());
    assert!(d
        .add_column(&id(), column("x", Work::Person), 1, &refuse)
        .is_err_and(|why| why.contains("not on offer")));
    assert!(d
        .add_column(&id(), column("x", Work::Person), 1, &ok)
        .is_ok());
    assert!(
        d.add_column(&id(), column("x", Work::Person), 1, &ok)
            .is_err(),
        "an id used twice"
    );
    assert_eq!(d.board(&id()).unwrap().columns[1].id, "x");
}

#[test]
fn remove_column_refuses_one_with_entries() {
    let mut d = drive(Board::made("a".into(), 2, &[4], 1));
    assert!(d.remove_column(&id(), "todo").is_err());
    assert!(d.remove_column(&id(), "work").is_ok());
    assert!(d.board(&id()).unwrap().column("work").is_none());
}

#[test]
fn move_column_reorders() {
    let mut d = drive(Board::made("a".into(), 2, &[], 1));
    let _ = d.move_column(&id(), "work", Toward::Up);
    assert_eq!(d.board(&id()).unwrap().columns[0].id, "work");
}

#[test]
fn turning_a_column_to_a_person_one_refuses_while_it_works() {
    let mut board = Board::made("a".into(), 2, &[], 1);
    board.columns[1].entries.push(Entry {
        issue: 4,
        task: Some("t".into()),
        run: Some("t".into()),
        ..Entry::default()
    });
    let mut d = drive(board);
    let person = column("work", Work::Person);
    assert!(d
        .edit_column(&id(), "work", person.clone(), &|t| t == "t", &ok)
        .is_err());
    assert!(d.edit_column(&id(), "work", person, &idle, &ok).is_ok());
    let entry = &d.board(&id()).unwrap().columns[1].entries[0];
    assert!(
        entry.run.is_none() && entry.task.is_some(),
        "its run forgotten, its task kept"
    );
}

#[test]
fn place_refuses_a_full_column_and_a_task_at_work() {
    let mut board = Board::made("a".into(), 2, &[4, 5], 1);
    board.columns[1].limit = Some(1);
    board.columns[1].entries.push(Entry {
        issue: 9,
        ..Entry::default()
    });
    let mut d = drive(board);
    assert!(d
        .place(&id(), 4, Some("work"), None, false, now())
        .is_err_and(|why| why.contains("full")));
    assert!(d
        .place(&id(), 4, None, None, true, now())
        .is_err_and(|why| why.contains("Stop")));
    assert!(d
        .place(&id(), 5, Some("todo"), Some(4), false, now())
        .is_ok());
    assert_eq!(d.board(&id()).unwrap().todo(), [5, 4]);
}

#[test]
fn place_into_done_closes_once_read() {
    let mut d = drive(Board::made("a".into(), 2, &[4], 1));
    let fx = d.place(&id(), 4, None, None, false, now()).unwrap();
    assert!(fx.iter().any(|e| match e {
        Effect::ReadIssues { .. } => true,
        Effect::Write(_)
        | Effect::Arm { .. }
        | Effect::Changed(_)
        | Effect::Signal(_)
        | Effect::GivePlace { .. }
        | Effect::Start { .. }
        | Effect::NextRun { .. }
        | Effect::CloseIssue { .. }
        | Effect::Held { .. } => false,
    }));
    assert_eq!(d.board(&id()).unwrap().done[0].issue, 4);
    assert!(d.closing.get(&id()).is_some_and(|c| c.contains(&4)));
}

#[test]
fn start_on_a_press_goes_ahead_past_a_full_team() {
    let mut board = Board::made("a".into(), 1, &[], 1);
    board.columns[1].starts = Starts::OnPress;
    board.columns[1].entries.push(Entry {
        issue: 4,
        ..Entry::default()
    });
    let mut d = drive(board);
    let flow = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    let fx = d
        .start(&id(), 4, false, vec!["n".into()], Ok(flow))
        .unwrap();
    assert_eq!(starts(&fx), 0, "held for its write");
    let rest = land(&mut d, fx);
    assert_eq!(starts(&rest), 1);
    assert_eq!(
        d.board(&id()).unwrap().columns[1].entries[0]
            .task
            .as_deref(),
        Some("n")
    );
}
