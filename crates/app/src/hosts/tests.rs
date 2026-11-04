use super::{
    HOST_IDLE_POLL, HOST_UNREACHABLE_POLLS, HostFailure, HostState, RECONNECT_FIRST, RECONNECT_MAX,
    Slots, Waits, bundled_binary_in, next_delay, waiting_line,
};
use futures::FutureExt as _;
use futures::executor::block_on;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn a_missing_bundled_binary_is_said_plainly() {
    let dir = std::env::temp_dir().join(format!("onehand-bundled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let found = bundled_binary_in(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    match found {
        Err(HostFailure::Other(text)) => assert_eq!(
            text,
            "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`"
        ),
        Ok(_) | Err(HostFailure::Busy { .. }) => panic!("expected the missing binary's words"),
    }
}

/// A connection whose pump is up while its flag is.
#[derive(Clone)]
struct Fake(Arc<AtomicBool>);

#[test]
fn a_ready_connection_is_reused_and_a_dead_one_made_again() {
    let slots = Slots::new(|c: &Fake| c.0.load(Ordering::SeqCst));
    let host = HostName("vps".into());
    let made: RefCell<Vec<Arc<AtomicBool>>> = RefCell::new(Vec::new());
    let make = || {
        let up = Arc::new(AtomicBool::new(true));
        made.borrow_mut().push(up.clone());
        async move { Ok::<_, HostFailure>(Fake(up)) }.boxed_local()
    };

    // Two asks while the first attempt is under way share it.
    let first = slots.get(&host, false, make);
    let second = slots.get(&host, false, make);
    block_on(first).ok().unwrap();
    block_on(second).ok().unwrap();
    assert_eq!(made.borrow().len(), 1);
    block_on(slots.get(&host, false, make)).ok().unwrap();
    assert_eq!(made.borrow().len(), 1);

    made.borrow()[0].store(false, Ordering::SeqCst);
    block_on(slots.get(&host, false, make)).ok().unwrap();
    assert_eq!(made.borrow().len(), 2);
    block_on(slots.get(&host, false, make)).ok().unwrap();
    assert_eq!(made.borrow().len(), 2);
}

/// A failing attach: every call to `make` counted in `made`.
fn failing(
    made: &RefCell<usize>,
) -> impl Fn() -> futures::future::LocalBoxFuture<'static, Result<Fake, HostFailure>> + '_ {
    move || {
        *made.borrow_mut() += 1;
        async { Err(HostFailure::Other("Could not attach to vps: no".into())) }.boxed_local()
    }
}

#[test]
fn a_failed_attach_is_tried_again_on_the_next_ask() {
    let slots = Slots::new(|c: &Fake| c.0.load(Ordering::SeqCst));
    let host = HostName("vps".into());
    let made = RefCell::new(0);
    assert!(block_on(slots.get(&host, false, failing(&made))).is_err());
    assert!(block_on(slots.get(&host, false, failing(&made))).is_err());
    assert_eq!(*made.borrow(), 2);
}

/// **Nobody awaiting an attach does not make it the answer.** The person who
/// asked went away mid-attach; whoever asks next must not be handed the
/// failure it ended in.
#[test]
fn a_finished_attach_nobody_awaited_is_not_handed_out() {
    let slots = Slots::new(|c: &Fake| c.0.load(Ordering::SeqCst));
    let host = HostName("vps".into());
    let made = RefCell::new(0);
    drop(slots.get(&host, false, failing(&made)));
    assert!(block_on(slots.get(&host, false, failing(&made))).is_err());
    assert_eq!(*made.borrow(), 2);
}

#[test]
fn a_late_attach_leaves_a_restart_in_its_place() {
    let slots = Slots::new(|c: &Fake| c.0.load(Ordering::SeqCst));
    let host = HostName("vps".into());
    let (send, late) = futures::channel::oneshot::channel::<Fake>();
    let first = slots.get(&host, false, || {
        async move { Ok(late.await.unwrap()) }.boxed_local()
    });
    let restarted = Arc::new(AtomicBool::new(true));
    let fresh = Fake(restarted.clone());
    block_on(slots.get(&host, true, || async move { Ok(fresh) }.boxed_local()))
        .ok()
        .unwrap();
    send.send(Fake(Arc::new(AtomicBool::new(true))))
        .ok()
        .unwrap();
    block_on(first).ok().unwrap();
    let now = block_on(slots.get(&host, false, || panic!("made again")))
        .ok()
        .unwrap();
    assert!(Arc::ptr_eq(&now.0, &restarted));
}

/// Long enough that a host is not asked over and over while its sessions
/// work, short enough that a turn ending is followed soon after.
#[test]
fn a_busy_host_is_asked_again_every_fifteen_seconds() {
    assert_eq!(HOST_IDLE_POLL, std::time::Duration::from_secs(15));
}

#[test]
fn the_waiting_line_counts_the_sessions_and_names_the_host() {
    let host = HostName("vps".into());
    assert_eq!(
        waiting_line(&host, 1),
        "Waiting for 1 session on vps to finish before restarting its host service onto this build"
    );
    assert_eq!(
        waiting_line(&host, 3),
        "Waiting for 3 sessions on vps to finish before restarting its host service onto this build"
    );
}

fn busy(host: &HostName, ids: &[&str]) -> Result<(), HostFailure> {
    Err(HostFailure::Busy {
        host: host.clone(),
        busy: ids.iter().map(|id| HostSession((*id).into())).collect(),
    })
}

/// A busy host waits, once however often it is found busy; the first attach
/// that lands releases every listener, once.
#[test]
fn a_busy_host_waits_until_an_attach_lands() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    assert!(
        waits.listen(&host).is_none(),
        "nothing waits on an idle host"
    );
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    waits.heard(&host, &busy(&host, &["1-1", "2-1"]), false);
    let (a, b) = (waits.listen(&host).unwrap(), waits.listen(&host).unwrap());
    assert_eq!(waits.busy(&host).len(), 2, "the latest sessions said");
    assert!(a.ptr_eq(&b), "one wait");
    assert!(a.clone().now_or_never().is_none());
    // One poll per wait.
    assert!(waits.poll(&host));
    assert!(!waits.poll(&host));

    waits.heard(&host, &Ok(()), false);
    assert_eq!(a.now_or_never(), Some(Ok(Ok(()))));
    assert_eq!(b.now_or_never(), Some(Ok(Ok(()))));
    assert!(waits.listen(&host).is_none(), "released once");
    assert!(waits.busy(&host).is_empty());
    // A later busy is a new wait, polled anew.
    waits.heard(&host, &busy(&host, &["3-1"]), false);
    assert!(waits.listen(&host).is_some());
    assert!(waits.poll(&host));
}

