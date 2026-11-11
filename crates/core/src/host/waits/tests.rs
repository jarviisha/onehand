use super::*;

/// What a host's waiting holds, to compare.
fn held(waiting: Option<Waiting>) -> Option<(Vec<u64>, Option<u64>, bool)> {
    waiting.map(|w| (w.uids, w.add, w.page))
}
use crate::workspace::HostName;

/// What waits on a host here is gathered once per host, and the end of
/// its wait is listened for once.
#[test]
fn one_wait_per_host_gathers_everything_that_waits_on_it() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = WindowWaits::default();
    assert!(flow.wait(&a, Want::Session(1)), "listened for");
    assert!(!flow.wait(&a, Want::Session(2)));
    assert!(!flow.wait(&a, Want::Session(2)), "a session once");
    assert!(!flow.wait(&a, Want::Add(7)));
    assert!(!flow.wait(&a, Want::Page));
    // Another host is another wait.
    assert!(flow.wait(&b, Want::Page));
    assert_eq!(
        held(flow.waiting.remove(&a)),
        Some((vec![1, 2], Some(7), true))
    );
    assert_eq!(held(flow.waiting.remove(&a)), None, "released once");
    assert_eq!(held(flow.waiting.remove(&b)), Some((vec![], None, true)));
    // Busy again later: listened for anew.
    assert!(flow.wait(&a, Want::Page));
}

#[test]
fn an_answer_lands_only_on_the_attempt_that_asked() {
    let mut flow = WindowWaits::default();
    let first = flow.attempt();
    let second = flow.attempt();
    assert_ne!(first, second);
    assert!(landed_on(Some(second), second));
    // The form was cancelled and opened again: the first attach is stale.
    assert!(!landed_on(Some(second), first));
    assert!(!landed_on(None, first));
}

#[test]
fn a_cancelled_form_is_not_carried_on_by_an_old_wait() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = WindowWaits::default();
    let _ = flow.wait(&a, Want::Add(1));
    let _ = flow.wait(&b, Want::Session(1));
    let _ = flow.wait(&b, Want::Add(1));
    flow.forget_add();
    // The waits go on for the sessions they hold; the form is off them, so
    // a form opened again is not committed by one that ended for this one.
    assert_eq!(held(flow.waiting.remove(&a)), Some((vec![], None, false)));
    assert_eq!(held(flow.waiting.remove(&b)), Some((vec![1], None, false)));
}

/// What no longer waits goes, and a host nothing here waits on is let go,
/// so its wait is not listened to from here any more.
#[test]
fn what_stops_waiting_is_let_go() {
    let (a, b) = (HostName("a".into()), HostName("b".into()));
    let mut flow = WindowWaits::default();
    flow.wait(&a, Want::Session(1));
    flow.wait(&a, Want::Session(2));
    flow.wait(&a, Want::Page);
    flow.wait(&b, Want::Add(4));
    // Session 2 closed, the page left, the form put away.
    let gone = flow.prune(|_, want| match want {
        Want::Session(uid) => uid == 1,
        Want::Page | Want::Add(_) | Want::Back(_) | Want::PageBack => false,
    });
    assert_eq!(gone, vec![(b.clone(), false)], "b no longer listened for");
    assert!(!flow.waiting.contains_key(&b), "nothing waits on b here");
    assert_eq!(held(flow.waiting.remove(&a)), Some((vec![1], None, false)));
    assert!(flow.is_empty());
}

/// What waits on a lost host being back is a wait of its own beside the busy
/// one, listened for once per host and let go as the others are.
#[test]
fn a_wait_on_a_host_being_back_stands_apart_from_its_busy_wait() {
    let a = HostName("a".into());
    let mut flow = WindowWaits::default();
    assert!(flow.wait(&a, Want::Back(1)), "listened for");
    assert!(!flow.wait(&a, Want::Back(1)), "a session once");
    assert!(!flow.wait(&a, Want::PageBack));
    assert!(flow.wait(&a, Want::Session(2)), "the busy wait is another");
    let _ = flow.prune(|_, want| match want {
        Want::Back(_) | Want::Session(_) => true,
        Want::PageBack | Want::Page | Want::Add(_) => false,
    });
    assert_eq!(held(flow.back.remove(&a)), Some((vec![1], None, false)));
    assert_eq!(held(flow.waiting.remove(&a)), Some((vec![2], None, false)));
    assert!(flow.is_empty());
}

/// A busy wait's end comes to what waited here, once, and carries the form
/// on only when it is the attempt the form still waits on.
#[test]
fn a_busy_wait_ends_once_carrying_on_the_form_that_waited() {
    use crate::host::drive::Landing;
    let a = HostName("a".into());
    let mut flow = WindowWaits::default();
    let attempt = flow.attempt();
    let _ = flow.wait(&a, Want::Session(1));
    let _ = flow.wait(&a, Want::Add(attempt));
    let (landed, released) = flow
        .idle_ended(&a, Some(attempt), Ok(()))
        .expect("it waited");
    assert!(released);
    assert!(landed.contains(&Landing::Add), "{landed:?}");
    assert!(flow.idle_ended(&a, Some(attempt), Ok(())).is_none(), "once");
    // A form opened again since is on another attach: not carried on.
    let _ = flow.wait(&a, Want::Add(attempt));
    let reopened = flow.attempt();
    let (landed, released) = flow
        .idle_ended(&a, Some(reopened), Ok(()))
        .expect("it waited");
    assert!(!released);
    assert!(!landed.contains(&Landing::Add), "{landed:?}");
}
