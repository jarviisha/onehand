//! The Issues view's carrying of its drive's reads against its contract: a
//! read asked for runs off the UI thread and its answer reaches the drive
//! once, kept for the work it was about.

use super::super::IssuesView;
use gpui::TestAppContext;
use onehand_core::issues::IssueKey;
use onehand_core::task::Task;
use onehand_core::task::work::drive::LeftSeen;
use onehand_core::task::work::issue_work;
use onehand_core::workflow::{Brief, Setup, Template};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

#[gpui::test]
fn what_the_work_left_is_read_off_the_ui_thread_and_kept_for_its_work(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = std::env::temp_dir().join(format!("onehand-issues-reads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (storage, project) = (dir.join("storage"), dir.join("project"));
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let task = Task::new(
        "t".into(),
        Template::blank("Work"),
        Brief {
            title: "t".into(),
            body: String::new(),
            instructions: None,
        },
        Setup {
            repo: project.clone(),
            dir: project.clone(),
            branch: None,
            agent: None,
            check: None,
            mode: None,
            forge: None,
        },
    );
    let key = IssueKey {
        file: onehand_core::issues::file_for(&storage, &project),
        number: 1,
    };
    let work = issue_work(key, [(&task, None, None)]).unwrap();

    let view = cx
        .update(|cx| IssuesView::new(&[], Rc::new(|_, _, _| {}), Rc::new(|_, _, _| {}), true, cx));
    view.update(cx, |view, cx| {
        view.set_storage(Some(storage.clone()), cx);
        view.set_root(&project, cx);
        view.state_for(&project, cx).unwrap().selected = Some(1);
        view.set_works(std::slice::from_ref(&work), &[PathBuf::new()], cx);
        view.refresh_left(cx);
        // Asked, not yet read: the read is off the UI thread.
        assert!(view.work.reading());
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    view.read_with(cx, |view, _| {
        assert!(!view.work.reading());
        assert!(matches!(
            view.work.left_seen(&work.work),
            LeftSeen::Read(..)
        ));
    });
    let _ = std::fs::remove_dir_all(&dir);
}
