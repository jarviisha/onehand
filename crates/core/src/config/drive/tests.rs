use super::*;

fn drive() -> SettingsDrive {
    SettingsDrive::new(Vec::new(), Appearance::System)
}

fn spec(name: &str) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        ..AgentSpec::default()
    }
}

/// The write of `fx`, answered `result`.
fn written(drive: &mut SettingsDrive, fx: Vec<Effect>, result: Result<(), String>) -> Vec<Effect> {
    let (pending, edit) = fx
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Write { pending, edit } => Some((pending, edit)),
            _ => None,
        })
        .expect("a write");
    drive.handle(Input::Written {
        pending,
        edit,
        result,
    })
}

fn shown(fx: &[Effect]) -> Vec<Value> {
    fx.iter()
        .filter_map(|effect| match effect {
            Effect::Show(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn failed(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter()
        .filter_map(|effect| match effect {
            Effect::Signal(Signal::SaveFailed { doing, .. }) => Some(*doing),
            _ => None,
        })
        .collect()
}

/// The same value again writes nothing.
#[test]
fn setting_the_same_value_writes_nothing() {
    let mut drive = drive();
    assert!(drive.set_appearance(Appearance::System).is_empty());
    assert!(drive.set_outside(true, true).is_empty());
}

/// A failed appearance write puts it back and says so once.
#[test]
fn a_failed_appearance_write_puts_it_back_and_says_once() {
    let mut drive = drive();
    let fx = drive.set_appearance(Appearance::Dark);
    assert_eq!(shown(&fx), [Value::Appearance(Appearance::Dark)]);
    let fx = written(&mut drive, fx, Err("disk full".into()));
    assert_eq!(shown(&fx), [Value::Appearance(Appearance::System)]);
    assert_eq!(failed(&fx), ["Saving the appearance"]);
    assert_eq!(drive.appearance, Appearance::System);
}

/// A failure whose setting a later write carries puts nothing back: that
/// write's answer decides.
#[test]
fn a_superseded_failure_puts_nothing_back() {
    let mut drive = drive();
    let first = drive.set_outside(false, true);
    let second = drive.set_outside(true, false);
    assert!(written(&mut drive, first, Err("busy".into())).is_empty());
    assert!(written(&mut drive, second, Ok(())).is_empty());
}

/// A failed agents write keeps the list on screen and says so.
#[test]
fn a_failed_agents_write_keeps_the_list_and_says_so() {
    let mut drive = drive();
    let fx = drive.edit_agents(AgentEdit::Put {
        old: None,
        new: spec("a"),
    });
    assert_eq!(drive.agents.len(), 1);
    let fx = written(&mut drive, fx, Err("read-only".into()));
    assert!(shown(&fx).is_empty());
    assert_eq!(failed(&fx), ["Saving the agents"]);
    assert_eq!(drive.agents.len(), 1);
}

/// A check keeps what the agent offers and clears its failure; a second
/// press while one runs starts nothing; one that never comes up fails after
/// its wait, and its late answer is dropped.
#[test]
fn a_check_runs_once_and_lands_once() {
    let mut drive = drive();
    let fx = drive.check(spec("a"), "/repo".into());
    let Some(Effect::Probe { token, .. }) =
        fx.into_iter().find(|e| matches!(e, Effect::Probe { .. }))
    else {
        panic!("a probe");
    };
    assert!(drive.check(spec("a"), "/repo".into()).is_empty());
    let offer = Offer {
        modes: vec!["plan".into()],
        options: Vec::new(),
    };
    drive.handle(Input::Probed {
        token,
        found: Ok(offer.clone()),
    });
    assert_eq!(drive.checking(&spec("a")), None);
    assert_eq!(drive.offers_seen, [(spec("a"), offer)]);

    let fx = drive.check(spec("a"), "/repo".into());
    let Some(Effect::Probe { token, .. }) =
        fx.into_iter().find(|e| matches!(e, Effect::Probe { .. }))
    else {
        panic!("a probe");
    };
    let fx = drive.handle(Input::Elapsed { token });
    assert!(matches!(fx[0], Effect::CallOff { token: t } if t == token));
    assert!(
        matches!(drive.checking(&spec("a")), Some(Checking::Failed(why)) if why.contains("90s"))
    );
    assert!(drive
        .handle(Input::Probed {
            token,
            found: Ok(Offer::default())
        })
        .is_empty());
}

/// What an agent says before it comes up is folded into its offer.
#[test]
fn a_probe_ends_when_the_agent_comes_up_or_fails() {
    let mut offer = Offer::default();
    assert_eq!(
        fold_probe(&mut offer, AcpEvent::SessionId("s".into())),
        None
    );
    assert_eq!(
        fold_probe(&mut offer, AcpEvent::Disconnected("gone".into())),
        Some(Err("gone".into()))
    );
}
