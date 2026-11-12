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
    assert_eq!(names(&effects), ["Read"]);
    let effects = drive.handle(Input::Read {
        root: root(),
        path: file(),
        read: Ok(("theirs".into(), mtime)),
        uid: 7,
    });
    let uid = match only(&effects) {
        Effect::Buffer { uid, text, .. } => {
            assert_eq!(text, "theirs");
            *uid
        }
        e @ (Effect::Read { .. }
        | Effect::Write { .. }
        | Effect::Forget { .. }
        | Effect::Signal(_)) => panic!("no buffer built: {}", name(e)),
    };
    assert_eq!(uid, 7, "the tab takes the id it was handed");
    (drive, uid)
}

fn name(e: &Effect) -> &'static str {
    match e {
        Effect::Read { .. } => "Read",
        Effect::Buffer { .. } => "Buffer",
        Effect::Write { .. } => "Write",
        Effect::Forget { .. } => "Forget",
        Effect::Signal(_) => "Signal",
    }
}

fn names(effects: &[Effect]) -> Vec<&'static str> {
    effects.iter().map(name).collect()
}

/// The one effect of `effects`, or a panic naming them all.
fn only(effects: &[Effect]) -> &Effect {
    assert_eq!(effects.len(), 1, "not one effect: {:?}", names(effects));
    &effects[0]
}

/// The signal `effects` hold as their one effect.
fn signal(effects: &[Effect]) -> &Signal {
    match only(effects) {
        Effect::Signal(signal) => signal,
        e @ (Effect::Read { .. }
        | Effect::Buffer { .. }
        | Effect::Write { .. }
        | Effect::Forget { .. }) => panic!("not a signal: {}", name(e)),
    }
}

/// The uid `effects` forget as their one effect.
fn forgot(effects: &[Effect]) -> u64 {
    match only(effects) {
        Effect::Forget { uid } => *uid,
        e @ (Effect::Read { .. }
        | Effect::Buffer { .. }
        | Effect::Write { .. }
        | Effect::Signal(_)) => panic!("not a forget: {}", name(e)),
    }
}

fn saved(mtime: Option<SystemTime>) -> SaveOutcome {
    SaveOutcome::Saved {
        mtime,
        text: std::sync::Arc::new("mine".into()),
    }
}

fn write_mtime(effects: &[Effect]) -> Option<SystemTime> {
    match only(effects) {
        Effect::Write { mtime, .. } => *mtime,
        e @ (Effect::Read { .. }
        | Effect::Buffer { .. }
        | Effect::Forget { .. }
        | Effect::Signal(_)) => panic!("not one write: {}", name(e)),
    }
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
    assert_eq!(
        signal(&effects),
        &Signal::OpenFailed {
            path: file(),
            why: "too large".into()
        }
    );
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
    assert_eq!(
        drive.standing(&root()),
        Some(&Standing::Gone {
            label: "a.rs".into()
        })
    );
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
    assert_eq!(
        signal(&effects),
        &Signal::SaveFailed {
            label: "a.rs".into(),
            why: "disk full".into()
        }
    );
    assert_eq!(drive.standing(&root()), None);
}

#[test]
fn closing_a_tab_with_edits_asks_and_discarding_drops_its_buffer() {
    let (mut drive, uid) = opened(None);
    drive.typed(&root(), uid);
    match drive.close_tab(&root(), 0) {
        Closing::Asks { unsaved, label, .. } => {
            assert_eq!(unsaved, 1);
            assert_eq!(label.as_deref(), Some("a.rs"));
        }
        Closing::Closed(_) => panic!("closed a tab with edits"),
    }
    match drive.close_all(&root()) {
        Closing::Asks {
            close,
            unsaved,
            label,
        } => {
            assert_eq!(close, Close::All(root()));
            assert_eq!(unsaved, 1);
            assert_eq!(label, None);
        }
        Closing::Closed(_) => panic!("closed tabs with edits"),
    }
    let effects = drive.discard(&Close::Tab(root(), uid));
    assert_eq!(forgot(&effects), uid);
    assert_eq!(drive.tabs(&root()).map(|t| t.files().len()), Some(0));
}

#[test]
fn closing_a_clean_tab_closes_it_at_once() {
    let (mut drive, uid) = opened(None);
    let effects = match drive.close_tab(&root(), 0) {
        Closing::Closed(effects) => effects,
        Closing::Asks { .. } => panic!("asked about a clean tab"),
    };
    assert_eq!(forgot(&effects), uid);
}

#[test]
fn a_project_forgotten_takes_its_buffers_and_a_late_read_opens_nothing() {
    let (mut drive, uid) = opened(None);
    let _ = drive.open(&root(), &root().join("b.rs"));
    let effects = drive.forget_root(&root());
    assert_eq!(forgot(&effects), uid);
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
    match signal(&effects) {
        Signal::SaveFailed { label, .. } => assert_eq!(label, "a.rs"),
        s @ Signal::OpenFailed { .. } => panic!("{s:?}"),
    }
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
