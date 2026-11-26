//! What a start checks before it cuts a worktree or starts an agent, and
//! what it found.
//!
//! **It judges the configuration that will actually run.** A new start runs
//! what Settings says now; a Resume or a Retry runs the run's own setup, so a
//! setting changed since does not clear what blocks it. The app gathers the
//! facts (nothing here reads the disk or the network), and draws what comes
//! back where the start is asked for; the callers that cut still do that
//! themselves, after this.

use crate::workflow::{Failure, Offer, Place, StepKind, Template};

/// What kind of start is checked: each has its own configuration and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// From the launcher: the workflow picked, cut off `HEAD` when it works
    /// on a worktree.
    NewRun,
    /// A run cut off, carried on where it stopped, on its own snapshot.
    Resume,
    /// A task's next run, on its last run's own snapshot and setup.
    Retry,
    /// A task's next run with what Settings say now: its own workflow at
    /// its newest version, judged as a new one is, on the configuration a
    /// new task of its kind would take.
    RetryCurrent,
}

impl Kind {
    /// It runs the task's last run's own setup, so what Settings say now
    /// neither blocks it nor clears a block.
    fn keeps_own(self) -> bool {
        match self {
            Self::Resume | Self::Retry => true,
            Self::NewRun | Self::RetryCurrent => false,
        }
    }

    /// Its workflow is judged anew, validation included: it is not one a
    /// run has already started on.
    fn judges_workflow(self) -> bool {
        match self {
            Self::NewRun | Self::RetryCurrent => true,
            Self::Resume | Self::Retry => false,
        }
    }

    /// What a block of its own setup adds, said after the block.
    fn keeps(self) -> &'static str {
        match self {
            Self::Retry => {
                " The run keeps its own setup; Retry with current settings runs with what \
                 Settings say now."
            }
            Self::Resume => {
                " The run keeps its own setup, so changing Settings does not change it."
            }
            Self::NewRun | Self::RetryCurrent => "",
        }
    }

    /// Where a block of its own setup is changed, when that is somewhere
    /// else: a Retry's by the retry that runs with what Settings say now.
    fn own_change(self) -> Option<Change> {
        match self {
            Self::Retry => Some(Change::RetryCurrent),
            Self::NewRun | Self::Resume | Self::RetryCurrent => None,
        }
    }
}

/// The name of the retry that runs with what Settings say now, as every
/// sentence and button names it.
pub const RETRY_CURRENT: &str = "Retry with current settings";

/// Where workflows are written, as a finding names it.
const WORKFLOWS_PAGE: &str = "the Workflows page";

/// Where a forge's account is seen to, as a finding names it.
const CONNECTIONS: &str = "Settings ▸ Connections";

/// Where a project's check command is set, as a finding names it.
const PROJECT_PAGE: &str = "the project's page";

/// Where what a finding blocks is changed, when that is somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A place in Settings or the config file, by its name there.
    At(&'static str),
    /// The retry that runs with what Settings say now, which the finding
    /// names itself and its dialog offers.
    RetryCurrent,
}

impl Change {
    /// What a finding's line adds for it, if anything.
    pub fn said(self) -> Option<String> {
        match self {
            Self::At(place) => Some(format!(" Changed in {place}.")),
            Self::RetryCurrent => None,
        }
    }
}

/// What a finding is about, in the order findings are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum About {
    Workflow,
    Agent,
    Mode,
    Settings,
    CheckCommand,
    Place,
    Base,
    Forge,
    PlaceTaken,
    Limits,
}

/// One thing the preflight found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub about: About,
    /// It stops the start; otherwise it is said and the start goes on.
    pub blocks: bool,
    pub text: String,
    /// Where it is changed, when that is somewhere else.
    pub change: Option<Change>,
    /// *Check the agent* now might clear it: it says what is not known only
    /// because the agent itself has not come up, never what a particular
    /// step would still need its own check for.
    pub checkable: bool,
}

/// The forge serving the project, and what its account said when last seen:
/// `None` until it has been asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forge {
    pub name: String,
    pub account: Option<Result<String, String>>,
}

/// An agent a step of the workflow names, and what is known of it.
#[derive(Debug, Clone)]
pub struct StepAgent {
    /// The step's place in the workflow, from 0.
    pub step: usize,
    pub agent: String,
    /// That agent is among the configured ones.
    pub configured: bool,
    /// What it offers, as `Facts::offered` for the run's own agent.
    pub offered: Option<Offer>,
}

