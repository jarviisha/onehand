//! Two runs of one project at a step only one may be at, as the carriers
//! carry them: the second waits, said so on its row, until the first is
//! done and its room is given on.

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
}
