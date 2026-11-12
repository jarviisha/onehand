use super::waits::{next_delay, HOST_UNREACHABLE_POLLS, RECONNECT_FIRST, RECONNECT_MAX};
use super::*;
use crate::host::waiting_line;

fn vps() -> HostName {
    HostName("vps".into())
}

fn busy(host: &HostName, ids: &[&str]) -> Result<(), HostFailure> {
    Err(HostFailure::Busy {
        host: host.clone(),
        busy: ids.iter().map(|id| HostSession((*id).into())).collect(),
        terms: 0,
    })
}

fn gone() -> Result<(), HostFailure> {
    Err(HostFailure::Other("gone".into()))
}

/// A person's ask, the connection up still alive when `alive`.
fn ask(drive: &mut HostDrive, alive: bool) -> Got {
    drive.ask(&vps(), false, Asker::Person, alive)
}

/// An attach made by itself, for a wait.
fn polled(drive: &mut HostDrive) -> Got {
    drive.ask(&vps(), false, Asker::Poll, false)
}

/// What `host`'s attempt `made` landing so is heard as, and the waits it
/// ended.
fn land(
    drive: &mut HostDrive,
    host: &HostName,
    made: u64,
    result: Result<(), HostFailure>,
) -> Landed {
    let fx = drive.handle(Input::Landed {
        host: host.clone(),
        made,
        result,
    });
    Landed {
        ready: drive.ready(host) == Some(made),
        fx,
    }
}

/// What an attempt's landing came to: whether its connection is the host's
/// now, and what the drive asked for.
struct Landed {
    ready: bool,
    fx: Vec<Effect>,
}

fn asker(by_poll: bool) -> Asker {
    match by_poll {
        true => Asker::Poll,
        false => Asker::Person,
    }
}

fn closed(drive: &mut HostDrive, host: &HostName, taken: bool, up: bool) -> Vec<Effect> {
    drive.handle(Input::Closed {
        host: host.clone(),
        taken,
        up,
    })
}

/// A drive that has heard `result` of a first attach to `host`.
fn heard(
    drive: &mut HostDrive,
    host: &HostName,
    result: Result<(), HostFailure>,
    by_poll: bool,
) -> bool {
    let made = make(drive.ask(host, true, asker(by_poll), false));
    let landed = land(drive, host, made, result);
    names(&landed.fx).contains(&"Refresh")
}

/// The attempt `got` makes, or a panic when it makes none.
fn make(got: Got) -> u64 {
    match got {
        Got::Make(made) => made,
        Got::Ready | Got::Join(_) | Got::Wait(_) => panic!("made nothing: {got:?}"),
    }
}

fn name(e: &Effect) -> &'static str {
    match e {
        Effect::Arm { .. } => "Arm",
        Effect::Attach { .. } => "Attach",
        Effect::Ended { .. } => "Ended",
        Effect::Refresh => "Refresh",
    }
}

fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter().map(name).collect()
}

/// How long the one clock `fx` arm runs.
fn armed_after(fx: &[Effect]) -> Duration {
    assert_eq!(fx.len(), 1, "{fx:?}");
    match &fx[0] {
        Effect::Arm { after, .. } => *after,
        Effect::Attach { .. } | Effect::Ended { .. } | Effect::Refresh => panic!("{fx:?}"),
    }
}

fn ended(fx: &[Effect]) -> Vec<(u64, WaitEnd)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Ended { id, how } => Some((*id, how.clone())),
            Effect::Arm { .. } | Effect::Attach { .. } | Effect::Refresh => None,
        })
        .collect()
}

/// Every wait ended by `result`, a first attach of `host`'s landing.
fn heard_ending(
    drive: &mut HostDrive,
    host: &HostName,
    result: Result<(), HostFailure>,
    by_poll: bool,
) -> Vec<(u64, WaitEnd)> {
    let made = make(drive.ask(host, true, asker(by_poll), false));
    ended(&land(drive, host, made, result).fx)
}

