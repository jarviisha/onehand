//! What the Issues page's starts come to on a real shell when they find
//! nothing to start on, a conversation asked for again, and *Run check* on a
//! project with no check command.

use crate::shell::rig::{folder, shell};
use gpui::{TestAppContext, VisualTestContext};
use onehand_issues::IssuesEvent;
use std::path::PathBuf;

/// How many notifications the shell's window has put up.
fn notes(cx: &mut VisualTestContext) -> usize {
    use gpui_component::WindowExt as _;
    cx.update(|window, cx| window.notifications(cx).len())
}

/// Tell the shell `event`, as an Issues view does.
fn tell(shell: &gpui::Entity<super::Shell>, event: IssuesEvent, cx: &mut VisualTestContext) {
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.on_issues_event(event, window, cx)));
    cx.run_until_parked();
}

/// *Run workflow…* on a project no longer in the workspace opens no picker,
/// and says nothing.
#[gpui::test]
fn a_run_on_a_project_gone_opens_nothing(cx: &mut TestAppContext) {
    let dir = folder("issues-gone");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("gone");
    tell(
        &shell,
        IssuesEvent::RunIssueWorkflow { root, number: 1 },
        cx,
    );
    assert!(cx.update(|_, cx| shell.read(cx).issue_picker.is_none()));
    assert_eq!(notes(cx), 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// *Open session* of a task with no session open says so, once.
#[gpui::test]
fn a_tasks_session_not_open_is_said(cx: &mut TestAppContext) {
    let dir = folder("issues-no-session");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    tell(&shell, IssuesEvent::OpenTaskSession("t1".into()), cx);
    assert_eq!(notes(cx), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// *Run check* on a project with no check command says so, once.
#[gpui::test]
fn a_run_check_with_no_command_says_so(cx: &mut TestAppContext) {
    let dir = folder("issues-no-check");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.run_check(PathBuf::from("/tmp"), window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(notes(cx), 1);
    let _ = std::fs::remove_dir_all(dir);
}

fn saved(root: PathBuf, agent: &str) -> Option<onehand_core::workspace::drive::Saved> {
    Some(onehand_core::workspace::drive::Saved {
        there: root.is_dir(),
        root,
        agent: agent.into(),
        resume: PathBuf::from("/nowhere/conversation"),
    })
}

fn agent() -> onehand_core::config::AgentSpec {
    onehand_core::config::AgentSpec {
        name: "a".into(),
        command: "true".into(),
        ..Default::default()
    }
}

/// A saved conversation whose agent is no longer set up is said once, and
/// its folder is not added back.
#[gpui::test]
fn a_conversation_whose_agent_is_gone_is_said(cx: &mut TestAppContext) {
    let dir = folder("conversation-agent-gone");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![agent()]);
    let root = dir.join("run");
    std::fs::create_dir_all(&root).unwrap();
    let found = saved(root.clone(), "gone");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.conversation_read(found, window, cx))
    });
    cx.run_until_parked();
    assert_eq!(notes(cx), 1);
    assert!(!cx.update(|_, cx| shell.read(cx).holds_root(&root)));
    let _ = std::fs::remove_dir_all(dir);
}

/// One whose agent is set up keeps its folder again, selects it and adds
/// one session resuming it.
#[gpui::test]
fn a_conversation_resumes_on_its_folder_kept_again(cx: &mut TestAppContext) {
    let dir = folder("conversation-kept");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![agent()]);
    let root = dir.join("run");
    std::fs::create_dir_all(&root).unwrap();
    let found = saved(root.clone(), "a");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.conversation_read(found, window, cx))
    });
    cx.run_until_parked();
    let (sessions, active) = cx.update(|_, cx| {
        let ws = &shell.read(cx).window.workspace;
        let idx = ws.local_index(&root).expect("kept again");
        (ws.roots[idx].sessions.len(), ws.active_root == idx)
    });
    assert_eq!(sessions, 1);
    assert!(active, "and selected");
    let _ = std::fs::remove_dir_all(dir);
}
