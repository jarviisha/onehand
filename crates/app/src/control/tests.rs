//! The tools' carrier against its contract: a switch's change reaches each
//! connection once and lets go of the ones gone, what it carries runs after
//! the call that asked, an open's deadline comes round and frees its request,
//! and a token let go leaves nothing listening.

use super::grant as grant_tools;
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
        back: None,
    };
    let projects = vec![onehand_core::control::Named::new("repo", "/repo")];
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
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let project = onehand_core::project::Project::new(
        onehand_core::project::ProjectId::mint(910_001),
        None,
        "/tmp".into(),
    );
    rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    let file = onehand_core::project::file::issues_file(&project.id);
    rig::show_workspace(&shell, cx);
    // An Issues view open on the project, which no rescan will reach.
    let view =
        cx.update(|_, cx| onehand_issues::testing::OpenView::new(Path::new("/tmp"), &file, cx));
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
    assert_eq!(
        ask(Peer::Outside, call("Crash"), cx),
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
        .map(|issue| issue.title().to_string())
        .collect();
    assert_eq!(titles, ["Crash"]);
    assert_eq!(
        ask(Peer::Outside, call(" crash "), cx),
        Ok("Issue #1 is already open with that title".to_string())
    );
    let kept = onehand_core::issues::load_blocking(&file).unwrap();
    assert_eq!(kept.listed().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// Ask `call` of the tools as `peer`, and return its answer once everything
/// it started has landed.
fn ask(peer: Peer, call: Call, cx: &mut gpui::VisualTestContext) -> Result<String, String> {
    let (tx, rx) = oneshot::channel();
    cx.update(|_, cx| perform(peer, call, tx, cx));
    // Some answers wait on a write landing on a thread of its own and are
    // then sent from a foreground task: waiting keeps the test's executor
    // running, where a bare `block_on` would starve the task that answers.
    let answer = cx.foreground_executor().block_test(rx).expect("answered");
    cx.run_until_parked();
    answer
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
    // Named none, it takes *Work an issue*, which this rig's library lacks.
    assert_eq!(
        ask(Peer::Outside, start, cx),
        Err("no workflow with id builtin:issue; list_workflows gives them".to_string())
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
                shell.open_off_screen("/tmp".into(), agent, Vec::new(), cx)
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
    let front = cx.update(|_, cx| crate::windows::front(cx).map(|(handle, _)| handle));
    assert_eq!(front, Some(front_window), "the rig's window is in front");
    assert_ne!(front, Some(behind_window));
    let token = Token::mint();
    let (tx, rx) = oneshot::channel();
    let at = behind.clone();
    cx.update(|_, cx| {
        let (uid, _) = other
            .update(cx, |shell, cx| {
                shell.open_off_screen(at, agent, Vec::new(), cx)
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

/// A project's folders share its one issues file: an issue written through
/// the project's name is the project's, and the workspace page lists it once.
#[gpui::test]
fn a_projects_folders_share_one_issues_file(cx: &mut TestAppContext) {
    use crate::shell::rig;
    use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
    let dir = rig::folder("shared-issues");
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let (home, worktree) = (dir.join("p"), dir.join("p-x"));
    for folder in [&home, &worktree] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (home, worktree) = (
        std::fs::canonicalize(home).unwrap(),
        std::fs::canonicalize(worktree).unwrap(),
    );
    let mut project = Project::new(ProjectId::mint(910_002), None, home.clone());
    project.worktrees.push(Worktree {
        path: worktree.clone(),
        branch: "x".into(),
        from: None,
        made_by: MadeBy::Split,
        offload: None,
    });
    rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    for folder in [&home, &worktree] {
        let at = folder.clone();
        cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    }
    cx.run_until_parked();
    let file = onehand_core::project::file::issues_file(&project.id);
    let told = cx.update(|_, cx| shell.read(cx).issue_files(cx));
    let kept = onehand_core::workbench::KeptIssues {
        file: file.clone(),
        home: home.clone(),
    };
    assert_eq!(told.get(&home), Some(&kept));
    assert_eq!(
        told.get(&worktree),
        Some(&kept),
        "one file for both folders"
    );
    rig::show_workspace(&shell, cx);
    let call = Call::CreateIssue {
        project: Some("p".into()),
        title: "Crash".into(),
        body: String::new(),
    };
    assert_eq!(
        ask(Peer::Outside, call, cx),
        Ok("Wrote issue #1: Crash".to_string())
    );
    let kept = onehand_core::issues::load_blocking(&file).unwrap();
    assert_eq!(kept.listed().len(), 1);
    for _ in 0..100 {
        if !rig::workspace_issues(&shell, cx).is_empty() {
            break;
        }
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(rig::workspace_issues(&shell, cx), ["Crash"], "listed once");
    let _ = std::fs::remove_dir_all(dir);
}

/// `open_session` naming a worktree's folder starts there, on its project's
/// row, and `list_projects` names the project's worktrees.
#[gpui::test]
fn open_session_on_a_worktree_folder_starts_there(cx: &mut TestAppContext) {
    use crate::shell::rig;
    use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
    let dir = rig::folder("open-in-worktree");
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), vec![rig::nowhere()]);
    let (home, worktree) = (dir.join("p"), dir.join("p-x"));
    for folder in [&home, &worktree] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (home, worktree) = (
        std::fs::canonicalize(home).unwrap(),
        std::fs::canonicalize(worktree).unwrap(),
    );
    let mut project = Project::new(ProjectId::mint(910_005), None, home.clone());
    project.worktrees.push(Worktree {
        path: worktree.clone(),
        branch: "x".into(),
        from: None,
        made_by: MadeBy::Split,
        offload: None,
    });
    rig::keep_projects(handle, &shell, vec![project], cx);
    let at = home.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let listed = ask(Peer::Outside, Call::ListProjects, cx).expect("listed");
    assert!(listed.contains("— worktrees: x"), "{listed}");
    let call = Call::OpenSession {
        project: worktree.display().to_string(),
        agent: "nowhere".into(),
        prompt: None,
    };
    ask(Peer::Outside, call, cx).expect("opened");
    cx.update(|_, cx| {
        let workspace = &shell.read(cx).workspace_window().workspace;
        let row = workspace.local_index(&home).expect("the project's row");
        let session = workspace.roots[row].sessions.last().expect("a session");
        assert_eq!(session.folder, worktree);
        assert_eq!(workspace.local_index(&worktree), Some(row));
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// A row of no project keeps no issues: a tool writing one there is told so.
#[gpui::test]
fn a_row_of_no_project_keeps_no_issues(cx: &mut TestAppContext) {
    use crate::shell::rig;
    let dir = rig::folder("no-project-issues");
    let (_, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let told = cx.update(|_, cx| shell.read(cx).issue_files(cx));
    assert!(!told.contains_key(Path::new("/tmp")));
    let call = Call::CreateIssue {
        project: Some("tmp".into()),
        title: "Crash".into(),
        body: String::new(),
    };
    assert_eq!(
        ask(Peer::Outside, call, cx),
        Err("that folder's project keeps no issues here".to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A folder no row of the window holds, a worktree its project records,
/// keeps its project's issues: what a worker's session there writes into.
#[gpui::test]
fn a_recorded_worktree_with_no_row_keeps_its_projects_issues(cx: &mut TestAppContext) {
    use crate::shell::rig;
    use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
    let dir = rig::folder("worktree-no-row");
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let (home, worktree) = (dir.join("p"), dir.join("p-run"));
    let mut project = Project::new(ProjectId::mint(910_003), None, home.clone());
    project.worktrees.push(Worktree {
        path: worktree.clone(),
        branch: "run".into(),
        from: None,
        made_by: MadeBy::Run,
        offload: None,
    });
    rig::keep_projects(handle, &shell, vec![project.clone()], cx);
    let kept = cx.update(|_, cx| shell.read(cx).issues_file(&worktree, cx));
    let kept = kept.expect("the project's file");
    assert_eq!(
        kept.file,
        onehand_core::project::file::issues_file(&project.id)
    );
    assert_eq!(kept.home, home);
    let _ = std::fs::remove_dir_all(dir);
}

/// A review lane's session is granted `review_verdict` before its agent
/// starts: the session holds its token from the moment it opens, and the
/// first `tools/list` its agent sends already has it.
#[gpui::test]
fn a_review_lane_lists_review_verdict_at_its_first_listing(cx: &mut TestAppContext) {
    let dir = crate::shell::rig::folder("review-listing");
    let spec = crate::shell::rig::nowhere();
    let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), vec![spec]);
    let grant = onehand_core::task::Grant {
        tools: vec!["list_issues".into(), "review_verdict".into()],
        labels: Vec::new(),
    };
    let tokens = cx.update(|_, cx| {
        // A uid no other test's session has: the tools' drive is the
        // process's, shared by tests running beside this one.
        cx.update_global::<Shared, _>(|shared, _| while shared.next_uid() < 900_140 {});
        shell.update(cx, |shell, cx| {
            let (uid, _) =
                crate::shell::open_step_session(shell, "/tmp".into(), "nowhere", &grant, cx)
                    .expect("opened");
            drive().tokens_of(uid)
        })
    });
    let [token] = tokens.as_slice() else {
        panic!("{tokens:?}");
    };
    let mut conn = control::Conn::default();
    let line = |method: &str, params: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
            .to_string()
    };
    let hello = line(
        "initialize",
        serde_json::json!({"_meta": {"onehand": {"session": token}}}),
    );
    control::handle(&mut conn, &hello, standing);
    let control::Handled::Reply(listed) = control::handle(
        &mut conn,
        &line("tools/list", serde_json::json!({})),
        standing,
    ) else {
        panic!("not answered");
    };
    assert!(listed.contains("\"review_verdict\""), "{listed}");
    assert!(listed.contains("\"list_issues\""), "{listed}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A session granted its tools before it held its token, as a warm agent's
/// is, tells that token's connections once it claims it: the agent may have
/// listed them while they were not its own yet.
#[gpui::test]
fn a_claim_tells_a_granted_sessions_connections(cx: &mut TestAppContext) {
    let uid = 900_131;
    let grant = onehand_core::task::Grant {
        tools: vec!["review_verdict".into()],
        labels: Vec::new(),
    };
    cx.update(|cx| grant_tools(uid, &grant, cx));
    let token = Token::mint();
    let (told, mut heard) = futures::channel::mpsc::unbounded();
    (front().joined)(&token_of(&token), told);
    token.claim(uid);
    let note = heard.try_recv().expect("told");
    assert!(note.contains("list_changed"), "{note}");
    // One claimed with nothing given is told nothing.
    let bare = Token::mint();
    let (told, mut heard) = futures::channel::mpsc::unbounded();
    (front().joined)(&token_of(&bare), told);
    bare.claim(900_132);
    assert!(heard.try_recv().is_err());
    cx.update(|cx| forget(uid, cx));
}
