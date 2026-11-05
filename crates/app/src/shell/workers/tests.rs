use super::{home, region_closes};

#[test]
fn the_region_follows_a_lead_with_workers() {
    assert!(!region_closes(false, true));
}

#[test]
fn the_region_closes_on_a_session_without_workers() {
    assert!(region_closes(false, false));
}

#[test]
fn the_region_closes_on_a_page() {
    assert!(region_closes(true, true));
}

/// A worker in its worktree (project 3) stands under its lead's project.
#[test]
fn a_worker_stands_for_its_lead_s_project() {
    assert_eq!(home(Some((3, 0)), Some((0, 2))), Some(0));
}

/// A lead in another window is not here to stand under.
#[test]
fn a_worker_of_a_lead_elsewhere_stands_for_its_own() {
    assert_eq!(home(Some((3, 0)), None), Some(3));
}

#[test]
fn a_session_gone_stands_nowhere() {
    assert_eq!(home(None, None), None);
}
