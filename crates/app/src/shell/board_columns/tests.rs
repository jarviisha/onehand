//! The column form: a column added, edited, moved and removed, and each
//! refusal said in the form, which stays.

use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Entity, TestAppContext, VisualTestContext};
use onehand_core::board::drive::BoardDrive;
use onehand_core::board::{Board, Entry, MovesOn, Starts};
use onehand_core::project::{Project, ProjectId, Projects, ProjectsDrive, Toward};
use std::path::PathBuf;

const BRANCH: &str = "builtin:branch";

/// A shell holding a project whose board is `board`, its root open.
fn rig<'a>(
    name: &str,
    seed: u64,
    board: Board,
    cx: &'a mut TestAppContext,
) -> (Entity<crate::shell::Shell>, PathBuf, ProjectId, &'a mut VisualTestContext) {
    let dir = folder(name);
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(seed), None, root.clone());
    let id = project.id.clone();
    let branch = onehand_core::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == BRANCH)
        .unwrap();
    cx.update(|_, cx| crate::workflow::offer(vec![branch], cx));
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects = ProjectsDrive::new(Projects::from_iter([project]), dir.join("projects"));
            shared.boards =
                BoardDrive::new(Default::default(), Default::default(), dir.join("projects"));
            let _ = shared.boards.set_board(&id, Some(board));
        })
    });
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    cx.run_until_parked();
    (shell, root, id, cx)
}

fn board_of(id: &ProjectId, cx: &mut VisualTestContext) -> Board {
    cx.update(|_, cx| Shared::global(cx).boards.board(id).cloned().unwrap())
}

/// Fill the open form's name and limit, then save it.
fn save(
    shell: &Entity<crate::shell::Shell>,
    name: &str,
    limit: &str,
    cx: &mut VisualTestContext,
) {
    let (name, limit) = (name.to_string(), limit.to_string());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let draft = shell.column_draft().expect("a form open");
            let (n, l) = (draft.name.clone(), draft.limit.clone());
            n.update(cx, |input, cx| input.set_value(name, window, cx));
            l.update(cx, |input, cx| input.set_value(limit, window, cx));
            shell.save_column(window, cx);
        })
    });
    cx.run_until_parked();
}

fn said(shell: &Entity<crate::shell::Shell>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| shell.read(cx).column_draft().and_then(|d| d.error.clone()))
}

fn open(
    shell: &Entity<crate::shell::Shell>,
    root: &PathBuf,
    editing: Option<&str>,
    cx: &mut VisualTestContext,
) {
    let (root, editing) = (root.clone(), editing.map(String::from));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.open_column_form(root, editing, window, cx))
    });
}

#[gpui::test]
fn a_column_is_added_last_with_its_settings(cx: &mut TestAppContext) {
    let (shell, root, id, cx) = rig("col-add", 101, Board::made(BRANCH.into(), 2, &[], 0), cx);
    open(&shell, &root, None, cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.set_column_work(false, cx);
            shell.set_column_workflow(BRANCH.into(), cx);
            shell.set_column_starts(Starts::OnPress, cx);
            shell.set_column_moves_on(MovesOn::ByPerson, cx);
        })
    });
    save(&shell, "Ship", "2", cx);
    assert!(cx.update(|_, cx| shell.read(cx).column_draft().is_none()), "closed");
    let board = board_of(&id, cx);
    let last = board.columns.last().unwrap();
    assert_eq!(last.name, "Ship");
    assert_eq!((last.starts, last.moves_on, last.limit), (Starts::OnPress, MovesOn::ByPerson, Some(2)));
    assert!(last.id.starts_with("col-"));
}

#[gpui::test]
fn a_bad_limit_or_no_workflow_keeps_the_form_saying_why(cx: &mut TestAppContext) {
    let (shell, root, _, cx) = rig("col-bad", 102, Board::made(BRANCH.into(), 2, &[], 0), cx);
    open(&shell, &root, None, cx);
    save(&shell, "Review", "0", cx);
    assert!(said(&shell, cx).is_some_and(|why| why.contains("whole number")));
    save(&shell, "Review", "two", cx);
    assert!(said(&shell, cx).is_some_and(|why| why.contains("whole number")));
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.set_column_work(false, cx)));
    save(&shell, "Review", "", cx);
    assert!(said(&shell, cx).is_some_and(|why| why.contains("Pick the workflow")));
    save(&shell, "", "", cx);
    assert!(said(&shell, cx).is_some());
}

