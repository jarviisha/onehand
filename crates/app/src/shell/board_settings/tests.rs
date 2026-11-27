//! Editing a board's workflow from the board: the Workflows page opened on
//! it, a step open, the unsaved form asked about first, and a duplicate
//! saved for one board bound to that board alone.

use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Entity, TestAppContext, VisualTestContext};
use onehand_core::board::Board;
use onehand_core::project::{Project, ProjectId, Projects, ProjectsDrive};
use onehand_core::workflow::Template;

const BRANCH: &str = "builtin:branch";

/// The shipped branch workflow, offered alone.
fn offer_branch(cx: &mut VisualTestContext) -> Template {
    let branch = onehand_core::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == BRANCH)
        .unwrap();
    let offered = branch.clone();
    cx.update(|_, cx| crate::workflow::offer(vec![offered], cx));
    branch
}

fn show(
    shell: &Entity<super::Shell>,
    step: Option<&str>,
    for_board: Option<ProjectId>,
    cx: &mut VisualTestContext,
) {
    let step = step.map(str::to_string);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show_workflow(BRANCH, step.as_deref(), for_board, window, cx)
        })
    });
    cx.run_until_parked();
}

/// The id of the workflow the page has open, and the step open in it.
fn opened(
    shell: &Entity<super::Shell>,
    cx: &mut VisualTestContext,
) -> Option<(String, Option<usize>)> {
    cx.update(|_, cx| shell.read(cx).workflows_page.read(cx).opened())
}

