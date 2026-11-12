//! An agent's *Test* on a real shell: what it found of the agent's command,
//! filed under the command line, so renaming the agent keeps it, and kept
//! for the process.

use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use gpui::{TestAppContext, VisualTestContext};
use onehand_core::config::AgentSpec;
use onehand_core::config::drive::Tested;
use std::time::Duration;

fn agent(name: &str, command: &str) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        command: command.into(),
        ..Default::default()
    }
}

/// What the last *Test* of `spec` found, once it is no longer running.
fn tested(spec: &AgentSpec, cx: &mut VisualTestContext) -> Tested {
    for _ in 0..200 {
        cx.run_until_parked();
        let found = cx.update(|_, cx| Shared::global(cx).settings.tested(spec).cloned());
        match found {
            Some(Tested::Running) | None => {}
            Some(done @ (Tested::Found(..) | Tested::Missing)) => return done,
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the test never landed");
}

/// A command on `PATH` is found at its path, one that is not is missing,
/// and the finding follows the command line rather than the name.
#[gpui::test]
fn a_test_finds_the_command_or_says_it_is_missing(cx: &mut TestAppContext) {
    let dir = folder("agent-test");
    let found = agent("a", "true");
    let missing = agent("b", "onehand-no-such-command-here");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![found.clone(), missing.clone()]);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.check_agent(0, cx);
            shell.check_agent(1, cx);
        })
    });
    assert!(matches!(tested(&found, cx), Tested::Found(..)));
    assert_eq!(tested(&missing, cx), Tested::Missing);
    let renamed = agent("renamed", "true");
    assert!(matches!(tested(&renamed, cx), Tested::Found(..)));
    let _ = std::fs::remove_dir_all(dir);
}

/// A second *Test* of an agent while one runs starts no second look.
#[gpui::test]
fn a_second_test_while_one_runs_starts_nothing(cx: &mut TestAppContext) {
    let dir = folder("agent-test-twice");
    let found = agent("a", "true");
    let (_, shell, cx) = shell(cx, dir.join("tasks"), vec![found.clone()]);
    let looks = cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.check_agent(0, cx));
        let fx = gpui::BorrowAppContext::update_global::<Shared, _>(cx, |shared, _| {
            shared.settings.test(found.clone(), None)
        });
        fx.len()
    });
    assert_eq!(looks, 0, "nothing more while it runs");
    assert!(matches!(tested(&found, cx), Tested::Found(..)));
    let _ = std::fs::remove_dir_all(dir);
}