#[test]
fn a_ready_connection_is_reused_and_a_dead_one_made_again() {
    let mut drive = HostDrive::default();
    // Two asks while the first attempt is under way share it.
    assert_eq!(ask(&mut drive, false), Got::Make(1));
    assert_eq!(ask(&mut drive, false), Got::Join(1));
    assert!(land(&mut drive, &vps(), 1, Ok(())).ready);
    assert_eq!(drive.ready(&vps()), Some(1));
    assert_eq!(ask(&mut drive, true), Got::Ready);
    // Its stream ended: made again.
    assert_eq!(ask(&mut drive, false), Got::Make(2));
}

/// A person's ask while an attach made by itself is under way waits on it,
/// with nothing made beside it; should that fail, it attaches again, taking
/// the host, and what that comes to is heard as its own.
#[test]
fn a_persons_ask_waits_on_an_attach_made_by_itself() {
    let mut drive = HostDrive::default();
    assert_eq!(polled(&mut drive), Got::Make(1));
    assert_eq!(ask(&mut drive, false), Got::Wait(1));
    let taken = Err(HostFailure::Taken(
        "vps was attached from another machine".into(),
    ));
    land(&mut drive, &vps(), 1, taken);
    assert_eq!(drive.state(&vps()), HostState::Taken);
    assert_eq!(ask(&mut drive, false), Got::Make(2), "made again, taking");
    land(&mut drive, &vps(), 2, Ok(()));
    assert_eq!(drive.state(&vps()), HostState::Live);
}

/// A poll that joins a person's attach is heard as the person's: a miss
/// ends a wait at once, not only after a few in a row.
#[test]
fn a_poll_joining_a_persons_attach_is_heard_as_the_persons() {
    let mut drive = HostDrive::default();
    heard(&mut drive, &vps(), busy(&vps(), &["1-1"]), false);
    let (id, _) = drive.listen(&vps(), Kind::Busy).unwrap();
    assert_eq!(ask(&mut drive, false), Got::Make(2));
    assert_eq!(polled(&mut drive), Got::Join(2));
    let fx = land(&mut drive, &vps(), 2, gone()).fx;
    assert_eq!(ended(&fx), [(id, Err("gone".to_string()))]);
}

/// An attempt a restart replaced is not what is known of the host: its late
/// result is not heard, and leaves the restart in its place.
#[test]
fn a_replaced_attempts_late_result_is_not_its_own() {
    let mut drive = HostDrive::default();
    assert_eq!(polled(&mut drive), Got::Make(1));
    assert_eq!(drive.ask(&vps(), true, Asker::Person, false), Got::Make(2));
    assert!(land(&mut drive, &vps(), 2, Ok(())).ready);
    let late = land(&mut drive, &vps(), 1, Err(HostFailure::Other("no".into())));
    assert!(!late.ready && late.fx.is_empty(), "stale");
    assert_eq!(drive.state(&vps()), HostState::Live);
    assert!(!land(&mut drive, &vps(), 1, Ok(())).ready, "nor kept");
    assert_eq!(drive.ready(&vps()), Some(2));
}

/// A failed attempt leaves nothing behind: the next ask tries again.
#[test]
fn a_failed_attach_is_tried_again_on_the_next_ask() {
    let mut drive = HostDrive::default();
    assert_eq!(ask(&mut drive, false), Got::Make(1));
    land(&mut drive, &vps(), 1, gone());
    assert_eq!(ask(&mut drive, false), Got::Make(2));
}

#[test]
fn a_busy_host_is_asked_again_every_fifteen_seconds() {
    assert_eq!(HOST_IDLE_POLL, Duration::from_secs(15));
}

#[test]
fn the_waiting_line_counts_the_sessions_and_names_the_host() {
    let host = vps();
    assert_eq!(
        waiting_line(&host, 1, 0),
        "Waiting for 1 session on vps to finish before restarting its host service onto this build"
    );
    assert_eq!(
        waiting_line(&host, 3, 0),
        "Waiting for 3 sessions on vps to finish before restarting its host service onto this build"
    );
    assert_eq!(
        waiting_line(&host, 0, 1),
        "Waiting for 1 terminal on vps to close before restarting its host service onto this build"
    );
    assert_eq!(
        waiting_line(&host, 2, 3),
        "Waiting for 2 sessions to finish and 3 terminals to close on vps before restarting its \
         host service onto this build"
    );
}

