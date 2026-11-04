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
    // The reader runs on a thread of its own, and a wake from it is activity
    // the test executor refuses: it is let finish before the grid first looks.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let view = tab.view().clone();
    cx.add_window(|window, cx| Root::new(view.clone(), window, cx));
    cx.run_until_parked();
    assert_eq!(exits.get(), 1);
    cx.update(|cx| assert!(view.read(cx).row_text(0).contains("hello")));
    assert_eq!(tab.process_id(), None);
}
