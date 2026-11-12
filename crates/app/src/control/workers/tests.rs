//! A worker's open carried through a window, on a real shell, each agent
//! played over a channel: its session starts in the lead's window, is
//! watched, gets its first prompt once its agent is up, and the lead is
//! answered once; a window closed before the start, its lead with it, is
//! answered so, and leaves nothing of the worker.

use super::super::{Token, drive, perform};
use crate::chat::session::tests::carrier::up;
use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use futures::channel::{mpsc, oneshot};
use gpui::{BorrowAppContext as _, TestAppContext, VisualTestContext};
use onehand_core::acp::{AcpEvent, AcpRequest};
use onehand_core::config::AgentSpec;
use onehand_core::control::{Call, Peer};
use std::time::Duration;

fn agent() -> AgentSpec {
    AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    }
}

/// The next session started here takes its events from the returned
/// channel.
fn canned(cx: &mut VisualTestContext) -> mpsc::Sender<AcpEvent> {
    let (tx, rx) = mpsc::channel(64);
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared
                .canned
                .push(onehand_io::acp::Events::from_channel(rx))
        })
    });
    tx
}

/// A lead session on `/tmp`, switched on, and a token that speaks for it.
/// Its uid is `from` or later: the tools' drive is the process's, shared by
/// every test running at once.
fn lead(
    shell: &gpui::Entity<crate::shell::Shell>,
    from: u64,
    cx: &mut VisualTestContext,
) -> (u64, Token) {
    cx.update(|_, cx| cx.update_global::<Shared, _>(|shared, _| while shared.next_uid() < from {}));
    let token = Token::mint();
    let uid = cx.update(|_, cx| {
        shell
            .update(cx, |shell, cx| {
                shell.run_unattended("/tmp".into(), agent(), Vec::new(), cx)
            })
            .expect("the lead")
            .0
    });
    token.claim(uid);
    let _ = drive().switch(uid, true);
    (uid, token)
}

fn open(project: &str, token: &Token) -> (Peer, Call) {
    let call = Call::OpenWorker {
        project: project.into(),
        agent: "a".into(),
        prompt: Some("go".into()),
        dirs: Vec::new(),
        request: format!("r-{project}"),
        turn: Some(1),
        model: None,
        name: None,
    };
    (Peer::Session(token.0.clone()), call)
}

/// Run the loop until `done`, letting what runs off the UI thread land.
fn until(cx: &mut VisualTestContext, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..300 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never came to pass");
}

#[gpui::test]
fn a_worker_opens_through_its_window_and_gets_its_prompt_once(cx: &mut TestAppContext) {
    let dir = folder("worker-open");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![agent()]);
    let _lead_events = canned(cx);
    let (lead, token) = lead(&shell, 910_100, cx);
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let at = project.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let mut worker_events = canned(cx);
    let (peer, call) = open("project", &token);
    let (tx, mut answer) = oneshot::channel();
    cx.update(|_, cx| cx.defer(move |cx| perform(peer, call, tx, cx)));
    // Its session starts in the lead's window, watched.
    let mut worker = None;
    until(cx, |_| {
        worker = super::super::workers_of(lead).first().map(|w| w.uid);
        worker.is_some()
    });
    let worker = worker.unwrap();
    let on = cx.update(|_, cx| shell.read(cx).session_project(worker));
    assert_eq!(on, Some(project.clone()), "in the lead's window");
    assert!(
        answer.try_recv().unwrap().is_none(),
        "not answered before it is up"
    );
    // Its agent comes up: the first prompt goes, once, and the lead hears.
    let mut requests = up(&mut worker_events);
    let mut said = None;
    until(cx, |_| {
        said = answer.try_recv().unwrap();
        said.is_some()
    });
    let said = said.unwrap().expect("opened");
    assert!(said.contains("turn: 1 accepted"), "{said}");
    assert_eq!(
        super::super::lead_of(worker),
        Some(lead),
        "the relation kept"
    );
    let mut prompts = 0;
    while let Ok(request) = requests.try_recv() {
        match request {
            AcpRequest::Prompt { text, .. } => {
                assert_eq!(text, "go");
                prompts += 1;
            }
            AcpRequest::Cancel
            | AcpRequest::PermissionResponse { .. }
            | AcpRequest::ElicitationResponse { .. }
            | AcpRequest::SetMode(_)
            | AcpRequest::SetConfigOption { .. }
            | AcpRequest::StopBackground { .. } => {}
        }
    }
    assert_eq!(prompts, 1);
    let _ = drive().forget(lead);
    drop(token);
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_window_closed_before_the_start_answers_so_and_leaves_nothing(cx: &mut TestAppContext) {
    let dir = folder("worker-closed");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![agent()]);
    let _lead_events = canned(cx);
    let (lead, token) = lead(&shell, 910_200, cx);
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let at = project.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let _worker_events = canned(cx);
    let (peer, call) = open("project", &token);
    let (tx, mut answer) = oneshot::channel();
    // Asked, then the window goes before the folder's read lands.
    cx.update(|window, cx| {
        perform(peer, call, tx, cx);
        window.remove_window();
    });
    drop(shell);
    let mut said = None;
    until(cx, |_| {
        said = answer.try_recv().unwrap();
        said.is_some()
    });
    let why = said.unwrap().expect_err("refused");
    // The lead was a session of that window, so it went with it.
    assert!(why.contains("the lead went"), "{why}");
    assert!(super::super::workers_of(lead).is_empty());
    // The window is gone: the app is reached without it.
    let canned_left = cx.cx.update(|cx| Shared::global(cx).canned.len());
    assert_eq!(canned_left, 1, "no worker session started");
    let _ = drive().forget(lead);
    drop(token);
    let _ = std::fs::remove_dir_all(dir);
}
