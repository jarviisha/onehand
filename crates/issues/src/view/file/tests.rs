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
    let file = file_of(&dir);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let draft = Draft {
        title: "one".into(),
        ..Draft::default()
    };
    issues::drive::write_blocking(&file, issues::drive::Change::Create(draft), 1).unwrap();
    (dir, project)
}

/// The issues file the project of a test's folder keeps.
fn file_of(dir: &Path) -> PathBuf {
    dir.join("storage").join("issues.json")
}

/// What a view is told: `project`, and the file its issues are kept in.
fn told(dir: &Path, project: &Path) -> std::collections::HashMap<PathBuf, PathBuf> {
    [(project.to_path_buf(), file_of(dir))]
        .into_iter()
        .collect()
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
        view.set_files(told(dir, project), cx);
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

/// Two folders of one project share its file: the same entity, so an issue
/// written through one shows through the other.
#[gpui::test]
fn two_folders_of_one_file_show_one_list(cx: &mut TestAppContext) {
    let (dir, project) = kept("two-folders");
    let worktree = dir.join("worktree");
    std::fs::create_dir_all(&worktree).unwrap();
    cx.executor().allow_parking();
    let view = cx
        .update(|cx| IssuesView::new(&[], Rc::new(|_, _, _| {}), Rc::new(|_, _, _| {}), false, cx));
    let (a, b) = view.update(cx, |view, cx| {
        let mut files = told(&dir, &project);
        files.insert(worktree.clone(), file_of(&dir));
        view.set_files(files, cx);
        view.set_root(&project, cx);
        let a = view.state_for(&project, cx).unwrap().file.clone();
        view.set_root(&worktree, cx);
        let b = view.state_for(&worktree, cx).unwrap().file.clone();
        (a, b)
    });
    assert_eq!(a.entity_id(), b.entity_id(), "one file, one entity");
}

/// A folder the view is told no file for keeps no issues.
#[gpui::test]
fn a_folder_told_no_file_keeps_nothing(cx: &mut TestAppContext) {
    let (_, project) = kept("no-file");
    cx.executor().allow_parking();
    let view = cx
        .update(|cx| IssuesView::new(&[], Rc::new(|_, _, _| {}), Rc::new(|_, _, _| {}), false, cx));
    let kept = view.update(cx, |view, cx| {
        view.set_files(Default::default(), cx);
        view.set_root(&project, cx);
        view.state_for(&project, cx).is_some()
    });
    assert!(!kept);
}
