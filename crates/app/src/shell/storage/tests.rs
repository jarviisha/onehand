//! The recents' carrier on a real shell, its drive writing into a folder of
//! the test's own.

use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, TestAppContext, VisualTestContext};
use onehand_core::config::recents::RecentsDrive;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Write the recents to `path` from now on.
fn recents_at(path: PathBuf, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents = RecentsDrive::new(Default::default(), path, Vec::new())
        })
    });
}

/// The recents file's text once `done` holds of it.
fn file_once(path: &Path, cx: &mut VisualTestContext, done: impl Fn(&str) -> bool) -> String {
    for _ in 0..200 {
        cx.run_until_parked();
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if done(&text) {
            return text;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the recents never read as expected");
}

fn notes(cx: &mut VisualTestContext) -> usize {
    use gpui_component::WindowExt as _;
    cx.update(|window, cx| window.notifications(cx).len())
}

/// Deleting the workspace closes its window, takes it off the list and
/// removes its folder, and nothing written on the way out puts it back.
#[gpui::test]
fn deleting_removes_the_folder_after_the_window_closes(cx: &mut TestAppContext) {
    let dir = folder("delete");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    recents_at(dir.join("state.toml"), cx);
    let storage = cx.update(|_, cx| shell.read(cx).storage_dir().clone());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let fx = shell.window.workspace.save();
            shell.carry_workspace(fx, window, cx);
        })
    });
    file_once(&storage.join("onehand-workspace.toml"), cx, |text| {
        !text.is_empty()
    });
    cx.update(|_, cx| {
        let name = shell.read(cx).window.workspace.name.clone();
        let fx = cx
            .update_global::<Shared, _>(|shared, _| shared.recents.opened(storage.clone(), &name));
        super::carry_recents(fx, cx);
    });
    assert_eq!(notes(cx), 0);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.delete_workspace(window, cx)));
    // The window is gone: what is left is read from the app.
    let app = &mut cx.cx;
    // Once parked, the window has closed and every write on the way out is
    // queued; the removal is on the same ordered writer, so once that writer
    // has emptied, the folder has gone or never will.
    app.run_until_parked();
    let writer = app.update(|cx| Shared::global(cx).writer.clone());
    assert!(
        writer.flush(Duration::from_secs(10)),
        "the writes never landed"
    );
    let left: Vec<_> = std::fs::read_dir(&storage)
        .map(|dir| dir.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        !storage.exists(),
        "{storage:?} is still there, holding {left:?}"
    );
    let listed = app.update(|cx| Shared::global(cx).recents.listed().to_vec());
    assert!(listed.iter().all(|l| l.dir != storage));
    let _ = std::fs::remove_dir_all(dir);
}
