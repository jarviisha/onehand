use super::*;
use onehand_core::workflow::StepKind;

#[test]
fn grid_columns_steps_down() {
    for (width, columns) in [
        (64., 4),
        (59., 4),
        (58.9, 3),
        (44., 3),
        (43.9, 2),
        (29., 2),
        (28.9, 1),
        (5., 1),
    ] {
        assert_eq!(grid_columns(gpui::rems(width)), columns, "at {width}rem");
    }
}

#[test]
fn tone_by_kind() {
    assert!(matches!(tone(&StepKind::Push), Tone::Forge));
    assert!(matches!(tone(&StepKind::PullRequest), Tone::Forge));
    assert!(matches!(
        tone(&StepKind::Approval { of: "plan".into() }),
        Tone::Approval
    ));
    assert!(matches!(
        tone(&StepKind::Command {
            command: None,
            on_fail: "plan".into()
        }),
        Tone::Command
    ));
}
