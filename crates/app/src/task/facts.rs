//! What a start is checked against: the facts the app holds, gathered for
//! the preflight core decides with, and the agent a task starts on.

use crate::state::Shared;
use gpui::App;
use onehand_core::config::{self, AgentSpec};
use onehand_core::preflight::{Facts, Forge, StepAgent};
use onehand_core::task::Task;
use onehand_core::workflow::{Current, Field, Setup, Template};

/// The connector serving the project at `root`, or every reason none does.
pub(crate) fn connector_for(
    root: &std::path::Path,
) -> Result<&'static dyn onehand_core::connector::Connector, String> {
    let all = crate::features::connectors();
    onehand_core::connector::serving(all, root).map(|at| all[at])
}

/// `forge` as a preflight reads it: its name, and its account as last seen.
pub(crate) fn forge(forge: &'static dyn onehand_core::connector::Connector, cx: &App) -> Forge {
    forge_named(forge.name(), cx)
}

/// The forge named `name` as a preflight reads it.
fn forge_named(name: &str, cx: &App) -> Forge {
    Forge {
        name: name.to_string(),
        account: crate::accounts::of(name, cx),
    }
}

/// What the agent `spec` offered when it last came up in this process,
/// started as it is configured now; `None` when it has not.
pub(crate) fn offered(spec: &AgentSpec, cx: &App) -> Option<onehand_core::workflow::Offer> {
    onehand_core::task::offered(spec, &Shared::global(cx).settings.offers_seen)
}

/// Each agent a step of `workflow` names, found by exact name: a step's agent
/// never falls back to the first configured.
pub(crate) fn step_agents(workflow: &Result<Template, String>, cx: &App) -> Vec<StepAgent> {
    let Ok(template) = workflow else {
        return Vec::new();
    };
    let agents = &Shared::global(cx).settings.agents;
    template
        .steps
        .iter()
        .enumerate()
        .filter_map(|(step, spec)| {
            let name = spec.agent()?;
            let found = config::agent_or_first(agents, Some(name));
            Some(StepAgent {
                step,
                agent: name.to_string(),
                configured: found.is_some(),
                offered: found.and_then(|agent| offered(agent, cx)),
            })
        })
        .collect()
}

/// The first agent configured, by name: what a task naming none starts.
pub(crate) fn first_agent(cx: &App) -> Option<String> {
    config::agent_or_first(&Shared::global(cx).settings.agents, None).map(|spec| spec.name.clone())
}

/// The agent a task starts, with every build it makes pointed at one shared
/// directory.
pub(crate) fn spec_for(agent: Option<&str>, cx: &App) -> Option<AgentSpec> {
    let agents = &Shared::global(cx).settings.agents;
    // A gone agent's run starts on the first configured: by now the preflight
    // has had its say, and a run let through has to start on something.
    let base = config::agent_or_first(agents, agent)
        .or_else(|| config::agent_or_first(agents, None))?
        .clone();
    Some(onehand_core::task::run_spec(base))
}

/// The agent a lane of a run starts on: the one named `agent`, found by
/// exact name since a step's agent never falls back, or, `agent` empty, the
/// first configured, as a run naming none starts.
pub(crate) fn lane_spec(agent: &str, cx: &App) -> Option<AgentSpec> {
    let named = (!agent.is_empty()).then_some(agent);
    let base = config::agent_or_first(&Shared::global(cx).settings.agents, named)?.clone();
    Some(onehand_core::task::run_spec(base))
}

/// The mode a lead's worker on agent `agent` starts in.
pub(crate) fn worker_mode(agent: &str, cx: &App) -> Option<String> {
    config::worker_mode(&Shared::global(cx).settings.agents, agent)
}

/// A Resume or a Retry of `task` on `template`: judged by its last run's own
/// setup, never what Settings says now, since that is what will run.
pub(crate) fn of_task(task: &Task, template: Template, cx: &App) -> Facts {
    let setup = task.runs().last().map_or(task.setup(), |run| &run.setup);
    with_setup(setup, Ok(template), cx)
}

/// A start on `workflow`, or why there is none, that runs with `setup`: a
/// retry with current settings is judged on the setup it will take, not the
/// last run's.
pub(crate) fn with_setup(setup: &Setup, workflow: Result<Template, String>, cx: &App) -> Facts {
    let shared = Shared::global(cx);
    // A setup naming no agent starts the first one configured; one whose
    // agent is gone is judged as naming it, which the preflight says.
    let agent = setup.agent.clone().or_else(|| first_agent(cx));
    let spec = agent
        .as_deref()
        .and_then(|name| shared.settings.agents.iter().find(|spec| spec.name == name));
    Facts {
        step_agents: step_agents(&workflow, cx),
        workflow,
        agent_configured: spec.is_some(),
        offered: spec.and_then(|spec| offered(spec, cx)),
        agent,
        mode: setup.mode.as_deref().and_then(config::mode_set),
        has_check: setup.check.is_some(),
        in_git: true,
        forge: setup.forge.as_deref().map(|name| forge_named(name, cx)),
        queued_behind: None,
        shared_checkout: None,
        forge_read: true,
    }
}

/// What a new task would be given now, for a retry with current settings.
pub(crate) fn now_for(check: Option<String>, cx: &App) -> Current {
    onehand_core::task::now_for(first_agent(cx), check)
}

/// Whether a retry of `task` with current settings would run with another
/// timeout than its last run did: when a run that timed out is offered that
/// second way out.
pub(crate) fn timeout_moved(task: &Task, cx: &App) -> bool {
    let Some(last) = task.runs().last() else {
        return false;
    };
    // The check command moves no timeout, so it is not asked for.
    let now = now_for(None, cx);
    let newest = crate::workflow::newest(&last.template, cx);
    onehand_core::workflow::Run::with_current(last, newest, &now, false).is_ok_and(|plan| {
        plan.changes
            .iter()
            .any(|change| change.field == Field::Timeout)
    })
}
