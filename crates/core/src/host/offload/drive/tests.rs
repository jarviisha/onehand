use super::blocking::{came_to, Settled};
use super::*;
use crate::host::offload::{Back, Changes};

fn vps() -> HostName {
    HostName("vps".into())
}

fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter()
        .map(|effect| match effect {
            Effect::Arm(_) => "Arm",
            Effect::Run(job) => job.name(),
            Effect::Signal(Signal::Sent { .. }) => "Sent",
            Effect::Signal(Signal::Offloaded { .. }) => "Offloaded",
            Effect::Signal(Signal::TookOver { .. }) => "TookOver",
        })
        .collect()
}

fn state(key: Holder) -> OffloadState {
    fresh(key)
}

fn stopped(moved: u64) -> OffloadState {
    OffloadState {
        moved,
        stopped: Some(Changes {
            files: vec!["a.rs".into()],
            refs: Vec::new(),
        }),
        ..fresh(Holder::Host)
    }
}

/// A drive keeping `/a` with `key`, the poll's clock already armed.
fn kept(key: Holder) -> OffloadDrive {
    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(state(key)));
    drive
}

fn brought(path: &str, poll: bool, came: Came) -> Input {
    Input::BroughtBack {
        path: path.into(),
        poll,
        took: None,
        brought: Brought {
            came,
            here: false,
            backed: false,
        },
    }
}

fn kept_offload(key: Option<Holder>, moving: Option<Moving>) -> Offload {
    Offload {
        moving,
        ..Offload::new(vps(), key.map(fresh), None)
    }
}

fn on_branch(moving: Option<Moving>) -> Offload {
    let branch = BranchState {
        name: "onehand/vps".into(),
        ahead: None,
        refused: None,
    };
    Offload {
        moving,
        ..Offload::new(vps(), None, Some(branch))
    }
}

const SEEN: HostSeen = HostSeen {
    live: true,
    taken: false,
};
const AWAY: HostSeen = HostSeen {
    live: false,
    taken: false,
};

/// Writers here are off while the host holds the code, while it goes up,
/// and while nothing kept has been read; the code is here, settled, only
/// when this machine holds it and nothing moves it.
#[test]
fn the_lock_follows_who_holds_the_code() {
    assert!(!kept_offload(Some(Holder::Laptop), None).locked());
    assert!(kept_offload(Some(Holder::Laptop), Some(Moving::Up)).locked());
    assert!(kept_offload(Some(Holder::Host), None).locked());
    assert!(kept_offload(Some(Holder::Host), Some(Moving::Back)).locked());
    assert!(kept_offload(None, None).locked());
    assert!(kept_offload(Some(Holder::Laptop), None).here());
    assert!(!kept_offload(Some(Holder::Laptop), Some(Moving::Up)).here());
    assert!(!kept_offload(Some(Holder::Host), None).here());
}

/// On a branch nothing here is ever locked or sent up before a prompt,
/// whatever was kept; it runs here again whenever nothing moves it.
#[test]
fn a_branch_is_never_locked() {
    assert!(!on_branch(None).locked());
    assert!(!on_branch(Some(Moving::Up)).locked());
    assert!(on_branch(None).here());
    assert!(!on_branch(Some(Moving::Back)).here());
    assert!(!on_branch(None).sends_up());
    // Updating the host from here sends nothing before a prompt.
    assert!(!on_branch(Some(Moving::Up)).going_up());
    assert!(kept_offload(Some(Holder::Laptop), Some(Moving::Up)).going_up());
    // Counted with or without its host: a merge here shows at once.
    assert_eq!(on_branch(None).due(AWAY), Some(Due::Branch));
    assert_eq!(on_branch(Some(Moving::Back)).due(SEEN), None);
    assert_eq!(
        kept_offload(Some(Holder::Host), None).due(SEEN),
        Some(Due::Back)
    );
    assert_eq!(kept_offload(Some(Holder::Laptop), None).due(SEEN), None);
}

