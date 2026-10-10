use super::*;

/// The rail at its narrowest, in rems at the default rem size.
const RAIL: f32 = 14.5;
/// The width the Workbench opens at.
const DOCK_PREF: f32 = 30.;

/// What the rule shows, in plain rems, so the cases read as the lab's do.
#[derive(Debug, PartialEq)]
enum Shown {
    Conversation,
    Split(f32),
    WorkbenchFocus,
}

fn shown(avail: f32, dock: f32, open: bool, zoom: f32, was_split: bool) -> Shown {
    match presentation(rems(avail), rems(dock), open, zoom, was_split) {
        Presentation::Conversation => Shown::Conversation,
        Presentation::Split(width) => Shown::Split(width.0),
        Presentation::WorkbenchFocus => Shown::WorkbenchFocus,
    }
}

#[test]
fn presentation_follows_the_budget() {
    let at = |avail, open, zoom| shown(avail, DOCK_PREF, open, zoom, false);
    // An 800px window leaves 35.5rem beside the rail: no room for 30 + 24.
    assert_eq!(at(50. - RAIL, true, 1.), Shown::WorkbenchFocus);
    assert_eq!(at(100. - RAIL, true, 1.), Shown::Split(DOCK_PREF));
    // The conversation's zoom raises its minimum: 83rem holds 30 + 30 at
    // 100%, but at 200% the chat wants 60 and leaves 23.
    assert_eq!(at(83., true, 1.), Shown::Split(DOCK_PREF));
    assert_eq!(at(83., true, 2.), Shown::WorkbenchFocus);
    assert_eq!(at(35.5, false, 1.), Shown::Conversation);
}

#[test]
fn a_narrow_window_draws_the_dock_narrower_without_forgetting_its_width() {
    // Dragged wide to 50rem; 70rem of room leaves the chat its 30.
    assert_eq!(shown(70., 50., true, 1., true), Shown::Split(40.));
    // Down to the dock's minimum before the Workbench takes the area.
    assert_eq!(shown(54., 50., true, 1., true), Shown::Split(DOCK_MIN.0));
    assert_eq!(shown(53., 50., true, 1., true), Shown::WorkbenchFocus);
    // Room again: the dragged width, untouched.
    assert_eq!(shown(120., 50., true, 1., true), Shown::Split(50.));
}

#[test]
fn a_split_holds_at_its_threshold_and_needs_slack_to_come_back() {
    let edge = CHAT_MIN.0 + DOCK_MIN.0;
    assert!(matches!(
        shown(edge, DOCK_PREF, true, 1., true),
        Shown::Split(_)
    ));
    assert_eq!(
        shown(edge, DOCK_PREF, true, 1., false),
        Shown::WorkbenchFocus
    );
    assert!(matches!(
        shown(edge + SPLIT_SLACK.0, DOCK_PREF, true, 1., false),
        Shown::Split(_)
    ));
}

#[test]
fn the_terminal_leaves_the_conversation_room_to_read() {
    let at = |wanted, area| terminal_height(rems(wanted), rems(area)).0;
    let reading = BAR_H.0 + READING_MIN_H.0;
    // Room for both: the dragged height.
    assert_eq!(at(15., 60.), 15.);
    // Dragged tall: cut where the conversation would drop under its minimum.
    assert_eq!(at(50., 40.), 40. - reading);
    // A short window never shrinks it to nothing.
    assert_eq!(at(15., reading), TERM_MIN_H.0);
}
