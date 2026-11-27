use super::*;
use crate::project::{ProjectId, Toward};
use crate::workflow::{Place, Template};

fn now() -> Now {
    Now {
        secs: 100,
        at: std::time::Instant::now(),
    }
}

fn id() -> ProjectId {
    ProjectId::mint(1)
}

fn drive() -> BoardDrive {
    BoardDrive::new(HashMap::new(), HashMap::new(), "/data/projects".into())
}

fn board(todo: &[u64]) -> Board {
    Board::made("mine".into(), 3, todo, 1)
}

/// A drive whose project has a board holding `todo`, written.
fn drive_with_todo(todo: &[u64]) -> BoardDrive {
    let mut d = drive();
    let w = write_of(d.set_board(&id(), Some(board(todo))));
    let _ = d.handle(
        Input::Written {
            id: id(),
            pending: w.pending,
            result: Ok(()),
        },
        now(),
    );
    d
}

fn todo_of(d: &BoardDrive) -> Vec<u64> {
    d.board(&id()).unwrap().todo()
}

/// The write among `fx`, taken out.
fn write_of(fx: Vec<Effect>) -> Box<BoardWrite> {
    fx.into_iter()
        .find_map(|e| match e {
            Effect::Write(w) => Some(w),
            Effect::Arm { .. }
            | Effect::Changed(_)
            | Effect::Signal(_)
            | Effect::ReadIssues { .. }
            | Effect::GivePlace { .. }
            | Effect::Start { .. }
            | Effect::CloseIssue { .. } => None,
        })
        .expect("a write")
}

fn flow(place: Place) -> Template {
    let mut template = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    template.place = place;
    template
}

#[test]
fn add_todo_appends_once() {
    let mut d = drive_with_todo(&[]);
    let _ = d.add_todo(&id(), 4, now());
    assert!(d.add_todo(&id(), 4, now()).is_empty());
    assert_eq!(todo_of(&d), [4]);
}

#[test]
fn move_todo_swaps_neighbours() {
    let mut d = drive_with_todo(&[4, 9, 2]);
    let _ = d.move_todo(&id(), 2, Toward::Up, now());
    assert_eq!(todo_of(&d), [4, 2, 9]);
    assert!(d.move_todo(&id(), 4, Toward::Up, now()).is_empty());
    assert_eq!(todo_of(&d), [4, 2, 9]);
}

#[test]
fn place_todo_inserts_a_new_issue_at_the_index() {
    let mut d = drive_with_todo(&[4, 9]);
    let _ = d.place_todo(&id(), 7, 1, now());
    assert_eq!(todo_of(&d), [4, 7, 9]);
}

#[test]
fn place_todo_moves_an_issue_within_to_do() {
    let mut d = drive_with_todo(&[4, 9, 2]);
    let _ = d.place_todo(&id(), 2, 0, now());
    assert_eq!(todo_of(&d), [2, 4, 9]);
}

#[test]
fn place_todo_clamps_past_the_end() {
    let mut d = drive_with_todo(&[4, 9]);
    let _ = d.place_todo(&id(), 4, 50, now());
    assert_eq!(todo_of(&d), [9, 4]);
}

#[test]
fn place_todo_to_its_own_place_writes_nothing() {
    let mut d = drive_with_todo(&[4, 9, 2]);
    assert!(d.place_todo(&id(), 9, 1, now()).is_empty());
}

#[test]
fn drop_todo_removes_only_named() {
    let mut d = drive_with_todo(&[4, 9, 2]);
    let _ = d.drop_todo(&id(), 9, now());
    assert_eq!(todo_of(&d), [4, 2]);
}