/// A host holding the code is attached by the poll itself, unless another
/// machine holds it or it refused the poll's attach.
#[test]
fn the_poll_attaches_for_code_its_host_holds() {
    let held = kept_offload(Some(Holder::Host), None);
    assert_eq!(held.due(AWAY), Some(Due::Attach));
    let taken = HostSeen {
        live: false,
        taken: true,
    };
    assert_eq!(held.due(taken), None);
    let refused = Offload {
        refused: true,
        ..kept_offload(Some(Holder::Host), None)
    };
    assert_eq!(refused.due(AWAY), None);
    assert_eq!(refused.due(SEEN), Some(Due::Back));
    // A failure that may pass, the network gone, say, is tried again.
    let failed = Offload {
        failed: Some("Could not ask vps about its copy".into()),
        ..kept_offload(Some(Holder::Host), None)
    };
    assert_eq!(failed.due(AWAY), Some(Due::Attach));
    assert_eq!(
        kept_offload(Some(Holder::Host), Some(Moving::Back)).due(AWAY),
        None
    );
}

/// Kept without what was kept, the folder reads it; nothing kept is this
/// machine's, and a file that cannot be read is taken for the host's, with
/// why. The poll's clock is armed once, with the first folder.
#[test]
fn a_folder_is_read_once_and_the_poll_armed_once() {
    let mut drive = OffloadDrive::default();
    let fx = drive.track("/a".into(), vps(), None);
    assert_eq!(names(&fx), ["Arm", "Load"]);
    assert!(drive["/a".as_ref() as &Path].locked(), "until read");
    assert_eq!(
        names(&drive.track("/a".into(), vps(), None)),
        Vec::<&str>::new()
    );
    assert_eq!(names(&drive.track("/b".into(), vps(), None)), ["Load"]);
    drive.handle(Input::Loaded {
        path: "/a".into(),
        read: Ok(None),
    });
    assert!(drive[Path::new("/a")].sends_up());
    drive.handle(Input::Loaded {
        path: "/b".into(),
        read: Err("bad json".into()),
    });
    let b = &drive[Path::new("/b")];
    assert!(b.locked());
    assert_eq!(
        b.failed.as_deref(),
        Some("Could not read what was kept: bad json")
    );
}

/// The send before a prompt is claimed at once: a second press, or a
/// check, waits on it. What it comes to is kept and said to the prompt.
#[test]
fn the_send_up_is_claimed_once_and_said() {
    let mut drive = kept(Holder::Laptop);
    let (claimed, fx) = drive.claim_send(Path::new("/a"));
    assert!(claimed);
    assert_eq!(names(&fx), ["SendUp"]);
    let Effect::Run(job) = &fx[0] else {
        unreachable!()
    };
    assert_eq!(job.reach, Reach::Take);
    assert!(drive[Path::new("/a")].going_up());
    assert!(!drive.claim_send(Path::new("/a")).0, "a second press waits");
    assert!(!drive.claim_send(Path::new("/elsewhere")).0);
    let fx = drive.handle(Input::SentUp {
        path: "/a".into(),
        went: Err((
            "vps did not take the code: busy".into(),
            Some(Box::new(state(Holder::Laptop))),
        )),
    });
    assert!(matches!(
        &fx[..],
        [Effect::Signal(Signal::Sent { result: Err(why), .. })] if why.contains("did not take")
    ));
    let a = &drive[Path::new("/a")];
    assert_eq!(a.moving, None);
    assert!(a.failed.is_some());
    let fx = drive.claim_send(Path::new("/a")).1;
    assert_eq!(names(&fx), ["SendUp"]);
    let fx = drive.handle(Input::SentUp {
        path: "/a".into(),
        went: Ok(state(Holder::Host)),
    });
    assert!(matches!(
        &fx[..],
        [Effect::Signal(Signal::Sent { result: Ok(()), .. })]
    ));
    let a = &drive[Path::new("/a")];
    assert!(a.locked() && a.failed.is_none());
    assert!(!drive.claim_send(Path::new("/a")).0, "the host holds it");
}

