//! Working an issue by hand on a real shell, where it cannot be: a project
//! no longer in the workspace, and a workspace keeping no issues, each said
//! once and starting nothing.

use crate::shell::rig::{folder, shell};
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;
use std::path::Path;

/// The refusals the window said since the last ask.
fn said(shell: &gpui::Entity<super::Shell>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    cx.update(|_, cx| shell.update(cx, |shell, _| std::mem::take(&mut shell.refused)))
}

/// How many sessions the window's workspace holds, and how many notices
/// the window has put up.
fn standing(shell: &gpui::Entity<super::Shell>, cx: &mut VisualTestContext) -> (usize, usize) {
    cx.run_until_parked();
    let sessions = cx.update(|_, cx| {
        let shell = shell.read(cx);
        shell
            .window
            .workspace
            .roots
            .iter()
            .map(|root| root.sessions.len())
            .sum()
    });
    let notes = cx.update(|window, cx| window.notifications(cx).len());
    (sessions, notes)
}

fn work_here(root: &Path, shell: &gpui::Entity<super::Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.work_issue_here(root, 1, "work it", window, cx)
        })
    });
}

/// A project gone from the workspace starts nothing, and says so once.
#[gpui::test]
fn a_project_gone_starts_nothing(cx: &mut TestAppContext) {
    let dir = folder("work-here-gone");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let before = standing(&shell, cx);
    said(&shell, cx);
    work_here(&dir.join("gone"), &shell, cx);
    assert_eq!(standing(&shell, cx), (before.0, before.1 + 1));
    assert_eq!(
        said(&shell, cx),
        ["That project is no longer in this workspace"]
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A workspace keeping no issues starts nothing on its project, and says
/// so once.
#[gpui::test]
fn a_workspace_keeping_no_issues_starts_nothing(cx: &mut TestAppContext) {
    let dir = folder("work-here-unkept");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let before = standing(&shell, cx);
    said(&shell, cx);
    work_here(Path::new("/tmp"), &shell, cx);
    assert_eq!(standing(&shell, cx), (before.0, before.1 + 1));
    assert_eq!(
        said(&shell, cx),
        ["This workspace keeps no issues, so that issue cannot be noted"]
    );
    let _ = std::fs::remove_dir_all(dir);
}