#[test]
fn place_todo_before_lands_just_before_its_target() {
    let mut d = drive_with_todo(&[1, 2, 3, 4]);
    let _ = d.place_todo_before(&id(), 4, 2, now());
    assert_eq!(todo_of(&d), [1, 4, 2, 3]);
    let _ = d.place_todo_before(&id(), 9, 1, now());
    assert_eq!(todo_of(&d), [9, 1, 4, 2, 3], "from the Backlog");
    assert!(d.place_todo_before(&id(), 2, 2, now()).is_empty());
    assert!(
        d.place_todo_before(&id(), 2, 7, now()).is_empty(),
        "no such target"
    );
}

#[test]
fn set_team_clamps() {
    let mut d = drive_with_todo(&[]);
    let _ = d.set_team(&id(), 0);
    assert_eq!(d.board(&id()).unwrap().team, 1);
    let _ = d.set_team(&id(), 12);
    assert_eq!(d.board(&id()).unwrap().team, 8);
}

#[test]
fn a_project_without_a_board_ignores_todo_changes() {
    let mut d = drive();
    assert!(d.add_todo(&id(), 4, now()).is_empty());
    assert!(d.set_team(&id(), 3).is_empty());
    assert!(d.board(&id()).is_none());
}

#[test]
fn bind_starts_small_and_keeps_what_a_rebind_had() {
    let mut d = drive();
    let flow = flow(Place::Worktree);
    assert!(d.bind(&id(), Some(&flow), now()).is_ok());
    let bound = d.board(&id()).unwrap().clone();
    assert_eq!((bound.workflow(), bound.team), ("builtin:branch", 2));
    assert!(bound.todo().is_empty());
    let _ = d.set_board(&id(), Some(board(&[4])));
    let mut copy = flow.clone();
    copy.id = "copy".into();
    assert!(d
        .bind_copy(&id(), &copy, now())
        .is_some_and(|bound| bound.is_ok()));
    let kept = d.board(&id()).unwrap().clone();
    assert_eq!((kept.workflow(), kept.team), ("copy", 3));
    assert_eq!(kept.todo(), [4]);
}

#[test]
fn bind_refuses_what_a_board_cannot_run() {
    let mut d = drive();
    assert!(d
        .bind(&id(), None, now())
        .is_err_and(|why| why.contains("gone")));
    let checkout = flow(Place::Checkout);
    assert!(d.bind(&id(), Some(&checkout), now()).is_err());
    assert!(d.board(&id()).is_none());
    assert!(
        d.bind_copy(&id(), &checkout, now()).is_none(),
        "no board to copy for"
    );
}

#[test]
fn unbinding_asks_only_over_a_to_do() {
    assert!(!drive().unbinding_asks(&id()));
    assert!(!drive_with_todo(&[]).unbinding_asks(&id()));
    assert!(drive_with_todo(&[4]).unbinding_asks(&id()));
}

#[test]
fn unbind_writes_none() {
    let mut d = drive_with_todo(&[4]);
    let w = write_of(d.unbind(&id()));
    assert!(w.board.is_none());
    assert_eq!(
        w.dir,
        std::path::Path::new("/data/projects").join(id().as_str())
    );
    assert!(d.board(&id()).is_none());
}

#[test]
fn a_failed_write_puts_the_board_back() {
    let mut d = drive_with_todo(&[4]);
    let w = write_of(d.add_todo(&id(), 9, now()));
    assert_eq!(w.board.as_ref().unwrap().todo(), [4, 9]);
    let fx = d.handle(
        Input::Written {
            id: id(),
            pending: w.pending,
            result: Err("disk full".into()),
        },
        now(),
    );
    assert_eq!(todo_of(&d), [4]);
    let changed = fx.iter().any(|e| match e {
        Effect::Changed(of) => *of == id(),
        Effect::Write(_)
        | Effect::Arm { .. }
        | Effect::Signal(_)
        | Effect::ReadIssues { .. }
        | Effect::GivePlace { .. }
        | Effect::Start { .. }
        | Effect::CloseIssue { .. } => false,
    });
    let said = fx.iter().any(|e| match e {
        Effect::Signal(Signal::SaveFailed { id: of, why }) => *of == id() && why == "disk full",
        Effect::Signal(Signal::NotStarted { .. }) => false,
        Effect::Write(_)
        | Effect::Arm { .. }
        | Effect::Changed(_)
        | Effect::ReadIssues { .. }
        | Effect::GivePlace { .. }
        | Effect::Start { .. }
        | Effect::CloseIssue { .. } => false,
    });
    assert!(changed && said);
}

