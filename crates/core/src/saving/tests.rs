use super::{OrderedWriter, Saving};
use std::sync::{Arc, Mutex};

#[test]
fn two_changes_in_a_row_land_in_order() {
    let writer = OrderedWriter::spawn("onehand-test-writer");
    let landed = Arc::new(Mutex::new(Vec::new()));
    let first = {
        let landed = landed.clone();
        writer.write(move || {
            // Slow, so a write that did not wait its turn would land first.
            std::thread::sleep(std::time::Duration::from_millis(30));
            landed.lock().unwrap().push(1);
            Ok(())
        })
    };
    let second = {
        let landed = landed.clone();
        writer.write(move || {
            landed.lock().unwrap().push(2);
            Ok(())
        })
    };
    futures::executor::block_on(async {
        assert_eq!(second.await, Ok(()));
        assert_eq!(first.await, Ok(()));
    });
    assert_eq!(*landed.lock().unwrap(), [1, 2]);
}

#[test]
fn the_newest_change_failing_puts_back_the_last_saved_value() {
    let mut name = Saving::new("one".to_string());
    let a = name.change("two".to_string());
    assert_eq!(name.landed(a, "two".to_string(), Ok(())), None);
    let b = name.change("three".to_string());
    assert_eq!(name.shown(), "three");
    let said = name.landed(b, "three".to_string(), Err("read-only".to_string()));
    assert_eq!(said.as_deref(), Some("read-only"));
    assert_eq!(name.shown(), "two", "back to the last value known saved");
}

#[test]
fn a_superseded_change_failing_does_nothing() {
    let mut name = Saving::new("one".to_string());
    let a = name.change("two".to_string());
    let b = name.change("three".to_string());
    assert_eq!(
        name.landed(a, "two".to_string(), Err("busy".to_string())),
        None
    );
    assert_eq!(
        name.shown(),
        "three",
        "the newer change carries what is shown"
    );
    assert_eq!(name.landed(b, "three".to_string(), Ok(())), None);
    assert_eq!(name.shown(), "three");
}
