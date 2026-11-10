use super::*;
use crate::chat::model::Link;
use crate::worktree::occupants::Occupants;

fn asked(prompt: &str) -> Open {
    Open {
        lead: 1,
        project: "repo".into(),
        agent: "agent".into(),
        prompt: Some(prompt.into()),
        dirs: Vec::new(),
        request: "r1".into(),
        turn: Some(1),
        model: None,
        name: None,
    }
}

fn projects() -> Vec<(String, PathBuf)> {
    vec![("repo".into(), PathBuf::from("/repo"))]
}

fn resolved() -> Resolved {
    Resolved {
        folder: PathBuf::from("/repo"),
        root: PathBuf::from("/repo"),
        same: vec![PathBuf::from("/repo")],
        dirs: Vec::new(),
        occupants: Occupants::None,
        canonical: vec![(PathBuf::from("/repo"), PathBuf::from("/repo"))],
    }
}

fn world() -> World {
    World {
        lead_here: true,
        counts: vec![(PathBuf::from("/repo"), 0)],
        live: vec![1],
        opened: Vec::new(),
    }
}

fn lead_on() -> ControlDrive {
    let mut drive = ControlDrive::default();
    drive.switch(1, true);
    drive
}

/// A drive whose lead 1's open of `r1` started worker 7: its key.
fn up(drive: &mut ControlDrive) -> Key {
    let Ok(Begun::Resolve { key, .. }) = drive.open_worker(asked("go"), &projects(), None) else {
        panic!("a new request resolves");
    };
    let fx = drive.resolved(&key, Ok(resolved()), &world());
    assert!(
        matches!(&fx[..], [Effect::Start { folder, .. }] if folder == "repo"),
        "{fx:?}"
    );
    let fx = drive.started(&key, Ok(7));
    assert!(fx.iter().any(|e| matches!(e, Effect::Watch(7))), "{fx:?}");
    key
}

fn connected() -> Chat {
    let mut chat = Chat::default();
    chat.link = Link::Connected;
    chat
}

fn answer(fx: &[Effect]) -> Option<&Result<String, String>> {
    fx.iter().find_map(|effect| match effect {
        Effect::Answer { outcome, .. } => Some(outcome),
        _ => None,
    })
}

fn facts(intake: Option<Intake>) -> Facts {
    Facts {
        intake,
        sent: 1,
        mode: "the agent's own".into(),
        model: "the agent's own".into(),
        name: "repo".into(),
    }
}

/// A worker comes up: its first prompt goes, then the relation is written
/// and the open answered with what it runs with.
#[test]
fn a_worker_up_is_given_its_prompt_then_answered() {
    let mut drive = lead_on();
    up(&mut drive);
    assert_eq!(drive.lead_of(7), Some(1), "a worker from its start");
    assert!(
        drive.woke(7, Some(&Chat::default()), true).is_empty(),
        "still connecting"
    );
    let fx = drive.woke(7, Some(&connected()), true);
    assert!(matches!(&fx[..], [Effect::Begin { uid: 7, prompt: Some(p) }] if p == "go"));
    let fx = drive.begun(7, facts(Some(Intake::Sent)));
    let said = answer(&fx).unwrap().as_ref().unwrap();
    assert!(
        said.contains("name: repo") && said.ends_with("turn: 1 accepted"),
        "{said}"
    );
    assert_eq!(
        drive.workers_of(1),
        [Worker {
            uid: 7,
            turn: Some(1),
            sent: Some(1)
        }]
    );
    // The same request again answers the same, opening nothing.
    let Ok(Begun::Said(again)) = drive.open_worker(asked("go"), &projects(), None) else {
        panic!("answered again");
    };
    assert_eq!(&again, said);
}

/// A first prompt the agent does not take fails the open, closing its
/// session and freeing its request, folder and place.
#[test]
fn a_prompt_not_taken_leaves_nothing_behind() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.begun(7, facts(Some(Intake::Queued)));
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Drop {
            uid: 7,
            added: false,
            ..
        }
    )));
    assert_eq!(
        answer(&fx),
        Some(&Err("the prompt was not accepted".into()))
    );
    assert!(drive.workers_of(1).is_empty());
    assert!(matches!(
        drive.open_worker(asked("go"), &projects(), None),
        Ok(Begun::Resolve { .. })
    ));
}