/// A busy host waits, once however often it is found busy, its clock armed
/// once; the first attach that lands releases the wait, once.
#[test]
fn a_busy_host_waits_until_an_attach_lands() {
    let host = vps();
    let mut drive = HostDrive::default();
    assert!(
        drive.listen(&host, Kind::Busy).is_none(),
        "nothing waits on an idle host"
    );
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    heard(&mut drive, &host, busy(&host, &["1-1", "2-1"]), false);
    let (a, armed) = drive.listen(&host, Kind::Busy).unwrap();
    assert_eq!(armed_after(&armed), HOST_IDLE_POLL);
    let (b, armed) = drive.listen(&host, Kind::Busy).unwrap();
    assert!(armed.is_empty(), "one clock per wait");
    assert_eq!(a, b, "one wait");
    assert_eq!(drive.busy(&host).0.len(), 2, "the latest sessions said");

    assert_eq!(
        heard_ending(&mut drive, &host, Ok(()), false),
        [(a, Ok(()))]
    );
    assert!(drive.listen(&host, Kind::Busy).is_none(), "released once");
    assert!(drive.busy(&host).0.is_empty());
    // A later busy is a new wait, armed anew.
    heard(&mut drive, &host, busy(&host, &["3-1"]), false);
    let (c, armed) = drive.listen(&host, Kind::Busy).unwrap();
    assert_ne!(c, a);
    assert_eq!(armed.len(), 1);
}

/// Restart now's yes ends the wait as any attach that lands does; another
/// host's attach ends nothing here.
#[test]
fn a_restart_that_lands_ends_the_wait() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (id, _) = drive.listen(&host, Kind::Busy).unwrap();
    assert!(heard_ending(&mut drive, &HostName("other".into()), Ok(()), false).is_empty());
    assert_eq!(
        heard_ending(&mut drive, &host, Ok(()), false),
        [(id, Ok(()))]
    );
}

/// A host that cannot be reached is not waited on: the wait hears why, so
/// each place says the real failure rather than that it waits.
#[test]
fn an_unreachable_host_releases_its_waiters_with_the_failure() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (id, _) = drive.listen(&host, Kind::Busy).unwrap();
    assert_eq!(
        heard_ending(&mut drive, &host, gone(), false),
        [(id, Err("gone".to_string()))]
    );
    assert!(
        drive.listen(&host, Kind::Busy).is_none(),
        "the wait is over"
    );
}

/// When the last listener goes, so do the wait and its clock; one listening
/// again starts both anew. The clock attaches only while its wait stands,
/// and arms itself again once that attach came back.
#[test]
fn the_wait_ends_with_its_last_listener() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (a, _) = drive.listen(&host, Kind::Busy).unwrap();
    drive.listen(&host, Kind::Busy).unwrap();
    assert_eq!(
        names(&drive.handle(Input::Elapsed {
            host: host.clone(),
            kind: Kind::Busy,
            id: a
        })),
        ["Attach"]
    );
    assert_eq!(
        names(&drive.handle(Input::Polled {
            host: host.clone(),
            kind: Kind::Busy,
            id: a
        })),
        ["Arm"]
    );
    assert!(!drive.leave(&host, a), "one still listens");
    assert!(drive.leave(&host, a), "the last");
    assert!(
        drive
            .handle(Input::Elapsed {
                host: host.clone(),
                kind: Kind::Busy,
                id: a
            })
            .is_empty(),
        "no clock"
    );
    assert!(drive
        .handle(Input::Polled {
            host: host.clone(),
            kind: Kind::Busy,
            id: a
        })
        .is_empty());
    assert!(drive.busy(&host).0.is_empty(), "the wait is gone");
    assert!(drive.listen(&host, Kind::Busy).is_none());
    // A leave for a wait already gone touches the next one not at all.
    heard(&mut drive, &host, busy(&host, &["2-1"]), false);
    let (c, armed) = drive.listen(&host, Kind::Busy).unwrap();
    assert!(!drive.leave(&host, a));
    assert_eq!(armed.len(), 1, "armed anew");
    assert!(!drive
        .handle(Input::Elapsed {
            host: host.clone(),
            kind: Kind::Busy,
            id: c
        })
        .is_empty());
}

