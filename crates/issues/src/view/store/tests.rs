//! The shared issues file's carrier against its contract: one file for
//! every view of it, read once off the UI thread; a write lands on disk and
//! its answer reaches the drive.

use super::IssuesFile;
use gpui::TestAppContext;
use onehand_core::issues::drive::Signal;
use onehand_core::issues::{self, Draft};
use std::path::PathBuf;
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

/// A change written lands on disk once, its answer reaches the drive, which
/// shows what the file holds, and the change's maker hears it once.
#[gpui::test]
fn a_write_lands_once_and_its_maker_hears_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("write");
    let file = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    file.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let ears = heard.clone();
    let _said = cx.update(|cx| {
        cx.subscribe(&file, move |_, signal: &Signal, _| {
            ears.borrow_mut().push(signal.clone())
        })
    });
    let of = file.update(cx, |file, cx| {
        file.write(
            onehand_core::issues::drive::Change::SetOpen {
                number: 1,
                open: false,
            },
            cx,
        )
    });
    for _ in 0..50 {
        settle(cx);
        if !heard.borrow().is_empty() {
            break;
        }
    }
    assert_eq!(
        *heard.borrow(),
        [Signal::Wrote {
            of,
            done: Ok(Some(1))
        }]
    );
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
