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

/// The open's key and effects, when it resolves; a panic when it does not.
fn resolves(begun: Result<Begun, String>) -> (Key, Vec<Effect>) {
    match begun {
        Ok(Begun::Resolve { key, fx, .. }) => (key, fx),
        Ok(Begun::Said(said)) => panic!("said: {said}"),
        Err(why) => panic!("refused: {why}"),
    }
}

/// Each effect by its name, with the fields the tests read.
fn shown(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|effect| match effect {
            Effect::ToolsChanged(uid) => format!("ToolsChanged {uid}"),
            Effect::Start { folder, .. } => format!("Start {folder}"),
            Effect::Watch(uid) => format!("Watch {uid}"),
            Effect::Unwatch(uid) => format!("Unwatch {uid}"),
            Effect::Pick { uid, .. } => format!("Pick {uid}"),
            Effect::Begin { uid, prompt } => format!("Begin {uid} {prompt:?}"),
            Effect::Arm { token, wait } => format!("Arm {token} {wait:?}"),
            Effect::Drop { uid, added, .. } => format!("Drop {uid} {added}"),
            Effect::Answer { .. } => "Answer".into(),
            Effect::Changed => "Changed".into(),
        })
        .collect()
}

fn standing(standing: Standing) -> String {
    match standing {
        Standing::On => "On".into(),
        Standing::Off => "Off".into(),
        Standing::Only(tools) => format!("Only {tools:?}"),
        Standing::Refused(why) => format!("Refused {why}"),
    }
}

/// A drive whose lead 1's open of `r1` started worker 7: its key.
fn up(drive: &mut ControlDrive) -> Key {
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    assert_eq!(shown(&fx), ["Start repo"]);
    let fx = drive.handle(Input::Started {
        key: key.clone(),
        started: Ok(7),
    });
    assert!(shown(&fx).contains(&"Watch 7".to_string()), "{fx:?}");
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
        Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Arm { .. }
        | Effect::Drop { .. }
        | Effect::Changed => None,
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
        drive
            .handle(Input::Woke {
                uid: 7,
                chat: Some(&Chat::default()),
                lead_here: true
            })
            .is_empty(),
        "still connecting"
    );
    let fx = drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    assert_eq!(shown(&fx), [r#"Begin 7 Some("go")"#]);
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
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
    let again = match drive.open_worker(asked("go"), &projects(), None) {
        Ok(Begun::Said(again)) => again,
        Ok(Begun::Resolve { key, .. }) => panic!("opened again: {key:?}"),
        Err(why) => panic!("refused: {why}"),
    };
    assert_eq!(&again, said);
}

/// A first prompt the agent does not take fails the open, closing its
/// session and freeing its request, folder and place.
#[test]
fn a_prompt_not_taken_leaves_nothing_behind() {
    let mut drive = lead_on();
    up(&mut drive);
    let fx = drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Queued)),
    });
    assert!(shown(&fx).contains(&"Drop 7 false".to_string()), "{fx:?}");
    assert_eq!(
        answer(&fx),
        Some(&Err("the prompt was not accepted".into()))
    );
    assert!(drive.workers_of(1).is_empty());
    resolves(drive.open_worker(asked("go"), &projects(), None));
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
    assert!(
        shown(&fx).iter().any(|e| e.starts_with("Drop 7 ")),
        "{fx:?}"
    );

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
    let fx = drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: false,
    });
    assert!(answer(&fx).is_some_and(Result::is_err));
}

