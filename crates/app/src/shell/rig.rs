//! A real shell for the app's tests: one window on a workspace of `/tmp`,
//! the process's state with the agents a test names, and the tasks written
//! into a folder of the test's own.

use crate::state::Shared;
use crate::task::Tasks;
use gpui::{BorrowAppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::config::AgentSpec;
use onehand_core::config::drive::SettingsDrive;
use onehand_core::workspace::Workspace;
use std::path::PathBuf;

/// A folder of the test's own, emptied.
pub(crate) fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-shell-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    cx.update(|cx| {
        let mut shared = Shared::for_test();
        shared.settings = SettingsDrive::new(agents, Default::default());
        cx.set_global(shared);
        cx.set_global(Tasks::in_dir(store));
    });
    let mut made = None;
    let window = cx.add_window(|window, cx| {
        let ws = Workspace::seeded("/tmp");
        let shell = gpui::AppContext::new(cx, |cx| crate::shell::Shell::new(ws, window, cx));
        made = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let handle: gpui::AnyWindowHandle = window.into();
    let shell = made.expect("the shell");
    let weak = shell.downgrade();
    cx.update(|cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.windows.push(crate::state::OpenWindow {
                storage_dir: None,
                handle,
                shell: weak,
            })
        })
    });
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    (handle, shell, cx)
}

/// Bind `shell`'s workspace to `dir`, where it keeps its issues.
pub(crate) fn keep_in(
    shell: &Entity<crate::shell::Shell>,
    dir: PathBuf,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let fx = shell.window.workspace.bind(dir);
            shell.carry_workspace(fx, window, cx);
        })
    });
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
    let window = cx.cx.add_window(|window, cx| {
        let ws = Workspace::seeded(root);
        let shell = gpui::AppContext::new(cx, |cx| crate::shell::Shell::new(ws, window, cx));
        made = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let handle: gpui::AnyWindowHandle = window.into();
    let shell = made.expect("the shell");
    let weak = shell.downgrade();
    cx.cx.update(|cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.windows.push(crate::state::OpenWindow {
                storage_dir: None,
                handle,
                shell: weak,
            })
        })
    });
    (handle, shell)
}