/// A lead switched off or closed abandons its opens at once.
#[test]
fn a_lead_that_goes_abandons_its_open() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.switch(1, false);
    assert_eq!(
        answer(&fx),
        Some(&Err("the lead went before its worker was ready".into()))
    );
    assert!(fx.iter().any(|e| matches!(e, Effect::Drop { uid: 7, .. })));

    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.forget(1);
    assert!(answer(&fx).is_some_and(Result::is_err));
    assert!(drive.lead_of(7).is_none());
}

/// A lead whose window closed is found gone at the next wake.
#[test]
fn a_wake_without_its_lead_fails() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.woke(7, Some(&connected()), false);
    assert!(answer(&fx).is_some_and(Result::is_err));
}

/// Past its deadline an open fails; a deadline of an open already over
/// does nothing.
#[test]
fn an_open_past_its_deadline_fails_once() {
    let mut drive = lead_on();
    let Ok(Begun::Resolve { key, token, .. }) = drive.open_worker(asked("go"), &projects(), None)
    else {
        panic!("resolves");
    };
    drive.resolved(&key, Ok(resolved()), &world());
    drive.started(&key, Ok(7));
    let fx = drive.elapsed(token);
    assert_eq!(
        answer(&fx),
        Some(&Err("the agent was not ready within 120 seconds".into()))
    );
    assert!(drive.elapsed(token).is_empty());
}

/// A session open on the folder, a process in it, or a lead at its cap
/// refuses the open before anything starts.
#[test]
fn a_folder_in_use_or_a_full_cap_refuses() {
    let mut drive = lead_on();
    let Ok(Begun::Resolve { key, .. }) = drive.open_worker(asked("go"), &projects(), None) else {
        panic!("resolves");
    };
    let busy = World {
        counts: vec![(PathBuf::from("/repo"), 1)],
        ..world()
    };
    let fx = drive.resolved(&key, Ok(resolved()), &busy);
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("already has a session")));

    let Ok(Begun::Resolve { key, .. }) = drive.open_worker(asked("go"), &projects(), None) else {
        panic!("a failed request opens afresh");
    };
    let full = World {
        opened: (2..6).map(|uid| (uid, PathBuf::from("/x"))).collect(),
        ..world()
    };
    let fx = drive.resolved(&key, Ok(resolved()), &full);
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("4 sessions open")));
    // The folder was given back.
    let Ok(Begun::Resolve { key, .. }) = drive.open_worker(asked("go"), &projects(), None) else {
        panic!("resolves");
    };
    assert!(matches!(
        &drive.resolved(&key, Ok(resolved()), &world())[..],
        [Effect::Start { .. }]
    ));
}

/// A prompt needs a turn, and a turn a prompt.
#[test]
fn a_prompt_and_its_turn_go_together() {
    let mut drive = lead_on();
    let mut open = asked("go");
    open.turn = None;
    assert!(drive.open_worker(open, &projects(), None).is_err());
}

/// Standing, read on every message.
#[test]
fn standing_follows_the_switches() {
    let mut drive = ControlDrive::default();
    assert!(
        matches!(drive.standing(&Peer::Outside), Standing::Refused(why) if why.contains("Settings"))
    );
    drive.set_outside(true);
    assert!(matches!(drive.standing(&Peer::Outside), Standing::On));
    let token = drive.mint();
    let peer = Peer::Session(token.clone());
    assert!(matches!(drive.standing(&peer), Standing::Off), "unclaimed");
    drive.claim(&token, 3);
    drive.grant(
        3,
        &Grant {
            tools: vec!["list_issues".into()],
            labels: Vec::new(),
        },
    );
    assert!(matches!(drive.standing(&peer), Standing::Only(tools) if tools == ["list_issues"]));
    assert!(matches!(
        &drive.switch(3, true)[..],
        [Effect::ToolsChanged(3)]
    ));
    assert!(matches!(drive.standing(&peer), Standing::On));
    drive.release(&token);
    assert!(matches!(drive.standing(&peer), Standing::Refused(_)));
}

/// Give back: offered while the person holds a worker whose lead is open and
/// on; given back, the lead holds it.
#[test]
fn a_worker_given_back_is_the_leads_again() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.woke(7, Some(&connected()), true);
    drive.begun(7, facts(Some(Intake::Sent)));
    assert!(!drive.give_back_offered(7, 1, true), "the lead holds it");
    assert!(drive.give_back_offered(7, 2, true));
    assert!(!drive.give_back_offered(7, 2, false), "its lead is gone");
    assert!(drive.give_back(7, 2, true));
    assert!(!drive.give_back_offered(7, 2, true));
    assert_eq!(drive.last(7).map(|last| last.sent), Some(2));
}
