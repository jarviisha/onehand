//! Where a retry of a run starts and what it carries over from the last.

use super::{Outcome, Run};
use crate::workflow::prompt;
use crate::workflow::template::{StepKind, Template};

impl Run {
    /// Where a retry of `prev` on `template` starts: the first step it
    /// cannot carry over, one the last run had not passed, or one that
    /// differs in `template`, or reads a step that does. The step count when
    /// every step carries over.
    pub fn retry_start(prev: &Run, template: &Template) -> usize {
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
    pub fn retry_offered(prev: &Run, template: &Template) -> usize {
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
    pub fn recheck(template: &Template, start: usize) -> usize {
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
        run
    }
}
