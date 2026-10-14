use super::{OrderedWriter, Saves, Saving};
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

#[test]
fn a_superseded_write_failing_puts_nothing_back() {
    let mut saves = Saves::default();
    saves.change("theme", "light", "dark");
    let older = saves.write_all();
    saves.change("theme", "dark", "dim");
    let newer = saves.write_all();
    assert_eq!(saves.landed(older, Err("busy".to_string())), None);
    assert_eq!(saves.landed(newer, Ok(())), None);
}

#[test]
fn the_newest_write_failing_puts_back_the_last_saved_value_not_the_one_before() {
    let mut saves = Saves::default();
    saves.change("name", "one", "two");
    let first = saves.write_all();
    assert_eq!(saves.landed(first, Ok(())), None);
    saves.change("name", "two", "three");
    let older = saves.write_all();
    saves.change("name", "three", "four");
    let newer = saves.write_all();
    // "three" was never saved: its write failed behind a newer one.
    assert_eq!(saves.landed(older, Err("busy".to_string())), None);
    let said = saves.landed(newer, Err("read-only".to_string()));
    assert_eq!(said, Some(("read-only".to_string(), vec!["two"])));
}

#[test]
fn two_things_in_one_file_are_put_back_apart() {
    let mut saves = Saves::default();
    saves.change("theme", "light", "dark");
    let theme = saves.write_of(["theme"]);
    saves.change("keys", "a", "b");
    let keys = saves.write_of(["keys"]);
    // The later write carries only the shortcut, so the theme's own failure
    // is still the newest for the theme.
    assert_eq!(
        saves.landed(theme, Err("full".to_string())),
        Some(("full".to_string(), vec!["light"]))
    );
    assert_eq!(saves.landed(keys, Ok(())), None);
}

#[test]
fn a_whole_snapshot_after_a_change_decides_for_it() {
    let mut saves = Saves::default();
    saves.change("pin", "off", "on");
    let pin = saves.write_all();
    // A dragged size: nothing changes that is put back, but the write
    // carries the pin as well.
    let size = saves.write_all();
    assert_eq!(saves.landed(pin, Err("busy".to_string())), None);
    assert_eq!(
        saves.landed(size, Err("full".to_string())),
        Some(("full".to_string(), vec!["off"]))
    );
}

#[test]
fn a_write_that_lands_advances_what_is_saved() {
    let mut saves = Saves::default();
    saves.change("pin", "off", "on");
    let on = saves.write_all();
    assert_eq!(saves.landed(on, Ok(())), None);
    saves.change("pin", "on", "off");
    let off = saves.write_all();
    assert_eq!(
        saves.landed(off, Err("full".to_string())),
        Some(("full".to_string(), vec!["on"]))
    );
    // Nothing changed since, so a newest write failing has nothing to put
    // back, and still says so once.
    let size = saves.write_all();
    assert_eq!(
        saves.landed(size, Err("full".to_string())),
        Some(("full".to_string(), vec![]))
    );
}

#[test]
fn a_write_waited_for_lands_after_the_ones_before_it() {
    let writer = OrderedWriter::spawn("onehand-test-writer");
    let landed = Arc::new(Mutex::new(Vec::new()));
    let first = {
        let landed = landed.clone();
        writer.write(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            landed.lock().unwrap().push(1);
            Ok(())
        })
    };
    let within = std::time::Duration::from_secs(5);
    let second = {
        let landed = landed.clone();
        writer.write_within(
            move || {
                landed.lock().unwrap().push(2);
                Ok(2)
            },
            within,
        )
    };
    assert_eq!(second, Ok(2));
    assert!(writer.flush(within));
    assert_eq!(futures::executor::block_on(first), Ok(()));
    assert_eq!(*landed.lock().unwrap(), [1, 2]);
}

/// A change made after a write was sent, and still waiting for its own write
/// (a debounced rename), is not put back by that write failing: its own
/// write decides. The failure is still said, since no later write was made.
#[test]
fn a_change_still_to_be_written_is_not_put_back_by_an_older_failure() {
    let mut saves = Saves::default();
    saves.change("name", "one", "two");
    let sent = saves.write_all();
    saves.change("name", "two", "three");
    let said = saves.landed(sent, Err("read-only".to_string()));
    assert_eq!(said, Some(("read-only".to_string(), vec![])));
    let next = saves.write_all();
    let said = saves.landed(next, Err("read-only".to_string()));
    assert_eq!(said, Some(("read-only".to_string(), vec!["one"])));
}
