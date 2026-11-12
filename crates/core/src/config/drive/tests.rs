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
            Effect::Show(_)
            | Effect::Probe { .. }
            | Effect::Look { .. }
            | Effect::CallOff { .. }
            | Effect::Changed
            | Effect::Signal(_) => None,
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
            Effect::Write { .. }
            | Effect::Probe { .. }
            | Effect::Look { .. }
            | Effect::CallOff { .. }
            | Effect::Changed
            | Effect::Signal(_) => None,
        })
        .collect()
}

fn failed(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter()
        .filter_map(|effect| match effect {
            Effect::Signal(Signal::SaveFailed { doing, .. }) => Some(*doing),
            Effect::Show(_)
            | Effect::Write { .. }
            | Effect::Probe { .. }
            | Effect::Look { .. }
            | Effect::CallOff { .. }
            | Effect::Changed => None,
        })
        .collect()
}

fn name(effect: &Effect) -> &'static str {
    match effect {
        Effect::Show(_) => "Show",
        Effect::Write { .. } => "Write",
        Effect::Probe { .. } => "Probe",
        Effect::Look { .. } => "Look",
        Effect::CallOff { .. } => "CallOff",
        Effect::Changed => "Changed",
        Effect::Signal(_) => "Signal",
    }
}

fn names(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter().map(name).collect()
}

/// The token of the probe `fx` ask for.
fn probe(fx: &[Effect]) -> u64 {
    fx.iter()
        .find_map(|effect| match effect {
            Effect::Probe { token, .. } => Some(*token),
            Effect::Show(_)
            | Effect::Write { .. }
            | Effect::Look { .. }
            | Effect::CallOff { .. }
            | Effect::Changed
            | Effect::Signal(_) => None,
        })
        .expect("a probe")
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
    let token = probe(&fx);
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
    let token = probe(&fx);
    let fx = drive.handle(Input::Elapsed { token });
    match &fx[0] {
        Effect::CallOff { token: t } => assert_eq!(*t, token),
        e @ (Effect::Show(_)
        | Effect::Write { .. }
        | Effect::Probe { .. }
        | Effect::Look { .. }
        | Effect::Changed
        | Effect::Signal(_)) => panic!("not a call-off: {}", name(e)),
    }
    match drive.checking(&spec("a")) {
        Some(Checking::Failed(why)) => assert!(why.contains("90s")),
        seen @ (Some(Checking::Running) | None) => panic!("not failed: {seen:?}"),
    }
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

/// The workflows issues are worked with are put in force at once, written
/// after, and put back when the write fails; the same pair writes nothing.
#[test]
fn workflows_are_shown_then_written_and_put_back_on_failure() {
    use std::collections::BTreeMap;
    let mut settings = SettingsDrive::new(Vec::new(), Default::default());
    let before = ("work".to_string(), BTreeMap::new());
    assert!(settings
        .set_workflows(before.clone(), "work".into(), BTreeMap::new())
        .is_empty());
    let labels = BTreeMap::from([("bug".to_string(), "fix".to_string())]);
    let fx = settings.set_workflows(before, "plan".into(), labels.clone());
    let after = Value::Workflows {
        default: "plan".into(),
        by_label: labels,
    };
    assert_eq!(shown(&fx), [after]);
    let write = fx.into_iter().find_map(|e| match e {
        Effect::Write { pending, edit } => Some((pending, edit)),
        Effect::Show(_)
        | Effect::Probe { .. }
        | Effect::Look { .. }
        | Effect::CallOff { .. }
        | Effect::Changed
        | Effect::Signal(_) => None,
    });
    let (pending, edit) = write.expect("a write");
    let fx = settings.handle(Input::Written {
        pending,
        edit,
        result: Err("read-only".into()),
    });
    assert_eq!(
        shown(&fx),
        [Value::Workflows {
            default: "work".into(),
            by_label: BTreeMap::new(),
        }]
    );
}

fn command(name: &str, command: &str) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        command: command.into(),
        ..Default::default()
    }
}

/// A *Test* runs once per command line at a time, and what it found is
/// filed by the command line, so a rename keeps it.
#[test]
fn a_test_runs_once_and_is_filed_by_its_command_line() {
    let mut settings = SettingsDrive::new(Vec::new(), Appearance::default());
    let spec = command("a", "agent-x");
    let fx = settings.test(spec.clone(), None);
    assert_eq!(names(&fx), ["Changed", "Look"]);
    assert_eq!(settings.tested(&spec), Some(&Tested::Running));
    assert!(
        settings.test(spec.clone(), None).is_empty(),
        "one at a time"
    );
    let fx = settings.handle(Input::Looked {
        spec: spec.clone(),
        found: Some((PathBuf::from("/bin/agent-x"), None)),
    });
    assert_eq!(names(&fx), ["Changed"]);
    let renamed = command("b", "agent-x");
    assert_eq!(
        settings.tested(&renamed),
        Some(&Tested::Found(PathBuf::from("/bin/agent-x"), None))
    );
    assert_eq!(settings.tested(&command("a", "other")), None);
    let _ = settings.test(renamed.clone(), None);
    let _ = settings.handle(Input::Looked {
        spec: renamed.clone(),
        found: None,
    });
    assert_eq!(settings.tested(&spec), Some(&Tested::Missing));
}

/// Two agents on one launcher are two agents: what a *Test* found of one is
/// not shown under the other.
#[test]
fn a_test_belongs_to_the_whole_command_line() {
    let mut settings = SettingsDrive::new(Vec::new(), Appearance::default());
    let on = |args: &[&str]| AgentSpec {
        args: args.iter().map(|a| a.to_string()).collect(),
        ..command("a", "npx")
    };
    let _ = settings.test(on(&["-y", "one"]), None);
    assert_eq!(settings.tested(&on(&["-y", "one"])), Some(&Tested::Running));
    assert_eq!(settings.tested(&on(&["-y", "two"])), None);
}
