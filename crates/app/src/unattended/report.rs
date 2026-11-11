//! A run's agent and its transcript's first lines, and the project a run
//! worked in kept or dropped once it ends.

use crate::chat::session::{ChatSession, note};
use crate::state::Shared;
use gpui::{App, Entity};
use onehand_core::config::{self, AgentSpec};
use onehand_core::task::Task;
use onehand_core::unattended::drive;
use std::path::Path;

/// The agent a run starts, with every build it makes pointed at one shared
/// directory.
pub(crate) fn spec_for(agent: Option<&str>, cx: &App) -> Option<AgentSpec> {
    let agents = &Shared::global(cx).settings.agents;
    // A gone agent's run starts on the first configured: by now the preflight
    // has had its say, and a run let through has to start on something.
    let base = config::agent_or_first(agents, agent)
        .or_else(|| config::agent_or_first(agents, None))?
        .clone();
    Some(drive::run_spec(base))
}

/// The lines a run's transcript opens with, as core words them. Without
/// them it would start at a prompt nobody on screen typed.
pub(crate) fn opening(task: &Task, session: &Entity<ChatSession>, cx: &mut App) {
    for line in drive::opening(task) {
        note(session, line, cx);
    }
}

/// Keep or drop the project a run worked in, in every window holding it. Only
/// a project the run added itself is dropped: one a person had open stays.
pub(super) fn teardown(dir: &Path, keep: bool, cx: &mut App) {
    let holding: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    for (handle, shell) in holding {
        let Some(shell) = shell.upgrade() else {
            continue;
        };
        let _ = handle.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| match keep {
                true => shell.take_over_unattended(dir, window, cx),
                false => shell.end_unattended(dir, window, cx),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use onehand_core::config::{AgentSpec, Preset};
    use onehand_core::unattended::drive::run_spec;

    /// A run's build directory reaches the agent through its environment, so a
    /// preset keeps launching as a preset.
    #[test]
    fn a_run_puts_the_build_directory_in_the_agents_environment() {
        let base = AgentSpec {
            name: "Codex".into(),
            preset: Some(Preset::Codex.into()),
            ..Default::default()
        };
        let run = run_spec(base.clone());
        assert_eq!(run.preset, base.preset);
        assert!(run.command.is_empty(), "{:?}", run.command);
        if let Some(dir) = onehand_core::unattended::target_dir() {
            assert_eq!(
                run.env.get("CARGO_TARGET_DIR"),
                Some(&dir.display().to_string())
            );
        }
    }
}