/// What the app knows when a start is asked for.
#[derive(Debug, Clone)]
pub struct Facts {
    /// The workflow that will run, or why there is none.
    pub workflow: Result<Template, String>,
    /// The agent the setup names; `None` when nothing is configured at all.
    pub agent: Option<String>,
    /// That agent is among the configured ones.
    pub agent_configured: bool,
    /// The mode the run starts in, if one is set.
    pub mode: Option<String>,
    /// What the agent offers, when learned in this process from the spec it
    /// is configured with now; `None` when not known yet.
    pub offered: Option<Offer>,
    /// One for each agent step that names an agent of its own.
    pub step_agents: Vec<StepAgent>,
    /// The run has a check command to run.
    pub has_check: bool,
    /// The project is in git.
    pub in_git: bool,
    /// The forge the work goes to, `None` on a project no forge serves.
    pub forge: Option<Forge>,
    /// Which forge serves the project has been read: until it has, `None`
    /// above says nothing.
    pub forge_read: bool,
    /// The task whose place this start would queue behind.
    pub queued_behind: Option<String>,
    /// A person's own session working in the checkout the start works in,
    /// by its name.
    pub shared_checkout: Option<String>,
}

/// Why a start of `kind` on `facts` is refused: what the first finding that
/// blocks says, if one does.
pub fn refusal(kind: Kind, facts: &Facts) -> Option<String> {
    preflight(kind, facts)
        .into_iter()
        .find(|f| f.blocks)
        .map(|f| f.text)
}

/// Everything `facts` says about a start of `kind`: blocks first, the
/// workflow's own problems first among them, then what only informs.
pub fn preflight(kind: Kind, facts: &Facts) -> Vec<Finding> {
    let mut found = Vec::new();
    let (own, keeps, own_change) = (kind.keeps_own(), kind.keeps(), kind.own_change());

    // The workflow.
    let template = match &facts.workflow {
        Ok(template) => Some(template),
        Err(why) => {
            found.push(finding(About::Workflow, true, capital(why), None));
            None
        }
    };
    if let Some(template) = template.filter(|_| kind.judges_workflow()) {
        for problem in crate::workflow::validate(template) {
            found.push(finding(
                About::Workflow,
                true,
                format!("The workflow cannot run: {problem}"),
                Some(Change::At(WORKFLOWS_PAGE)),
            ));
        }
    }

    // Each agent a step names.
    if let Some(template) = template {
        for named in facts.step_agents.iter().filter(|named| !named.configured) {
            let label = template
                .steps
                .get(named.step)
                .map_or("A step", |s| &s.label);
            found.push(finding(
                About::Agent,
                true,
                format!(
                    "{label} names {}, which is not in the agent list.",
                    named.agent
                ),
                Some(Change::At("Settings ▸ Agents")),
            ));
        }
    }

    // The agent and its mode.
    match (&facts.agent, facts.agent_configured) {
        (None, _) => found.push(finding(
            About::Agent,
            true,
            "No agent is configured to run it.".to_string(),
            Some(Change::At("Settings ▸ Agents")),
        )),
        (Some(name), false) => found.push(finding(
            About::Agent,
            true,
            format!("The agent `{name}` is no longer configured.{keeps}"),
            own_change.or(Some(Change::At("Settings ▸ Agents"))),
        )),
        (Some(_), true) => {}
    }
    // Whether the mode's own "not known yet" below already said the agent
    // has not come up: the step settings below then say nothing more of it.
    let mut mode_unknown = false;
    if let Some(mode) = facts.mode.as_deref().filter(|m| !m.trim().is_empty()) {
        let change = own_change;
        match &facts.offered {
            Some(offered) => {
                if let Some(why) = mode_refused(mode, &offered.modes) {
                    found.push(finding(
                        About::Mode,
                        true,
                        format!("{}.{keeps}", capital(&why)),
                        change,
                    ));
                }
            }
            None => {
                mode_unknown = true;
                found.push(checkable_finding(
                    About::Mode,
                    format!(
                        "Mode `{mode}` is not known yet: the agent has not come up since \
                         onehand started or since it was edited, and it is checked when it does."
                    ),
                ));
            }
        }
    }

    // Each agent step's own mode and options, for every kind: a Resume or a
    // Retry judges its run's own snapshot, the same as its starting mode.
    if let Some(template) = template {
        let change = match own {
            true => own_change,
            false => Some(Change::At(WORKFLOWS_PAGE)),
        };
        found.extend(settings::step_settings(
            template,
            facts.offered.as_ref(),
            &facts.step_agents,
            keeps,
            change,
            mode_unknown,
        ));
    }

    // The check command.
    if let Some(template) = template {
        let commands = template
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Command { .. }));
        if let Some(why) = template.lacks_check(facts.has_check) {
            found.push(finding(
                About::CheckCommand,
                true,
                format!("{}.{keeps}", capital(&why)),
                own_change.or(Some(Change::At(PROJECT_PAGE))),
            ));
        } else if !commands {
            found.push(finding(
                About::CheckCommand,
                false,
                "The workflow runs no command: nothing verifies the work.".to_string(),
                None,
            ));
        }
    }

    // Where it works, and what the branch is cut off.
    let worktree = template.is_some_and(|t| t.place == Place::Worktree);
    match kind {
        Kind::NewRun if worktree && !facts.in_git => found.push(finding(
            About::Place,
            true,
            "The project is not in git, so no worktree can be cut for it.".to_string(),
            None,
        )),
        Kind::NewRun if worktree => found.push(finding(
            About::Base,
            false,
            "The branch is cut off `HEAD`.".to_string(),
            None,
        )),
        // Only a workflow that reads is known to work in the checkout.
        Kind::NewRun if template.is_some() => {
            if let Some(session) = &facts.shared_checkout {
                found.push(finding(
                    About::Place,
                    false,
                    format!(
                        "Your session “{session}” works in this checkout too: the run edits \
                         the same files, and neither sees the other's edits coming."
                    ),
                    None,
                ));
            }
        }
        Kind::NewRun | Kind::Resume | Kind::Retry | Kind::RetryCurrent => {}
    }

    // The forge, which only forge steps need.
    let forge_steps = template.is_some_and(|t| {
        t.steps.iter().any(|step| match step.kind {
            StepKind::Push | StepKind::PullRequest | StepKind::StatusChecks { .. } => true,
            StepKind::Agent { .. }
            | StepKind::Review { .. }
            | StepKind::Command { .. }
            | StepKind::Approval { .. } => false,
        })
    });
    let refused = match &facts.forge {
        Some(Forge {
            name,
            account: Some(Err(why)),
        }) if forge_steps => Some((name, why)),
        _ => None,
    };
    if let Some((name, why)) = refused {
        found.push(finding(
            About::Forge,
            true,
            format!("{name} cannot be used: {why}"),
            Some(Change::At(CONNECTIONS)),
        ));
    }
    if forge_steps && facts.forge_read && facts.forge.is_none() {
        found.push(finding(
            About::Forge,
            false,
            "No forge serves the project: the forge steps pass at once, and the branch is \
             the result."
                .to_string(),
            None,
        ));
    }

    // The limits the run is held to, from the snapshot that will run.
    if let Some(template) = template {
        found.push(finding(About::Limits, false, limits_line(template), None));
    }

    // The place.
    if let Some(ahead) = &facts.queued_behind {
        found.push(finding(
            About::PlaceTaken,
            false,
            format!("The place is taken: the run will queue behind {ahead}."),
            None,
        ));
    }

    found.sort_by_key(|f| (!f.blocks, f.about));
    found
}

