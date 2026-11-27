use super::*;
use crate::board::line::EntryAct;
use crate::board::model::Column;
use crate::board::{Board, Entry, MovesOn, Starts, Work};

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

/// Todo, Plan (a workflow), Review (a person's), issue 4 in Plan and 9 in
/// Todo.
fn board() -> Board {
    let mut board = Board::made("w".into(), 2, &[9], 0);
    board.columns[1].id = "plan".into();
    board.columns[1].name = "PLAN".into();
    board.columns[1].entries.push(Entry {
        issue: 4,
        task: Some("t".into()),
        run: Some("t".into()),
        ..Entry::default()
    });
    board.columns.push(column("review", Work::Person));
    board
}

fn standing(working: bool) -> Standing {
    Standing {
        working,
        starting: false,
        start: None,
    }
}

fn words(offers: &[(Offer, Option<String>)], task: bool) -> Vec<String> {
    offers.iter().map(|(o, _)| o.words(task)).collect()
}

#[test]
fn a_person_column_offers_its_moves() {
    let got = offers(&board(), 9, None, &standing(false));
    assert_eq!(
        words(&got, false),
        [
            "Move to PLAN",
            "Move to Done",
            "Move to REVIEW",
            "Move up",
            "Move down",
            "Back to Backlog"
        ]
    );
}

#[test]
fn a_done_entry_offers_move_on_first() {
    let act = EntryAct::MoveTo(Some("review".into()));
    let got = offers(&board(), 4, Some(&act), &standing(false));
    assert_eq!(
        words(&got, true)[..3],
        ["Move to REVIEW", "Move back to To do", "Move to Done"]
    );
}

#[test]
fn start_comes_first_where_it_waits_for_start() {
    let got = offers(&board(), 4, Some(&EntryAct::Start), &standing(false));
    assert_eq!(got[0].0, Offer::Start);
}

#[test]
fn every_move_refuses_while_it_works() {
    let got = offers(&board(), 4, None, &standing(true));
    assert!(got.iter().all(|(o, why)| match o {
        Offer::Start => true,
        Offer::MoveTo { .. } | Offer::Up | Offer::Down | Offer::Back => {
            why.as_deref() == Some("It is at work: Stop it first")
        }
    }));
}

#[test]
fn a_full_column_refuses_the_move_there() {
    let mut b = board();
    b.columns[2].limit = Some(1);
    b.columns[2].entries.push(Entry {
        issue: 7,
        ..Entry::default()
    });
    let got = offers(&b, 9, None, &standing(false));
    let review = got
        .iter()
        .find(|(o, _)| o.words(false) == "Move to REVIEW")
        .unwrap();
    assert_eq!(review.1.as_deref(), Some("REVIEW is full"));
}

#[test]
fn up_and_down_refuse_at_the_ends() {
    let got = offers(&board(), 9, None, &standing(false));
    let why = |w: &str| {
        got.iter()
            .find(|(o, _)| o.words(false) == w)
            .unwrap()
            .1
            .clone()
    };
    assert!(why("Move up").is_some());
    assert!(why("Move down").is_some());
}

#[test]
fn back_reads_as_removal_with_a_task() {
    assert_eq!(Offer::Back.words(true), "Remove from board");
    assert_eq!(Offer::Back.words(false), "Back to Backlog");
}

#[test]
fn start_refuses_where_its_workflow_is_gone() {
    let gone = Standing {
        start: Some("Its workflow is gone".into()),
        ..standing(false)
    };
    let got = offers(&board(), 4, Some(&EntryAct::Start), &gone);
    assert_eq!(got[0], (Offer::Start, Some("Its workflow is gone".into())));
}

#[test]
fn every_move_refuses_while_it_starts() {
    let starting = Standing {
        starting: true,
        ..standing(false)
    };
    let got = offers(&board(), 4, None, &starting);
    assert!(got
        .iter()
        .all(|(_, why)| why.as_deref() == Some("It is starting: wait for it to start")));
}
