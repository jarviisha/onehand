use super::{BandAct, band};

#[test]
fn a_worker_its_lead_holds_names_the_lead_and_the_way_back() {
    assert_eq!(
        band(Some("Refactor"), false, false, true),
        (
            "Refactor is coordinating this worker".to_string(),
            Some(BandAct::BackToLead)
        )
    );
}

/// A lead in another window: this pane knows no name for it, and it is not
/// a session this window shows.
#[test]
fn a_lead_elsewhere_is_said_without_the_way_back() {
    assert_eq!(
        band(None, false, false, false),
        ("Its lead is coordinating this worker".to_string(), None)
    );
}

#[test]
fn a_worker_the_person_holds_offers_it_back() {
    assert_eq!(
        band(Some("Refactor"), true, true, true),
        ("You hold this worker".to_string(), Some(BandAct::GiveBack))
    );
}

/// Its lead switched off: still the person's, nothing to give back to.
#[test]
fn a_held_worker_not_offered_back_says_only_that() {
    assert_eq!(
        band(Some("Refactor"), true, false, true),
        ("You hold this worker".to_string(), None)
    );
}

/// Held, the lead's name no longer matters: the band speaks of the person.
#[test]
fn a_held_worker_of_a_lead_elsewhere_says_only_that() {
    assert_eq!(
        band(None, true, false, false),
        ("You hold this worker".to_string(), None)
    );
}