/// Being found busy makes no wait by itself: only a listener does, so an
/// attach nobody waits on (a restart's first look) leaves nothing behind.
#[test]
fn a_busy_host_nobody_listens_to_is_not_waited_on() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    assert!(!drive.waited(&host));
    assert!(drive.listen(&host, Kind::Busy).is_some());
    assert!(drive.waited(&host));
}

/// One poll that misses the host is a blip; enough in a row end the wait
/// on why. A miss outside the poll (Restart now) ends it at once.
#[test]
fn a_wait_outlives_a_blip_but_not_a_host_gone() {
    assert_eq!(HOST_UNREACHABLE_POLLS, 2);
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (id, _) = drive.listen(&host, Kind::Busy).unwrap();
    assert!(
        heard_ending(&mut drive, &host, gone(), true).is_empty(),
        "a blip"
    );
    // Found busy again: the count starts over.
    heard(&mut drive, &host, busy(&host, &["1-1"]), true);
    assert!(heard_ending(&mut drive, &host, gone(), true).is_empty());
    assert_eq!(
        heard_ending(&mut drive, &host, gone(), true),
        [(id, Err("gone".to_string()))]
    );

    heard(&mut drive, &host, busy(&host, &["2-1"]), false);
    let (id, _) = drive.listen(&host, Kind::Busy).unwrap();
    assert_eq!(
        heard_ending(&mut drive, &host, gone(), false),
        [(id, Err("gone".to_string()))]
    );
}

/// Two seconds after a connection goes, then twice as long each miss, never
/// more than a minute apart.
#[test]
fn next_delay_doubles_to_the_cap() {
    assert_eq!(RECONNECT_FIRST, Duration::from_secs(2));
    assert_eq!(RECONNECT_MAX, Duration::from_secs(60));
    assert_eq!(next_delay(None), RECONNECT_FIRST);
    let mut delay = None;
    let mut said = Vec::new();
    for _ in 0..8 {
        let next = next_delay(delay);
        said.push(next.as_secs());
        delay = Some(next);
    }
    assert_eq!(said, [2, 4, 8, 16, 32, 60, 60, 60]);
}

/// The clock of the wait for a lost host doubles each time it comes round.
#[test]
fn a_lost_hosts_clock_backs_off() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, Ok(()), false);
    closed(&mut drive, &host, false, false);
    let (id, armed) = drive.listen(&host, Kind::Back).unwrap();
    let after = |fx: &[Effect]| armed_after(fx).as_secs();
    assert_eq!(after(&armed), 2);
    let mut said = Vec::new();
    for _ in 0..4 {
        assert!(!drive
            .handle(Input::Elapsed {
                host: host.clone(),
                kind: Kind::Back,
                id
            })
            .is_empty());
        said.push(after(&drive.handle(Input::Polled {
            host: host.clone(),
            kind: Kind::Back,
            id,
        })));
    }
    assert_eq!(said, [4, 8, 16, 32]);
}

/// A host another machine attached to is that machine's now: nothing here
/// attaches to it again by itself.
#[test]
fn taken_never_reconnects() {
    let host = vps();
    let mut drive = HostDrive::default();
    assert_eq!(drive.state(&host), HostState::Unknown);
    let made = make(ask(&mut drive, false));
    assert_eq!(drive.state(&host), HostState::Attaching);
    land(&mut drive, &host, made, Ok(()));
    assert_eq!(drive.state(&host), HostState::Live);
    closed(&mut drive, &host, true, false);
    assert_eq!(drive.state(&host), HostState::Taken);
    assert!(
        drive.listen(&host, Kind::Back).is_none(),
        "no loop on a taken host"
    );
}

/// A closing heard after a newer attach came up changes nothing.
#[test]
fn a_closing_behind_a_newer_attach_is_not_heard() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, Ok(()), false);
    assert!(closed(&mut drive, &host, false, true).is_empty());
    assert_eq!(drive.state(&host), HostState::Live);
    // Seen closed before the watch on its closing is heard: lost already.
    drive.settle(&host, Some(false));
    assert!(drive.state(&host).lost());
}