#[test]
fn set_team_waits_then_writes_once() {
    let mut d = drive_with_todo(&[]);
    let token = |fx: &[Effect]| {
        fx.iter()
            .find_map(|e| match e {
                Effect::Arm { token, .. } => Some(*token),
                Effect::Write(_)
                | Effect::Changed(_)
                | Effect::Signal(_)
                | Effect::ReadIssues { .. }
                | Effect::GivePlace { .. }
                | Effect::Start { .. }
                | Effect::CloseIssue { .. } => None,
            })
            .expect("a wait")
    };
    let first = token(&d.set_team(&id(), 4));
    let second = token(&d.set_team(&id(), 5));
    assert!(d.handle(Input::Elapsed { token: first }, now()).is_empty());
    let w = write_of(d.handle(Input::Elapsed { token: second }, now()));
    assert_eq!(w.board.unwrap().team, 5);
}

#[test]
fn flush_writes_a_waiting_team() {
    let mut d = drive_with_todo(&[]);
    let _ = d.set_team(&id(), 6);
    let w = write_of(d.flush());
    assert_eq!(w.board.unwrap().team, 6);
    assert!(d.flush().is_empty(), "nothing waits any more");
}

#[test]
fn an_unreadable_board_is_never_written() {
    let mut unreadable = HashMap::new();
    unreadable.insert(id(), "too new".to_string());
    let mut d = BoardDrive::new(HashMap::new(), unreadable, "/data/projects".into());
    assert!(d.add_todo(&id(), 4, now()).is_empty());
    assert!(d.set_team(&id(), 3).is_empty());
    assert!(d.unbind(&id()).is_empty());
    assert_eq!(d.unreadable(&id()), Some("too new"));
}

#[test]
fn binding_an_unreadable_board_says_why() {
    let mut unreadable = HashMap::new();
    unreadable.insert(id(), "too new".to_string());
    let mut d = BoardDrive::new(HashMap::new(), unreadable, "/data/projects".into());
    let flow = flow(Place::Worktree);
    assert!(d
        .bind(&id(), Some(&flow), now())
        .is_err_and(|why| why.contains("too new")));
    assert!(d
        .bind_copy(&id(), &flow, now())
        .is_some_and(|bound| bound.is_err_and(|why| why.contains("too new"))));
}

#[test]
fn a_change_says_changed() {
    let mut d = drive_with_todo(&[]);
    let fx = d.add_todo(&id(), 4, now());
    assert!(fx.iter().any(|e| match e {
        Effect::Changed(of) => *of == id(),
        Effect::Write(_)
        | Effect::Arm { .. }
        | Effect::Signal(_)
        | Effect::ReadIssues { .. }
        | Effect::GivePlace { .. }
        | Effect::Start { .. }
        | Effect::CloseIssue { .. } => false,
    }));
}

#[test]
fn a_drop_on_a_row_lands_just_before_it() {
    let todo = [1, 2, 3, 4];
    // From below: the target keeps its place.
    assert_eq!(place_before(&todo, 4, 2), Some(1));
    // From above: the target moves up one once the issue is out.
    assert_eq!(place_before(&todo, 1, 3), Some(1));
    // Onto the row after itself: its own place, which writes nothing.
    assert_eq!(place_before(&todo, 2, 3), Some(1));
    // From the Backlog.
    assert_eq!(place_before(&todo, 9, 1), Some(0));
    assert_eq!(place_before(&todo, 3, 3), None);
}