/// A send whose folder was forgotten meanwhile still answers its prompt,
/// and keeps nothing.
#[test]
fn a_late_send_is_said_and_kept_nowhere() {
    let mut drive = kept(Holder::Laptop);
    drive.claim_send(Path::new("/a"));
    drive.forget(Path::new("/a"));
    let fx = drive.handle(Input::SentUp {
        path: "/a".into(),
        went: Ok(state(Holder::Host)),
    });
    assert_eq!(names(&fx), ["Sent"]);
    assert!(drive.get(Path::new("/a")).is_none());
}

/// A turn's end checks a folder whose host holds the code, over an attach
/// already up; one whose code is here, or moving, is left.
#[test]
fn a_turn_end_checks_only_code_the_host_holds() {
    let mut drive = kept(Holder::Laptop);
    assert!(drive.check(Path::new("/a")).is_empty());
    let mut drive = kept(Holder::Host);
    let fx = drive.check(Path::new("/a"));
    assert_eq!(names(&fx), ["CheckBack"]);
    let Effect::Run(job) = &fx[0] else {
        unreachable!()
    };
    assert_eq!(job.reach, Reach::Live, "a check never attaches by itself");
    assert!(drive.check(Path::new("/a")).is_empty(), "moving");
    assert!(drive.check(Path::new("/elsewhere")).is_empty());
}

/// What a copy back comes to is kept so. Still stopped, the time the code
/// last moved and the failure standing are kept; come, the failure and a
/// refusal go; with the host still working nothing changes but that the
/// code no longer moves; a failure keeps what it says to keep, and why.
#[test]
fn a_copy_back_lands_as_it_came() {
    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(stopped(7)));
    let a = |drive: &OffloadDrive| {
        let o = &drive[Path::new("/a")];
        (o.moving, o.state.clone(), o.failed.clone(), o.refused)
    };
    let fail = |drive: &mut OffloadDrive| {
        let o = drive.roots.get_mut(Path::new("/a")).unwrap();
        o.failed = Some("was".into());
        o.refused = true;
    };
    fail(&mut drive);
    drive.check(Path::new("/a"));
    drive.handle(brought("/a", false, Ok(Some(stopped(99)))));
    let (moving, state, failed, refused) = a(&drive);
    assert_eq!(moving, None);
    assert_eq!(state.map(|s| s.moved), Some(7));
    assert_eq!(failed.as_deref(), Some("was"));
    assert!(refused);

    drive.check(Path::new("/a"));
    drive.handle(brought("/a", false, Ok(None)));
    let (moving, state, failed, _) = a(&drive);
    assert_eq!(moving, None);
    assert_eq!(state.map(|s| s.moved), Some(7));
    assert_eq!(failed.as_deref(), Some("was"));

    drive.check(Path::new("/a"));
    drive.handle(brought("/a", false, Err(("Could not list".into(), None))));
    let (_, state, failed, _) = a(&drive);
    assert_eq!(failed.as_deref(), Some("Could not list"));
    assert!(state.is_some_and(|s| s.stopped.is_some()));

    drive.check(Path::new("/a"));
    let came = OffloadState {
        moved: 12,
        ..fresh(Holder::Laptop)
    };
    drive.handle(brought("/a", false, Ok(Some(came))));
    let (_, state, failed, refused) = a(&drive);
    assert_eq!(failed, None);
    assert!(!refused);
    assert_eq!(state.map(|s| (s.key, s.moved)), Some((Holder::Laptop, 12)));
    assert!(drive[Path::new("/a")].here());
}

