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
    assert_eq!(
        line(&b, 4, false, false, false, None),
        (Line::Since(77), None)
    );
}

#[test]
fn a_column_starting_on_a_press_waits_for_start() {
    let b = board(
        column("plan", flow(), Starts::OnPress, MovesOn::ByItself),
        entry(None, false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false, None),
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
        line(&b, 4, false, false, false, None),
        (Line::WaitingForTeam, None)
    );
    assert_eq!(
        line(&b, 4, false, false, true, None),
        (Line::Starting, None)
    );
}

#[test]
fn a_task_at_its_column_says_its_own_line() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), false),
        None,
    );
    assert_eq!(line(&b, 4, true, true, false, None), (Line::Task, None));
}

#[test]
fn a_start_that_never_landed_offers_start() {
    let b = board(
        column("plan", flow(), Starts::OnEntry, MovesOn::ByItself),
        entry(Some("t"), false),
        None,
    );
    assert_eq!(
        line(&b, 4, false, false, false, None),
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
        line(&b, 4, true, false, false, None),
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
        line(&b, 4, true, false, false, None),
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
        line(&b, 4, false, false, false, None),
        (Line::DidNotStart, Some(EntryAct::Start)),
        "the run is not one of its task's"
    );
}

#[test]
fn a_column_that_cannot_start_says_why_instead_of_waiting() {
    for starts in [Starts::OnEntry, Starts::OnPress] {
        let b = board(
            column("ship", flow(), starts, MovesOn::ByItself),
            entry(None, false),
            None,
        );
        assert_eq!(
            line(&b, 4, false, false, false, Some("Its workflow, w, is gone")),
            (Line::CannotStart("Its workflow, w, is gone".into()), None)
        );
    }
}

#[test]
fn a_task_carried_in_at_work_is_its_task_not_waiting() {
    let mut carried = entry(None, false);
    carried.task = Some("t".into());
    let b = board(
        column("plan", flow(), Starts::OnPress, MovesOn::ByItself),
        carried,
        None,
    );
    assert_eq!(line(&b, 4, false, true, false, None), (Line::Task, None));
}

#[test]
fn waiting_counts_what_waits_on_a_person() {
    let mut b = board(
        column("plan", flow(), Starts::OnPress, MovesOn::ByPerson),
        entry(None, false),
        Some(column(
            "review",
            Work::Person,
            Starts::OnEntry,
            MovesOn::ByPerson,
        )),
    );
    let more = [
        (5, Some("r5"), true),  // done here, a person moves it on
        (6, Some("r6"), false), // a run asked for that never landed
        (7, Some("r7"), false), // at work
    ];
    for (issue, run, done) in more {
        b.columns[0].entries.push(Entry {
            issue,
            task: run.map(Into::into),
            run: run.map(Into::into),
            since: 1,
            done,
        });
    }
    b.columns[1].entries.push(Entry {
        issue: 8,
        ..Entry::default()
    });
    let facts = |_: &Column, e: &Entry| Facts {
        run_here: e.issue == 7,
        working: e.issue == 7,
        starting: false,
        start: None,
    };
    // 5 is done here, 6 did not start; 4 waits for Start but To do counts
    // it, 7 works, 8 is a person's column at rest.
    assert_eq!(waiting(&b, facts), 2);
    let gone = |c: &Column, e: &Entry| Facts {
        start: Some("Its workflow, w, is gone".into()),
        ..facts(c, e)
    };
    // Nothing starts there: only 5, done, waits on a person, and 6 still
    // says it did not start.
    assert_eq!(waiting(&b, gone), 2);
}

#[test]
fn waiting_leaves_out_what_to_do_counts() {
    // A first column that runs a workflow on a press: its issue waiting for
    // Start is in To do already.
    let b = board(
        column("plan", flow(), Starts::OnPress, MovesOn::ByPerson),
        entry(None, false),
        None,
    );
    assert_eq!(b.todo(), vec![4]);
    assert_eq!(waiting(&b, |_, _| Facts::default()), 0);
}
