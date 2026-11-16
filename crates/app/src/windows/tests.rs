//! The window list on real shells: a session and a project found in the
//! window holding them, a shell skipping itself, and a closed window gone.

use super::*;
use crate::shell::rig::{another, folder, nowhere, shell};
use gpui::TestAppContext;

#[gpui::test]
fn a_session_and_a_project_are_found_in_the_window_holding_them(cx: &mut TestAppContext) {
    let dir = folder("windows-holding");
    let (first, one, cx) = shell(cx, dir.join("tasks"), vec![nowhere()]);
    let root = dir.join("other");
    std::fs::create_dir_all(&root).unwrap();
    let (second, two) = another(cx, &root);
    cx.update(|_, cx| {
        with_shell(second, cx, |shell, window, cx| {
            shell.new_session(window, cx)
        })
    });
    cx.run_until_parked();
    let uid = cx.update(|_, cx| two.read(cx).active_session_uid().expect("a session"));

    cx.update(|_, cx| {
        assert_eq!(live(cx).len(), 2);
        let (at, _) = holding_session(uid, cx).expect("held");
        assert_eq!(at, second);
        assert!(session(uid, cx).is_some());
        let held: Vec<_> = holding_root(&root, cx)
            .into_iter()
            .map(|(at, _)| at)
            .collect();
        assert_eq!(held, [second]);
        let rest: Vec<_> = others(one.entity_id(), cx)
            .into_iter()
            .map(|(at, _)| at)
            .collect();
        assert_eq!(rest, [second], "a shell walking the windows skips itself");
        assert!(holding_session(uid + 1000, cx).is_none());
    });

    cx.update(|_, cx| forget(second.window_id(), cx));
    cx.update(|_, cx| {
        let open: Vec<_> = live(cx).into_iter().map(|(at, _)| at).collect();
        assert_eq!(open, [first], "a closed window is gone from the list");
        assert!(session(uid, cx).is_none());
    });
}

#[gpui::test]
fn work_asked_inside_a_shells_update_reaches_it_once_that_is_over(cx: &mut TestAppContext) {
    let dir = folder("windows-later");
    let (first, one, cx) = shell(cx, dir.join("tasks"), vec![nowhere()]);
    let reached = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = reached.clone();
    // Asked from inside the shell's own update, which `with_shell` there would
    // panic on.
    cx.update(|_, cx| {
        one.update(cx, |_, cx| {
            later(first, cx, move |_, _, _| seen.set(true));
        })
    });
    cx.run_until_parked();
    assert!(reached.get());
}
