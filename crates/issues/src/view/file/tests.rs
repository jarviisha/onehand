//! Which landed write moves a view's selection: only the newest change's,
//! only while nothing else was picked since it was made.

use super::super::IssuesView;
use gpui::{Entity, TestAppContext};
use onehand_core::issues::drive::Change;
use onehand_core::issues::{self, Draft};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// A storage folder and a project whose issues file holds issue 1.
fn kept(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "onehand-issues-heard-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let (storage, project) = (dir.join("storage"), dir.join("project"));
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let file = issues::file_for(&storage, &project);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let draft = Draft {
        title: "one".into(),
        ..Draft::default()
    };
    issues::drive::write_blocking(&file, issues::drive::Change::Create(draft), 1).unwrap();
    (dir, project)
}

fn settle(cx: &mut TestAppContext) {
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn view(cx: &mut TestAppContext, dir: &Path, project: &Path) -> Entity<IssuesView> {
    cx.executor().allow_parking();
    let view = cx
        .update(|cx| IssuesView::new(&[], Rc::new(|_, _, _| {}), Rc::new(|_, _, _| {}), false, cx));
    view.update(cx, |view, cx| {
        view.set_storage(Some(dir.join("storage")), cx);
        view.set_root(project, cx);
        let file = view.state_for(project, cx).unwrap().file.clone();
        file.update(cx, |file, cx| file.load_if_stale(cx));
    });
    settle(cx);
    view
}

fn create(title: &str) -> Change {
    Change::Create(Draft {
        title: title.into(),
        ..Draft::default()
    })
}

fn selected(view: &Entity<IssuesView>, project: &Path, cx: &mut TestAppContext) -> Option<u64> {
    view.read_with(cx, |view, _| view.roots[project].selected)
}

#[gpui::test]
fn only_the_newest_write_moves_the_selection(cx: &mut TestAppContext) {
    let (dir, project) = kept("newest");
    let view = view(cx, &dir, &project);
    view.update(cx, |view, cx| {
        view.change(create("two"), cx);
        view.change(create("three"), cx);
    });
    settle(cx);
    assert_eq!(selected(&view, &project, cx), Some(3));
    view.read_with(cx, |view, _| assert!(!view.roots[&project].writing));
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn a_write_landing_after_another_pick_leaves_it(cx: &mut TestAppContext) {
    let (dir, project) = kept("picked");
    let view = view(cx, &dir, &project);
    view.update(cx, |view, cx| {
        view.change(create("two"), cx);
        // The person picks issue 1 before the write lands.
        view.state_mut().unwrap().selected = Some(1);
    });
    settle(cx);
    assert_eq!(selected(&view, &project, cx), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}
