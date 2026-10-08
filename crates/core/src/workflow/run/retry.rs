//! Where a retry of a run starts and what it carries over from the last.

use super::{Outcome, Run, Setup};
use crate::workflow::prompt;
use crate::workflow::template::{StepKind, Template};

/// What a task's next run takes when it runs with what Settings say now:
/// what a new task of its kind would be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Now {
    pub agent: Option<String>,
    pub mode: Option<String>,
    pub check: Option<String>,
    /// The timeout a new task of its kind is given; `None` keeps the
    /// workflow's own.
    pub timeout: Option<String>,
}

/// One thing a retry with current settings runs differently, as it was and
/// as it will be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub field: Field,
    pub old: String,
    pub new: String,
}

/// What a retry carries over from the last run, or takes from Settings now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    WorkflowVersion,
    Agent,
    Mode,
    CheckCommand,
    Timeout,
}

impl Field {
    /// Every field, in the order a retry plan lists what changed. A new
    /// variant is added here as well as to the matches below, which make it
    /// known.
    pub const ALL: [Self; 5] = [
        Self::WorkflowVersion,
        Self::Agent,
        Self::Mode,
        Self::CheckCommand,
        Self::Timeout,
    ];

    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkflowVersion => "Workflow version",
            Self::Agent => "Agent",
            Self::Mode => "Mode",
            Self::CheckCommand => "Check command",
            Self::Timeout => "Timeout",
        }
    }

    /// What a run of `template` with `setup` has for this field, as said.
    pub fn shown(self, template: &Template, setup: &Setup) -> String {
        let shown = |value: &Option<String>, none: &str| value.clone().unwrap_or(none.into());
        match self {
            Self::WorkflowVersion => format!("version {}", template.version),
            Self::Agent => shown(&setup.agent, "the first configured"),
            Self::Mode => shown(&setup.mode, "as the agent starts"),
            Self::CheckCommand => shown(&setup.check, "none"),
            Self::Timeout => template.timeout.clone(),
        }
    }
}

/// A retry of a run with what Settings say now, before anything starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithCurrent {
    /// The task's own workflow at its newest version, its timeout set.
    pub template: Template,
    /// The last run's setup, its agent, mode and check command as now.
    pub setup: Setup,
    /// What differs from the last run, in a fixed order.
    pub changes: Vec<Changed>,
    /// The step it starts at, and why there.
    pub start: usize,
    pub why: StartWhy,
}

/// Why a retry with current settings starts where it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartWhy {
    /// The first step it cannot carry over, as a Retry would.
    CarriedOver,
    /// The first command step whose command changes.
    CommandChanged,
    /// The work changed since the last run stopped, and is checked again.
    WorkChanged,
}

impl StartWhy {
    /// As the dialog says it, after the step.
    pub fn said(self) -> &'static str {
        match self {
            Self::CarriedOver => "the first it cannot carry over",
            Self::CommandChanged => "the first step whose command changes",
            Self::WorkChanged => "where the changed work is checked again",
        }
    }
}

impl Run {
    /// Where a retry of `prev` on `template` starts: the first step it
    /// cannot carry over, one the last run had not passed, or one that
    /// differs in `template`, or reads a step that does. The step count when
    /// every step carries over.
    pub(crate) fn retry_start(prev: &Run, template: &Template) -> usize {
        let same = |step: &str| {
            let find = |t: &Template| t.steps.iter().find(|s| s.id == step).cloned();
            find(template).is_some_and(|s| Some(s) == find(&prev.template))
        };
        // Passed means passed in the last run's own template: a step dropped
        // earlier on in `template` must not move one it failed into the past.
        // A run that ended done passed them all, its last step included,
        // though finishing never moves `step` past it.
        let passed = |step: &str| {
            prev.template
                .index_of(step)
                .is_some_and(|at| at < prev.step || prev.outcome == Some(Outcome::Done))
        };
        template
            .steps
            .iter()
            .position(|step| {
                let reads: Vec<&str> = match &step.kind {
                    StepKind::Agent { prompt, .. } => prompt::refs(prompt)
                        .into_iter()
                        .filter_map(|name| name.strip_prefix("output."))
                        .collect(),
                    StepKind::Approval { of } => vec![of.as_str()],
                    kind => kind.sends_back_to().into_iter().collect(),
                };
                !passed(&step.id)
                    || !same(&step.id)
                    || reads.iter().any(|read| !read.is_empty() && !same(read))
            })
            .unwrap_or(template.steps.len())
    }

    /// The step a retry of `prev` on `template` is offered from: where it
    /// would start, or the first step when the last run got to the end,
    /// since a retry that starts past the last step runs nothing.
    pub(crate) fn retry_offered(prev: &Run, template: &Template) -> usize {
        match Run::retry_start(prev, template) {
            at if at >= template.steps.len() => 0,
            at => at,
        }
    }

    /// Where a retry that would start at `start` on `template` starts instead
    /// when the work changed since the last run stopped: at the last command
    /// step up to it. What that check passed on is no longer what is there,
    /// and a push past it would send the commit it passed on rather than the
    /// work. `start` itself when no command step comes up to it.
    pub(crate) fn recheck(template: &Template, start: usize) -> usize {
        let upto = (start + 1).min(template.steps.len());
        template.steps[..upto]
            .iter()
            .rposition(|step| match step.kind {
                StepKind::Command { .. } => true,
                StepKind::Agent { .. }
                | StepKind::Approval { .. }
                | StepKind::Push
                | StepKind::PullRequest
                | StepKind::StatusChecks { .. } => false,
            })
            .unwrap_or(start)
    }