#[gpui::test]
fn a_name_used_twice_is_refused_in_the_form(cx: &mut TestAppContext) {
    let (shell, root, _, cx) = rig("col-twice", 103, Board::made(BRANCH.into(), 2, &[], 0), cx);
    open(&shell, &root, None, cx);
    save(&shell, "to do", "", cx);
    assert!(said(&shell, cx).is_some_and(|why| why.contains("Two columns")));
}

#[gpui::test]
fn editing_keeps_the_columns_id_and_issues(cx: &mut TestAppContext) {
    let (shell, root, id, cx) = rig("col-edit", 104, Board::made(BRANCH.into(), 2, &[4], 0), cx);
    open(&shell, &root, Some("todo"), cx);
    save(&shell, "Ready", "", cx);
    let board = board_of(&id, cx);
    assert_eq!((board.columns[0].id.as_str(), board.columns[0].name.as_str()), ("todo", "Ready"));
    assert_eq!(board.todo(), [4]);
}

#[gpui::test]
fn turning_a_column_to_a_persons_refuses_while_a_task_works_there(cx: &mut TestAppContext) {
    let mut board = Board::made(BRANCH.into(), 2, &[], 0);
    board.columns[1].entries.push(Entry {
        issue: 4,
        task: Some("busy".into()),
        run: Some("busy".into()),
        ..Entry::default()
    });
    let (shell, root, id, cx) = rig("col-busy", 105, board, cx);
    // The task works: it holds its place.
    cx.update(|window, cx| {
        let handle = window.window_handle();
        let template = onehand_core::workflow::builtin::all().remove(0);
        let brief = onehand_core::workflow::Brief {
            title: "Busy".into(),
            body: String::new(),
            instructions: None,
            closes: None,
        };
        let setup = onehand_core::workflow::Setup {
            repo: root.clone(),
            dir: root.clone(),
            branch: None,
            agent: None,
            check: None,
            mode: None,
            forge: None,
        };
        let task = onehand_core::task::Task::new("busy".into(), template, brief, setup);
        crate::task::hold_place(task, handle, cx);
    });
    cx.run_until_parked();
    open(&shell, &root, Some("work"), cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.set_column_work(true, cx)));
    save(&shell, "Work", "", cx);
    let why = said(&shell, cx);
    let busy = cx.update(|_, cx| crate::task::is_working("busy", cx));
    assert!(why.as_deref().is_some_and(|why| why.contains("Stop")), "{why:?} busy={busy}");
    assert!(board_of(&id, cx).columns[1].entries[0].run.is_some());
}

#[gpui::test]
fn a_column_removed_while_its_form_is_open_refuses_the_save(cx: &mut TestAppContext) {
    let mut board = Board::made(BRANCH.into(), 2, &[], 0);
    board.columns.push(onehand_core::board::model::Column {
        id: "review".into(),
        name: "Review".into(),
        work: onehand_core::board::Work::Person,
        starts: Starts::OnEntry,
        moves_on: MovesOn::ByItself,
        limit: None,
        entries: Vec::new(),
    });
    let (shell, root, id, cx) = rig("col-gone", 106, board, cx);
    open(&shell, &root, Some("review"), cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.remove_board_column(root.clone(), "review".into(), cx))
    });
    cx.run_until_parked();
    save(&shell, "Review", "", cx);
    assert!(said(&shell, cx).is_some_and(|why| why.contains("No column")));
    assert!(board_of(&id, cx).column("review").is_none());
}

#[gpui::test]
fn move_up_on_the_first_column_writes_nothing(cx: &mut TestAppContext) {
    let (shell, root, id, cx) = rig("col-move", 107, Board::made(BRANCH.into(), 2, &[], 0), cx);
    let before = board_of(&id, cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.move_board_column(root.clone(), "todo".into(), Toward::Up, cx))
    });
    assert_eq!(board_of(&id, cx), before);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.move_board_column(root.clone(), "todo".into(), Toward::Down, cx))
    });
    assert_eq!(board_of(&id, cx).columns[0].id, "work");
}

#[gpui::test]
fn remove_refuses_a_column_with_issues(cx: &mut TestAppContext) {
    let (shell, root, id, cx) = rig("col-remove", 108, Board::made(BRANCH.into(), 2, &[4], 0), cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.remove_board_column(root.clone(), "todo".into(), cx))
    });
    assert!(board_of(&id, cx).column("todo").is_some());
    let said = cx.update(|_, cx| shell.read(cx).board_refusal(&root).map(str::to_string));
    assert!(said.is_some_and(|why| why.contains("Move its issue out first")));
}
