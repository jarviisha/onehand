//! A real shell for the app's tests: one window on a workspace of `/tmp`,
//! the process's state with the agents a test names, and the tasks written
//! into a folder of the test's own.

use crate::state::Shared;
use crate::task::Tasks;
use gpui::{Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::config::AgentSpec;
use onehand_core::config::drive::SettingsDrive;
use onehand_core::workspace::Workspace;
use std::path::PathBuf;

pub(crate) mod strain;

/// A folder of the test's own, emptied.
pub(crate) fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-shell-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An agent that cannot start: its session opens and its stream ends, and
/// nothing is spawned.
pub(crate) fn nowhere() -> AgentSpec {
    AgentSpec {
        name: "nowhere".into(),
        command: "/nonexistent/onehand-test-agent".into(),
        ..Default::default()
    }
}

/// A shell on a workspace of `/tmp`, the tasks writing into `store`, and
/// `agents` configured.
pub(crate) fn shell(
    cx: &mut TestAppContext,
    store: PathBuf,
    agents: Vec<AgentSpec>,
) -> (
    gpui::AnyWindowHandle,
    Entity<crate::shell::Shell>,
    &mut VisualTestContext,
) {
    shell_on(cx, store, agents, Workspace::seeded("/tmp"))
}

/// A shell on `ws`, the tasks writing into `store`, and `agents`
/// configured. The workspace is kept in a folder of this test's own beside
/// `store`, whatever `ws` named.
pub(crate) fn shell_on(
    cx: &mut TestAppContext,
    store: PathBuf,
    agents: Vec<AgentSpec>,
    ws: Workspace,
) -> (
    gpui::AnyWindowHandle,
    Entity<crate::shell::Shell>,
    &mut VisualTestContext,
) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    // A folder of this test's own: every rig's workspace is of `/tmp`, and
    // tests running side by side must not share one.
    let storage = store.with_extension("workspace");
    // Made, as a workspace's folder is by its first write, so its issues
    // have a workspace to be kept in.
    std::fs::create_dir_all(&storage).unwrap();
    cx.update(|cx| {
        let mut shared = Shared::for_test();
        shared.settings = SettingsDrive::new(agents, Default::default(), Vec::new());
        cx.set_global(shared);
        cx.set_global(Tasks::in_dir(store));
    });
    let mut made = None;
    let mut ws = ws;
    let window = cx.add_window(|window, cx| {
        ws.storage_dir = storage.clone();
        let shell = gpui::AppContext::new(cx, |cx| crate::shell::Shell::new(ws, window, cx));
        made = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let handle: gpui::AnyWindowHandle = window.into();
    let shell = made.expect("the shell");
    let weak = shell.downgrade();
    cx.update(|cx| crate::windows::register(storage, handle, weak, cx));
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    (handle, shell, cx)
}

/// The titles of the open issues `shell`'s workspace page lists.
pub(crate) fn workspace_issues(
    shell: &Entity<crate::shell::Shell>,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    cx.update(|_, cx| shell.read(cx).workspace_page.read(cx).open_titles())
}

/// Put `shell`'s workspace page on screen.
pub(crate) fn show_workspace(shell: &Entity<crate::shell::Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_workspace(window, cx)));
}

/// A second window beside the rig's, on a workspace of `root`, registered as
/// the rig's is.
pub(crate) fn another(
    cx: &mut VisualTestContext,
    root: &std::path::Path,
) -> (gpui::AnyWindowHandle, Entity<crate::shell::Shell>) {
    let mut made = None;
    let mut storage = PathBuf::new();
    let window = cx.cx.add_window(|window, cx| {
        let ws = Workspace::seeded(root);
        storage = ws.storage_dir.clone();
        let shell = gpui::AppContext::new(cx, |cx| crate::shell::Shell::new(ws, window, cx));
        made = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let handle: gpui::AnyWindowHandle = window.into();
    let shell = made.expect("the shell");
    let weak = shell.downgrade();
    cx.cx
        .update(|cx| crate::windows::register(storage, handle, weak, cx));
    (handle, shell)
}

/// Keep `projects` as every project of the process, each one's folder made
/// in the store, then place `shell`'s rows of them as a window opening does.
pub(crate) fn keep_projects(
    handle: gpui::AnyWindowHandle,
    shell: &Entity<crate::shell::Shell>,
    projects: Vec<onehand_core::project::Project>,
    cx: &mut VisualTestContext,
) {
    use gpui::BorrowAppContext as _;
    let store = onehand_core::project::file::store_dir();
    for project in &projects {
        std::fs::create_dir_all(store.join(project.id.as_str())).unwrap();
    }
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.projects =
                onehand_core::project::ProjectsDrive::new(projects.into_iter().collect(), store);
        });
        crate::projects::place_rows(handle, shell.clone(), cx);
    });
    cx.run_until_parked();
}

