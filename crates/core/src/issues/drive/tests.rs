// A test that only reads the drive after a change lets its effects go.
#![allow(unused_must_use)]

use super::*;
use crate::connector::fake::Fake;
use crate::issues::Draft;

static FORGE: Fake = Fake::SERVING;
static CONNECTORS: [&'static dyn Connector; 1] = [&FORGE];

/// Effects as names, for asserting their order.
fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter()
        .map(|effect| match effect {
            Effect::Read { .. } => "Read",
            Effect::Sync { .. } => "Sync",
            Effect::Publish { .. } => "Publish",
            Effect::Changed => "Changed",
            Effect::Write { .. } => "Write",
            Effect::Signal(_) => "Signal",
        })
        .collect()
}

/// The time `secs` seconds after the epoch.
fn at(secs: u64) -> Now {
    Now {
        secs,
        at: std::time::Instant::now(),
    }
}

fn token(fx: &[Effect]) -> u64 {
    fx.iter()
        .find_map(|effect| match effect {
            Effect::Read { token } => Some(*token),
            _ => None,
        })
        .expect("a read")
}

/// A file with one issue, kept in step with the forge when `in_step`.
fn issues(in_step: bool) -> Issues {
    let mut kept = Issues::default();
    let draft = Draft {
        title: "one".into(),
        ..Draft::default()
    };
    kept.create(draft, 1).unwrap();
    if in_step {
        kept.sync_with(Some("Forge".into()));
    }
    kept
}

/// A drive whose first read found `kept`, served by the forge, at `now`.
fn read(kept: Issues, now: u64) -> (FileDrive, Vec<Effect>) {
    let mut drive = FileDrive::new("/p".into(), "/p.json".into(), &CONNECTORS);
    let token = token(&drive.load());
    let fx = drive.handle(Input::Read {
        token,
        read: Ok(kept),
        forge: Some(&FORGE),
        templates: Vec::new(),
        now: at(now),
    });
    (drive, fx)
}

fn synced(drive: &mut FileDrive, now: u64) -> Vec<Effect> {
    drive.handle(Input::Synced {
        done: Ok((issues(true), Report::default())),
        now: at(now),
    })
}

// Reading.

#[test]
fn a_file_is_read_once_marked_and_not_again_until_marked() {
    let mut drive = FileDrive::new("/p".into(), "/p.json".into(), &CONNECTORS);
    assert_eq!(names(&drive.load_if_stale()), ["Read"], "it starts unread");
    assert!(drive.load_if_stale().is_empty());
    drive.mark_stale();
    assert_eq!(names(&drive.load_if_stale()), ["Read"]);
}

/// A read superseded by a newer one is dropped.
#[test]
fn an_older_read_landing_is_dropped() {
    let mut drive = FileDrive::new("/p".into(), "/p.json".into(), &CONNECTORS);
    let old = token(&drive.load());
    drive.load();
    let fx = drive.handle(Input::Read {
        token: old,
        read: Ok(issues(false)),
        forge: None,
        templates: Vec::new(),
        now: at(1),
    });
    assert!(fx.is_empty());
    assert!(drive.issues.is_none());
}

#[test]
fn a_read_that_fails_says_why_and_keeps_what_was_shown() {
    let (mut drive, _) = read(issues(false), 1);
    let token = token(&drive.load());
    drive.handle(Input::Read {
        token,
        read: Err("unreadable".into()),
        forge: None,
        templates: Vec::new(),
        now: at(2),
    });
    assert_eq!(drive.failed.as_deref(), Some("unreadable"));
    assert!(drive.issues.is_some());
}

// Syncing.

/// A file kept in step syncs after its first read; one that is not, never.
#[test]
fn a_read_syncs_a_file_kept_in_step_and_only_that() {
    let (_, fx) = read(issues(true), 100);
    assert_eq!(names(&fx), ["Changed", "Changed", "Sync"]);
    let (_, fx) = read(issues(false), 100);
    assert_eq!(names(&fx), ["Changed"]);
}

