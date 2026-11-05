use super::{BandAct, Standing, band};

#[test]
fn a_worker_its_lead_holds_names_the_lead_and_the_way_back() {
    assert_eq!(
        band(Standing::Coordinated {
            lead: Some("Refactor")
        }),
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
        band(Standing::Coordinated { lead: None }),
        ("Its lead is coordinating this worker".to_string(), None)
    );
}

#[test]
fn a_worker_the_person_holds_offers_it_back() {
    assert_eq!(
        band(Standing::Held { offered: true }),
        ("You hold this worker".to_string(), Some(BandAct::GiveBack))
    );
}

/// Its lead switched off or elsewhere: still the person's, nothing to give
/// back to.
#[test]
fn a_held_worker_not_offered_back_says_only_that() {
    assert_eq!(
        band(Standing::Held { offered: false }),
        ("You hold this worker".to_string(), None)
    );
}
