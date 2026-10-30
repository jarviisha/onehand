use super::{shows_body, tails_fit};
use onehand_core::chat::Link;

const TAIL: f32 = 72.;

#[test]
fn every_tail_is_kept_when_there_is_room() {
    assert!(tails_fit(600., 600., true, 2, TAIL));
}

/// Measured with its tails and over: they go first.
#[test]
fn the_tails_go_first() {
    assert!(!tails_fit(600., 640., true, 2, TAIL));
}

/// Dropped, they come back only once they fit again, so the stack never
/// flips between the two from one frame to the next.
#[test]
fn dropped_tails_come_back_only_once_they_fit() {
    assert!(!tails_fit(600., 500., false, 2, TAIL));
    assert!(tails_fit(700., 500., false, 2, TAIL));
}

/// No composer is drawn, so none may take the caret, until the worker is up.
#[test]
fn a_worker_connecting_for_the_first_time_shows_no_body() {
    assert!(!shows_body(false, Link::Connecting, false));
    assert!(!shows_body(true, Link::Connecting, false));
    assert!(shows_body(true, Link::Connected, false));
}

/// A reconnect keeps the conversation on screen, as the pane does.
#[test]
fn a_worker_reconnecting_keeps_its_body() {
    assert!(shows_body(true, Link::Connecting, true));
}
