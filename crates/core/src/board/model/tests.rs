use super::*;

fn made() -> Board {
    Board::made("builtin:board".into(), 3, &[4, 9], 100)
}

#[test]
fn made_is_to_do_then_one_work_column() {
    let board = made();
    let ids: Vec<&str> = board.columns.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["todo", "work"]);
    let todo = &board.columns[0];
    assert_eq!(todo.work, Work::Person);
    assert_eq!(todo.moves_on, MovesOn::ByItself);
    let since: Vec<(u64, u64)> = todo.entries.iter().map(|e| (e.issue, e.since)).collect();
    assert_eq!(since, [(4, 100), (9, 100)]);
    let work = &board.columns[1];
    assert_eq!(
        work.work,
        Work::Workflow {
            id: "builtin:board".into()
        }
    );
    assert_eq!(work.starts, Starts::OnEntry);
    assert_eq!(work.moves_on, MovesOn::ByItself);
    assert!(work.entries.is_empty());
    assert!(board.done.is_empty());
    assert_eq!(board.team, 3);
    assert_eq!(board.schema_version, SCHEMA_VERSION);
    assert_eq!(board.workflow(), "builtin:board");
    assert_eq!(board.todo(), [4, 9]);
}

#[test]
fn with_todo_keeps_since_of_an_issue_that_stays() {
    let board = Board::made("w".into(), 2, &[4], 100).with_todo(&[9, 4], 200);
    let since: Vec<(u64, u64)> = (board.columns[0].entries.iter())
        .map(|e| (e.issue, e.since))
        .collect();
    assert_eq!(since, [(9, 200), (4, 100)]);
}

#[test]
fn with_team_clamps() {
    assert_eq!(made().with_team(0).team, 1);
    assert_eq!(made().with_team(12).team, MOST_TEAM);
    assert_eq!(made().with_team(5).team, 5);
}

#[test]
fn rebound_keeps_team_and_to_do() {
    let board = made().rebound("other".into());
    assert_eq!(board.workflow(), "other");
    assert_eq!(board.todo(), [4, 9]);
    assert_eq!(board.team, 3);
}

#[test]
fn shape_refuses_what_step_one_does_not_know() {
    assert!(made().shape().is_ok());
    let mut third = made();
    third.columns.push(third.columns[0].clone());
    assert!(third.shape().is_err());
    let mut person = made();
    person.columns[1].work = Work::Person;
    assert!(person.shape().is_err());
    let mut flow = made();
    flow.columns[0].work = Work::Workflow { id: "w".into() };
    assert!(flow.shape().is_err());
    let mut done = made();
    done.done.push(done.columns[0].entries[0].clone());
    assert!(done.shape().is_err());
    let mut newer = made();
    newer.schema_version = SCHEMA_VERSION + 1;
    assert!(newer.shape().is_err());
}