/// One finding about `about`, offering no task instead, not resolved by
/// *Check the agent*.
fn finding(about: About, blocks: bool, text: String, change: Option<Change>) -> Finding {
    Finding {
        about,
        blocks,
        text,
        change,
        checkable: false,
    }
}

/// `finding`, but resolved by pressing *Check the agent* now: what it says
/// is not known only because the agent itself has not come up.
fn checkable_finding(about: About, text: String) -> Finding {
    Finding {
        checkable: true,
        ..finding(about, false, text, None)
    }
}

/// The task a start working in `place` would queue behind, `holder` holding
/// the checkout it works in: a start on a worktree is cut a place of its own
/// and never waits.
pub fn queued_behind_for(place: Place, holder: Option<String>) -> Option<String> {
    match place {
        Place::Checkout => holder,
        Place::Worktree => None,
    }
}

/// What a run fails on when what `about` would have blocked is found only
/// once it runs: a configuration failure is a preflight block found late,
/// so the way out offered is the one that changes the configuration.
pub fn found_late(about: About) -> Failure {
    match about {
        About::Workflow | About::Agent | About::Mode | About::Settings | About::CheckCommand => {
            Failure::Configuration
        }
        About::Forge => Failure::Forge,
        About::Place | About::Base | About::PlaceTaken | About::Limits => Failure::Other,
    }
}

/// Why an agent offering `offered` cannot start in `mode`, if it cannot.
pub fn mode_refused(mode: &str, offered: &[String]) -> Option<String> {
    (!offered.iter().any(|m| m == mode)).then(|| {
        let offers = match offered.is_empty() {
            true => "none".to_string(),
            false => offered.join(", "),
        };
        format!("the agent offers no mode `{mode}` (it offers: {offers})")
    })
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

mod settings;

/// The limits a run of `template` is held to, as a line.
fn limits_line(template: &Template) -> String {
    let time = match template.timeout_limit() {
        Some(_) => format!(
            "{} of agent work, onehand's own steps and waiting on a person not counted",
            template.timeout_said()
        ),
        None => "no time limit".to_string(),
    };
    let turns = match template.misses {
        1 => "turn",
        _ => "turns",
    };
    format!(
        "Limits: {time}; {} failed {turns} in a stretch of steps before it stops.",
        template.misses
    )
}

#[cfg(test)]
mod tests;
