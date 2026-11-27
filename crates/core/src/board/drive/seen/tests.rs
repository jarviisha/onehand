use super::super::*;
use super::Seen;
use crate::project::ProjectId;
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

/// A drive whose project's board holds `todo`, at version 1 when `old`.
fn drive(todo: &[u64], old: bool) -> BoardDrive {
    let board = Board::made("w".into(), 2, todo, 1);
    let board = match old {
        true => board.unadopted(),
        false => board,
    };
    let mut boards = HashMap::new();
    boards.insert(id(), board);
    BoardDrive::new(boards, HashMap::new(), "/data/projects".into())
}

fn seen(task: &str, issue: u64, standing: Option<bool>, at: u64) -> Seen {
    Seen {
        id: task.into(),
        project: id(),
        issue,
        run: Some(task.into()),
        standing,
        recency: (at, 0),
    }
}

fn held(fx: &[Effect]) -> Option<HashSet<String>> {
    fx.iter().find_map(|e| match e {
        Effect::Held { tasks } => Some(tasks.clone()),
        Effect::Write(_)
        | Effect::Arm { .. }
        | Effect::Changed(_)
        | Effect::Signal(_)
        | Effect::ReadIssues { .. }
        | Effect::GivePlace { .. }
        | Effect::Start { .. }
        | Effect::CloseIssue { .. } => None,
    })
}

fn read(d: &mut BoardDrive, tasks: Vec<Seen>) -> Vec<Effect> {
    d.handle(Input::TasksRead { tasks }, now())
}

#[test]
fn adoption_places_live_and_failed_tasks_in_work_and_done_ones_in_done() {
    let mut d = drive(&[4, 9], true);
    let fx = read(
        &mut d,
        vec![
            seen("late", 6, Some(false), 30),
            seen("early", 5, Some(false), 10),
            seen("old", 7, Some(true), 1),
            seen("new", 8, Some(true), 50),
            seen("off", 9, None, 60),
        ],
    );
    let board = d.board(&id()).unwrap();
    assert!(!board.adopts());
    let work: Vec<&str> = board
        .work()
        .iter()
        .filter_map(|e| e.task.as_deref())
        .collect();
    assert_eq!(work, ["early", "late"], "oldest first");
    let done: Vec<&str> = board
        .done()
        .iter()
        .filter_map(|e| e.task.as_deref())
        .collect();
    assert_eq!(done, ["new", "old"], "newest first");
    assert_eq!(
        board.todo(),
        [4, 9],
        "an issue off the board stays in To do"
    );
    assert_eq!(
        held(&fx),
        Some(["early", "late"].map(String::from).into_iter().collect())
    );
}

#[test]
fn adoption_happens_once() {
    let mut d = drive(&[], true);
    let _ = read(&mut d, vec![seen("a", 5, Some(false), 1)]);
    let before = d.board(&id()).cloned();
    let _ = read(
        &mut d,
        vec![seen("a", 5, Some(false), 1), seen("b", 6, Some(false), 2)],
    );
    assert_eq!(d.board(&id()).cloned(), before);
}

#[test]
fn an_entry_naming_a_lost_task_is_dropped() {
    let mut d = drive(&[], true);
    let _ = read(&mut d, vec![seen("a", 5, Some(false), 1)]);
    let fx = read(&mut d, Vec::new());
    assert!(d.board(&id()).unwrap().work().is_empty());
    assert_eq!(held(&fx), Some(HashSet::new()));
}

#[test]
fn an_entry_whose_start_is_on_its_way_is_kept() {
    let mut d = drive(&[4], false);
    d.starting.entry(id()).or_default().insert(4);
    let board = d.board(&id()).unwrap().started(4, "t", 2);
    d.boards.boards.insert(id(), board);
    let _ = read(&mut d, Vec::new());
    assert_eq!(d.board(&id()).unwrap().work().len(), 1);
}

#[test]
fn held_names_work_entries_only() {
    let mut d = drive(&[], true);
    let fx = read(
        &mut d,
        vec![seen("a", 5, Some(false), 1), seen("b", 6, Some(true), 2)],
    );
    assert_eq!(held(&fx), Some(["a".to_string()].into_iter().collect()));
}
