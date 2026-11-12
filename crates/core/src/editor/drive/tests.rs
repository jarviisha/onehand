use super::*;
use std::time::Duration;

const ROOT: &str = "/p";

fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

fn file() -> PathBuf {
    root().join("a.rs")
}

/// A drive with `a.rs` of `/p` open, read at `mtime`, and its tab's uid.
fn opened(mtime: Option<SystemTime>) -> (EditorDrive, u64) {
    let mut drive = EditorDrive::default();
    drive.set_root(&root(), &Disk::Local);
    let (took, effects) = drive.open(&root(), &file());
    assert!(took);
    assert!(matches!(effects.as_slice(), [Effect::Read { .. }]));
    let effects = drive.handle(Input::Read {
        root: root(),
        path: file(),
        read: Ok(("theirs".into(), mtime)),
        uid: 7,
    });
    let [Effect::Buffer { uid, text, .. }] = effects.as_slice() else {
        panic!("no buffer built");
    };
    assert_eq!(text, "theirs");
    assert_eq!(*uid, 7, "the tab takes the id it was handed");
    (drive, *uid)
}

fn saved(mtime: Option<SystemTime>) -> SaveOutcome {
    SaveOutcome::Saved {
        mtime,
        text: std::sync::Arc::new("mine".into()),
    }
}

fn write_mtime(effects: &[Effect]) -> Option<SystemTime> {
    let [Effect::Write { mtime, .. }] = effects else {
        panic!("not one write");
    };
    *mtime
}

#[test]
fn a_file_open_already_is_focused_never_read_again() {
    let (mut drive, _) = opened(None);
    let (took, effects) = drive.open(&root(), &file());
    assert!(took);
    assert!(effects.is_empty());
    assert_eq!(drive.opening(), None);
}

#[test]
fn a_project_with_no_disk_does_not_take_a_file() {
    let mut drive = EditorDrive::default();
    let (took, effects) = drive.open(&root(), &file());
    assert!(!took);
    assert!(effects.is_empty());
}

#[test]
fn a_read_that_fails_is_said_once_and_opens_nothing() {
    let mut drive = EditorDrive::default();
    drive.set_root(&root(), &Disk::Local);
    let _ = drive.open(&root(), &file());
    let effects = drive.handle(Input::Read {
        root: root(),
        path: file(),
        read: Err("too large".into()),
        uid: 7,
    });
    assert!(matches!(
        effects.as_slice(),
        [Effect::Signal(Signal::OpenFailed { why, .. })] if why == "too large"
    ));
    assert_eq!(drive.tabs(&root()).map(|t| t.files().len()), None);
}

#[test]
fn a_save_asked_while_one_writes_runs_once_it_lands_with_its_time() {
    let seen = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let landed = seen + Duration::from_secs(1);
    let (mut drive, uid) = opened(Some(seen));
    drive.typed(&root(), uid);
    assert_eq!(write_mtime(&drive.save_active(&root())), Some(seen));
    assert!(drive.save(&root(), uid).is_empty());

    let again = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: saved(Some(landed)),
        still: false,
    });
    // The second carries the time the first wrote, so it is no conflict
    // against it, and the dot stays for what was typed meanwhile.
    assert_eq!(write_mtime(&again), Some(landed));
    assert_eq!(drive.unsaved(&root()), 1);

    let done = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: saved(Some(landed)),
        still: true,
    });
    assert!(done.is_empty());
    assert_eq!(drive.unsaved(&root()), 0);
}

#[test]
fn a_locked_folder_is_not_written() {
    let (mut drive, uid) = opened(None);
    drive.typed(&root(), uid);
    drive.set_locked(&HashMap::from([(root(), "on vps".into())]));
    assert!(drive.save(&root(), uid).is_empty());
    assert_eq!(drive.locked(&root()), Some("on vps"));
    assert_eq!(drive.unsaved(&root()), 1);
}

#[test]
fn a_conflict_stands_until_a_save_answers_it_and_arms_the_overwrite() {
    let seen = SystemTime::UNIX_EPOCH;
    let theirs = seen + Duration::from_secs(5);
    let (mut drive, uid) = opened(Some(seen));
    let _ = drive.save(&root(), uid);
    let effects = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: SaveOutcome::Conflict {
            disk_mtime: Some(theirs),
        },
        still: false,
    });
    assert!(effects.is_empty());
    assert_eq!(
        drive.standing(&root()),
        Some(&Standing::Changed {
            label: "a.rs".into()
        })
    );
    assert_eq!(write_mtime(&drive.save(&root(), uid)), Some(theirs));
    let _ = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: saved(Some(theirs)),
        still: true,
    });
    assert_eq!(drive.standing(&root()), None);
}

