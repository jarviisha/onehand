use super::*;
use crate::board::model::Column;
use crate::board::{Board, Entry, MovesOn, Starts, Work};

fn column(id: &str, work: Work, starts: Starts, moves_on: MovesOn) -> Column {
    Column {
        id: id.into(),
        name: id.to_uppercase(),
        work,
        starts,
        moves_on,
        limit: None,
        entries: Vec::new(),
    }
}

fn flow() -> Work {
    Work::Workflow { id: "w".into() }
}

/// A board of one column, `here`, holding `entry`, then `next` when given.
fn board(here: Column, entry: Entry, next: Option<Column>) -> Board {
    let mut board = Board::made("w".into(), 2, &[], 0);
    board.columns.clear();
    let mut here = here;
    here.entries.push(entry);
    board.columns.push(here);
    board.columns.extend(next);
    board
}

fn entry(run: Option<&str>, done: bool) -> Entry {
    Entry {
        issue: 4,
        task: run.map(Into::into),
        run: run.map(Into::into),
        since: 77,
        done,
    }
}

#[test]
fn a_person_column_says_since_when() {
    let b = board(
        column("todo", Work::Person, Starts::OnEntry, MovesOn::ByPerson),
        entry(None, false),
        None,
    );
    assert_eq!(line(&b, 4, false, false, false), (Line::Since(77), None));
}

#[test]
fn a_column_starting_on_a_press_waits_for_start() {
    let b = board(
        column("plan", flow(), Starts::OnPress, MovesOn::ByItself),
        entry(None, false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false),
        (Line::WaitingForStart, Some(EntryAct::Start))
    );
}

#[test]
fn a_column_starting_on_entry_waits_for_the_team() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(None, false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false),
        (Line::WaitingForTeam, None)
    );
    assert_eq!(line(&b, 4, false, false, true), (Line::Starting, None));
}

#[test]
fn a_task_at_its_column_says_its_own_line() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), false),
        None,
    );
    assert_eq!(line(&b, 4, true, true, false), (Line::Task, None));
}

#[test]
fn a_start_that_never_landed_offers_start() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false),
        (Line::DidNotStart, Some(EntryAct::Start))
    );
}

#[test]
fn done_here_moves_on_by_a_person_or_waits_for_room() {
    let next = column("ship", Work::Person, Starts::OnEntry, MovesOn::ByItself);
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByPerson),
        entry(Some("t"), true),
        Some(next.clone()),
    );
    assert_eq!(
        line(&b, 4, true, false, false),
        (Line::DoneHere, Some(EntryAct::MoveTo(Some("ship".into()))))
    );
    let mut full = next;
    full.limit = Some(1);
    full.entries.push(Entry {
        issue: 9,
        ..Entry::default()
    });
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), true),
        Some(full),
    );
    assert_eq!(
        line(&b, 4, true, false, false),
        (Line::WaitingForRoom("SHIP".into()), None)
    );
}

#[test]
fn words_say_each_line() {
    assert_eq!(Line::WaitingForStart.words(), "Waiting for Start");
    assert_eq!(Line::WaitingForTeam.words(), "Waiting for a team place");
    assert_eq!(Line::DoneHere.words(), "Done here");
    assert_eq!(
        Line::WaitingForRoom("Ship".into()).words(),
        "Waiting for room in Ship"
    );
    assert_eq!(Line::DidNotStart.words(), "Did not start");
}

/// A run asked for that its task never made (a next run refused) reads as a
/// start that did not land, though the task is kept.
#[test]
fn a_run_the_task_never_made_did_not_start() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false),
        (Line::DidNotStart, Some(EntryAct::Start)),
        "the run is not one of its task's"
    );
}