    /// Where a retry of `prev` on `template` starts, [`Run::retry_start`] or
    /// earlier at step `from` when `template` has it, and how many answers
    /// of the steps before that it carries over.
    pub fn retry_plan(prev: &Run, template: &Template, from: Option<&str>) -> (usize, usize) {
        let start = Run::retry_from(prev, template, from);
        (start, Run::carried(prev, template, start).count())
    }

    /// The steps a Retry of `prev` on its own `template` may start from, the
    /// first up to the returned latest, and the one it offers first. With the
    /// work `changed` since the last run stopped, nothing past the last
    /// command step up to the start is offered: what that check passed on is
    /// not what is there now, and a push past it would send the old commit.
    pub fn retry_offer(prev: &Run, template: &Template, changed: bool) -> (usize, usize) {
        let mut latest = Run::retry_start(prev, template);
        if changed {
            latest = Run::recheck(template, latest);
        }
        (latest, Run::retry_offered(prev, template).min(latest))
    }

    /// [`Run::retry_start`], or step `from` when `template` has it earlier.
    fn retry_from(prev: &Run, template: &Template, from: Option<&str>) -> usize {
        let start = Run::retry_start(prev, template);
        from.and_then(|step| template.index_of(step))
            .map_or(start, |at| at.min(start))
    }

    /// The answers `prev` kept for the steps of `template` before `start`.
    fn carried<'a>(
        prev: &'a Run,
        template: &'a Template,
        start: usize,
    ) -> impl Iterator<Item = (String, String)> + 'a {
        template.steps[..start]
            .iter()
            .filter_map(|step| Some((step.id.clone(), prev.outputs.get(&step.id)?.clone())))
    }

    /// A new run of the task `prev` was a run of, on `template`, starting
    /// where [`Run::retry_plan`] says, with what the steps before the start
    /// kept carried over.
    pub(crate) fn retry_of(prev: &Run, id: String, template: Template, from: Option<&str>) -> Self {
        let start = Run::retry_from(prev, &template, from);
        let outputs = Run::carried(prev, &template, start).collect();
        let mut run = Run::new(id, template, prev.brief.clone(), prev.setup.clone());
        run.step = start;
        run.furthest = start;
        run.outputs = outputs;
        // What was checked stays checked: a retry that starts at the push
        // pushes the commit the last check passed on.
        run.marks.verified_at = prev.marks.verified_at.clone();
        // The same branch, so the same pull request: a retry from after the
        // pull request step works on the one the last run opened.
        run.pull_request = prev.pull_request.clone();
        run
    }

    /// `prev` retried with `now`, on `newest`: the task's own workflow by id
    /// at the newest version on offer, or why there is none.
    ///
    /// It starts at the earlier of the carry-over start and **the first
    /// command step before it whose command changes**: a step's command is
    /// its own, or the check command when it names none. Not the last command
    /// step: a changed project check before a step with a command of its own
    /// would otherwise push work the new check never ran on. A changed agent,
    /// mode or timeout moves no start.
    pub fn with_current(
        prev: &Run,
        newest: Result<Template, String>,
        now: &Now,
        changed: bool,
    ) -> Result<WithCurrent, String> {
        let mut template = newest?;
        let problems = crate::workflow::validate(&template);
        if !problems.is_empty() {
            let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
            return Err(format!(
                "the workflow `{}` no longer validates: {}",
                template.name,
                said.join("; ")
            ));
        }
        if let Some(timeout) = &now.timeout {
            template.timeout = timeout.clone();
        }
        let setup = Setup {
            agent: now.agent.clone(),
            mode: now.mode.clone(),
            check: now.check.clone(),
            ..prev.setup.clone()
        };
        let carry = Run::retry_offered(prev, &template);
        let command = |template: &Template, at: &str, check: &Option<String>| match &template
            .steps
            .iter()
            .find(|step| step.id == at)?
            .kind
        {
            StepKind::Command { command, .. } => command.clone().or_else(|| check.clone()),
            StepKind::Agent { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest
            | StepKind::StatusChecks { .. } => None,
        };
        let changed_command = template.steps[..carry].iter().position(|step| {
            matches!(step.kind, StepKind::Command { .. })
                && command(&template, &step.id, &setup.check)
                    != command(&prev.template, &step.id, &prev.setup.check)
        });
        let (mut start, mut why) = match changed_command {
            Some(at) if at < carry => (at, StartWhy::CommandChanged),
            Some(_) | None => (carry, StartWhy::CarriedOver),
        };
        if changed && Run::recheck(&template, start) < start {
            (start, why) = (Run::recheck(&template, start), StartWhy::WorkChanged);
        }
        let changes = Field::ALL
            .into_iter()
            .filter_map(|field| {
                let old = field.shown(&prev.template, &prev.setup);
                let new = field.shown(&template, &setup);
                (old != new).then_some(Changed { field, old, new })
            })
            .collect();
        Ok(WithCurrent {
            template,
            setup,
            changes,
            start,
            why,
        })
    }
}
