use super::*;

#[test]
fn an_issue_shown_before_the_read_lands_stays_selected() {
    let mut issues = Issues::default();
    issues
        .create(
            Draft {
                title: "one".into(),
                ..Draft::default()
            },
            1,
        )
        .unwrap();
    // The entry `show_issue` makes, then the read `load` lands into it.
    let mut state = RootIssues::default();
    state.show(1);
    state.land(issues);
    assert_eq!(state.selected, Some(1));
    assert!(state.issues.is_some());
}
