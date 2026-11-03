use super::{HostFlow, Want, landed_on, needs_restart};
use crate::hosts::HostFailure;
use onehand_core::workspace::HostName;

#[test]
fn a_yes_restarts_the_service_only_while_it_is_still_busy() {
    let busy: Result<(), HostFailure> = Err(HostFailure::Busy {
        host: HostName("box".into()),
        busy: Vec::new(),
    });
    assert!(needs_restart(&busy));
    // Already on this build, since an earlier yes: nothing to stop.
    assert!(!needs_restart(&Ok::<(), HostFailure>(())));
    assert!(!needs_restart(&Err::<(), _>(HostFailure::Other(
        "gone".into()
    ))));
}

#[test]
fn one_question_per_host_gathers_every_session_that_waits_on_it() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = HostFlow::default();
    let asked = flow.join(&a, Want::Session(1));
    assert!(asked.is_some());
    assert!(flow.join(&a, Want::Session(2)).is_none());
    assert!(flow.join(&a, Want::Add).is_none());
    // Another host is another question.
    let other = flow.join(&b, Want::Session(3));
    assert!(other.is_some());
    assert_eq!(flow.take(&a), Some((vec![1, 2], true)));
    assert_eq!(flow.take(&a), None);
    assert_eq!(flow.take(&b), Some((vec![3], false)));
}

#[test]
fn a_question_put_away_unanswered_is_asked_again() {
    let host = HostName("a".into());
    let mut flow = HostFlow::default();
    let asked = flow.join(&host, Want::Session(1));
    // Kept: the dialog, and the answer it held, are gone.
    drop(asked);
    assert!(flow.join(&host, Want::Session(2)).is_some());
    assert_eq!(flow.take(&host), Some((vec![2], false)));
}

#[test]
fn an_answer_lands_only_on_the_attempt_that_asked() {
    let mut flow = HostFlow::default();
    let first = flow.attempt();
    let second = flow.attempt();
    assert_ne!(first, second);
    assert!(landed_on(Some(second), second));
    // The form was cancelled and opened again: the first attach is stale.
    assert!(!landed_on(Some(second), first));
    assert!(!landed_on(None, first));
}