/// Restart now's yes ends the wait as any attach that lands does: the
/// restart's attach is one.
#[test]
fn a_restart_that_lands_ends_the_wait() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    let ended = waits.listen(&host).unwrap();
    // Another host's attach releases nothing here.
    waits.heard(&HostName("other".into()), &Ok(()), false);
    assert!(ended.clone().now_or_never().is_none());
    // `hosts::restart` lands on this build.
    waits.heard(&host, &Ok(()), false);
    assert_eq!(ended.now_or_never(), Some(Ok(Ok(()))));
}

/// A host that cannot be reached is not waited on: every listener hears
/// why, so each place says the real failure rather than that it waits.
#[test]
fn an_unreachable_host_releases_its_waiters_with_the_failure() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    let ended = waits.listen(&host).unwrap();
    waits.heard(
        &host,
        &Err::<(), _>(HostFailure::Other("gone".into())),
        false,
    );
    assert_eq!(ended.now_or_never(), Some(Ok(Err("gone".to_string()))));
    assert!(waits.listen(&host).is_none(), "the wait is over");
}

/// When the last listener goes, so do the wait and its poll; one listening
/// again starts both anew.
#[test]
fn the_wait_ends_with_its_last_listener() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    let (a, b) = (waits.listen(&host).unwrap(), waits.listen(&host).unwrap());
    assert!(waits.poll(&host));
    waits.leave(&host, &a);
    assert!(waits.polling(&host, &b), "one still listens");
    waits.leave(&host, &b);
    assert!(!waits.polling(&host, &b), "no poll");
    assert!(waits.busy(&host).is_empty(), "the wait is gone");
    assert!(waits.listen(&host).is_none());
    // A leave for a wait already gone touches the next one not at all.
    waits.heard(&host, &busy(&host, &["2-1"]), false);
    let c = waits.listen(&host).unwrap();
    waits.leave(&host, &a);
    assert!(waits.poll(&host), "polled anew");
    assert!(waits.polling(&host, &c));
}

