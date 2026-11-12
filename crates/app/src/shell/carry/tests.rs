//! The workspace drive's carrier against its contract: a burst's write goes
//! once, after its wait, through the one writer; a failed write reaches the
//! drive and is put back on screen; a session ended leaves nothing running;
//! a window closing writes the burst still waiting.

use super::super::Shell;
use crate::state::Shared;
use gpui::{Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::config::WorkspaceConfig;
use onehand_core::workspace::Workspace;
use onehand_core::workspace::drive::SAVE_DEBOUNCE;
use std::path::PathBuf;
use std::time::Duration;

fn shell(cx: &mut TestAppContext, ws: Workspace) -> (Entity<Shell>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    cx.update(|cx| cx.set_global(Shared::for_test()));
    let mut made = None;
    let window = cx.add_window(|window, cx| {
        let shell = gpui::AppContext::new(cx, |cx| Shell::new(ws, window, cx));
        made = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    (made.expect("the shell"), cx)
}

/// A workspace of `/tmp`, bound to `dir`.
fn bound(dir: PathBuf) -> Workspace {
    let mut ws = Workspace::seeded("/tmp");
    ws.storage_dir = Some(dir);
    ws
}

/// A folder of its own to write into.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-carry-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Let the writer's thread land what it was given, and its answer come back.
fn settle(cx: &mut VisualTestContext) {
    let writer = cx.update(|_, cx| Shared::global(cx).writer.clone());
    for _ in 0..10 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
}

fn rename(shell: &Entity<Shell>, name: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let fx = shell.window.workspace.rename(name);
            shell.carry_workspace(fx, window, cx);
        })
    });
}

/// A burst of names is one write, after the last one's wait, and it lands
/// in the file with the last name.
#[gpui::test]
fn a_burst_is_written_once_after_its_wait(cx: &mut TestAppContext) {
    let dir = folder("burst");
    let (shell, cx) = shell(cx, bound(dir.clone()));
    rename(&shell, "One", cx);
    rename(&shell, "Two", cx);
    settle(cx);
    assert!(
        !dir.join(WorkspaceConfig::FILE).exists(),
        "nothing before the wait"
    );
    cx.executor().advance_clock(SAVE_DEBOUNCE * 2);
    settle(cx);
    let written = std::fs::read_to_string(dir.join(WorkspaceConfig::FILE)).unwrap();
    assert!(
        written.contains("Two") && !written.contains("One"),
        "{written}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A write that fails reaches the drive, which puts the name back, on screen
/// and in its field.
#[gpui::test]
fn a_failed_write_puts_the_name_back(cx: &mut TestAppContext) {
    let file =
        std::env::temp_dir().join(format!("onehand-carry-unwritable-{}", std::process::id()));
    std::fs::write(&file, b"").unwrap();
    let (shell, cx) = shell(cx, bound(file.join("store")));
    rename(&shell, "Mine", cx);
    cx.executor().advance_clock(SAVE_DEBOUNCE * 2);
    settle(cx);
    let (name, field) = cx.update(|_, cx| {
        let shell = shell.read(cx);
        (
            shell.window.workspace.name.clone(),
            shell.workspace_name.read(cx).value().to_string(),
        )
    });
    assert_eq!((name.as_str(), field.as_str()), ("Workspace", "Workspace"));
    std::fs::remove_file(file).ok();
}

/// A window closing writes the burst still waiting, once, at once.
#[gpui::test]
fn a_window_closing_writes_the_waiting_burst(cx: &mut TestAppContext) {
    let dir = folder("flush");
    let (shell, cx) = shell(cx, bound(dir.clone()));
    rename(&shell, "Closing", cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.flush_workspace(cx)));
    let written = std::fs::read_to_string(dir.join(WorkspaceConfig::FILE)).unwrap();
    assert!(written.contains("Closing"), "{written}");
    std::fs::remove_file(dir.join(WorkspaceConfig::FILE)).unwrap();
    cx.executor().advance_clock(SAVE_DEBOUNCE * 2);
    settle(cx);
    assert!(
        !dir.join(WorkspaceConfig::FILE).exists(),
        "its wait writes nothing more"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A run's session ended lets its agent go, and the project it added goes
/// with it.
#[gpui::test]
fn an_ended_run_session_leaves_nothing_running(cx: &mut TestAppContext) {
    let (shell, cx) = shell(cx, Workspace::seeded("/tmp"));
    let spec = crate::shell::rig::nowhere();
    let (uid, session) = cx
        .update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.run_unattended(PathBuf::from("/tmp/onehand-run"), spec, Vec::new(), cx)
            })
        })
        .expect("a run's session");
    let weak = session.downgrade();
    drop(session);
    assert_eq!(
        shell.read_with(cx, |s, _| s.window.workspace.roots.len()),
        2
    );
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.end_session(uid, window, cx)));
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
    assert_eq!(
        shell.read_with(cx, |s, _| s.window.workspace.roots.len()),
        1
    );
}
