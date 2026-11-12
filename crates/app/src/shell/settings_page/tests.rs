//! An agent's *Test* on a real shell: what it found of the agent's command,
//! filed under the command line, so renaming the agent keeps it.

use crate::settings::{AgentCheck, check_key};
use crate::shell::rig::{folder, shell};
use gpui::{TestAppContext, VisualTestContext};
use onehand_core::config::AgentSpec;
use std::time::Duration;

fn agent(name: &str, command: &str) -> AgentSpec {
    AgentSpec {
        name: name.into(),
        command: command.into(),
        ..Default::default()
    }
}

/// What the last *Test* of `spec` found, once it is no longer running.
fn tested(
    shell: &gpui::Entity<super::Shell>,
    spec: &AgentSpec,
    cx: &mut VisualTestContext,
) -> AgentCheck {
    for _ in 0..200 {
        cx.run_until_parked();
        let found = cx.update(|_, cx| shell.read(cx).agent_check(&check_key(spec)).cloned());
        match found {
            Some(AgentCheck::Running) | None => {}
            Some(done @ (AgentCheck::Found(..) | AgentCheck::Missing)) => return done,
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
    assert!(matches!(tested(&shell, &found, cx), AgentCheck::Found(..)));
    assert_eq!(tested(&shell, &missing, cx), AgentCheck::Missing);
    let renamed = agent("renamed", "true");
    assert!(matches!(
        tested(&shell, &renamed, cx),
        AgentCheck::Found(..)
    ));
    let _ = std::fs::remove_dir_all(dir);
}