/// Being found busy makes no wait by itself: only a listener does, so an
/// attach nobody waits on (a restart's first look) leaves nothing behind.
#[test]
fn a_busy_host_nobody_listens_to_is_not_waited_on() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    assert!(!waits.waited(&host));
    assert!(!waits.poll(&host), "nothing to poll");
    assert!(waits.listen(&host).is_some());
    assert!(waits.waited(&host));
}

/// One poll that misses the host is a blip; enough in a row end the wait
/// on why. A miss outside the poll (Restart now) ends it at once.
#[test]
fn a_wait_outlives_a_blip_but_not_a_host_gone() {
    assert_eq!(HOST_UNREACHABLE_POLLS, 2);
    let host = HostName("vps".into());
    let gone = || Err::<(), _>(HostFailure::Other("gone".into()));
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]), false);
    let ended = waits.listen(&host).unwrap();
    waits.heard(&host, &gone(), true);
    assert!(ended.clone().now_or_never().is_none(), "a blip");
    // Found busy again: the count starts over.
    waits.heard(&host, &busy(&host, &["1-1"]), true);
    waits.heard(&host, &gone(), true);
    assert!(ended.clone().now_or_never().is_none());
    waits.heard(&host, &gone(), true);
    assert_eq!(ended.now_or_never(), Some(Ok(Err("gone".to_string()))));

    waits.heard(&host, &busy(&host, &["2-1"]), false);
    let ended = waits.listen(&host).unwrap();
    waits.heard(&host, &gone(), false);
    assert_eq!(ended.now_or_never(), Some(Ok(Err("gone".to_string()))));
}

/// Two seconds after a connection goes, then twice as long each miss, never
/// more than a minute apart.
#[test]
fn next_delay_doubles_to_the_cap() {
    use std::time::Duration;
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

/// A host another machine attached to is that machine's now: nothing here
/// attaches to it again by itself.
#[test]
fn taken_never_reconnects() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    assert_eq!(waits.state(&host), HostState::Unknown);
    waits.attaching(&host);
    assert_eq!(waits.state(&host), HostState::Attaching);
    waits.heard(&host, &Ok(()), false);
    assert_eq!(waits.state(&host), HostState::Live);
    waits.closed(&host, true);
    assert_eq!(waits.state(&host), HostState::Taken);
    assert!(
        waits.listen_back(&host).is_none(),
        "no loop on a taken host"
    );
}

/// A connection lost is attached again while anything listens: misses never
/// end the wait, the last listener going does, and an attach that lands
/// releases every listener.
#[test]
fn lost_is_listened_to_until_the_last_listener_goes() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    assert!(
        waits.listen_back(&host).is_none(),
        "never attached: not lost"
    );
    waits.heard(&host, &Ok(()), false);
    assert!(waits.listen_back(&host).is_none(), "live: not lost");
    waits.closed(&host, false);
    assert_eq!(
        waits.state(&host),
        HostState::Lost("lost the connection to vps".into())
    );
    let (a, b) = (
        waits.listen_back(&host).unwrap(),
        waits.listen_back(&host).unwrap(),
    );
    assert!(a.ptr_eq(&b), "one loop");
    assert!(waits.poll_back(&host));
    assert!(!waits.poll_back(&host), "one loop per host");
    let gone = || Err::<(), _>(HostFailure::Other("gone".into()));
    for _ in 0..5 {
        waits.heard(&host, &gone(), true);
    }
    assert!(a.clone().now_or_never().is_none(), "misses never end it");
    assert!(matches!(waits.state(&host), HostState::Lost(_)));
    waits.leave(&host, &a);
    assert!(waits.polling_back(&host, &b), "one still listens");
    waits.leave(&host, &b);
    assert!(!waits.polling_back(&host, &b), "the loop stops");
    // Still lost: one listening again starts a new loop.
    let c = waits.listen_back(&host).unwrap();
    assert!(!c.ptr_eq(&b));
    assert!(waits.poll_back(&host));
    waits.heard(&host, &Ok(()), true);
    assert_eq!(c.now_or_never(), Some(Ok(Ok(()))));
    assert_eq!(waits.state(&host), HostState::Live);
}

/// A lost host found running another build hands over to the busy wait.
#[test]
fn a_lost_host_found_busy_ends_the_loop_on_why() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &Ok(()), false);
    waits.closed(&host, false);
    let back = waits.listen_back(&host).unwrap();
    waits.heard(&host, &busy(&host, &["1-1"]), true);
    assert!(matches!(back.now_or_never(), Some(Ok(Err(_)))));
    assert!(waits.listen(&host).is_some(), "the busy wait takes over");
}
