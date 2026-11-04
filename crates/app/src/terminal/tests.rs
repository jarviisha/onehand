use super::{place_of, selection_after};

/// Closing a tab ahead of the selected one must move the selection back by
/// one, not leave the index where it was.
///
/// This is the case clamping got wrong and nothing caught: three shells with
/// the middle one on screen, close the first, and an index of 1 now names
/// the *third* — so the panel silently swapped to the one shell the user had
/// neither been reading nor asked to close.
#[test]
fn selection_follows_its_own_tab() {
    assert_eq!(
        selection_after(1, &[0], 2),
        0,
        "closing ahead pulls it back"
    );
    assert_eq!(
        selection_after(1, &[2], 2),
        1,
        "closing behind moves nothing"
    );
    assert_eq!(
        selection_after(2, &[0, 1], 1),
        0,
        "two ahead, two steps back"
    );
}

/// The selected tab going itself falls to whatever took its place, and to
/// the last tab when it was the last.
#[test]
fn closing_the_selected_tab_falls_to_a_neighbour() {
    assert_eq!(
        selection_after(1, &[1], 2),
        1,
        "the tab that shifted up into it"
    );
    assert_eq!(
        selection_after(2, &[2], 2),
        1,
        "nothing after it, so the one before"
    );
    assert_eq!(
        selection_after(0, &[0], 0),
        0,
        "the last tab leaves nothing to select"
    );
}

/// A sweep is the same rule: what matters is how many went *ahead* of the
/// selection, not how many went.
#[test]
fn a_sweep_counts_only_what_was_ahead() {
    assert_eq!(selection_after(3, &[0, 4], 3), 2);
    assert_eq!(selection_after(3, &[4, 5], 4), 3);
}

/// A closing question answered after the strip changed finds the shell
/// it asked about, or nothing at all, and the selection stays on the shell
/// it was on either way.
#[test]
fn an_answer_finds_its_tab_by_id_or_nothing() {
    // Three shells with ids 1, 2 and 3, the third selected; the question
    // is about the second.
    let (mut ids, mut active) = (vec![1, 2, 3], 2);
    // The first exits while the question is open.
    ids.remove(0);
    active = selection_after(active, &[0], ids.len());
    assert_eq!((ids[active], place_of(&ids, 2)), (3, Some(0)));
    // The answer closes the second, and the third stays selected.
    let at = place_of(&ids, 2).unwrap();
    ids.remove(at);
    active = selection_after(active, &[at], ids.len());
    assert_eq!(ids[active], 3);
    // A tab already gone answers nothing, so nothing else closes.
    assert_eq!(place_of(&ids, 2), None);
}

mod on_host {
    use super::super::TerminalPanel;
    use gpui::{Entity, TestAppContext, WindowHandle};
    use gpui_component::Root;
    use onehand_core::host::client::{HostConn, Recorded};
    use onehand_core::host::wire::{Call, HostTerm};
    use onehand_core::workspace::HostName;
    use std::path::PathBuf;

    const TERM: HostTerm = HostTerm(7);

    /// A dock in a window, on a host project holding one tab for [`TERM`],
    /// read and written over a connection that writes down what it is told.
    ///
    /// The grid reads its stream on a thread of its own, and a wake from it
    /// is activity the test executor refuses. So a stream is only ended
    /// before the grid's reader is first polled ([`ended_early`]) or after
    /// its tab is gone.
    fn dock(cx: &mut TestAppContext) -> (Entity<TerminalPanel>, WindowHandle<Root>, Recorded) {
        cx.update(gpui_component::init);
        let panel = cx.update(TerminalPanel::new);
        let window = cx.add_window({
            let panel = panel.clone();
            |window, cx| Root::new(panel, window, cx)
        });
        let (conn, recorded) = HostConn::recording("vps");
        panel.update(cx, |panel, cx| {
            let home = (Some(HostName("vps".into())), PathBuf::from("/srv/app"));
            panel.root = Some(home.clone());
            panel.add_host_tab(home, conn, TERM, cx);
        });
        (panel, window, recorded)
    }

    /// End the tab's stream and let its reader thread finish before the
    /// grid's task first looks: reading what was fired drops the watch's end.
    fn ended_early(recorded: &Recorded) -> Vec<Call> {
        let fired = recorded.fired();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fired
    }

    /// Let the tab go, then the connection, so the reader's end wakes nothing.
    fn let_go(panel: &Entity<TerminalPanel>, recorded: Recorded, cx: &mut TestAppContext) {
        panel.update(cx, |panel, cx| {
            panel.forget_root(&PathBuf::from("/srv/app"), Some(HostName("vps".into())), cx)
        });
        cx.run_until_parked();
        drop(recorded);
    }

    fn closes(calls: &[Call]) -> Vec<HostTerm> {
        calls
            .iter()
            .filter_map(|call| match call {
                Call::TermClose { term } => Some(*term),
                _ => None,
            })
            .collect()
    }

    /// A tab whose stream ended (the shell exited, the attach was lost) goes,
    /// and asks the host to end nothing: whatever is still there runs on.
    #[gpui::test]
    fn a_host_tab_that_exits_closes_nothing_on_the_host(cx: &mut TestAppContext) {
        let (panel, _window, recorded) = dock(cx);
        assert!(panel.read_with(cx, |panel, _| panel.has_shell()));
        let mut fired = ended_early(&recorded);
        for _ in 0..200 {
            cx.run_until_parked();
            if !panel.read_with(cx, |panel, _| panel.has_shell()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!panel.read_with(cx, |panel, _| panel.has_shell()));
        fired.extend(recorded.fired());
        assert_eq!(closes(&fired), []);
    }

    /// Closing a tab ends its shell on the host, once.
    #[gpui::test]
    fn closing_a_host_tab_closes_its_terminal(cx: &mut TestAppContext) {
        let (panel, window, recorded) = dock(cx);
        cx.run_until_parked();
        let home = panel.read_with(cx, |panel, _| panel.root.clone().unwrap());
        window
            .update(cx, |_, window, cx| {
                panel.update(cx, |panel, cx| {
                    let id = panel.shells[&home].tabs[0].id;
                    panel.close_tab_id(&home, id, window, cx);
                })
            })
            .unwrap();
        assert!(!panel.read_with(cx, |panel, _| panel.has_shell()));
        cx.run_until_parked();
        assert_eq!(closes(&recorded.fired()), [TERM]);
    }

    /// A shell on a host is no process of this machine's, so removing a
    /// folder here never warns about it.
    #[gpui::test]
    fn host_tabs_are_not_running_processes(cx: &mut TestAppContext) {
        let (panel, _window, recorded) = dock(cx);
        cx.run_until_parked();
        assert!(panel.read_with(cx, |panel, _| panel.has_shell()));
        assert_eq!(panel.read_with(cx, |panel, _| panel.processes()), []);
        let_go(&panel, recorded, cx);
    }
}