/// A copy back that ran: stopped or raced, the key stays with the host and
/// what stopped it is kept; come, it is kept before the key comes back.
#[test]
fn a_copy_back_that_ran_keeps_the_key_until_the_code_came() {
    let changes = Changes {
        files: vec!["a.rs".into()],
        refs: Vec::new(),
    };
    let sent = Manifest::default();
    match came_to(Back::Stopped(changes.clone()), sent.clone(), 5) {
        Settled::Keep(s) => {
            assert_eq!((s.key, s.moved), (Holder::Host, 5));
            assert_eq!(s.stopped, Some(changes.clone()));
        }
        Settled::HandBack(_) => panic!("stopped is kept"),
    }
    match came_to(Back::Raced(sent.clone(), changes.clone()), sent.clone(), 5) {
        Settled::Keep(s) => assert_eq!((s.key, s.stopped), (Holder::Host, Some(changes))),
        Settled::HandBack(_) => panic!("raced is kept"),
    }
    match came_to(Back::Came(sent.clone()), sent, 5) {
        Settled::HandBack(s) => assert_eq!((s.key, s.stopped), (Holder::Host, None)),
        Settled::Keep(_) => panic!("come hands the key back"),
    }
}

/// *Take the host's code* waits for nothing moving; what it came to is
/// said once, where it was pressed, naming the backup once there is one.
#[test]
fn taking_the_hosts_code_says_how_it_went() {
    let mut drive = kept(Holder::Laptop);
    let fx = drive.take_over(Path::new("/a"), "/b/backup".into());
    assert!(matches!(
        &fx[..],
        [Effect::Signal(Signal::TookOver { result: Err(why), .. })] if why.contains("moving")
    ));
    assert!(drive[Path::new("/a")].failed.is_some());

    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(stopped(1)));
    let fx = drive.take_over(Path::new("/a"), "/b/backup".into());
    assert_eq!(names(&fx), ["TakeOver"]);
    let Effect::Run(job) = &fx[0] else {
        unreachable!()
    };
    assert_eq!(job.reach, Reach::Take);
    let took = |came, here, backed| Input::BroughtBack {
        path: "/a".into(),
        poll: false,
        took: Some("/b/backup".into()),
        brought: Brought { came, here, backed },
    };
    let fx = drive.handle(took(Ok(Some(fresh(Holder::Laptop))), false, true));
    assert!(matches!(
        &fx[..],
        [Effect::Signal(Signal::TookOver {
            result: Ok(true),
            ..
        })]
    ));
    assert!(drive[Path::new("/a")].here());

    // The host had handed it back already: nothing was taken.
    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(stopped(1)));
    drive.take_over(Path::new("/a"), "/b/backup".into());
    let fx = drive.handle(took(Ok(Some(fresh(Holder::Laptop))), true, false));
    assert!(matches!(
        &fx[..],
        [Effect::Signal(Signal::TookOver {
            result: Ok(false),
            ..
        })]
    ));

    // Changed here again after the backup: said with where the backup is.
    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(stopped(1)));
    drive.take_over(Path::new("/a"), "/b/backup".into());
    let fx = drive.handle(took(Ok(Some(stopped(2))), false, true));
    let Effect::Signal(Signal::TookOver {
        result: Err(why), ..
    }) = &fx[0]
    else {
        panic!("a failure")
    };
    assert_eq!(
        why,
        "Files changed here while the code came back; they stay as this machine has them; \
         this machine's versions are in /b/backup"
    );
    assert_eq!(drive[Path::new("/a")].failed.as_ref(), Some(why));
    for (came, said) in [
        (
            Ok(None),
            "vps is working on the code again; wait for it to finish",
        ),
        (
            Ok(Some(fresh(Holder::Host))),
            "vps kept the code; the next check brings it back",
        ),
        (
            Err(("Could not attach to vps: gone".into(), None)),
            "Could not attach to vps: gone",
        ),
    ] {
        let mut drive = OffloadDrive::default();
        drive.track("/a".into(), vps(), Some(stopped(1)));
        drive.take_over(Path::new("/a"), "/b/backup".into());
        let fx = drive.handle(took(came, false, false));
        assert!(matches!(
            &fx[..],
            [Effect::Signal(Signal::TookOver { result: Err(why), .. })] if why == said
        ));
    }
}