/// Past its deadline an open fails; a deadline of an open already over
/// does nothing.
#[test]
fn an_open_past_its_deadline_fails_once() {
    let mut drive = lead_on();
    let (key, fx) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let token = match fx[..] {
        [Effect::Arm { token, wait }] => {
            assert_eq!(wait, OPEN_DEADLINE);
            token
        }
        []
        | [_, _, ..]
        | [Effect::ToolsChanged(_)
        | Effect::Start { .. }
        | Effect::Watch(_)
        | Effect::Unwatch(_)
        | Effect::Pick { .. }
        | Effect::Begin { .. }
        | Effect::Drop { .. }
        | Effect::Answer { .. }
        | Effect::Changed] => panic!("the deadline is armed at the call: {fx:?}"),
    };
    drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    drive.handle(Input::Started {
        key: key.clone(),
        started: Ok(7),
    });
    let fx = drive.handle(Input::Elapsed { token });
    assert_eq!(
        answer(&fx),
        Some(&Err("the agent was not ready within 120 seconds".into()))
    );
    assert!(drive.handle(Input::Elapsed { token }).is_empty());
}

/// A session open on the folder, a process in it, or a lead at its cap
/// refuses the open before anything starts.
#[test]
fn a_folder_in_use_or_a_full_cap_refuses() {
    let mut drive = lead_on();
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let busy = World {
        counts: vec![(PathBuf::from("/repo"), 1)],
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: busy,
    });
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("already has a session")));

    // A failed request opens afresh.
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let full = World {
        opened: (2..6).map(|uid| (uid, PathBuf::from("/x"))).collect(),
        ..world()
    };
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: full,
    });
    assert!(answer(&fx).is_some_and(|o| o.as_ref().unwrap_err().contains("4 sessions open")));
    // The folder was given back.
    let (key, _) = resolves(drive.open_worker(asked("go"), &projects(), None));
    let fx = drive.handle(Input::Resolved {
        key: key.clone(),
        resolved: Ok(resolved()),
        world: world(),
    });
    assert_eq!(shown(&fx), ["Start repo"]);
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
    let first = standing(drive.standing(&Peer::Outside));
    assert!(
        first.starts_with("Refused ") && first.contains("Settings"),
        "{first}"
    );
    drive.set_outside(true);
    assert_eq!(standing(drive.standing(&Peer::Outside)), "On");
    let token = drive.mint();
    let peer = Peer::Session(token.clone());
    assert_eq!(standing(drive.standing(&peer)), "Off", "unclaimed");
    drive.claim(&token, 3);
    drive.grant(
        3,
        &Grant {
            tools: vec!["list_issues".into()],
            labels: Vec::new(),
        },
    );
    assert_eq!(standing(drive.standing(&peer)), r#"Only ["list_issues"]"#);
    assert_eq!(shown(&drive.switch(3, true)), ["ToolsChanged 3"]);
    assert_eq!(standing(drive.standing(&peer)), "On");
    drive.release(&token);
    assert!(standing(drive.standing(&peer)).starts_with("Refused "));
}

/// Give back: offered while the person holds a worker whose lead is open and
/// on; given back, the lead holds it.
#[test]
fn a_worker_given_back_is_the_leads_again() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    assert!(!drive.give_back_offered(7, 1, true), "the lead holds it");
    assert!(drive.give_back_offered(7, 2, true));
    assert!(!drive.give_back_offered(7, 2, false), "its lead is gone");
    assert!(drive.give_back(7, 2, true));
    assert!(!drive.give_back_offered(7, 2, true));
    assert_eq!(drive.last(7).map(|last| last.sent), Some(2));
}

/// A turn the worker's agent did not take is refused and moves nothing on.
#[test]
fn a_turn_not_taken_moves_nothing() {
    let mut drive = lead_on();
    up(&mut drive);
    drive.handle(Input::Woke {
        uid: 7,
        chat: Some(&connected()),
        lead_here: true,
    });
    drive.handle(Input::Begun {
        uid: 7,
        facts: facts(Some(Intake::Sent)),
    });
    assert!(drive.sent(7, 2, "next", &Intake::Queued, 2).is_err());
    assert_eq!(drive.last(7).map(|last| last.turn), Some(Some(1)));
    assert_eq!(
        drive.sent(7, 2, "next", &Intake::Sent, 2),
        Ok("turn 2 accepted".into())
    );
    assert_eq!(drive.last(7).map(|last| last.turn), Some(Some(2)));
}
