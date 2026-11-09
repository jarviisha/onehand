//! The hosts' carrier against its contract: a wait's end reaches every
//! listener once, and the last listener going ends the wait and its clock,
//! so nothing attaches for it. An attempt's landing is played on the drive
//! here: a real attach runs on a runtime the test scheduler does not drive.

use super::{HostFailure, held, idle, perform, waited};
use crate::state::Shared;
use futures::FutureExt as _;
use gpui::TestAppContext;
use onehand_core::host::drive::{Got, HOST_IDLE_POLL};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;

fn app(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| cx.set_global(Shared::for_test()));
}

fn vps() -> HostName {
    HostName("vps".into())
}

/// The drive hears a person's attach to `host` land on `result`.
fn lands(cx: &mut TestAppContext, result: Result<(), HostFailure>) {
    cx.update(|cx| {
        let fx = {
            let held = held(cx);
            let mut h = held.borrow_mut();
            let Got::Make(made) = h.drive.ask(&vps(), true, true, false, false) else {
                unreachable!()
            };
            h.drive.landed(&vps(), made, &result).fx
        };
        perform(cx, fx);
    });
}

fn busy() -> Result<(), HostFailure> {
    Err(HostFailure::Busy {
        host: vps(),
        busy: vec![HostSession("1-1".into())],
        terms: 0,
    })
}

/// Every place waiting on a busy host hears the wait end, once, with how.
#[gpui::test]
fn a_waits_end_reaches_every_listener_once(cx: &mut TestAppContext) {
    app(cx);
    lands(cx, busy());
    let (a, b) = cx.update(|cx| (idle(cx, &vps()).unwrap(), idle(cx, &vps()).unwrap()));
    let (mut a, b) = (a.boxed_local(), b.boxed_local());
    assert!((&mut a).now_or_never().is_none(), "it waits");
    lands(cx, Ok(()));
    cx.run_until_parked();
    assert_eq!(a.now_or_never(), Some(Ok(())));
    assert_eq!(b.now_or_never(), Some(Ok(())));
    assert!(cx.update(|cx| !waited(cx, &vps())), "ended once");
}

/// The last listener going ends the wait: its clock coming round attaches
/// for nothing.
#[gpui::test]
fn the_last_listener_going_ends_the_wait_and_its_clock(cx: &mut TestAppContext) {
    app(cx);
    lands(cx, busy());
    let listening = cx.update(|cx| idle(cx, &vps()).unwrap());
    assert!(cx.update(|cx| waited(cx, &vps())));
    drop(listening);
    assert!(cx.update(|cx| !waited(cx, &vps())));
    cx.executor().advance_clock(HOST_IDLE_POLL * 2);
    cx.run_until_parked();
    let attempts = cx.update(|cx| held(cx).borrow().attempts.len());
    assert_eq!(attempts, 0, "nothing attached for a wait that went");
}
