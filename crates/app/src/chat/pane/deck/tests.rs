use super::summary;
use crate::chat::pane::SessionSignal::{AwaitingUser, Busy, Failed, UnseenTurn};

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