/// A sync that only comes round waits out the gap since the last; one asked
/// for goes at once.
#[test]
fn a_sync_coming_round_waits_out_the_gap() {
    let (mut drive, _) = read(issues(true), 100);
    synced(&mut drive, 100);
    assert!(drive
        .sync(false, at(100 + SYNC_GAP.as_secs() - 1))
        .is_empty());
    assert_eq!(
        names(&drive.sync(false, at(100 + SYNC_GAP.as_secs()))),
        ["Changed", "Sync"]
    );
    synced(&mut drive, 200);
    assert_eq!(names(&drive.sync(true, at(201))), ["Changed", "Sync"]);
}

/// Asked for while one runs, a sync runs once more when it lands, and only
/// once.
#[test]
fn a_sync_asked_for_while_one_runs_follows_it_once() {
    let (mut drive, _) = read(issues(true), 100);
    assert!(drive.syncing);
    assert!(drive.sync(true, at(101)).is_empty());
    assert!(drive.sync(true, at(102)).is_empty());
    assert_eq!(
        names(&synced(&mut drive, 103)),
        ["Changed", "Changed", "Sync"]
    );
    assert_eq!(names(&synced(&mut drive, 104)), ["Changed"]);
}

#[test]
fn a_sync_that_fails_is_kept_with_its_time() {
    let (mut drive, _) = read(issues(true), 100);
    drive.handle(Input::Synced {
        done: Err("offline".into()),
        now: at(101),
    });
    assert!(!drive.syncing);
    assert!(matches!(&drive.synced, Some((101, Err(why))) if why == "offline"));
}

// Writing.

/// A change is shown at once; its write landing syncs a file kept in step;
/// the newest refused reads the file again, an older one refused does not.
#[test]
fn a_write_landing_syncs_and_the_newest_refused_reads_again() {
    let (mut drive, _) = read(issues(true), 100);
    synced(&mut drive, 100);
    assert_eq!(
        names(&drive.show(
            &Change::SetOpen {
                number: 1,
                open: false
            },
            at(2)
        )),
        ["Changed"]
    );
    assert!(!drive.issues.as_ref().unwrap().get(1).unwrap().open);
    let (first, _) = drive.write(Change::SyncWith(None), Maker::View, at(100));
    let (second, _) = drive.write(Change::SyncWith(None), Maker::View, at(100));
    assert!(!drive.newest(first) && drive.newest(second));
    let fx = drive.handle(Input::Written {
        of: first,
        done: Err("refused".into()),
        now: at(101),
    });
    assert_eq!(
        names(&fx),
        ["Changed", "Signal"],
        "an older refusal reads nothing"
    );
    assert!(matches!(
        &fx[1],
        Effect::Signal(Signal::Wrote { of, done: Err(_) }) if *of == first
    ));
    let fx = drive.handle(Input::Written {
        of: second,
        done: Err("refused".into()),
        now: at(101),
    });
    assert_eq!(names(&fx), ["Changed", "Signal", "Read"]);
    let (third, _) = drive.write(Change::SyncWith(None), Maker::View, at(100));
    let fx = drive.handle(Input::Written {
        of: third,
        done: Ok((issues(true), Applied::Named(1))),
        now: at(102),
    });
    assert_eq!(names(&fx), ["Changed", "Signal", "Changed", "Sync"]);
}

#[test]
fn a_change_that_does_not_apply_is_left_for_its_write() {
    let (mut drive, _) = read(issues(false), 1);
    assert!(drive
        .show(
            &Change::SetOpen {
                number: 9,
                open: false
            },
            at(2)
        )
        .is_empty());
}

// Publishing.