/// *Check again* attaches, taking the host; landed, what stood in the way
/// goes and the code is checked; failed, why stands on the page.
#[test]
fn checking_again_attaches_and_clears_what_stood_in_the_way() {
    let mut drive = OffloadDrive::default();
    drive.track("/a".into(), vps(), Some(stopped(1)));
    {
        let o = drive.roots.get_mut(Path::new("/a")).unwrap();
        o.refused = true;
        o.failed = Some("refused".into());
    }
    let fx = drive.check_again(Path::new("/a"));
    assert_eq!(names(&fx), ["Attach"]);
    let fx = drive.handle(Input::Attached {
        path: "/a".into(),
        then: Then::Again,
        result: Err(HostFailure::Busy {
            host: vps(),
            busy: Vec::new(),
            terms: 1,
        }),
    });
    assert!(fx.is_empty());
    assert_eq!(
        drive[Path::new("/a")].failed.as_deref(),
        Some("Waiting for 1 terminal on vps to close before restarting its host service onto this build")
    );
    let fx = drive.handle(Input::Attached {
        path: "/a".into(),
        then: Then::Again,
        result: Ok(()),
    });
    assert_eq!(names(&fx), ["CheckBack"]);
    let a = &drive[Path::new("/a")];
    assert!(a.failed.is_none() && !a.refused);
}

/// The poll's pass goes one folder at a time, in the folders' order, then
/// arms its clock again. A host it could not reach is not tried again that
/// pass; one that refused stands refused; a taken one is left.
#[test]
fn the_poll_goes_one_folder_at_a_time() {
    let mut drive = OffloadDrive::default();
    for path in ["/a", "/b", "/c"] {
        drive.track(path.into(), vps(), Some(state(Holder::Host)));
    }
    drive.track(
        "/d".into(),
        HostName("box".into()),
        Some(state(Holder::Host)),
    );
    drive.track("/e".into(), vps(), Some(state(Holder::Laptop)));
    let fx = drive.elapsed(|_| AWAY);
    assert_eq!(names(&fx), ["AttachQuietly"]);
    assert_eq!(
        names(&drive.elapsed(|_| AWAY)),
        Vec::<&str>::new(),
        "one pass"
    );
    let attached = |path: &str, result| Input::Attached {
        path: path.into(),
        then: Then::Poll,
        result,
    };
    // `/a`'s host is out of reach: `/b` and `/c`, on it too, are skipped.
    let fx = drive.handle(attached("/a", Err(HostFailure::Other("gone".into()))));
    let Effect::Run(job) = &fx[0] else {
        panic!("{:?}", names(&fx))
    };
    assert_eq!(job.path(), Path::new("/d"));
    assert!(
        drive[Path::new("/a")].failed.is_none(),
        "a blip is not said"
    );
    // `/d`'s refused: it stands so, and the pass is over.
    let fx = drive.handle(attached(
        "/d",
        Err(HostFailure::Refused("not linux".into())),
    ));
    assert_eq!(names(&fx), ["Arm"]);
    let d = &drive[Path::new("/d")];
    assert!(d.refused);
    assert_eq!(d.failed.as_deref(), Some("not linux"));

    // The next pass, every host live: each folder checked over it, one at a
    // time, the refused one too, which an attach that is up reaches.
    let fx = drive.elapsed(|_| SEEN);
    assert_eq!(names(&fx), ["CheckBack"]);
    let fx = drive.handle(brought("/a", true, Ok(None)));
    assert_eq!(names(&fx), ["CheckBack"]);
    // A check from a turn's end landing meanwhile moves the pass on nothing.
    drive.check(Path::new("/e"));
    assert!(drive.handle(brought("/e", false, Ok(None))).is_empty());
    let fx = drive.handle(brought("/b", true, Ok(None)));
    assert_eq!(names(&fx), ["CheckBack"]);
    let fx = drive.handle(brought("/c", true, Ok(None)));
    assert_eq!(names(&fx), ["CheckBack"]);
    let fx = drive.handle(brought("/d", true, Ok(None)));
    assert_eq!(names(&fx), ["Arm"]);

    // Taken by another machine: nothing is attached for it.
    let taken = HostSeen {
        live: false,
        taken: true,
    };
    assert_eq!(names(&drive.elapsed(|_| taken)), ["Arm"]);
}