/// A connection lost is attached again while anything listens: misses never
/// end the wait, the last listener going does, and an attach that lands
/// releases it.
#[test]
fn lost_is_listened_to_until_the_last_listener_goes() {
    let host = vps();
    let mut drive = HostDrive::default();
    assert!(
        drive.listen(&host, Kind::Back).is_none(),
        "never attached: not lost"
    );
    heard(&mut drive, &host, Ok(()), false);
    assert!(drive.listen(&host, Kind::Back).is_none(), "live: not lost");
    closed(&mut drive, &host, false, false);
    assert_eq!(
        drive.state(&host),
        HostState::Lost("lost the connection to vps".into())
    );
    let (a, armed) = drive.listen(&host, Kind::Back).unwrap();
    assert_eq!(armed.len(), 1);
    let (b, armed) = drive.listen(&host, Kind::Back).unwrap();
    assert_eq!(a, b, "one loop");
    assert!(armed.is_empty(), "one loop per host");
    assert!(drive.retrying(&host));
    for _ in 0..5 {
        assert!(
            heard_ending(&mut drive, &host, gone(), true).is_empty(),
            "misses never end it"
        );
    }
    assert!(drive.state(&host).lost());
    assert!(!drive.leave(&host, a));
    assert!(drive.leave(&host, a), "the loop stops");
    assert!(!drive.retrying(&host));
    // Still lost: one listening again starts a new loop.
    let (c, armed) = drive.listen(&host, Kind::Back).unwrap();
    assert_ne!(c, a);
    assert_eq!(armed.len(), 1);
    assert_eq!(heard_ending(&mut drive, &host, Ok(()), true), [(c, Ok(()))]);
    assert_eq!(drive.state(&host), HostState::Live);
}

/// A lost host found running another build hands over to the busy wait.
#[test]
fn a_lost_host_found_busy_ends_the_loop_on_why() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, Ok(()), false);
    closed(&mut drive, &host, false, false);
    let (back, _) = drive.listen(&host, Kind::Back).unwrap();
    let said = heard_ending(&mut drive, &host, busy(&host, &["1-1"]), true);
    assert!(matches!(&said[..], [(id, Err(_))] if *id == back));
    assert!(
        drive.listen(&host, Kind::Busy).is_some(),
        "the busy wait takes over"
    );
}

/// A lost host's own attach finding another machine's holding the host ends
/// the wait for it to be back, and the busy wait, on why: it is taken, and
/// nothing attaches to it again by itself.
#[test]
fn a_held_host_ends_the_back_wait_as_taken() {
    let host = vps();
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, Ok(()), false);
    closed(&mut drive, &host, false, false);
    let (back, _) = drive.listen(&host, Kind::Back).unwrap();
    let held = || {
        Err(HostFailure::Taken(
            "vps was attached from another machine".into(),
        ))
    };
    assert_eq!(
        heard_ending(&mut drive, &host, held(), true),
        [(back, Err("vps was attached from another machine".into()))]
    );
    assert_eq!(drive.state(&host), HostState::Taken);
    assert!(!drive.retrying(&host));
    assert!(
        drive.listen(&host, Kind::Back).is_none(),
        "no loop on a taken host"
    );

    // So too a busy host's poll.
    let host = HostName("box".into());
    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (idle, _) = drive.listen(&host, Kind::Busy).unwrap();
    let said = heard_ending(&mut drive, &host, held(), true);
    assert!(matches!(&said[..], [(id, Err(_))] if *id == idle));
    assert_eq!(drive.state(&host), HostState::Taken);
}

