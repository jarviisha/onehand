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
