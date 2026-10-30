use super::{Arrival, arrival, region_closes};

#[test]
fn a_plain_session_shows_itself() {
    assert_eq!(arrival(7, None, false), Arrival::Show);
}

#[test]
fn a_worker_shows_its_lead_at_it() {
    assert_eq!(
        arrival(7, Some(1), true),
        Arrival::Lead { lead: 1, worker: 7 }
    );
}

/// A lead in another window has nothing here to show it through.
#[test]
fn a_worker_whose_lead_is_elsewhere_shows_itself() {
    assert_eq!(arrival(7, Some(1), false), Arrival::Show);
}

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
