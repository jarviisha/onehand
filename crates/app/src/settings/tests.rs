use super::{DraftShift, check_key, draft_after_promote, draft_shift};
use onehand_core::config::AgentSpec;

/// Two agents on one launcher are two agents: a result filed for one must
/// not be shown under the other.
#[test]
fn a_test_result_belongs_to_the_whole_command_line() {
    let spec = |args: &[&str]| AgentSpec {
        name: "a".into(),
        command: "npx".into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        auth: Default::default(),
    };
    assert_ne!(
        check_key(&spec(&["-y", "one"])),
        check_key(&spec(&["-y", "two"]))
    );
    assert_eq!(
        check_key(&spec(&["-y", "one"])),
        check_key(&spec(&["-y", "one"]))
    );
}

/// Making an agent the default moves a form open on any agent ahead of it.
#[test]
fn making_an_agent_default_keeps_the_form_on_its_agent() {
    assert_eq!(draft_after_promote(None, 2), None);
    // The agent being edited is the one moved to the front.
    assert_eq!(draft_after_promote(Some(2), 2), Some(0));
    // Ahead of it: pushed down by one.
    assert_eq!(draft_after_promote(Some(0), 2), Some(1));
    assert_eq!(draft_after_promote(Some(1), 2), Some(2));
    // Behind it: nothing moved.
    assert_eq!(draft_after_promote(Some(3), 2), Some(3));
}

/// Deleting an agent moves a form open on another one with it.
///
/// Every case here is a save that would otherwise land on the wrong agent:
/// the list shifts under a position the form is still holding.
#[test]
fn deleting_an_agent_moves_the_form_off_the_hole() {
    assert_eq!(draft_shift(None, 0), DraftShift::Keep);
    // Deleted below the form: nothing the form points at has moved.
    assert_eq!(draft_shift(Some(0), 2), DraftShift::Keep);
    // Deleted above it: everything after the hole slid down one.
    assert_eq!(draft_shift(Some(2), 0), DraftShift::MoveTo(1));
    assert_eq!(draft_shift(Some(1), 0), DraftShift::MoveTo(0));
    // The agent being edited is the one that went.
    assert_eq!(draft_shift(Some(1), 1), DraftShift::Clear);
}