/// A host this build cannot attach to, or no service binary to attach with,
/// is never tried again by itself: it is not lost, and a wait for it to be
/// back, or on its busy sessions, ends on why.
#[test]
fn a_refused_host_is_not_retried() {
    let host = vps();
    let refused = || {
        Err(HostFailure::Refused(
            "vps is not a machine this build runs on".into(),
        ))
    };
    let mut drive = HostDrive::default();
    heard(&mut drive, &host, refused(), false);
    assert_eq!(drive.state(&host), HostState::Unknown);
    assert!(drive.listen(&host, Kind::Back).is_none(), "never looped on");

    heard(&mut drive, &host, Ok(()), false);
    closed(&mut drive, &host, false, false);
    let (back, _) = drive.listen(&host, Kind::Back).unwrap();
    assert!(drive.retrying(&host));
    assert_eq!(
        heard_ending(&mut drive, &host, refused(), true),
        [(
            back,
            Err("vps is not a machine this build runs on".to_string())
        )]
    );
    assert!(!drive.retrying(&host));
    assert!(drive.listen(&host, Kind::Back).is_none());

    heard(&mut drive, &host, busy(&host, &["1-1"]), false);
    let (idle, _) = drive.listen(&host, Kind::Busy).unwrap();
    let said = heard_ending(&mut drive, &host, refused(), true);
    assert!(
        matches!(&said[..], [(id, Err(_))] if *id == idle),
        "the busy wait ends too"
    );
}

/// What is known of a host changes only when it does, so windows are drawn
/// again only then.
#[test]
fn only_a_change_of_state_is_told() {
    let host = vps();
    let mut drive = HostDrive::default();
    assert!(heard(&mut drive, &host, Ok(()), false));
    assert!(!heard(&mut drive, &host, Ok(()), false));
    assert_eq!(names(&closed(&mut drive, &host, false, false)), ["Refresh"]);
    assert!(closed(&mut drive, &host, false, false).is_empty());
    assert!(heard(&mut drive, &host, gone(), true), "a new why is news");
    assert!(!heard(&mut drive, &host, gone(), true));
}

/// What waited on a host being back restarts unless the host was taken, or
/// the wait ended on why while the host was not found busy.
#[test]
fn what_waited_on_a_host_being_back_restarts_only_when_it_may() {
    let host = vps();
    assert_eq!(back_failure(&host, &HostState::Live, false, Ok(())), None);
    assert_eq!(
        back_failure(&host, &HostState::Attaching, true, Err("busy".into())),
        None
    );
    assert_eq!(
        back_failure(&host, &HostState::Unknown, false, Err("refused".into())).as_deref(),
        Some("refused")
    );
    for (busy, end) in [(false, Ok(())), (true, Err("busy".to_string()))] {
        assert_eq!(
            back_failure(&host, &HostState::Taken, busy, end).as_deref(),
            Some("vps was attached from another machine"),
            "never attached again by itself, however its wait ended"
        );
    }
}

/// A yes restarts the service only while it is still busy, and a failed
/// restart is said once.
#[test]
fn a_yes_restarts_only_while_busy_and_its_failure_is_said_once() {
    let busy_host = HostFailure::Busy {
        host: vps(),
        busy: Vec::new(),
        terms: 0,
    };
    assert!(needs_restart::<()>(&Err(busy_host.clone())));
    assert!(!needs_restart(&Ok(())));
    assert!(!needs_restart::<()>(&Err(HostFailure::Other(
        "gone".into()
    ))));
    assert!(said_by_wait(true, &HostFailure::Other("gone".into())));
    assert!(!said_by_wait(false, &HostFailure::Other("gone".into())));
    assert!(!said_by_wait(true, &busy_host));
    assert_eq!(
        restart_failure(busy_host),
        "vps still runs another build of the host service"
    );
}

/// A host simply back restarts what waited quietly, listing the page only if
/// it waited; a busy host's wait ending restarts each session onto the new
/// service, said, and lists the page whatever waited.
#[test]
fn a_wait_landing_restarts_what_waited_as_its_wait_says() {
    use super::Landing::*;
    assert_eq!(
        back_landed(vec![1], false, None),
        [Restart {
            uid: 1,
            after_service: false
        }]
    );
    assert_eq!(
        idle_landed(vec![1], false, true, Ok(())),
        [
            Restart {
                uid: 1,
                after_service: true
            },
            List,
            Add
        ]
    );
}

#[test]
fn a_wait_that_fails_ends_what_waited_on_why() {
    use super::Landing::*;
    assert_eq!(
        back_landed(vec![1], true, Some("taken".into())),
        [
            BackFailed {
                uid: 1,
                why: "taken".into()
            },
            PageUnreached("taken".into())
        ]
    );
    assert_eq!(
        idle_landed(vec![], true, true, Err("gone".into())),
        [PageFailed, AddFailed("gone".into())]
    );
}