/// The recent conversations `shell`'s workspace page lists, by the row each
/// is listed under and its session id.
pub(crate) fn workspace_recent(
    shell: &Entity<crate::shell::Shell>,
    cx: &mut VisualTestContext,
) -> Vec<(PathBuf, String)> {
    cx.update(|_, cx| shell.read(cx).workspace_page.read(cx).recent_keys())
}

/// Run git in `dir`, failing the test when it fails.
pub(crate) fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git must be installed to run this test");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A repository at `<dir>/main` holding `sub/a.txt`, one commit on `main`,
/// and a linked worktree of it at `<dir>/wt` on branch `w`, both canonical.
pub(crate) fn repo_with_worktree(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let main = dir.join("main");
    std::fs::create_dir_all(main.join("sub")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.com"]);
    git(&main, &["config", "user.name", "T"]);
    std::fs::write(main.join("sub").join("a.txt"), "x").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "one"]);
    let wt = dir.join("wt");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "w", wt.to_str().unwrap()],
    );
    (
        std::fs::canonicalize(main).unwrap(),
        std::fs::canonicalize(wt).unwrap(),
    )
}

/// The next session started here takes its events from the returned
/// channel.
pub(crate) fn canned(
    cx: &mut VisualTestContext,
) -> futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent> {
    use gpui::BorrowAppContext as _;
    let (tx, rx) = futures::channel::mpsc::channel(64);
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared
                .canned
                .push_back(onehand_io::acp::Events::from_channel(rx))
        })
    });
    tx
}

/// Open a session of `agent` on `dir` and put it on screen, its agent the
/// canned one handed back, already up.
pub(crate) fn shown_session(
    shell: &Entity<crate::shell::Shell>,
    dir: PathBuf,
    agent: AgentSpec,
    cx: &mut VisualTestContext,
) -> (
    u64,
    futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent>,
    tokio::sync::mpsc::UnboundedReceiver<onehand_core::acp::AcpRequest>,
) {
    let mut events = canned(cx);
    let uid = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let (uid, _) = shell
                .open_off_screen(dir, agent, Vec::new(), cx)
                .expect("a session");
            shell.show_session(uid, window, cx);
            uid
        })
    });
    let requests = crate::chat::session::tests::carrier::up(&mut events);
    cx.run_until_parked();
    (uid, events, requests)
}

/// Step the conversation pane's zoom in `steps` times, as its key does.
pub(crate) fn zoom_pane(
    shell: &Entity<crate::shell::Shell>,
    steps: usize,
    cx: &mut VisualTestContext,
) {
    cx.update(|_, cx| {
        let pane = shell.read(cx).chat.clone();
        pane.update(cx, |pane, cx| {
            for _ in 0..steps {
                pane.zoom_mut().zoom_in();
            }
            cx.notify();
        })
    });
    cx.run_until_parked();
}

/// A conversation as a build kept it: `sid`, ran in `folder`, of `project`,
/// titled `title` or untitled.
pub(crate) fn kept_conversation(
    sid: &str,
    folder: &std::path::Path,
    project: &onehand_core::project::ProjectId,
    title: Option<&str>,
) {
    let dir = onehand_core::chat::conversations_dir().join(sid);
    std::fs::create_dir_all(&dir).unwrap();
    let meta = serde_json::json!({
        "format": 2,
        "session_id": sid,
        "root": folder.display().to_string(),
        "project": project.as_str(),
        "agent": "Claude",
        "created": 1,
        "updated": 2,
        "items": 1,
        "title": title,
        "preview": "hello",
        "prefs": {},
    });
    std::fs::write(dir.join("meta.json"), meta.to_string()).unwrap();
}