/// A folder the pass meant to attach for that a turn's end claimed
/// meanwhile is skipped once attached.
#[test]
fn a_pass_skips_a_folder_already_moving() {
    let mut drive = kept(Holder::Host);
    let fx = drive.elapsed(|_| AWAY);
    assert_eq!(names(&fx), ["AttachQuietly"]);
    drive.roots.get_mut(Path::new("/a")).unwrap().moving = Some(Moving::Back);
    let fx = drive.handle(Input::Attached {
        path: "/a".into(),
        then: Then::Poll,
        result: Ok(()),
    });
    assert_eq!(names(&fx), ["Arm"]);
}

/// An update's failure stands until the next update, whatever a fetch says
/// meanwhile; a fetch's own failure goes with the next fetch.
#[test]
fn an_update_failure_outlives_a_fetch() {
    let mut drive = OffloadDrive::default();
    let fx = drive.track_branch("/a".into(), vps(), "onehand/vps".into());
    assert_eq!(names(&fx), ["Arm", "Refresh"]);
    let counted = |by, counted| Input::Counted {
        path: "/a".into(),
        by,
        poll: false,
        counted,
    };
    drive.handle(counted(Moving::Back, Ok(0)));
    assert_eq!(names(&drive.update_host(Path::new("/a"))), ["UpdateHost"]);
    assert!(
        drive.update_host(Path::new("/a")).is_empty(),
        "one at a time"
    );
    drive.handle(counted(Moving::Up, Err("conflict in a.rs".into())));
    drive.check(Path::new("/a"));
    drive.handle(counted(Moving::Back, Ok(2)));
    let branch = |drive: &OffloadDrive| drive[Path::new("/a")].branch.clone().unwrap();
    assert_eq!(branch(&drive).refused.as_deref(), Some("conflict in a.rs"));
    assert_eq!(branch(&drive).ahead, Some(2));
    drive.check(Path::new("/a"));
    drive.handle(counted(Moving::Back, Err("fetch failed".into())));
    assert_eq!(
        drive[Path::new("/a")].failed.as_deref(),
        Some("fetch failed")
    );
    drive.check(Path::new("/a"));
    drive.handle(counted(Moving::Back, Ok(2)));
    assert_eq!(drive[Path::new("/a")].failed, None);
    drive.update_host(Path::new("/a"));
    drive.handle(counted(Moving::Up, Ok(0)));
    assert_eq!(branch(&drive).refused, None);
    assert_eq!(drive[Path::new("/a")].moving, None);
    // A folder already kept is left as it is.
    assert!(drive
        .track_branch("/a".into(), vps(), "onehand/vps".into())
        .is_empty());
}

/// The form's offload checks `rsync` first, unless on a branch, and says
/// how it went; a check that fails goes no further.
#[test]
fn an_offload_checks_rsync_before_it_attaches() {
    let mut drive = OffloadDrive::default();
    assert_eq!(names(&drive.offload("/a".into(), vps(), true)), ["Offload"]);
    let fx = drive.offload("/a".into(), vps(), false);
    assert_eq!(names(&fx), ["Check"]);
    let Effect::Run(job) = &fx[0] else {
        unreachable!()
    };
    assert_eq!(job.reach, Reach::Nothing);
    let fx = drive.handle(Input::Checked {
        path: "/a".into(),
        host: vps(),
        result: Err("rsync is not installed on this machine".into()),
    });
    assert_eq!(names(&fx), ["Offloaded"]);
    let fx = drive.handle(Input::Checked {
        path: "/a".into(),
        host: vps(),
        result: Ok(()),
    });
    assert_eq!(names(&fx), ["Offload"]);
    assert!(drive.is_empty(), "kept only once the window keeps it");
}
