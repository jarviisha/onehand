use super::*;

/// The rail at its narrowest, in rems at the default rem size.
const RAIL: f32 = 14.5;
/// The width the Workbench opens at.
const DOCK_PREF: f32 = 30.;

#[test]
fn presentation_follows_the_budget() {
    let shown = |avail, open, zoom| presentation(avail, DOCK_PREF, open, zoom, false);
    // An 800px window leaves 35.5rem beside the rail: no room for 30 + 24.
    assert_eq!(shown(50. - RAIL, true, 1.), Presentation::WorkbenchFocus);
    assert_eq!(shown(100. - RAIL, true, 1.), Presentation::Split(DOCK_PREF));
    // The conversation's zoom raises its minimum: 83rem holds 30 + 30 at
    // 100%, but at 200% the chat wants 60 and leaves 23.
    assert_eq!(shown(83., true, 1.), Presentation::Split(DOCK_PREF));
    assert_eq!(shown(83., true, 2.), Presentation::WorkbenchFocus);
    assert_eq!(shown(35.5, false, 1.), Presentation::Conversation);
}

#[test]
fn a_narrow_window_draws_the_dock_narrower_without_forgetting_its_width() {
    // Dragged wide to 50rem; 70rem of room leaves the chat its 30.
    assert_eq!(
        presentation(70., 50., true, 1., true),
        Presentation::Split(40.)
    );
    // Down to the dock's minimum before the Workbench takes the area.
    assert_eq!(
        presentation(54., 50., true, 1., true),
        Presentation::Split(DOCK_MIN)
    );
    assert_eq!(
        presentation(53., 50., true, 1., true),
        Presentation::WorkbenchFocus
    );
    // Room again: the dragged width, untouched.
    assert_eq!(
        presentation(120., 50., true, 1., true),
        Presentation::Split(50.)
    );
}

#[test]
fn a_split_holds_at_its_threshold_and_needs_slack_to_come_back() {
    let edge = CHAT_MIN + DOCK_MIN;
    assert!(matches!(
        presentation(edge, DOCK_PREF, true, 1., true),
        Presentation::Split(_)
    ));
    assert_eq!(
        presentation(edge, DOCK_PREF, true, 1., false),
        Presentation::WorkbenchFocus
    );
    assert!(matches!(
        presentation(edge + SPLIT_SLACK, DOCK_PREF, true, 1., false),
        Presentation::Split(_)
    ));
}
