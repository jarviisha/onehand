//! The shared issues file's carrier against its contract: one file for
//! every view of it, read once off the UI thread; a write lands on disk and
//! its answer reaches the drive.

use super::IssuesFile;
use gpui::TestAppContext;
use onehand_core::issues::drive::{Applied, Change, Signal};
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
    issues::drive::write_blocking(&path, Change::Create(draft), 1).unwrap();
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
            done: Ok(Applied::Named(1))
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

static FORGE: onehand_core::connector::fake::Fake = onehand_core::connector::fake::Fake::SERVING;
static CONNECTORS: [&dyn onehand_core::connector::Connector; 1] = [&FORGE];

/// How many times `path` has been written.
fn writes(path: &std::path::Path) -> u64 {
    issues::load_blocking(path).unwrap().revision()
}

/// The view's timer syncs the file of the project it is on every five
/// minutes, and not the file of a project it holds but is not on.
#[gpui::test]
fn the_view_syncs_its_projects_file_every_five_minutes(cx: &mut TestAppContext) {
    use super::super::IssuesView;
    cx.executor().allow_parking();
    let dir = std::env::temp_dir().join(format!("onehand-issues-clock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let storage = dir.join("storage");
    let (on, off) = (dir.join("on"), dir.join("off"));
    for folder in [&storage, &on, &off] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let view = cx.update(|cx| {
        IssuesView::new(
            &CONNECTORS,
            std::rc::Rc::new(|_, _, _| {}),
            std::rc::Rc::new(|_, _, _| {}),
            false,
            cx,
        )
    });
    let files = view.update(cx, |view, cx| {
        view.set_storage(Some(storage.clone()), cx);
        let mut files = Vec::new();
        for root in [&off, &on] {
            view.set_root(root, cx);
            files.push(view.state_for(root, cx).unwrap().file.clone());
        }
        files
    });
    for file in &files {
        file.update(cx, |file, cx| file.load_if_stale(cx));
    }
    settle(cx);
    // Kept in step as shown, not yet on disk: a read syncs nothing.
    for file in &files {
        file.update(cx, |file, cx| {
            file.show(&Change::SyncWith(Some(FORGE.name.into())), cx)
        });
    }
    let paths: Vec<_> = [&off, &on]
        .into_iter()
        .map(|root| issues::file_for(&storage, root))
        .collect();
    let before: Vec<_> = paths.iter().map(|path| writes(path)).collect();
    cx.executor().advance_clock(Duration::from_secs(240));
    settle(cx);
    let at_four: Vec<_> = paths.iter().map(|path| writes(path)).collect();
    assert_eq!(at_four, before);
    cx.executor().advance_clock(Duration::from_secs(60));
    settle(cx);
    let at_five: Vec<_> = paths.iter().map(|path| writes(path)).collect();
    assert_eq!(at_five, [before[0], before[1] + 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a writer outside every view heard, as it heard it.
type Heard = std::rc::Rc<std::cell::RefCell<Vec<Result<Applied, String>>>>;

fn ear(heard: &Heard) -> super::Answer {
    let heard = heard.clone();
    Box::new(move |done, _| heard.borrow_mut().push(done))
}

fn until_heard(heard: &Heard, cx: &mut TestAppContext) {
    for _ in 0..50 {
        settle(cx);
        if !heard.borrow().is_empty() {
            return;
        }
    }
    panic!("never heard");
}

/// A change made outside every view lands once, its maker hears what it
/// came to once, and the file every view shares shows it without a read.
#[gpui::test]
fn an_outside_write_lands_once_and_its_maker_hears_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("outside");
    let file = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    file.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let heard = Heard::default();
    let note = Change::Note {
        number: 1,
        text: "a run said".into(),
    };
    file.update(cx, |file, cx| file.write_outside(note, ear(&heard), cx));
    // Not inside the update that landed it.
    assert!(heard.borrow().is_empty());
    until_heard(&heard, cx);
    settle(cx);
    assert_eq!(*heard.borrow(), [Ok(Applied::Named(1))]);
    let said = file.read_with(cx, |file, _| {
        file.issues.as_ref().unwrap().get(1).unwrap().notes.len()
    });
    assert_eq!(said, 1);
    assert_eq!(
        issues::load_blocking(&path)
            .unwrap()
            .get(1)
            .unwrap()
            .notes
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A view's own write landing after an outside write was made is still the
/// newest of the views', so it still moves that view's selection.
#[gpui::test]
fn a_views_write_after_an_outside_one_is_still_the_newest(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("newest");
    let file = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    file.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let heard = Heard::default();
    let created = file.update(cx, |file, cx| {
        let of = file.write(
            Change::Create(Draft {
                title: "two".into(),
                ..Draft::default()
            }),
            cx,
        );
        let note = Change::Note {
            number: 1,
            text: "a run said".into(),
        };
        file.write_outside(note, ear(&heard), cx);
        of
    });
    until_heard(&heard, cx);
    assert!(file.read_with(cx, |file, _| file.newest(created)));
    let listed = issues::load_blocking(&path).unwrap().listed().len();
    assert_eq!(listed, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refused outside write answers why, once, and what is shown is what
/// is on disk.
#[gpui::test]
fn a_refused_outside_write_answers_why_once(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("refused");
    let file = cx.update(|cx| IssuesFile::get(&dir, path.clone(), &[], cx));
    file.update(cx, |file, cx| file.load_if_stale(cx));
    settle(cx);
    let heard = Heard::default();
    let elsewhere = Change::Note {
        number: 9,
        text: "nowhere".into(),
    };
    file.update(cx, |file, cx| {
        file.write_outside(elsewhere, ear(&heard), cx)
    });
    until_heard(&heard, cx);
    settle(cx);
    assert_eq!(heard.borrow().len(), 1);
    assert!(heard.borrow()[0].is_err());
    let shown = file.read_with(cx, |file, _| file.issues.clone());
    assert_eq!(shown, Some(issues::load_blocking(&path).unwrap()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An outside write on a file no view holds still lands and answers.
#[gpui::test]
fn an_outside_write_with_no_view_holding_its_file_answers(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, path) = file("unheld");
    let heard = Heard::default();
    let answer = ear(&heard);
    let create = Change::CreateUnlessOpen(Draft {
        title: "one".into(),
        ..Draft::default()
    });
    cx.update(|cx| super::super::write_outside(&dir, path.clone(), create, &[], answer, cx));
    until_heard(&heard, cx);
    assert_eq!(*heard.borrow(), [Ok(Applied::Already(1))]);
    let _ = std::fs::remove_dir_all(&dir);
}
