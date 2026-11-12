//! Retrying a task on a real shell: a check runs again at once, asking
//! nobody, and its new run asks for its place like any start.

use crate::shell::rig::{folder, shell};
use gpui::TestAppContext;
use onehand_core::task::Task;
use onehand_core::workflow::Setup;
use std::time::Duration;

/// Let the work off the UI thread land, and its answers come back, until
/// `done`.
fn settle(cx: &mut gpui::VisualTestContext, done: impl Fn(&mut gpui::VisualTestContext) -> bool) {
    for _ in 0..200 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never settled");
}

/// Whether each of task `id`'s runs has ended, oldest first.
fn ended(id: &str, cx: &mut gpui::VisualTestContext) -> Vec<bool> {
    cx.update(|_, cx| crate::task::task(id, cx))
        .map(|task| {
            task.runs()
                .iter()
                .map(|run| run.outcome.is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// A check that ran is retried at once, with no question, and runs again.
#[gpui::test]
fn a_checks_retry_runs_it_again_at_once(cx: &mut TestAppContext) {
    let dir = folder("retry-check");
    let (window, shell, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let setup = Setup {
        repo: root.clone(),
        dir: root,
        branch: None,
        agent: None,
        check: Some("true".into()),
        mode: None,
        forge: None,
    };
    let task = Task::check("c1".into(), "true".into(), setup);
    cx.update(|_, cx| crate::task::submit(task, window, cx));
    settle(cx, |cx| ended("c1", cx) == [true]);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.begin_retry("c1".into(), window, cx))
    });
    settle(cx, |cx| ended("c1", cx) == [true, true]);
    let _ = std::fs::remove_dir_all(dir);
}