/// An issue is sent once at a time, only with a forge, and the file is read
/// again once it lands, whatever it came to.
#[test]
fn an_issue_is_published_one_at_a_time_and_read_back() {
    let mut drive = FileDrive::new("/p".into(), "/p.json".into(), &CONNECTORS);
    assert!(drive.publish(1).is_empty(), "no forge yet");
    let (mut drive, _) = read(issues(true), 1);
    assert_eq!(names(&drive.publish(1)), ["Changed", "Publish"]);
    assert_eq!(drive.publishing, Some(1));
    assert!(drive.publish(2).is_empty(), "one at a time");
    let fx = drive.handle(Input::Published {
        number: 1,
        done: Err("refused".into()),
    });
    assert_eq!(names(&fx), ["Changed", "Signal", "Read"]);
    assert!(matches!(
        &fx[1],
        Effect::Signal(Signal::PublishFailed { number: 1, why }) if why == "refused"
    ));
    assert_eq!(drive.publishing, None);
}

#[test]
fn only_an_issue_not_yet_on_a_forge_kept_in_step_is_publishable() {
    let kept = issues(true);
    let issue = kept.get(1).unwrap();
    assert!(publishable(&kept, Some(&FORGE), issue));
    assert!(!publishable(&kept, None, issue));
    assert!(!publishable(&issues(false), Some(&FORGE), issue));
}

// What is shown while writes are on their way.

