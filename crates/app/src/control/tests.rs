//! The tools' carrier against its contract: a switch's change reaches each
//! connection once and lets go of the ones gone, what it carries runs after
//! the call that asked, an open's deadline comes round and frees its request,
//! and a token let go leaves nothing listening.

use super::*;
use futures::StreamExt as _;
use gpui::TestAppContext;
use onehand_core::control::drive::{Begun, Open};
use std::time::Duration;

fn token_of(token: &Token) -> String {
    token.0.clone()
}

/// A session's switch tells each of its connections once, after the call
/// that switched it; switching to where it already is tells nobody; a token
/// let go is refused and keeps no connection.
#[gpui::test]
fn a_switch_tells_each_connection_once(cx: &mut TestAppContext) {
    let uid = 900_101;
    let token = Token::mint();
    token.claim(uid);
    let name = token_of(&token);
    let (told, mut heard) = futures::channel::mpsc::unbounded();
    let (gone, _) = futures::channel::mpsc::unbounded::<String>();
    (front().joined)(&name, told);
    (front().joined)(&name, gone);
    let early = cx.update(|cx| {
        switch(uid, true, cx);
        heard.try_recv().is_ok()
    });
    assert!(!early, "carried after the call, not inside it");
    cx.run_until_parked();
    let note = futures::executor::block_on(heard.next()).unwrap();
    assert!(note.contains("list_changed"), "{note}");
    assert_eq!(conns()[&name].len(), 1, "the one gone is let go");
    cx.update(|cx| switch(uid, true, cx));
    cx.run_until_parked();
    assert!(heard.try_recv().is_err());
    assert!(matches!(
        standing(&Peer::Session(name.clone())),
        Standing::On
    ));
    cx.update(|cx| forget(uid, cx));
    drop(token);
    assert!(matches!(
        standing(&Peer::Session(name.clone())),
        Standing::Refused(_)
    ));
    assert!(!conns().contains_key(&name));
}

/// An open's deadline, armed when it is asked, comes round on the clock and
/// fails the open, so the same request opens afresh.
#[gpui::test]
fn an_opens_deadline_comes_round_and_frees_its_request(cx: &mut TestAppContext) {
    let lead = 900_102;
    drive().switch(lead, true);
    let open = || Open {
        lead,
        project: "repo".into(),
        agent: "agent".into(),
        prompt: None,
        dirs: Vec::new(),
        request: "r".into(),
        turn: None,
        model: None,
        name: None,
    };
    let projects = vec![("repo".to_string(), PathBuf::from("/repo"))];
    let Ok(Begun::Resolve { fx, .. }) = drive().open_worker(open(), &projects, None) else {
        panic!("a new request");
    };
    let [Effect::Arm { token, .. }] = fx[..] else {
        panic!("the deadline is armed at the call");
    };
    assert!(matches!(
        drive().open_worker(open(), &projects, None),
        Ok(Begun::Said(_))
    ));
    let wait = Duration::from_secs(1);
    cx.update(|cx| carry(vec![Effect::Arm { token, wait }], cx));
    cx.executor().advance_clock(wait);
    cx.run_until_parked();
    assert!(matches!(
        drive().open_worker(open(), &projects, None),
        Ok(Begun::Resolve { .. })
    ));
    let fx = drive().forget(lead);
    assert!(!fx.is_empty());
}

