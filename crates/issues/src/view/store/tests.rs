//! The shared issues file's carrier against its contract: one file for
//! every view of it, read once off the UI thread; a write lands on disk and
//! its answer reaches the drive.

use super::IssuesFile;
use gpui::TestAppContext;
use onehand_core::issues::{self, Draft};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

/// A file of its own holding one issue, in a folder no forge serves.
fn file(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "onehand-issues-carry-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("issues.json");
    let draft = Draft {
        title: "one".into(),
        ..Draft::default()
    };
    issues::update_blocking(&path, move |kept| kept.create(draft, 1)).unwrap();
    (dir, path)
}

/// Let the background read land.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait on this thread for `future`, which the writer's own thread answers.
fn wait<F: std::future::Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(50)),
        }
    }
}

#[gpui::test]
fn a_file_is_one_for_every_view_and_read_once(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("one");
    let a = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    let b = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    assert_eq!(a.entity_id(), b.entity_id());
    a.update(cx, |file, cx| file.load_if_stale(cx));
    b.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let listed = a.read_with(cx, |file, _| {
        file.issues.as_ref().map(|kept| kept.listed().len())
    });
    assert_eq!(listed, Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A change written lands on disk and its answer reaches the drive, which
/// shows what the file holds.
#[gpui::test]
fn a_write_lands_and_its_answer_reaches_the_drive(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("write");
    let file = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    file.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let (of, written) = file.update(cx, |file, _| file.write(|kept| kept.set_open(1, false, 2)));
    let kept = wait(written).ok().map(|(kept, _)| kept);
    assert!(kept.is_some(), "the write landed");
    file.update(cx, |file, cx| file.written(of, kept, cx));
    let open = file.read_with(cx, |file, _| {
        file.issues
            .as_ref()
            .and_then(|kept| kept.get(1))
            .map(|issue| issue.open)
    });
    assert_eq!(open, Some(false));
    assert!(!issues::load_blocking(&path).unwrap().get(1).unwrap().open);
    let _ = std::fs::remove_dir_all(&dir);
}
