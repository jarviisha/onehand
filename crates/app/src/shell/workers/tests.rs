use super::region_closes;

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
