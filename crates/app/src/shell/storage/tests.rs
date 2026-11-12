//! The recents' carrier on a real shell, its drive writing into a folder of
//! the test's own: a bind's write lands once, an unbind forgets in the file,
//! and only an unbind's failed forget is said.

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
            shared.recents = RecentsDrive::new(Default::default(), path)
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

/// Let the writer's answers land.
fn settle(cx: &mut VisualTestContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn a_bind_is_remembered_and_an_unbind_forgotten_in_the_file(cx: &mut TestAppContext) {
    let dir = folder("recents-file");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let path = dir.join("state.toml");
    recents_at(path.clone(), cx);
    let stored = dir.join("stored");
    std::fs::create_dir_all(&stored).unwrap();
    let at = stored.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.bind_storage(at, window, cx)));
    let shown = stored.display().to_string();
    file_once(&path, cx, |text| text.contains(&shown));
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.unbind_storage(window, cx)));
    file_once(&path, cx, |text| !text.contains(&shown));
    assert_eq!(notes(cx), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn only_an_unbinds_failed_forget_is_said(cx: &mut TestAppContext) {
    let dir = folder("recents-failed");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    // A file inside a file: no write reaches it.
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, "").unwrap();
    recents_at(blocker.join("state.toml"), cx);
    let stored = dir.join("stored");
    std::fs::create_dir_all(&stored).unwrap();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.bind_storage(stored, window, cx)));
    settle(cx);
    assert_eq!(notes(cx), 0, "a touch's failure is only logged");
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.unbind_storage(window, cx)));
    settle(cx);
    assert_eq!(notes(cx), 1, "an unbind's is said once");
    let _ = std::fs::remove_dir_all(dir);
}
