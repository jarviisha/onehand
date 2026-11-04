use super::{HostFlow, Waiting, Want, landed_on, needs_restart, release};
use crate::hosts::HostFailure;
use onehand_core::workspace::HostName;

#[test]
fn a_yes_restarts_the_service_only_while_it_is_still_busy() {
    let busy: Result<(), HostFailure> = Err(HostFailure::Busy {
        host: HostName("box".into()),
        busy: Vec::new(),
    });
    assert!(needs_restart(&busy));
    // Done waiting since the question was asked: nothing to stop.
    assert!(!needs_restart(&Ok::<(), HostFailure>(())));
    assert!(!needs_restart(&Err::<(), _>(HostFailure::Other(
        "gone".into()
    ))));
}

/// What waits on a host here is gathered once per host, and the end of
/// its wait is listened for once.
#[test]
fn one_wait_per_host_gathers_everything_that_waits_on_it() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = HostFlow::default();
    assert!(flow.join(&a, Want::Session(1)), "listened for");
    assert!(!flow.join(&a, Want::Session(2)));
    assert!(!flow.join(&a, Want::Session(2)), "a session once");
    assert!(!flow.join(&a, Want::Add(7)));
    assert!(!flow.join(&a, Want::Page));
    // Another host is another wait.
    assert!(flow.join(&b, Want::Page));
    assert_eq!(
        flow.take(&a),
        Some(Waiting {
            uids: vec![1, 2],
            add: Some(7),
        })
    );
    assert_eq!(flow.take(&a), None, "released once");
    assert_eq!(flow.take(&b), Some(Waiting::default()));
    // Busy again later: listened for anew.
    assert!(flow.join(&a, Want::Page));
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

/// The form waits on its own attempt; one opened again since waits on
/// another, and the end of the first's wait does not carry it on.
#[test]
fn the_form_is_carried_on_only_by_the_wait_it_joined() {
    let mut flow = HostFlow::default();
    let held = flow.attempt();
    let mut form = Some(held);
    assert!(release(&mut form, Some(held)));
    assert_eq!(form, None);
    let mut reopened = Some(flow.attempt());
    assert!(!release(&mut reopened, Some(held)));
    assert!(reopened.is_some());
    assert!(!release(&mut reopened, None));
}

#[test]
fn a_cancelled_form_is_not_carried_on_by_an_old_wait() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = HostFlow::default();
    flow.join(&a, Want::Add(1));
    flow.join(&b, Want::Session(1));
    flow.join(&b, Want::Add(1));
    flow.forget_add();
    // The waits go on for the sessions they hold; the form is off them, so
    // a form opened again is not committed by one that ended for this one.
    assert_eq!(flow.take(&a), Some(Waiting::default()));
    assert_eq!(
        flow.take(&b),
        Some(Waiting {
            uids: vec![1],
            add: None,
        })
    );
}