/// `create_issue` writes its issue once, through the file's drive, and
/// answers so; the same title again, while that issue is open, is answered
/// as already open and written nowhere.
#[gpui::test]
fn a_tools_issue_is_written_once_and_not_again_while_open(cx: &mut TestAppContext) {
    use crate::shell::rig;
    let dir = rig::folder("tool-issue");
    let (_, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let storage = dir.join("storage");
    rig::keep_in(&shell, storage.clone(), cx);
    rig::show_workspace(&shell, cx);
    // An Issues view open on the project, which no rescan will reach.
    let view =
        cx.update(|_, cx| onehand_issues::testing::OpenView::new(&storage, Path::new("/tmp"), cx));
    // Its first read lands before anything is written.
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let call = |title: &str| Call::CreateIssue {
        project: Some("tmp".into()),
        title: title.into(),
        body: "on open".into(),
    };
    let ask = |call: Call, cx: &mut gpui::VisualTestContext| {
        let (answer, heard) = oneshot::channel();
        cx.update(|_, cx| perform(Peer::Outside, call, answer, cx));
        for _ in 0..100 {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
        }
        futures::executor::block_on(heard).unwrap()
    };
    assert_eq!(
        ask(call("Crash"), cx),
        Ok("Wrote issue #1: Crash".to_string())
    );
    // The workspace page, which reads the file itself, is told.
    for _ in 0..100 {
        if !rig::workspace_issues(&shell, cx).is_empty() {
            break;
        }
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig::workspace_issues(&shell, cx), ["Crash"]);
    let shown = cx.update(|_, cx| view.shown(cx)).unwrap();
    let titles: Vec<_> = shown
        .listed()
        .iter()
        .map(|issue| issue.title.clone())
        .collect();
    assert_eq!(titles, ["Crash"]);
    assert_eq!(
        ask(call(" crash "), cx),
        Ok("Issue #1 is already open with that title".to_string())
    );
    let file = onehand_core::issues::file_for(&storage, Path::new("/tmp"));
    let kept = onehand_core::issues::load_blocking(&file).unwrap();
    assert_eq!(kept.listed().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// Ask `call` of the tools as `peer`, and read the answer once everything
/// it started has landed.
fn ask(peer: Peer, call: Call, cx: &mut gpui::VisualTestContext) -> Result<String, String> {
    let (tx, rx) = oneshot::channel();
    cx.update(|_, cx| perform(peer, call, tx, cx));
    cx.run_until_parked();
    futures::executor::block_on(rx).expect("answered")
}

/// An outside program's call lands in the window in front, and is refused
/// what only a session may ask, or what it names too little for.
#[gpui::test]
fn an_outside_call_lands_in_the_window_in_front(cx: &mut TestAppContext) {
    let dir = crate::shell::rig::folder("call-front");
    let (_, _, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), Vec::new());
    let listed = ask(Peer::Outside, Call::ListProjects, cx).expect("listed");
    assert!(listed.contains("/tmp"), "{listed}");
    assert_eq!(
        ask(Peer::Outside, Call::ListIssues { project: None }, cx),
        Err("name a project".to_string())
    );
    let start = Call::StartIssue {
        project: "tmp".into(),
        issue: 1,
        workflow: None,
    };
    assert_eq!(
        ask(Peer::Outside, start, cx),
        Err("name a workflow; list_workflows gives them".to_string())
    );
    let worker = Call::OpenWorker {
        project: "tmp".into(),
        agent: "a".into(),
        prompt: None,
        dirs: Vec::new(),
        request: "r".into(),
        turn: None,
        model: None,
        name: None,
    };
    assert_eq!(
        ask(Peer::Outside, worker, cx),
        Err("`open_worker` is for a session's agent".to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A session's call lands in the window holding it; a token no session
/// holds is refused.
#[gpui::test]
fn a_sessions_call_lands_in_its_own_window(cx: &mut TestAppContext) {
    let dir = crate::shell::rig::folder("call-own");
    let agent = onehand_core::config::AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    };
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![agent.clone()]);
    let token = Token::mint();
    let name = token_of(&token);
    let (tx, rx) = oneshot::channel();
    cx.update(|_, cx| {
        let (uid, _) = shell
            .update(cx, |shell, cx| {
                shell.run_unattended("/tmp".into(), agent, Vec::new(), cx)
            })
            .expect("a session");
        token.claim(uid);
        perform(Peer::Session(name.clone()), Call::ListProjects, tx, cx);
    });
    let listed = futures::executor::block_on(rx).expect("answered");
    assert!(listed.expect("listed").contains("/tmp"));
    assert_eq!(
        ask(Peer::Session("nobody".into()), Call::ListProjects, cx),
        Err("no such session".to_string())
    );
    drop(token);
    let _ = std::fs::remove_dir_all(dir);
}

/// A session's call lands in the window holding it even when another
/// window is in front, where an outside program's lands.
#[gpui::test]
fn a_sessions_call_lands_in_its_window_not_the_one_in_front(cx: &mut TestAppContext) {
    let dir = crate::shell::rig::folder("call-two-windows");
    let agent = onehand_core::config::AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    };
    let (front_window, _, cx) =
        crate::shell::rig::shell(cx, dir.join("tasks"), vec![agent.clone()]);
    let behind = dir.join("behind");
    std::fs::create_dir_all(&behind).unwrap();
    let (behind_window, other) = crate::shell::rig::another(cx, &behind);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let front = cx.update(|_, cx| crate::state::front(cx).map(|(handle, _)| handle));
    assert_eq!(front, Some(front_window), "the rig's window is in front");
    assert_ne!(front, Some(behind_window));
    let token = Token::mint();
    let (tx, rx) = oneshot::channel();
    let at = behind.clone();
    cx.update(|_, cx| {
        let (uid, _) = other
            .update(cx, |shell, cx| {
                shell.run_unattended(at, agent, Vec::new(), cx)
            })
            .expect("a session");
        token.claim(uid);
        perform(Peer::Session(token_of(&token)), Call::ListProjects, tx, cx);
    });
    let theirs = futures::executor::block_on(rx)
        .expect("answered")
        .expect("listed");
    let shown = behind.display().to_string();
    assert!(theirs.contains(&shown), "{theirs}");
    assert!(
        !theirs.contains("/tmp —"),
        "not the front window's: {theirs}"
    );
    let outside = ask(Peer::Outside, Call::ListProjects, cx).expect("listed");
    assert!(outside.contains("/tmp"), "{outside}");
    assert!(!outside.contains(&shown), "{outside}");
    drop(token);
    let _ = std::fs::remove_dir_all(dir);
}
