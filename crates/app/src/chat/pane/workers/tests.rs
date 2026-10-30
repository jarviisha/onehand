use super::{short_name, shows_body};
use onehand_core::chat::Link;

#[test]
fn the_short_name_is_before_the_dot() {
    assert_eq!(short_name("b · report r1"), "b");
    assert_eq!(short_name("onehand-sim-b"), "onehand-sim-b");
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

use super::ordered;
use crate::chat::pane::SessionSignal::{AwaitingUser, Busy, Failed};

#[test]
fn a_fresh_order_puts_the_most_urgent_first() {
    let now = [
        (1, Some(Busy)),
        (2, Some(Failed)),
        (3, None),
        (4, Some(AwaitingUser)),
    ];
    assert_eq!(ordered(&[], &now, true), vec![2, 4, 1, 3]);
}

#[test]
fn ties_keep_the_opening_order() {
    let now = [(5, Some(Busy)), (2, Some(Busy)), (9, Some(Busy))];
    assert_eq!(ordered(&[], &now, true), vec![5, 2, 9]);
}

#[test]
fn a_frozen_order_moves_nothing_when_a_state_changes() {
    let now = [(1, Some(Failed)), (2, None), (3, Some(Busy))];
    assert_eq!(ordered(&[3, 2, 1], &now, false), vec![3, 2, 1]);
}

#[test]
fn a_new_worker_joins_the_end_and_a_gone_one_leaves() {
    let now = [(1, None), (3, None), (4, Some(Failed))];
    assert_eq!(ordered(&[3, 2, 1], &now, false), vec![3, 1, 4]);
}
