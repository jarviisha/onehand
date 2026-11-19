use super::*;

fn drive() -> SettingsDrive {
    SettingsDrive::new(Vec::new(), Appearance::System, Vec::new())
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
            | Effect::ProbeHost { .. }
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
            | Effect::ProbeHost { .. }
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
            | Effect::ProbeHost { .. }
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
        Effect::ProbeHost { .. } => "ProbeHost",
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
            | Effect::ProbeHost { .. }
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
        | Effect::ProbeHost { .. }
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
    let mut settings = SettingsDrive::new(Vec::new(), Appearance::default(), Vec::new());
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
    let mut settings = SettingsDrive::new(Vec::new(), Appearance::default(), Vec::new());
    let on = |args: &[&str]| AgentSpec {
        args: args.iter().map(|a| a.to_string()).collect(),
        ..command("a", "npx")
    };
    let _ = settings.test(on(&["-y", "one"]), None);
    assert_eq!(settings.tested(&on(&["-y", "one"])), Some(&Tested::Running));
    assert_eq!(settings.tested(&on(&["-y", "two"])), None);
}

mod hosts {
    use super::super::*;
    use crate::config::{HostEdit, HostOs, HostSpec};
    use crate::host::probe::Probed;
    use crate::workspace::HostName;

    fn named(name: &str) -> HostName {
        HostName(name.into())
    }

    fn probed(os: HostOs, arch: &str) -> Probed {
        Probed {
            os,
            arch: arch.into(),
            tools: Vec::new(),
        }
    }

    fn drive(hosts: Vec<HostSpec>) -> SettingsDrive {
        SettingsDrive::new(Vec::new(), Appearance::System, hosts)
    }

    /// Each effect by its kind, since an effect is not compared whole.
    fn kinds(fx: &[Effect]) -> Vec<&'static str> {
        fx.iter()
            .map(|effect| match effect {
                Effect::Show(_) => "show",
                Effect::Write { .. } => "write",
                Effect::Probe { .. } => "probe",
                Effect::ProbeHost { .. } => "probe-host",
                Effect::Look { .. } => "look",
                Effect::CallOff { .. } => "call-off",
                Effect::Changed => "changed",
                Effect::Signal(_) => "signal",
            })
            .collect()
    }

    fn writes(fx: &[Effect]) -> Vec<&Edit> {
        fx.iter()
            .filter_map(|effect| match effect {
                Effect::Write { edit, .. } => Some(edit),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_probed_host_is_declared_and_written() {
        let mut drive = drive(Vec::new());
        let fx = drive.probe_host(named("b"));
        assert_eq!(kinds(&fx), vec!["changed", "probe-host"]);
        assert!(matches!(&fx[1], Effect::ProbeHost { host } if *host == named("b")));
        assert_eq!(drive.host_probe(&named("b")), Some(&HostProbe::Running));
        assert!(drive.hosts.is_empty());

        let found = probed(HostOs::Linux, "x86_64");
        let fx = drive.handle(Input::HostProbed {
            host: named("b"),
            found: Ok(found.clone()),
        });
        let spec = HostSpec {
            name: named("b"),
            os: HostOs::Linux,
            arch: "x86_64".into(),
        };
        assert_eq!(drive.hosts, vec![spec.clone()]);
        assert_eq!(
            drive.host_probe(&named("b")),
            Some(&HostProbe::Found(found))
        );
        assert_eq!(kinds(&fx), vec!["changed", "write"]);
        assert_eq!(writes(&fx), vec![&Edit::Hosts(HostEdit::Put(spec))]);
    }

    #[test]
    fn a_failed_probe_declares_nothing() {
        let mut drive = drive(Vec::new());
        let _ = drive.probe_host(named("b"));
        let fx = drive.handle(Input::HostProbed {
            host: named("b"),
            found: Err("no route".into()),
        });
        assert!(drive.hosts.is_empty());
        assert_eq!(
            drive.host_probe(&named("b")),
            Some(&HostProbe::Failed("no route".into()))
        );
        assert_eq!(kinds(&fx), vec!["changed"]);
    }

    #[test]
    fn a_second_probe_while_one_runs_starts_nothing() {
        let mut drive = drive(Vec::new());
        let _ = drive.probe_host(named("b"));
        assert!(drive.probe_host(named("b")).is_empty());
    }

    #[test]
    fn checking_a_declared_host_replaces_it() {
        let mut drive = drive(vec![HostSpec::undeclared(named("b"))]);
        let _ = drive.probe_host(named("b"));
        let _ = drive.handle(Input::HostProbed {
            host: named("b"),
            found: Ok(probed(HostOs::Windows, "AMD64")),
        });
        assert_eq!(drive.hosts.len(), 1);
        assert_eq!(drive.hosts[0].os, HostOs::Windows);
    }

    #[test]
    fn a_probe_that_finds_what_is_declared_writes_nothing() {
        let mut drive = drive(vec![HostSpec::undeclared(named("b"))]);
        let _ = drive.probe_host(named("b"));
        let fx = drive.handle(Input::HostProbed {
            host: named("b"),
            found: Ok(probed(HostOs::Linux, "x86_64")),
        });
        assert_eq!(kinds(&fx), vec!["changed"]);
    }

    #[test]
    fn removing_writes_and_forgets_its_probe() {
        let mut drive = drive(vec![HostSpec::undeclared(named("b"))]);
        let _ = drive.probe_host(named("b"));
        let fx = drive.remove_host(named("b"));
        assert!(drive.hosts.is_empty());
        assert_eq!(drive.host_probe(&named("b")), None);
        assert_eq!(
            writes(&fx),
            vec![&Edit::Hosts(HostEdit::Remove(named("b")))]
        );
        // The probe still under way lands on a host that is gone: nothing is
        // declared again behind the person's back.
        let fx = drive.handle(Input::HostProbed {
            host: named("b"),
            found: Ok(probed(HostOs::Linux, "x86_64")),
        });
        assert!(drive.hosts.is_empty());
        assert!(fx.is_empty());
    }

    #[test]
    fn the_hosts_edit_lands_in_the_file() {
        let mut cfg = AppConfig::default();
        let spec = HostSpec::undeclared(named("b"));
        Edit::Hosts(HostEdit::Put(spec.clone())).apply(&mut cfg);
        assert_eq!(cfg.hosts, vec![spec]);
        Edit::Hosts(HostEdit::Remove(named("b"))).apply(&mut cfg);
        assert!(cfg.hosts.is_empty());
    }
}
