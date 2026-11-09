//! What a run starts with, as the configuration stands: its agent and mode,
//! the workflows an issue may be given, and what stops every run.

use super::{UnattendedDrive, World};
use crate::config::{self, AgentSpec};
use crate::preflight::Facts;
use crate::task::tasks::link::Seen;
use crate::unattended::{Choosing, Judging};
use crate::workflow::{Offer, Template};

impl UnattendedDrive {
    /// The agent a run starts, by name: the one set for runs, else the first
    /// configured.
    pub fn run_agent(&self, agents: &[AgentSpec]) -> Option<String> {
        self.agent
            .clone()
            .or_else(|| config::agent_or_first(agents, None).map(|spec| spec.name.clone()))
    }

    /// The mode a run started now starts in. Empty leaves the agent's own.
    pub fn run_mode(&self, agents: &[AgentSpec]) -> String {
        config::run_mode(
            self.mode.as_deref(),
            agents,
            self.run_agent(agents).as_deref(),
        )
    }

    /// The mode a lead's worker on agent `agent` starts in.
    pub fn worker_mode(&self, agent: &str, agents: &[AgentSpec]) -> Option<String> {
        config::worker_mode(
            self.mode.as_deref(),
            self.run_agent(agents).as_deref(),
            agents,
            agent,
        )
    }

    /// What runs start with, for a retry with current settings.
    pub fn run_settings(&self, agents: &[AgentSpec]) -> crate::unattended::RunSettings {
        crate::unattended::RunSettings {
            agent: self.agent.clone(),
            mode: self.run_mode(agents),
            timeout: self.timeout.clone(),
        }
    }

    /// The workflow `id` with the runs' timeout put over its own, not yet
    /// judged, or why there is none.
    pub fn template(&self, id: &str, world: &World) -> Result<Template, String> {
        let mut template = world
            .library
            .iter()
            .find(|template| template.id == id)
            .cloned()
            .ok_or_else(|| format!("there is no workflow `{id}`"))?;
        template.timeout = self.timeout.clone();
        Ok(template)
    }

    /// The workflow `id` as a run of it starts, or why it cannot run.
    pub fn workflow(&self, id: &str, world: &World) -> Result<Template, String> {
        self.template(id, world)
            .and_then(crate::unattended::fit_for_issue)
    }

    /// Why a project that `has` a check command or not cannot be worked by
    /// workflow `id`, or why that workflow cannot run anywhere.
    pub fn lacks_check(&self, has: bool, id: &str, world: &World) -> Option<String> {
        match self.workflow(id, world) {
            Ok(template) => crate::unattended::check_missing(&template, has),
            Err(why) => Some(why),
        }
    }

    /// What every new issue run on the configuration as it stands shares:
    /// the agent, its mode and what is known of it, and the slots.
    pub fn common<W: Copy>(&self, seen: &Seen<W>, world: &World) -> Facts {
        let agent = self.run_agent(&world.agents);
        let spec = agent
            .as_deref()
            .and_then(|name| world.agents.iter().find(|spec| spec.name == name));
        Facts {
            workflow: Err("no workflow is chosen yet".to_string()),
            agent_configured: spec.is_some(),
            offered: spec.and_then(|spec| offered(spec, &world.offers)),
            agent,
            mode: config::mode_set(&self.run_mode(&world.agents)),
            has_check: false,
            in_git: false,
            checked_out: None,
            forge: None,
            issue: None,
            slots: Some(self.slots(seen)),
            queued_behind: None,
            shared_checkout: None,
            forge_read: true,
            review: None,
        }
    }

    /// Why nothing at all may start, picked or found, over the workflows the
    /// config names, each as a run would take it.
    pub(super) fn cannot_start(&self, world: &World) -> Option<String> {
        let default = self.workflow(&self.workflow, world);
        let by_label: Vec<_> = self
            .workflows
            .iter()
            .map(|(label, id)| (label.clone(), self.workflow(id, world)))
            .collect();
        // A gone agent's run starts on the first configured.
        let agent_configured = config::agent_or_first(&world.agents, self.agent.as_deref())
            .or_else(|| config::agent_or_first(&world.agents, None))
            .is_some();
        crate::unattended::cannot_start(
            self.paused.as_deref(),
            &default,
            &by_label,
            agent_configured,
        )
    }

    /// How the search chooses each issue's workflow.
    pub(super) fn choosing(&self, world: &World) -> Choosing {
        let validated: Vec<_> = std::iter::once(&self.workflow)
            .chain(self.workflows.values())
            .map(|id| (id.clone(), self.workflow(id, world)))
            .collect();
        Choosing::new(self.workflow.clone(), self.workflows.clone(), &validated)
    }

    /// What a search judges each new task by.
    pub(super) fn judging<W: Copy>(&self, seen: &Seen<W>, world: &World) -> Judging {
        let templates = std::iter::once(&self.workflow)
            .chain(self.workflows.values())
            .map(|id| (id.clone(), self.template(id, world)))
            .collect();
        Judging {
            common: self.common(seen, world),
            templates,
            tasks: seen
                .all()
                .iter()
                .filter(|task| task.issue().is_some())
                .map(|task| (task.clone(), seen.working(&task.id)))
                .collect(),
        }
    }
}

/// `base` as a run starts it: with every build it makes pointed at one
/// shared directory, through the one process a run gets to set anything on.
pub fn run_spec(mut base: AgentSpec) -> AgentSpec {
    if let Some(target) = crate::unattended::target_dir() {
        base.env
            .insert("CARGO_TARGET_DIR".into(), target.display().to_string());
    }
    base
}

/// What agent `spec` offered when it last came up, by `offers`, started as
/// it is configured now; a run's agent is started through [`run_spec`], so
/// what it offered counts as the configured spec's too.
pub fn offered(spec: &AgentSpec, offers: &[(AgentSpec, Offer)]) -> Option<Offer> {
    let wrapped = run_spec(spec.clone());
    offers
        .iter()
        .find(|(seen, _)| seen == spec || *seen == wrapped)
        .map(|(_, offer)| offer.clone())
}
