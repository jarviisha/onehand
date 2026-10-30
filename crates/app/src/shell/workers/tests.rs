use super::{Arrival, Follow, arrival, follow};

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
    assert_eq!(follow(true, false, true), Follow::Stay);
}

#[test]
fn the_region_closes_on_a_session_without_workers() {
    assert_eq!(follow(true, false, false), Follow::Close);
}

#[test]
fn the_region_closes_on_a_page() {
    assert_eq!(follow(true, true, true), Follow::Close);
}

#[test]
fn a_closed_region_stays_closed() {
    assert_eq!(follow(false, true, false), Follow::Stay);
}
