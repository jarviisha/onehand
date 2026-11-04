use super::{HOST_IDLE_POLL, HostFailure, Slots, Waits, bundled_binary_in, waiting_line};
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
/// that lands releases everything waiting on it, once.
#[test]
fn a_busy_host_waits_until_an_attach_lands() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    assert!(
        waits.ended(&host).is_none(),
        "nothing waits on an idle host"
    );
    assert!(waits.heard(&host, &busy(&host, &["1-1"])), "a wait begins");
    assert!(
        !waits.heard(&host, &busy(&host, &["1-1", "2-1"])),
        "the same wait"
    );
    assert_eq!(waits.busy(&host).len(), 2, "the latest sessions said");
    let (a, b) = (waits.ended(&host).unwrap(), waits.ended(&host).unwrap());
    assert!(a.clone().now_or_never().is_none());
    // One poll per wait.
    assert!(waits.poll(&host));
    assert!(!waits.poll(&host));
    // A host that cannot be reached still waits.
    waits.heard(&host, &Err::<(), _>(HostFailure::Other("gone".into())));
    assert!(b.clone().now_or_never().is_none());

    waits.heard(&host, &Ok(()));
    assert!(a.now_or_never().is_some() && b.now_or_never().is_some());
    assert!(waits.ended(&host).is_none(), "released once");
    assert!(waits.busy(&host).is_empty());
    // A later busy is a new wait, polled anew.
    assert!(waits.heard(&host, &busy(&host, &["3-1"])));
    assert!(waits.poll(&host));
}

/// Restart now's yes ends the wait as any attach that lands does: the
/// restart's attach is one.
#[test]
fn a_restart_that_lands_ends_the_wait() {
    let host = HostName("vps".into());
    let mut waits = Waits::default();
    waits.heard(&host, &busy(&host, &["1-1"]));
    let ended = waits.ended(&host).unwrap();
    // Another host's attach releases nothing here.
    waits.heard(&HostName("other".into()), &Ok(()));
    assert!(ended.clone().now_or_never().is_none());
    // `hosts::restart` lands on this build.
    waits.heard(&host, &Ok(()));
    assert!(ended.now_or_never().is_some());
}
