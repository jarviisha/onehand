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

/// A board of an earlier version is read only in the shape it had then.
#[test]
fn shape_refuses_what_an_earlier_version_did_not_know() {
    let old = || {
        let mut board = made();
        board.schema_version = 2;
        board
    };
    assert!(old().shape().is_ok());
    let mut third = old();
    third.columns.push(third.columns[0].clone());
    assert!(third.shape().is_err());
    let mut person = old();
    person.columns[1].work = Work::Person;
    assert!(person.shape().is_err());
    let mut flow = old();
    flow.columns[0].work = Work::Workflow { id: "w".into() };
    assert!(flow.shape().is_err());
    let mut done = old();
    done.done.push(done.columns[0].entries[0].clone());
    assert!(done.shape().is_err(), "an issue on the board twice, and done with no task");
    let mut newer = made();
    newer.schema_version = SCHEMA_VERSION + 1;
    assert!(newer.shape().is_err());
}

fn work(issue: u64, task: &str) -> Entry {
    Entry {
        issue,
        task: Some(task.into()),
        run: Some(task.into()),
        since: 5,
        done: false,
    }
}

fn done(issue: u64, task: &str) -> Entry {
    Entry {
        run: None,
        ..work(issue, task)
    }
}

#[test]
fn version_one_is_read_and_adopts() {
    let mut board = made();
    board.schema_version = 1;
    assert!(board.shape().is_ok());
    assert!(board.adopts());
    assert!(!made().adopts());
}

#[test]
fn version_two_with_work_and_done_is_read() {
    let board = made().upgraded(vec![work(4, "a")], vec![done(7, "b")]);
    assert_eq!(board.schema_version, SCHEMA_VERSION);
    assert!(board.shape().is_ok(), "{:?}", board.shape());
    assert_eq!(board.work().len(), 1);
    assert_eq!(board.done().len(), 1);
}

#[test]
fn a_work_entry_without_a_task_is_refused_at_version_two() {
    let mut board = made();
    board.schema_version = 2;
    board.columns[1].entries.push(Entry {
        issue: 3,
        task: None,
        run: None,
        since: 0,
        done: false,
    });
    assert!(board.shape().is_err());
}

#[test]
fn upgraded_moves_to_do_issues_that_tasks_hold_and_caps_done() {
    let mut old = made();
    old.schema_version = 1;
    let many: Vec<Entry> = (100..160).map(|n| done(n, &n.to_string())).collect();
    let board = old.upgraded(vec![work(4, "a")], many);
    assert_eq!(board.todo(), [9]);
    assert_eq!(board.done().len(), DONE_KEPT);
    assert_eq!(board.done()[0].issue, 100, "newest first, as handed");
    assert_eq!(board.schema_version, SCHEMA_VERSION);
}



#[test]
fn removed_leaves_no_trace() {
    let board = made()
        .upgraded(vec![work(5, "a")], vec![done(7, "c")])
        .removed(5)
        .removed(7)
        .removed(9);
    assert!(board.work().is_empty() && board.done().is_empty());
    assert_eq!(board.todo(), [4]);
}

#[test]
fn names_tells_work_from_done() {
    let board = made().upgraded(vec![work(5, "a")], vec![done(7, "c")]);
    assert_eq!(board.names("a"), Some(Place::Work));
    assert_eq!(board.names("c"), Some(Place::Done));
    assert_eq!(board.names("z"), None);
}
