use super::super::*;
use crate::project::Toward;

fn column(id: &str, work: Work) -> Column {
    Column {
        id: id.into(),
        name: id.to_uppercase(),
        work,
        starts: Starts::OnEntry,
        moves_on: MovesOn::ByItself,
        limit: None,
        entries: Vec::new(),
    }
}

fn flow(id: &str) -> Work {
    Work::Workflow { id: id.into() }
}

/// To do, a workflow column, a person's review, a second workflow column.
fn four() -> Board {
    let mut board = Board::made("a".into(), 2, &[4, 9], 1);
    board.columns.push(column("review", Work::Person));
    board.columns.push(column("ship", flow("b")));
    board
}

fn entry(issue: u64, task: Option<&str>, run: Option<&str>, done: bool) -> Entry {
    Entry {
        issue,
        task: task.map(Into::into),
        run: run.map(Into::into),
        since: 0,
        done,
    }
}

#[test]
fn version_three_reads_any_columns() {
    let mut board = four();
    board.columns[1].entries.push(entry(5, None, None, false));
    board.columns[2]
        .entries
        .push(entry(6, Some("t"), None, false));
    assert!(board.shape().is_ok(), "{:?}", board.shape());
}

#[test]
fn an_issue_on_two_entries_is_refused() {
    let mut board = four();
    board.columns[2].entries.push(entry(4, None, None, false));
    assert!(board.shape().is_err());
}

#[test]
fn a_person_columns_entry_with_a_run_is_refused() {
    let mut board = four();
    board.columns[2]
        .entries
        .push(entry(6, Some("t"), Some("t"), false));
    assert!(board.shape().is_err());
    let mut done = four();
    done.columns[2]
        .entries
        .push(entry(6, Some("t"), None, true));
    assert!(done.shape().is_err());
}

#[test]
fn ids_and_names_must_be_there_and_limits_at_least_one() {
    let mut twice = four();
    twice.columns[3].id = "review".into();
    assert!(twice.shape().is_err());
    let mut nameless = four();
    nameless.columns[2].name = " ".into();
    assert!(nameless.shape().is_err());
    let mut none = four();
    none.columns[1].limit = Some(0);
    assert!(none.shape().is_err());
    let mut empty = four();
    empty.columns.clear();
    assert!(empty.shape().is_err());
}

#[test]
fn placed_moves_an_entry_and_clears_its_columns_state() {
    let mut board = four();
    board.columns[1]
        .entries
        .push(entry(5, Some("t"), Some("r"), true));
    let board = board.placed(5, 2, None, 50);
    assert!(board.columns[1].entries.is_empty());
    assert_eq!(
        board.columns[2].entries,
        [Entry {
            since: 50,
            ..entry(5, Some("t"), None, false)
        }]
    );
    let new = board.placed(7, 3, None, 60);
    assert_eq!(
        new.columns[3].entries,
        [Entry {
            since: 60,
            ..entry(7, None, None, false)
        }]
    );
}

#[test]
fn placed_before_lands_just_before() {
    let board = four().placed(9, 0, Some(4), 5);
    assert_eq!(board.todo(), [9, 4]);
    let board = four().placed(9, 0, Some(9), 5);
    assert_eq!(board.todo(), [4, 9], "before itself: where it was");
}

#[test]
fn to_done_caps() {
    let mut board = four();
    board.done = (100..100 + DONE_KEPT as u64)
        .map(|n| entry(n, Some("x"), None, false))
        .collect();
    let board = board.to_done(4, 70);
    assert_eq!(board.done.len(), DONE_KEPT);
    assert_eq!(board.done[0].issue, 4);
    assert_eq!(board.done[0].since, 70);
    assert_eq!(board.todo(), [9]);
}

#[test]
fn begun_and_ended_done() {
    let board = four().placed(4, 1, None, 3).begun(4, "t", "r");
    assert_eq!(
        board.columns[1].entries[0],
        Entry {
            since: 3,
            ..entry(4, Some("t"), Some("r"), false)
        }
    );
    let board = board.ended_done("t");
    assert!(board.columns[1].entries[0].done);
}

#[test]
fn column_edits_keep_entries() {
    let board = four()
        .with_column(1, column("plan", flow("p")))
        .column_moved("ship", Toward::Up);
    let ids: Vec<&str> = board.columns.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["todo", "plan", "work", "ship", "review"]);
    assert_eq!(board.todo(), [4, 9]);
    let board = board.without_column("plan");
    assert_eq!(board.columns.len(), 4);
}

#[test]
fn holds_names_any_column() {
    let mut board = four();
    board.columns[2]
        .entries
        .push(entry(6, Some("t"), None, false));
    assert!(board.holds("t"));
    assert_eq!(board.at(6).map(|(at, _)| at), Some(2));
}
