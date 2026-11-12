use super::*;
use std::time::Duration;

fn enable(id: &str, scope: Scope) -> Change {
    Change {
        id: id.into(),
        scope,
        verb: Verb::Enable,
    }
}

fn listed(drive: &mut PluginsDrive) -> u64 {
    let effects = drive.list_if_due();
    let [Effect::List { token, .. }] = effects.as_slice() else {
        panic!("no read asked for");
    };
    *token
}

fn landed(
    drive: &mut PluginsDrive,
    root: &str,
    change: Change,
    done: Result<(), String>,
    at: Instant,
) {
    let effects = drive.handle(Input::Changed {
        root: root.into(),
        change,
        done,
        now: crate::Now { secs: 0, at },
    });
    assert!(effects.is_empty());
}

#[test]
fn a_list_is_read_when_drawn_and_not_while_a_change_runs() {
    let mut drive = PluginsDrive::default();
    assert!(drive.list_if_due().is_empty());
    drive.set_root(Path::new("/a"));
    let _ = listed(&mut drive);
    assert!(drive.list_if_due().is_empty());

    let _ = drive.change(enable("x@m", Scope::Local));
    drive.mark_stale();
    assert!(drive.list_if_due().is_empty());
    landed(
        &mut drive,
        "/a",
        enable("x@m", Scope::Local),
        Ok(()),
        Instant::now(),
    );
    // The change's landing asks for the list again.
    let _ = listed(&mut drive);
}

#[test]
fn a_read_about_the_project_before_is_dropped() {
    let mut drive = PluginsDrive::default();
    drive.set_root(Path::new("/a"));
    let old = listed(&mut drive);
    drive.set_root(Path::new("/b"));
    let _ = drive.handle(Input::Listed {
        token: old,
        listed: Err("about /a".into()),
    });
    assert!(drive.catalog().is_none());
    let new = listed(&mut drive);
    let _ = drive.handle(Input::Listed {
        token: new,
        listed: Err("about /b".into()),
    });
    assert!(matches!(drive.catalog(), Some(Err(why)) if why == "about /b"));
}

#[test]
fn one_change_at_a_time() {
    let mut drive = PluginsDrive::default();
    drive.set_root(Path::new("/a"));
    assert!(matches!(
        drive.change(enable("one@m", Scope::Local)).as_slice(),
        [Effect::Change { .. }]
    ));
    assert!(drive.change(enable("two@m", Scope::Local)).is_empty());
    assert_eq!(drive.busy().map(|c| c.id.as_str()), Some("one@m"));
}

#[test]
fn a_refusal_is_said_only_in_the_project_it_was_made_in() {
    let mut drive = PluginsDrive::default();
    drive.set_root(Path::new("/a"));
    let _ = drive.change(enable("x@m", Scope::Local));
    landed(
        &mut drive,
        "/a",
        enable("x@m", Scope::Local),
        Err("no".into()),
        Instant::now(),
    );
    assert_eq!(drive.refused(), Some("no"));

    let _ = drive.change(enable("x@m", Scope::Local));
    assert_eq!(drive.refused(), None);
    drive.set_root(Path::new("/b"));
    landed(
        &mut drive,
        "/a",
        enable("x@m", Scope::Local),
        Err("no".into()),
        Instant::now(),
    );
    assert_eq!(drive.refused(), None);
    assert!(drive.busy().is_none());
}

#[test]
fn a_global_change_is_pending_everywhere_and_a_project_one_only_there() {
    let before = Instant::now();
    let since = before + Duration::from_millis(1);
    let after = since + Duration::from_millis(1);
    let mut drive = PluginsDrive::default();
    drive.set_root(Path::new("/a"));
    for (change, at) in [
        (enable("p@m", Scope::Local), after),
        (enable("g@m", Scope::User), after),
        (enable("old@m", Scope::Local), before),
        (
            Change {
                id: "c@m".into(),
                scope: Scope::Local,
                verb: Verb::CheckUpdates,
            },
            after,
        ),
    ] {
        let _ = drive.change(change.clone());
        landed(&mut drive, "/a", change, Ok(()), at);
    }
    assert_eq!(drive.pending(), 0);
    drive.agent_started(Some(since));
    assert_eq!(drive.pending(), 2);
    drive.set_root(Path::new("/b"));
    assert_eq!(drive.pending(), 1);
}

#[test]
fn an_install_sets_the_scope_the_next_one_takes() {
    let mut drive = PluginsDrive::default();
    drive.set_root(Path::new("/a"));
    assert_eq!(drive.install_scope(), Scope::User);
    let _ = drive.change(Change {
        id: "x@m".into(),
        scope: Scope::Project,
        verb: Verb::Install,
    });
    assert_eq!(drive.install_scope(), Scope::Project);
}
