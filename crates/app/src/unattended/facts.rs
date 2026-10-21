//! What a start on an issue is checked against: the facts the app holds,
//! gathered for the preflight core decides with.

use super::launch;
use crate::state::Shared;
use gpui::App;
use onehand_core::config::{self, AgentSpec};
use onehand_core::connector::Connector;
use onehand_core::preflight::{Facts, Forge, ProjectFacts};
use onehand_core::task::Task;
use onehand_core::unattended::{Issue, Tracker};
use onehand_core::workflow::Template;

/// What every new issue run on the configuration as it stands shares: the
/// agent, its mode and what is known of it, and the slots. The workflow, the
/// project and the issue are the caller's to fill in.
pub(crate) fn common(cx: &App) -> Facts {
    let shared = Shared::global(cx);
    let u = shared.unattended.as_ref();
    let agent = super::run_agent(cx);
    let spec = agent
        .as_deref()
        .and_then(|name| shared.agents.iter().find(|spec| spec.name == name));
    Facts {
        workflow: Err("no workflow is chosen yet".to_string()),
        agent_configured: spec.is_some(),
        offered: spec.and_then(|spec| offered(spec, cx)),
        agent,
        mode: u.and_then(|_| config::mode_set(&super::run_mode(cx))),
        has_check: false,
        in_git: false,
        checked_out: None,
        forge: None,
        issue: None,
        slots: u.map(|u| super::slots_of(u, cx)),
        queued_behind: None,
        shared_checkout: None,
        forge_read: true,
        review: None,
    }
}

/// A new issue run of workflow `id` on `issue`, living in `tracker`, in the
/// project whose check command exists or not as `has_check` says, on
/// `checked_out` (`None` outside git), its work going to `forge`.
pub(crate) fn issue_run(
    id: &str,
    tracker: &Tracker,
    issue: &Issue,
    has_check: bool,
    checked_out: Option<String>,
    forge: Option<&'static dyn Connector>,
    cx: &App,
) -> Facts {
    let project = ProjectFacts {
        has_check,
        checked_out,
        forge: forge.map(|f| self::forge(f, cx)),
    };
    // Only this issue's tasks are copied: this is asked on every frame the
    // form is drawn.
    let tasks = crate::task::issue_tasks(cx, |of| {
        of.tracker == tracker.to_ref() && of.number == issue.number
    });
    onehand_core::preflight::new_issue_run(
        common(cx),
        template(id, cx),
        tracker,
        issue,
        project,
        tasks,
    )
}

/// The workflow `id` as an issue's run would take it, its timeout the runs'
/// own, not yet judged: the preflight says what is wrong with it.
pub(crate) fn template(id: &str, cx: &App) -> Result<Template, String> {
    let timeout = Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| u.timeout.clone())
        .ok_or("unattended runs are not set up")?;
    launch::found(id, &timeout, cx)
}

/// `forge` as a preflight reads it: its name, and its account as last seen.
pub(crate) fn forge(forge: &'static dyn Connector, cx: &App) -> Forge {
    Forge {
        name: forge.name().to_string(),
        account: account(forge, cx),
    }
}

/// What `forge`'s account said when last asked: `None` before it answered.
fn account(forge: &'static dyn Connector, cx: &App) -> Option<Result<String, String>> {
    super::accounts(cx)?
        .into_iter()
        .find(|(c, _)| c.name() == forge.name())
        .map(|(_, said)| said)
}

/// What the agent `spec` offered when it last came up in this process,
/// started as it is configured now; `None` when it has not.
///
/// A run's agent is started through the spec runs wrap it in, so what it
/// offered counts as the configured spec's too.
pub(crate) fn offered(spec: &AgentSpec, cx: &App) -> Option<onehand_core::workflow::Offer> {
    let wrapped = super::report::wrapped(spec.clone());
    Shared::global(cx)
        .offers_seen
        .iter()
        .find(|(seen, _)| seen == spec || *seen == wrapped)
        .map(|(_, offer)| offer.clone())
}

/// A Resume or a Retry of `task` on `template`: judged by its last run's own
/// setup, never what Settings says now, since that is what will run.
pub(crate) fn of_task(task: &Task, template: Template, cx: &App) -> Facts {
    let setup = task.runs.last().map_or(&task.setup, |run| &run.setup);
    with_setup(task, setup, Ok(template), cx)
}

/// A start of `task` on `workflow`, or why there is none, that runs with
/// `setup`: a retry with current settings is judged on the setup it will
/// take, not the last run's.
pub(crate) fn with_setup(
    task: &Task,
    setup: &onehand_core::workflow::Setup,
    workflow: Result<Template, String>,
    cx: &App,
) -> Facts {
    let shared = Shared::global(cx);
    // A setup naming no agent starts the first one configured; one whose
    // agent is gone is judged as naming it, which the preflight says.
    let agent = setup
        .agent
        .clone()
        .or_else(|| config::agent_or_first(&shared.agents, None).map(|spec| spec.name.clone()));
    let spec = agent
        .as_deref()
        .and_then(|name| shared.agents.iter().find(|spec| spec.name == name));
    let forge = setup.forge.as_deref().map(|name| Forge {
        name: name.to_string(),
        account: onehand_core::connector::named(crate::features::connectors(), name)
            .and_then(|forge| account(forge, cx)),
    });
    // Held to the cap as the tick is: an issue's task not already counted.
    let slots = shared.unattended.as_ref().and_then(|u| {
        let counted = u.starting.contains(&task.id) || crate::task::is_working(&task.id, cx);
        (task.issue().is_some() && !counted).then(|| super::slots_of(u, cx))
    });
    Facts {
        workflow,
        agent_configured: spec.is_some(),
        offered: spec.and_then(|spec| offered(spec, cx)),
        agent,
        mode: setup.mode.as_deref().and_then(config::mode_set),
        has_check: setup.check.is_some(),
        in_git: true,
        checked_out: None,
        forge,
        issue: None,
        slots,
        queued_behind: None,
        shared_checkout: None,
        forge_read: true,
        review: None,
    }
}
