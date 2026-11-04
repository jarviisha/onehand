use super::{
    HostFlow, Waiting, Want, back_failure, landed_on, needs_restart, release, said_by_wait,
};
use crate::hosts::HostState;

/// What a host's waiting holds, to compare.
fn held(waiting: Option<Waiting>) -> Option<(Vec<u64>, Option<u64>, bool)> {
    waiting.map(|w| (w.uids, w.add, w.page))
}
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
        held(flow.take(&a, false)),
        Some((vec![1, 2], Some(7), true))
    );
    assert_eq!(held(flow.take(&a, false)), None, "released once");
    assert_eq!(held(flow.take(&b, false)), Some((vec![], None, true)));
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
    assert_eq!(held(flow.take(&a, false)), Some((vec![], None, false)));
    assert_eq!(held(flow.take(&b, false)), Some((vec![1], None, false)));
}

/// What no longer waits goes, and a host nothing here waits on is let go,
/// so its wait is not listened to from here any more.
#[test]
fn what_stops_waiting_is_let_go() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = HostFlow::default();
    flow.join(&a, Want::Session(1));
    flow.join(&a, Want::Session(2));
    flow.join(&a, Want::Page);
    flow.join(&b, Want::Add(4));
    // Session 2 closed, the page left, the form put away.
    flow.prune(|_, want| match want {
        Want::Session(uid) => uid == 1,
        Want::Page | Want::Add(_) | Want::Back(_) | Want::PageBack => false,
    });
    assert!(!flow.waiting.contains_key(&b), "nothing waits on b here");
    assert_eq!(held(flow.take(&a, false)), Some((vec![1], None, false)));
    assert!(flow.is_empty());
}

/// What waits on a lost host being back is a wait of its own beside the busy
/// one, listened for once per host and let go as the others are.
#[test]
fn a_wait_on_a_host_being_back_stands_apart_from_its_busy_wait() {
    let a = HostName("a".into());
    let mut flow = HostFlow::default();
    assert!(flow.join(&a, Want::Back(1)), "listened for");
    assert!(!flow.join(&a, Want::Back(1)), "a session once");
    assert!(!flow.join(&a, Want::PageBack));
    assert!(flow.join(&a, Want::Session(2)), "the busy wait is another");
    flow.prune(|_, want| match want {
        Want::Back(_) | Want::Session(_) => true,
        Want::PageBack | Want::Page | Want::Add(_) => false,
    });
    assert_eq!(held(flow.take(&a, true)), Some((vec![1], None, false)));
    assert_eq!(held(flow.take(&a, false)), Some((vec![2], None, false)));
    assert!(flow.is_empty());
}

/// A failed restart that ended a wait is said where the wait was, once;
/// with nothing waiting, or a host still busy, it is said on its own.
#[test]
fn a_failed_restart_is_said_once() {
    let gone = HostFailure::Other("Could not attach to box: gone".into());
    let busy = HostFailure::Busy {
        host: HostName("box".into()),
        busy: Vec::new(),
    };
    assert!(said_by_wait(true, &gone), "the wait ended on it: no toast");
    assert!(!said_by_wait(false, &gone), "nothing waited: a toast");
    assert!(!said_by_wait(true, &busy), "still waiting: said on its own");
}

/// A host taken by another machine is never attached again by itself, however
/// its wait ended; one found busy carries on onto its busy wait; a refused
/// host ends what waited on why.
#[test]
fn what_waited_on_a_host_being_back_restarts_only_when_it_may() {
    let host = HostName("vps".into());
    let taken = Some("vps was attached from another machine".to_string());
    assert_eq!(back_failure(&host, &HostState::Taken, false, Ok(())), taken);
    assert_eq!(
        back_failure(&host, &HostState::Taken, true, Err("busy".into())),
        taken
    );
    assert_eq!(back_failure(&host, &HostState::Live, false, Ok(())), None);
    assert_eq!(
        back_failure(&host, &HostState::Attaching, true, Err("busy".into())),
        None
    );
    assert_eq!(
        back_failure(&host, &HostState::Unknown, false, Err("refused".into())),
        Some("refused".into())
    );
}
