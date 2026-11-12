use super::*;

fn drive() -> RecentsDrive {
    RecentsDrive::new(AppState::default(), PathBuf::from("/nowhere/state.toml"))
}

/// The write an effect list holds, and its pending id.
fn written(fx: &[Effect]) -> (u64, &AppState) {
    let [effect] = fx else {
        panic!("one effect, not {}", fx.len());
    };
    match effect {
        Effect::Write { pending, state, .. } => (*pending, state),
        Effect::Signal(signal) => panic!("a write, not {signal:?}"),
    }
}

/// The one signal an effect list holds.
fn said(fx: &[Effect]) -> &Signal {
    let [effect] = fx else {
        panic!("one effect, not {}", fx.len());
    };
    match effect {
        Effect::Signal(signal) => signal,
        Effect::Write { pending, .. } => panic!("a signal, not write {pending}"),
    }
}

#[test]
fn each_change_writes_the_list_as_it_stands() {
    let mut d = drive();
    let fx = d.opened("/a".into());
    assert_eq!(written(&fx).1.recent_workspaces, vec![PathBuf::from("/a")]);
    let fx = d.bound("/b".into(), "Bee");
    let (_, state) = written(&fx);
    assert_eq!(state.recent_workspaces[0], PathBuf::from("/b"));
    assert_eq!(state.name_of(Path::new("/b")), Some("Bee"));
    let fx = d.forgot(Path::new("/a"), Forgot::Gone);
    assert_eq!(written(&fx).1.recent_workspaces, vec![PathBuf::from("/b")]);
}

#[test]
fn a_name_unchanged_or_of_a_workspace_not_remembered_writes_nothing() {
    let mut d = drive();
    let _ = d.bound("/b".into(), "Bee");
    assert!(d.named(Path::new("/b"), "Bee").is_empty());
    assert!(d.named(Path::new("/z"), "Zed").is_empty());
    assert_eq!(d.named(Path::new("/b"), "Bea").len(), 1);
}

#[test]
fn only_an_unbinds_failed_forget_is_said() {
    let mut d = drive();
    let (touch, _) = written(&d.opened("/a".into()));
    let (forget, _) = written(&d.forgot(Path::new("/a"), Forgot::Unbound));
    let failed = |pending| Input::Written {
        pending,
        result: Err("disk full".into()),
    };
    // The touch's failure lands after the forget was asked: still only logged.
    assert_eq!(
        said(&d.handle(failed(touch))),
        &Signal::WriteFailed("disk full".into())
    );
    assert_eq!(
        said(&d.handle(failed(forget))),
        &Signal::ForgetFailed("disk full".into())
    );
    let (gone, _) = written(&d.forgot(Path::new("/b"), Forgot::Gone));
    assert_eq!(
        said(&d.handle(failed(gone))),
        &Signal::WriteFailed("disk full".into())
    );
    let (landed, _) = written(&d.forgot(Path::new("/c"), Forgot::Unbound));
    assert!(d
        .handle(Input::Written {
            pending: landed,
            result: Ok(()),
        })
        .is_empty());
}

/// A run of forgets that fail is said once: the next is only logged, until a
/// write lands, after which a forget that fails is said again.
#[test]
fn a_run_of_failed_forgets_is_said_once_until_a_write_lands() {
    let mut d = drive();
    let forget = |d: &mut RecentsDrive, result: Result<(), String>| {
        let (pending, _) = written(&d.forgot(Path::new("/a"), Forgot::Unbound));
        d.handle(Input::Written { pending, result })
    };
    let full = || Err("disk full".to_string());
    assert_eq!(
        said(&forget(&mut d, full())),
        &Signal::ForgetFailed("disk full".into())
    );
    assert_eq!(
        said(&forget(&mut d, full())),
        &Signal::WriteFailed("disk full".into())
    );
    assert!(forget(&mut d, Ok(())).is_empty());
    assert_eq!(
        said(&forget(&mut d, full())),
        &Signal::ForgetFailed("disk full".into())
    );
}