#[test]
fn a_file_gone_from_disk_stands_as_gone() {
    let (mut drive, uid) = opened(Some(SystemTime::UNIX_EPOCH));
    let _ = drive.save(&root(), uid);
    let _ = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: SaveOutcome::Conflict { disk_mtime: None },
        still: false,
    });
    assert!(matches!(
        drive.standing(&root()),
        Some(Standing::Gone { .. })
    ));
}

#[test]
fn a_failed_write_is_said_once_and_leaves_nothing_standing() {
    let (mut drive, uid) = opened(Some(SystemTime::UNIX_EPOCH));
    let _ = drive.save(&root(), uid);
    let _ = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: SaveOutcome::Conflict {
            disk_mtime: Some(SystemTime::UNIX_EPOCH),
        },
        still: false,
    });
    let _ = drive.save(&root(), uid);
    let effects = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: SaveOutcome::Failed("disk full".into()),
        still: false,
    });
    assert!(matches!(
        effects.as_slice(),
        [Effect::Signal(Signal::SaveFailed { label, why })] if label == "a.rs" && why == "disk full"
    ));
    assert_eq!(drive.standing(&root()), None);
}

#[test]
fn closing_a_tab_with_edits_asks_and_discarding_drops_its_buffer() {
    let (mut drive, uid) = opened(None);
    drive.typed(&root(), uid);
    assert!(matches!(
        drive.close_tab(&root(), 0),
        Closing::Asks { unsaved: 1, label: Some(ref l), .. } if l == "a.rs"
    ));
    assert!(matches!(
        drive.close_all(&root()),
        Closing::Asks {
            close: Close::All(_),
            unsaved: 1,
            label: None
        }
    ));
    let effects = drive.discard(&Close::Tab(root(), uid));
    assert!(matches!(effects.as_slice(), [Effect::Forget { uid: u }] if *u == uid));
    assert_eq!(drive.tabs(&root()).map(|t| t.files().len()), Some(0));
}

#[test]
fn closing_a_clean_tab_closes_it_at_once() {
    let (mut drive, uid) = opened(None);
    let Closing::Closed(effects) = drive.close_tab(&root(), 0) else {
        panic!("asked about a clean tab");
    };
    assert!(matches!(effects.as_slice(), [Effect::Forget { uid: u }] if *u == uid));
}

#[test]
fn a_project_forgotten_takes_its_buffers_and_a_late_read_opens_nothing() {
    let (mut drive, uid) = opened(None);
    let _ = drive.open(&root(), &root().join("b.rs"));
    let effects = drive.forget_root(&root());
    assert!(matches!(effects.as_slice(), [Effect::Forget { uid: u }] if *u == uid));
    let late = drive.handle(Input::Read {
        root: root(),
        path: root().join("b.rs"),
        read: Ok(("late".into(), None)),
        uid: 7,
    });
    assert!(late.is_empty());
    assert!(drive.tabs(&root()).is_none());
}

#[test]
fn a_write_that_fails_after_its_tab_closed_is_still_said() {
    let (mut drive, uid) = opened(None);
    let _ = drive.save(&root(), uid);
    let _ = drive.discard(&Close::Tab(root(), uid));
    let effects = drive.handle(Input::Saved {
        root: root(),
        uid,
        outcome: SaveOutcome::Failed("disk full".into()),
        still: false,
    });
    assert!(matches!(
        effects.as_slice(),
        [Effect::Signal(Signal::SaveFailed { label, .. })] if label == "a.rs"
    ));
}

#[test]
fn a_read_that_fails_after_its_project_left_says_nothing() {
    let mut drive = EditorDrive::default();
    drive.set_root(&root(), &Disk::Local);
    let _ = drive.open(&root(), &file());
    let _ = drive.forget_root(&root());
    let effects = drive.handle(Input::Read {
        root: root(),
        path: file(),
        read: Err("gone".into()),
        uid: 7,
    });
    assert!(effects.is_empty());
}
