//! What the Issues page's starts come to on a real shell when they find
//! nothing to start on, and *Run check* on a project with no check command.

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

/// *Open session* of a task with no session open shows nothing, and says
/// nothing.
#[gpui::test]
fn a_tasks_session_not_open_shows_nothing(cx: &mut TestAppContext) {
    let dir = folder("issues-no-session");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    tell(&shell, IssuesEvent::OpenTaskSession("t1".into()), cx);
    assert_eq!(notes(cx), 0);
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
