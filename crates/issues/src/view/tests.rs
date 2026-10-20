use super::*;
use onehand_core::issues::Draft;

#[test]
fn an_issue_is_worked_by_the_latest_session_it_names_that_is_still_live() {
    let mut issues = Issues::default();
    let n = issues
        .create(
            Draft {
                title: "one".into(),
                ..Draft::default()
            },
            1,
        )
        .unwrap();
    issues.taken_up(n, "first", "old".into(), 2).unwrap();
    issues.taken_up(n, "second", "new".into(), 3).unwrap();
    let issue = issues.get(n).unwrap();
    let live = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    assert_eq!(working_in(issue, &live(&["old", "new"])), Some("new"));
    // The newer one ended: the older one, still going, is where the work is.
    assert_eq!(working_in(issue, &live(&["old"])), Some("old"));
    assert_eq!(working_in(issue, &live(&["elsewhere"])), None);
}
