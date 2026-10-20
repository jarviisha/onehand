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
    assert_eq!(terminal_press(true, false, false, false), Nothing);
    assert_eq!(terminal_press(false, false, true, true), Close);
    assert_eq!(
        terminal_press(false, false, true, false),
        Open {
            step_aside: false,
            spawn: true
        }
    );
    assert_eq!(
        terminal_press(false, true, true, true),
        Open {
            step_aside: true,
            spawn: false
        }
    );
    assert_eq!(
        terminal_press(false, false, false, true),
        Open {
            step_aside: false,
            spawn: false
        }
    );
}
