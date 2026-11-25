//! Two runs of one project at a step only one may be at, as the carriers
//! carry them: the second waits, said so on its row, until the first is
//! done and its room is given on; and a board, which pulls once room frees
//! and counts on its column's head what holds it.

use super::{app, begun, brief, kept, one_agent_step, placed, repo, temp_dir};
use crate::chat::session::ChatSession;
use crate::chat::session::tests::carrier;
use crate::task::start;
use gpui::{AnyWindowHandle, Entity, TestAppContext};
use onehand_core::acp::{AcpEvent, AcpRequest};
use onehand_core::chat::Chat;
use onehand_core::task::Task;
use onehand_core::workflow::{Outcome, Setup};
use std::path::{Path, PathBuf};

type Up = (
    Entity<ChatSession>,
    futures::channel::mpsc::Sender<AcpEvent>,
    tokio::sync::mpsc::UnboundedReceiver<AcpRequest>,
);

/// Task `id` of the project at `project`, in a folder of its own, whose one
/// step one run at a time may be at: started on session `uid`, its agent up.
fn started(
    id: &str,
    project: &Path,
    window: AnyWindowHandle,
    uid: u64,
    cx: &mut TestAppContext,
) -> Up {
    let mut template = one_agent_step();
    template.steps[0].limit = Some(1);
    let setup = Setup {
        repo: project.to_path_buf(),
        dir: repo(&format!("room-{id}")),
        branch: None,
        agent: None,
        check: None,
        mode: None,
        forge: None,
    };
    let id = placed(Task::new(id.into(), template, brief(), setup), window, cx);
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let begun = begun(&id, cx);
    cx.update(|cx| start(uid, &session, window, id, begun, cx));
    cx.run_until_parked();
    let requests = carrier::up(&mut tx);
    cx.run_until_parked();
    (session, tx, requests)
}

fn prompts(requests: &mut tokio::sync::mpsc::UnboundedReceiver<AcpRequest>) -> usize {
    std::iter::from_fn(|| requests.try_recv().ok())
        .filter(|r| matches!(r, AcpRequest::Prompt { .. }))
        .count()
}

/// The row of task `id` of the project at `project`, as the Tasks page has it.
fn at(id: &str, project: &Path, cx: &mut TestAppContext) -> String {
    let roots = [PathBuf::from(project)];
    cx.update(|cx| crate::task::rows(&roots, cx))
        .into_iter()
        .find(|row| row.id == id)
        .map(|row| row.at)
        .unwrap()
}

#[gpui::test]
fn a_waiting_run_is_said_to_wait_for_room(cx: &mut TestAppContext) {
    let (project, store) = (repo("room-project"), temp_dir("store"));
    let window = app(cx, &store);
    let (_s1, _tx1, mut first) = started("30", &project, window, 30, cx);
    let (_s2, _tx2, mut second) = started("31", &project, window, 31, cx);
    assert_eq!(prompts(&mut first), 1);
    assert_eq!(prompts(&mut second), 0, "the second waits, unprompted");
    assert_eq!(at("30", &project, cx), "Do");
    assert_eq!(at("31", &project, cx), "Waiting for room at Do");
    for id in ["30", "31"] {
        let _ = std::fs::remove_dir_all(kept(id, cx).setup().dir.clone());
    }
    let _ = std::fs::remove_dir_all(project);
    let _ = std::fs::remove_dir_all(store);
}