#[gpui::test]
fn opening_a_workflow_by_id_picks_it_and_opens_the_step(cx: &mut TestAppContext) {
    let dir = folder("board-edit-step");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let branch = offer_branch(cx);
    show(&shell, Some(&branch.steps[1].id), None, cx);
    assert!(cx.update(|_, cx| shell.read(cx).workflows_shown(cx)));
    assert_eq!(opened(&shell, cx), Some((BRANCH.to_string(), Some(1))));
    // Opened again on no step, the open workflow stays as it is.
    show(&shell, None, None, cx);
    assert_eq!(opened(&shell, cx), Some((BRANCH.to_string(), Some(1))));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn opening_with_unsaved_changes_asks_first(cx: &mut TestAppContext) {
    let dir = folder("board-edit-ask");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    offer_branch(cx);
    let page = cx.update(|_, cx| shell.read(cx).workflows_page.clone());
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.new_workflow(window, cx);
            page.rename_draft("Half written", window, cx);
        })
    });
    cx.run_until_parked();
    show(&shell, None, None, cx);
    use gpui_component::WindowExt as _;
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        opened(&shell, cx),
        Some((String::new(), None)),
        "kept until answered"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_duplicate_saved_for_a_board_rebinds_only_that_board(cx: &mut TestAppContext) {
    let dir = folder("board-edit-duplicate");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    offer_branch(cx);
    let (a, b) = (
        Project::new(ProjectId::mint(91), None, dir.join("a")),
        Project::new(ProjectId::mint(92), None, dir.join("b")),
    );
    let (ia, ib) = (a.id.clone(), b.id.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects = ProjectsDrive::new(Projects::from_iter([a, b]), dir.join("projects"));
            shared.boards = onehand_core::board::drive::BoardDrive::new(
                Default::default(),
                Default::default(),
                dir.join("projects"),
            );
            for id in [&ia, &ib] {
                let _ = shared
                    .boards
                    .set_board(id, Some(Board::made(BRANCH.into(), 3, &[4], 0)));
            }
        })
    });
    // A bind refused earlier in the window, which the copy's bind replaces.
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell.board_refused = Some((dir.join("a"), "refused before".into()))
        })
    });
    show(&shell, None, Some(ia.clone()), cx);
    let page = cx.update(|_, cx| shell.read(cx).workflows_page.clone());
    let notes = |cx: &mut VisualTestContext| {
        use gpui_component::WindowExt as _;
        cx.update(|window, cx| window.notifications(cx).len())
    };
    let first = notes(cx);
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.duplicate_open(window, cx);
            page.rename_draft("Board edit copy", window, cx);
            page.save_workflow_draft(window, cx);
        })
    });
    cx.run_until_parked();
    let board = |id: &ProjectId, cx: &mut VisualTestContext| {
        cx.update(|_, cx| Shared::global(cx).boards.board(id).cloned())
    };
    let (copy, _) = opened(&shell, cx).expect("the copy stays open");
    assert_ne!(copy, BRANCH);
    let bound = board(&ia, cx).expect("a board");
    assert_eq!(
        (bound.workflow(), bound.team, bound.todo()),
        (copy.as_str(), 3, vec![4])
    );
    assert_eq!(
        board(&ib, cx).map(|b| b.workflow().to_string()),
        Some(BRANCH.to_string())
    );
    assert_eq!(
        notes(cx),
        first + 2,
        "the save, then the board's new workflow"
    );
    assert!(cx.update(|_, cx| shell.read(cx).board_refused.is_none()));
    assert_eq!(
        super::now_uses("a", "Board edit copy"),
        "a's board now uses Board edit copy"
    );
    // Saved again, it binds nothing more and says nothing more.
    let before = notes(cx);
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.rename_draft("Board edit copy 2", window, cx);
            page.save_workflow_draft(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        board(&ib, cx).map(|b| b.workflow().to_string()),
        Some(BRANCH.to_string())
    );
    assert_eq!(notes(cx), before + 1, "only the save is said");
    let file = onehand_core::workflow::store::dir().join(format!("{copy}.toml"));
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn the_board_is_forgotten_on_another_pick_and_on_close(cx: &mut TestAppContext) {
    let dir = folder("board-edit-forget");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let branch = offer_branch(cx);
    let other = onehand_core::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id != BRANCH)
        .unwrap();
    let other_id = other.id.clone();
    cx.update(|_, cx| crate::workflow::offer(vec![branch, other], cx));
    let page = cx.update(|_, cx| shell.read(cx).workflows_page.clone());
    let held = |cx: &mut VisualTestContext| cx.update(|_, cx| page.read(cx).for_board().cloned());
    let board = ProjectId::mint(93);
    show(&shell, None, Some(board.clone()), cx);
    assert_eq!(held(cx), Some(board.clone()));
    cx.update(|window, cx| page.update(cx, |page, cx| page.pick_id(&other_id, window, cx)));
    assert_eq!(held(cx), None, "another workflow picked");
    show(&shell, None, Some(board.clone()), cx);
    assert_eq!(held(cx), Some(board));
    cx.update(|window, cx| page.update(cx, |page, cx| page.close_workflow_form(window, cx)));
    assert_eq!(held(cx), None, "the form closed");
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_copy_the_board_cannot_use_is_said_and_one_for_a_gone_board_binds_none(
    cx: &mut TestAppContext,
) {
    let dir = folder("board-copy-refused");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let mut copy = offer_branch(cx);
    copy.place = onehand_core::workflow::Place::Checkout;
    let (a, b) = (
        Project::new(ProjectId::mint(93), None, dir.join("a")),
        Project::new(ProjectId::mint(94), None, dir.join("b")),
    );
    let (ia, ib) = (a.id.clone(), b.id.clone());
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects = ProjectsDrive::new(Projects::from_iter([a, b]), dir.join("projects"));
            shared.boards = onehand_core::board::drive::BoardDrive::new(
                Default::default(),
                Default::default(),
                dir.join("projects"),
            );
            let _ = shared
                .boards
                .set_board(&ia, Some(Board::made(BRANCH.into(), 3, &[], 0)));
        })
    });
    let notes = |cx: &mut VisualTestContext| {
        use gpui_component::WindowExt as _;
        cx.update(|window, cx| window.notifications(cx).len())
    };
    let bind = |id: &ProjectId, cx: &mut VisualTestContext| {
        let (id, copy) = (id.clone(), copy.clone());
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.board_uses_copy(id, copy, window, cx))
        });
        cx.run_until_parked();
    };
    let before = notes(cx);
    bind(&ia, cx);
    assert_eq!(notes(cx), before + 1, "the refusal is said");
    assert!(cx.update(|_, cx| shell.read(cx).board_refused.is_some()));
    // No board at all: nothing is bound and nothing made.
    let before = notes(cx);
    bind(&ib, cx);
    assert_eq!(notes(cx), before);
    assert!(cx.update(|_, cx| { Shared::global(cx).boards.board(&ib).is_none() }));
    let _ = std::fs::remove_dir_all(dir);
}
