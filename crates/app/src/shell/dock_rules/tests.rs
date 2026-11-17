use super::*;
use std::path::{Path, PathBuf};

#[test]
fn a_session_switch_inside_a_project_moves_nothing() {
    let mut homes = TerminalHomes::seeded(Some(PathBuf::from("/a")), true);
    assert_eq!(homes.arrive(Path::new("/a"), true), None);
}

#[test]
fn each_project_gets_back_what_it_left() {
    let mut homes = TerminalHomes::seeded(Some(PathBuf::from("/a")), false);
    assert_eq!(
        homes.arrive(Path::new("/b"), true),
        Some(false),
        "never opened in /b"
    );
    assert_eq!(
        homes.arrive(Path::new("/a"), false),
        Some(true),
        "/a was left open"
    );
    assert_eq!(homes.arrive(Path::new("/b"), true), Some(false));
}

#[test]
fn a_page_files_the_state_and_the_next_arrival_restores_it() {
    let mut homes = TerminalHomes::seeded(Some(PathBuf::from("/a")), false);
    homes.put_away(true);
    assert_eq!(homes.arrive(Path::new("/a"), false), Some(true));
}

#[test]
fn a_forgotten_root_arrives_closed() {
    let mut homes = TerminalHomes::seeded(Some(PathBuf::from("/a")), true);
    homes.forget(Path::new("/a"));
    assert_eq!(homes.arrive(Path::new("/a"), true), Some(false));
}

#[test]
fn the_terminal_key_opens_an_empty_dock_rather_than_closing_it() {
    use TerminalPress::*;
    assert_eq!(terminal_press(true, false, false), Nothing);
    assert_eq!(terminal_press(false, true, true), Close);
    assert_eq!(terminal_press(false, true, false), Open { spawn: true });
    assert_eq!(terminal_press(false, false, true), Open { spawn: false });
}

/// The terminal spans the conversation and the Workbench, so a Workbench
/// filling the area hides no terminal: the key toggles it as in a split, and
/// nothing in the press reads the Workbench at all.
#[test]
fn the_terminal_key_toggles_under_a_workbench_that_fills_the_area() {
    use TerminalPress::*;
    assert_eq!(terminal_press(false, true, true), Close);
    assert_eq!(terminal_press(false, false, true), Open { spawn: false });
}

/// **A panel that is not on screen is not the one a panel command means.**
/// Closing the terminal remembered it as the panel last aimed at, so with the
/// caret in the rail the maximize key blew up a terminal that was not there,
/// and its strip's Hide then did nothing.
#[test]
fn a_remembered_panel_off_screen_falls_back_to_the_conversation() {
    use super::super::FocusedPanel as P;
    assert_eq!(addressed(P::Terminal, false, true), P::Chat);
    assert_eq!(addressed(P::Workbench, true, false), P::Chat);
    assert_eq!(addressed(P::Terminal, true, false), P::Terminal);
    assert_eq!(addressed(P::Workbench, false, true), P::Workbench);
    assert_eq!(addressed(P::Chat, false, false), P::Chat);
}
