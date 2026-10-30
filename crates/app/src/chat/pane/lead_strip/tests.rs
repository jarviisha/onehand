use super::{summary, waiting_first};
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

#[test]
fn no_one_waiting_names_no_one() {
    assert_eq!(
        waiting_first(&[(1, Some(Busy)), (2, None), (3, Some(UnseenTurn))]),
        None
    );
}

#[test]
fn the_most_urgent_waiting_is_named() {
    let workers = [
        (1, Some(AwaitingUser)),
        (2, Some(Failed)),
        (3, Some(Failed)),
    ];
    assert_eq!(waiting_first(&workers), Some(2));
}
