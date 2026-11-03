use super::{HostFailure, Slots, bundled_binary_in};
use futures::FutureExt as _;
use futures::executor::block_on;
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
        Err(HostFailure::Missing(text)) => assert_eq!(
            text,
            "onehand-host is not beside onehand; build it with `cargo build -p onehand-host`"
        ),
        Ok(_) | Err(HostFailure::Busy { .. }) | Err(HostFailure::Other(_)) => {
            panic!("expected Missing")
        }
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