/// The file as it stands after `writes` writes, each making an issue.
fn written(name: &str, writes: usize) -> Issues {
    let path = std::env::temp_dir().join(format!(
        "onehand-issues-drive-{name}-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut kept = Issues::default();
    for n in 0..writes {
        let draft = Draft {
            title: format!("issue {n}"),
            ..Draft::default()
        };
        kept = crate::issues::update_blocking(&path, move |kept| kept.create(draft, 1))
            .unwrap()
            .0;
    }
    let _ = std::fs::remove_file(&path);
    kept
}

fn close_first(kept: &mut Issues) -> Result<(), String> {
    kept.set_open(1, false, 2)
}

fn first_open(shown: &Option<Issues>) -> bool {
    shown.as_ref().unwrap().get(1).unwrap().open
}

#[test]
fn a_sync_landing_before_a_shown_changes_write_keeps_the_change() {
    let mut pending = Pending::default();
    let mut shown = Some(written("sync-held", 1));
    close_first(shown.as_mut().unwrap()).unwrap();
    let of = pending.send(Maker::View);
    let synced = written("sync-held", 1);
    pending.incoming(&mut shown, synced);
    assert!(!first_open(&shown), "the shown change stays");
    let mut kept = written("sync-held", 2);
    close_first(&mut kept).unwrap();
    assert_eq!(of, 1);
    pending.land(&mut shown, Some(kept.clone()));
    assert_eq!(shown, Some(kept));
}

#[test]
fn a_copy_newer_than_the_write_wins_once_the_write_lands() {
    let mut pending = Pending::default();
    let mut shown = Some(written("newer", 1));
    close_first(shown.as_mut().unwrap()).unwrap();
    pending.send(Maker::View);
    let mut kept = written("newer", 2);
    close_first(&mut kept).unwrap();
    let mut synced = written("newer", 3);
    close_first(&mut synced).unwrap();
    pending.incoming(&mut shown, synced.clone());
    pending.land(&mut shown, Some(kept));
    assert_eq!(shown, Some(synced));
}

#[test]
fn a_refused_write_lets_what_was_read_through() {
    let mut pending = Pending::default();
    let mut shown = Some(written("refused", 1));
    close_first(shown.as_mut().unwrap()).unwrap();
    pending.send(Maker::View);
    let read = written("refused", 1);
    pending.incoming(&mut shown, read.clone());
    pending.land(&mut shown, None);
    assert_eq!(shown, Some(read.clone()));
    let later = written("refused", 2);
    pending.incoming(&mut shown, later.clone());
    assert_eq!(shown, Some(later));
}

#[test]
fn an_older_write_landing_waits_for_the_newer_one() {
    let mut pending = Pending::default();
    let mut shown = Some(written("two", 1));
    close_first(shown.as_mut().unwrap()).unwrap();
    pending.send(Maker::View);
    shown.as_mut().unwrap().set_open(1, true, 3).unwrap();
    pending.send(Maker::View);
    let mut first = written("two", 2);
    close_first(&mut first).unwrap();
    pending.land(&mut shown, Some(first));
    assert!(first_open(&shown), "the newer change stays on screen");
    let second = written("two", 3);
    pending.land(&mut shown, Some(second.clone()));
    assert_eq!(shown, Some(second));
}

#[test]
fn an_issue_is_worked_by_the_latest_session_it_names_that_is_still_live() {
    let mut issues = Issues::default();
    let n = issues
        .create(
            Draft {
                title: "one".into(),
                ..Draft::default()
            },
            1,
        )
        .unwrap();
    issues.taken_up(n, "first", "old".into(), 2).unwrap();
    issues.taken_up(n, "second", "new".into(), 3).unwrap();
    let issue = issues.get(n).unwrap();
    let live = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    assert_eq!(issue.working_in(&live(&["old", "new"])), Some("new"));
    // The newer one ended: the older one, still going, is where the work is.
    assert_eq!(issue.working_in(&live(&["old"])), Some("old"));
    assert_eq!(issue.working_in(&live(&["elsewhere"])), None);
}

/// A named change is made as it says, naming the issue it touched.
#[test]
fn a_change_is_made_as_it_is_named() {
    let mut kept = issues(false);
    let made = Change::Create(Draft {
        title: "two".into(),
        ..Draft::default()
    })
    .apply(&mut kept, 2);
    assert_eq!(made, Ok(Applied::New(2)));
    assert_eq!(
        Change::SetOpen {
            number: 2,
            open: false
        }
        .apply(&mut kept, 3),
        Ok(Applied::Named(2))
    );
    assert!(!kept.get(2).unwrap().open);
    assert_eq!(
        Change::SyncWith(Some("Forge".into())).apply(&mut kept, 4),
        Ok(Applied::Nothing)
    );
    assert!(kept.in_step_with("Forge"));
    assert!(Change::SetOpen {
        number: 9,
        open: false
    }
    .apply(&mut kept, 5)
    .is_err());
}

#[test]
fn only_the_newest_write_moves_a_selection_nobody_moved_since() {
    assert_eq!(moves_selection(true, Some(3), None, None, false), Some(3));
    assert_eq!(moves_selection(false, Some(3), None, None, false), None);
    assert_eq!(moves_selection(true, Some(3), None, Some(1), false), None);
    assert_eq!(moves_selection(true, Some(3), Some(1), Some(1), true), None);
    assert_eq!(moves_selection(true, None, None, None, false), None);
}

/// A note, a session's note and a label taken off are made as they are
/// named, naming the issue; one about an issue that is not there is refused.
#[test]
fn the_outside_writers_changes_are_made_as_they_are_named() {
    let mut kept = issues(false);
    kept.issues[0].labels = vec!["auto".into()];
    let note = Change::Note {
        number: 1,
        text: " said ".into(),
    };
    assert_eq!(note.apply(&mut kept, 2), Ok(Applied::Named(1)));
    let taken = Change::TakenUp {
        number: 1,
        text: "Taken up".into(),
        session: "s1".into(),
    };
    assert_eq!(taken.apply(&mut kept, 3), Ok(Applied::Named(1)));
    let unlabel = Change::RemoveLabel {
        number: 1,
        label: "auto".into(),
    };
    assert_eq!(unlabel.apply(&mut kept, 4), Ok(Applied::Named(1)));
    let issue = kept.get(1).unwrap();
    let said: Vec<_> = issue
        .notes
        .iter()
        .map(|n| (n.text.as_str(), n.session.as_deref()))
        .collect();
    assert_eq!(said, [("said", None), ("Taken up", Some("s1"))]);
    assert!(issue.labels.is_empty());
    let blank = Change::Note {
        number: 1,
        text: "  ".into(),
    };
    assert!(blank.apply(&mut kept, 5).is_err());
    let elsewhere = Change::RemoveLabel {
        number: 9,
        label: "auto".into(),
    };
    assert!(elsewhere.apply(&mut kept, 5).is_err());
}

/// A new issue unless one is open with its title is decided on disk: it is
/// not shown before its write.
#[test]
fn a_create_unless_open_is_not_shown_before_its_write() {
    let (mut drive, _) = read(issues(false), 1);
    let before = drive.issues.clone();
    assert!(drive
        .show(
            &Change::CreateUnlessOpen(Draft {
                title: "two".into(),
                ..Draft::default()
            }),
            at(2)
        )
        .is_empty());
    assert_eq!(drive.issues, before);
}

/// An outside write never counts toward the newest of the views' writes: a
/// view's write landing after one was made still moves its selection.
#[test]
fn an_outside_write_never_becomes_a_views_newest() {
    let (mut drive, _) = read(issues(false), 1);
    let (create, _) = drive.write(
        Change::Create(Draft {
            title: "two".into(),
            ..Draft::default()
        }),
        Maker::View,
        at(2),
    );
    let (note, _) = drive.write(
        Change::Note {
            number: 1,
            text: "a run said".into(),
        },
        Maker::Outside,
        at(2),
    );
    assert!(drive.newest(create));
    assert!(!drive.newest(note));
    let fx = drive.handle(Input::Written {
        of: create,
        done: Ok((issues(false), Applied::New(2))),
        now: at(3),
    });
    let done = fx.iter().find_map(|effect| match effect {
        Effect::Signal(Signal::Wrote { of, done }) if *of == create => Some(done.clone()),
        Effect::Signal(Signal::Wrote { .. } | Signal::PublishFailed { .. })
        | Effect::Read { .. }
        | Effect::Sync { .. }
        | Effect::Publish { .. }
        | Effect::Changed
        | Effect::Write { .. } => None,
    });
    let number = done.unwrap().unwrap().issue();
    assert_eq!(
        moves_selection(drive.newest(create), number, Some(1), Some(1), false),
        Some(2)
    );
}

/// What comes in while an outside write is on its way is held until it
/// lands, as for any write; a refused outside write reads the file again
/// only when no write was made after it.
#[test]
fn an_outside_write_holds_what_comes_in_and_a_refused_last_one_reads_again() {
    let (mut drive, _) = read(issues(false), 1);
    let note = |text: &str| Change::Note {
        number: 1,
        text: text.into(),
    };
    let (first, _) = drive.write(note("one"), Maker::Outside, at(2));
    let (second, _) = drive.write(note("two"), Maker::Outside, at(2));
    let fx = drive.handle(Input::Written {
        of: first,
        done: Err("refused".into()),
        now: at(3),
    });
    assert_eq!(names(&fx), ["Changed", "Signal"], "a later write follows");
    let fx = drive.handle(Input::Written {
        of: second,
        done: Err("refused".into()),
        now: at(3),
    });
    assert_eq!(names(&fx), ["Changed", "Signal", "Read"]);

    let (mut drive, _) = read(issues(false), 1);
    let (of, _) = drive.write(note("held"), Maker::Outside, at(2));
    let token = token(&drive.load());
    let mut newer = issues(false);
    newer.revision = 5;
    drive.handle(Input::Read {
        token,
        read: Ok(newer.clone()),
        forge: Some(&FORGE),
        templates: Vec::new(),
        now: at(3),
    });
    assert_ne!(drive.issues.as_ref().map(Issues::revision), Some(5), "held");
    drive.handle(Input::Written {
        of,
        done: Ok((issues(false), Applied::Named(1))),
        now: at(4),
    });
    assert_eq!(drive.issues.as_ref().map(Issues::revision), Some(5));
}
