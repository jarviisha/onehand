use super::{foreground_busy, stream_tab};
use gpui::{TestAppContext, px};
use gpui_component::Root;
use std::cell::Cell;
use std::rc::Rc;

/// A shell at its prompt is its own foreground group; a command in front
/// is a different one; and a terminal that will not say is neither.
#[test]
fn busy_is_a_foreground_group_other_than_the_shell() {
    assert_eq!(foreground_busy(Some(100), Some(100)), Some(false));
    assert_eq!(foreground_busy(Some(100), Some(240)), Some(true));
    assert_eq!(foreground_busy(Some(100), None), None);
    assert_eq!(foreground_busy(None, Some(240)), None);
}

/// What the reader gives reaches the grid, and its end is the tab's exit,
/// told once. A tab fed by a stream has no process here.
#[gpui::test]
fn a_stream_tab_shows_what_its_reader_gives_and_exits_at_eof(cx: &mut TestAppContext) {
    // The reader runs on a thread of its own, and a wake from it is activity
    // the test executor refuses unless parking is allowed.
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let exits = Rc::new(Cell::new(0));
    let tab = cx.update(|cx| {
        let exits = exits.clone();
        stream_tab(
            "sh".into(),
            std::io::Cursor::new(b"hello".to_vec()),
            std::io::sink(),
            |_, _| {},
            px(14.),
            gpui::black(),
            cx,
            move |_, _| exits.set(exits.get() + 1),
        )
    });
    let view = tab.view().clone();
    cx.add_window(|window, cx| Root::new(view.clone(), window, cx));
    let start = std::time::Instant::now();
    while exits.get() == 0 && start.elapsed() < std::time::Duration::from_secs(5) {
        cx.run_until_parked();
        std::thread::yield_now();
    }
    cx.run_until_parked();
    assert_eq!(exits.get(), 1);
    cx.update(|cx| assert!(view.read(cx).row_text(0).contains("hello")));
    assert_eq!(tab.process_id(), None);
}

/// A locked tab's writer drops what it is given, and passes it on again
/// once unlocked.
#[test]
fn a_locked_writer_drops_what_is_typed() {
    use std::io::Write as _;
    use std::sync::{Arc, Mutex};
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let got = Arc::new(Mutex::new(Vec::new()));
    let (mut writer, shut) = super::gated(Sink(got.clone()));
    writer.write_all(b"a").unwrap();
    shut.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(writer.write(b"rm -rf\r").unwrap(), 7);
    shut.store(false, std::sync::atomic::Ordering::Relaxed);
    writer.write_all(b"b").unwrap();
    assert_eq!(&*got.lock().unwrap(), b"ab");
}
