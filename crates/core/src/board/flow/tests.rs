use super::*;
use crate::board::model::Column;
use crate::board::{Entry, MovesOn, Starts, Work};
use crate::issues::Issues;

fn issues(open: &[u64], closed: &[u64]) -> Issues {
    let one = |n: &u64, open: bool| format!(r#"{{"number":{n},"title":"t{n}","open":{open}}}"#);
    let all: Vec<String> = (open.iter().map(|n| one(n, true)))
        .chain(closed.iter().map(|n| one(n, false)))
        .collect();
    serde_json::from_str(&format!(r#"{{"next":99,"issues":[{}]}}"#, all.join(","))).unwrap()
}

fn column(id: &str, work: Work, moves_on: MovesOn) -> Column {
    Column {
        id: id.into(),
        name: id.into(),
        work,
        starts: Starts::OnEntry,
        moves_on,
        limit: None,
        entries: Vec::new(),
    }
}

fn flow(id: &str) -> Work {
    Work::Workflow { id: id.into() }
}

fn task(issue: u64, task: &str, done: bool) -> Entry {
    Entry {
        issue,
        task: Some(task.into()),
        run: Some(task.into()),
        since: 0,
        done,
    }
}

fn room(team: usize) -> Room {
    Room {
        team,
        first: HashMap::new(),
    }
}

fn yes(_: &str) -> bool {
    true
}

fn said(starts: &[Begin]) -> Vec<String> {
    starts
        .iter()
        .map(|b| match b {
            Begin::New { issue, column } => format!("new {issue} at {column}"),
            Begin::Next {
                issue,
                task,
                column,
            } => format!("next {issue} of {task} at {column}"),
        })
        .collect()
}

#[test]
fn the_migrated_shape_pulls_as_before() {
    let board = Board::made("a".into(), 2, &[4, 9, 11], 1);
    let got = look(&board, &issues(&[4, 9, 11], &[]), None, room(2), &yes, 5);
    assert_eq!(said(&got.starts), ["new 4 at 1", "new 9 at 1"]);
    assert_eq!(got.board.todo(), [11]);
    let work: Vec<u64> = got.board.columns[1].entries.iter().map(|e| e.issue).collect();
    assert_eq!(work, [4, 9]);
}

#[test]
fn a_done_entry_moves_on_by_itself_and_its_next_run_starts() {
    let mut board = Board::made("a".into(), 2, &[], 1);
    board.columns[1].entries.push(task(4, "t", true));
    board.columns.push(column("ship", flow("b"), MovesOn::ByItself));
    let got = look(&board, &issues(&[4], &[]), None, room(2), &yes, 5);
    assert_eq!(said(&got.starts), ["next 4 of t at 2"]);
    assert!(got.board.columns[1].entries.is_empty());
    assert_eq!(got.board.columns[2].entries[0].run, None);
}

#[test]
fn a_ready_entry_waits_when_the_next_column_is_full() {
    let mut board = Board::made("a".into(), 2, &[], 1);
    board.columns[1].entries.push(task(4, "t", true));
    let mut review = column("review", Work::Person, MovesOn::ByPerson);
    review.limit = Some(1);
    review.entries.push(Entry {
        issue: 8,
        task: Some("u".into()),
        ..Entry::default()
    });
    board.columns.push(review);
    let got = look(&board, &issues(&[4, 8], &[]), None, room(2), &yes, 5);
    assert_eq!(got.board.columns[1].entries.len(), 1, "it waits where it is");
}

#[test]
fn a_move_and_a_start_share_the_team() {
    let mut board = Board::made("a".into(), 2, &[4], 1);
    board.columns[1].entries.push(Entry {
        issue: 7,
        ..Entry::default()
    });
    let got = look(&board, &issues(&[4, 7], &[]), None, room(1), &yes, 5);
    assert_eq!(said(&got.starts), ["new 4 at 1"], "the move takes the one place");
}

#[test]
fn the_first_steps_room_is_per_workflow() {
    let mut board = Board::made("a".into(), 4, &[4, 5], 1);
    board.columns.push(column("ship", flow("b"), MovesOn::ByItself));
    board.columns[2].entries.push(Entry {
        issue: 6,
        task: Some("t".into()),
        ..Entry::default()
    });
    let mut r = room(4);
    r.first.insert("a".into(), Some((1, 0)));
    r.first.insert("b".into(), Some((1, 0)));
    let got = look(&board, &issues(&[4, 5, 6], &[]), None, r, &yes, 5);
    assert_eq!(said(&got.starts), ["new 4 at 1", "next 6 of t at 2"]);
}

#[test]
fn a_person_moves_on_column_keeps_its_entries() {
    let mut board = Board::made("a".into(), 2, &[4], 1);
    board.columns[0].moves_on = MovesOn::ByPerson;
    let got = look(&board, &issues(&[4], &[]), None, room(2), &yes, 5);
    assert!(got.starts.is_empty());
    assert_eq!(got.board.todo(), [4]);
}

#[test]
fn entering_done_closes() {
    let mut board = Board::made("a".into(), 2, &[], 1);
    board.columns[1].entries.push(task(4, "t", true));
    let got = look(&board, &issues(&[4], &[]), None, room(2), &yes, 5);
    assert_eq!(got.closing, [4]);
    assert_eq!(got.board.done[0].issue, 4);
}

#[test]
fn a_closed_issue_without_a_task_is_dropped_anywhere() {
    let mut board = Board::made("a".into(), 2, &[4], 1);
    board.columns[0].moves_on = MovesOn::ByPerson;
    board.columns[1].entries.push(Entry {
        issue: 5,
        ..Entry::default()
    });
    board.columns[1].entries.push(task(6, "t", false));
    let got = look(&board, &issues(&[], &[4, 5, 6]), None, room(0), &yes, 5);
    assert!(got.board.todo().is_empty());
    let left: Vec<u64> = got.board.columns[1].entries.iter().map(|e| e.issue).collect();
    assert_eq!(left, [6], "one a task works stays");
}

#[test]
fn an_unusable_column_starts_nothing() {
    let board = Board::made("a".into(), 2, &[4], 1);
    let got = look(&board, &issues(&[4], &[]), None, room(2), &|_| false, 5);
    assert!(got.starts.is_empty());
    assert_eq!(got.board.todo(), [4]);
}

#[test]
fn an_entry_started_or_done_is_not_started_again() {
    let mut board = Board::made("a".into(), 2, &[], 1);
    board.columns[1].entries.push(task(4, "t", false));
    let got = look(&board, &issues(&[4], &[]), None, room(2), &yes, 5);
    assert!(got.starts.is_empty());
}