#[gpui::test]
fn room_given_reaches_the_waiting_run(cx: &mut TestAppContext) {
    let (project, store) = (repo("room-given"), temp_dir("store"));
    let window = app(cx, &store);
    let (_s1, mut tx1, mut first) = started("32", &project, window, 32, cx);
    let (_s2, _tx2, mut second) = started("33", &project, window, 33, cx);
    assert_eq!(prompts(&mut first), 1);
    assert_eq!(prompts(&mut second), 0);
    // The first is done: its room goes to the second, which is prompted.
    carrier::turn(&mut tx1);
    cx.run_until_parked();
    let writer = cx.update(|cx| cx.global::<crate::task::Tasks>().writer.clone());
    assert!(writer.flush(std::time::Duration::from_secs(5)));
    cx.run_until_parked();
    assert_eq!(kept("32", cx).outcome(), Some(Outcome::Done));
    assert_eq!(prompts(&mut second), 1);
    assert_eq!(at("33", &project, cx), "Do");
    for id in ["32", "33"] {
        let _ = std::fs::remove_dir_all(kept(id, cx).setup().dir.clone());
    }
    let _ = std::fs::remove_dir_all(project);
    let _ = std::fs::remove_dir_all(store);
}

/// A task the board did not start fills the board's first step, so the
/// board pulls nothing; once it leaves, the board pulls its To do.
#[gpui::test]
fn room_freed_by_any_task_makes_the_board_pull(cx: &mut TestAppContext) {
    use crate::shell::rig::{folder, shell};
    use crate::state::Shared;
    use gpui::BorrowAppContext as _;
    use onehand_core::project::{Board, Project, ProjectId, Projects, ProjectsDrive};
    let dir = folder("room-freed-pull");
    cx.executor().allow_parking();
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let project = Project::new(ProjectId::mint(78), None, root.clone());
    let id = project.id.clone();
    let file = onehand_core::project::file::issues_file(&id);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"next":2,"issues":[{"number":1,"title":"one"}]}"#).unwrap();
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                ProjectsDrive::new(Projects::from_iter([project]), dir.join("projects"));
            let _ = shared.projects.set_board(
                &id,
                Some(Board {
                    workflow: "builtin:branch".into(),
                    team: 2,
                    todo: vec![1],
                }),
            );
        })
    });
    // The shipped branch workflow, its plan one task at a time.
    let mut branch = onehand_core::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    branch.steps[0].limit = Some(1);
    let plan = branch.steps[0].id.clone();
    cx.update(|_, cx| crate::workflow::offer(vec![branch], cx));
    // A task a person started holds the plan.
    let key = onehand_core::task::rooms::RoomKey {
        repo: root.clone(),
        workflow: "builtin:branch".into(),
        step: plan,
    };
    cx.update(|_, cx| {
        crate::task::with_drive(cx, |t| ((), t.ask_room("by-hand", key, 1)));
    });
    let at = root.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| crate::board::pull_now(&id, cx));
    cx.run_until_parked();
    let todo = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| {
            let projects = &Shared::global(cx).projects;
            projects
                .get(&id)
                .and_then(|p| p.board())
                .map(|b| b.todo.clone())
        })
    };
    assert_eq!(todo(cx), Some(vec![1]), "the plan has no room");

    // It leaves the plan.
    cx.update(|_, cx| {
        crate::task::with_drive(cx, |t| ((), t.leave_room("by-hand")));
    });
    cx.run_until_parked();
    assert_eq!(todo(cx), Some(Vec::new()), "the board pulled its To do");
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_limited_columns_head_says_how_much_of_its_room_is_held(cx: &mut TestAppContext) {
    let store = temp_dir("store");
    let _window = app(cx, &store);
    let root = PathBuf::from("/a-board");
    let mut template = one_agent_step();
    template.steps[0].limit = Some(1);
    let key = onehand_core::task::rooms::RoomKey {
        repo: root.clone(),
        workflow: template.id.clone(),
        step: "do".into(),
    };
    let project = onehand_core::project::ProjectId::mint(79);
    let heads = cx.update(|cx| {
        crate::task::with_drive(cx, |t| ((), t.ask_room("x", key, 1)));
        crate::task::board(&project, &root, &template, cx)
            .steps
            .into_iter()
            .map(|(label, head, _)| (label, head))
            .collect::<Vec<_>>()
    });
    assert_eq!(heads, [("Do".to_string(), "Do · 1 of 1".to_string())]);
    let _ = std::fs::remove_dir_all(store);
}
