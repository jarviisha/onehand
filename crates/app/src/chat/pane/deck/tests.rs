use super::summary;
use crate::chat::pane::SessionSignal::{AwaitingUser, Busy, Failed, UnseenTurn};
use crate::chat::pane::workers::step;

#[test]
fn the_summary_counts_each_state_most_urgent_first() {
    let signals = [
        Some(Busy),
        None,
        Some(AwaitingUser),
        Some(Busy),
        Some(UnseenTurn),
        Some(Failed),
    ];
    assert_eq!(
        summary(&signals),
        vec![
            (Some(Failed), 1),
            (Some(AwaitingUser), 1),
            (Some(Busy), 2),
            (Some(UnseenTurn), 1),
            (None, 1),
        ]
    );
}

#[test]
fn the_deck_steps_and_stops_at_its_ends() {
    assert_eq!(step(0, 3, true), 1);
    assert_eq!(step(2, 3, true), 2);
    assert_eq!(step(0, 3, false), 0);
    assert_eq!(step(2, 3, false), 1);
}
