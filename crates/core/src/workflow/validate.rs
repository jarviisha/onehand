//! What keeps a template from being saved or run. Checked once, when it is
//! saved and again before a run starts, so a run never meets a step that
//! names nothing or a prompt variable with no value.

use super::prompt;
use super::template::{GateKind, StepKind, Template, SCHEMA_VERSION};

/// One thing wrong with a template: the step it is in, by position, if it is
/// in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub step: Option<usize>,
    pub said: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.step {
            Some(at) => write!(f, "Step {}: {}", at + 1, self.said),
            None => f.write_str(&self.said),
        }
    }
}

/// Everything wrong with `template`; empty when it may be saved and run.
pub fn validate(template: &Template) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut whole = |said: String| problems.push(Problem { step: None, said });
    if template.schema_version == 0 || template.schema_version > SCHEMA_VERSION {
        whole(format!(
            "it is written for template schema {}, and this build reads schema {SCHEMA_VERSION}",
            template.schema_version
        ));
    }
    if template.name.trim().is_empty() {
        whole("it has no name".to_string());
    }
    if crate::unattended::parse_every(&template.timeout).is_none() {
        whole(format!(
            "the timeout `{}` is not a duration such as 45m, 2h or 90s",
            template.timeout
        ));
    }
    if template.steps.is_empty() {
        whole("it has no steps".to_string());
    }

    let steps = &template.steps;
    for (at, step) in steps.iter().enumerate() {
        let mut here = |said: String| {
            problems.push(Problem {
                step: Some(at),
                said,
            })
        };
        let earlier = &steps[..at];
        if !is_id(&step.id) {
            here(format!(
                "its id `{}` must be lowercase letters, digits, `-` and `_`",
                step.id
            ));
        } else if earlier.iter().any(|other| other.id == step.id) {
            here(format!("its id `{}` is used by an earlier step", step.id));
        }
        if step.label.trim().is_empty() {
            here("it has no label".to_string());
        }
        // An earlier agent step by id, and whether it keeps its answer.
        let agent = |id: &str| {
            earlier
                .iter()
                .find(|other| other.id == id)
                .map(|other| match &other.kind {
                    StepKind::Agent { keep_answer, .. } => Some(*keep_answer),
                    StepKind::Command { .. } | StepKind::Approval { .. } => None,
                })
        };
        match &step.kind {
            StepKind::Agent { prompt, gates, .. } => {
                if prompt.trim().is_empty() {
                    here("its prompt is empty".to_string());
                }
                for name in prompt::refs(prompt) {
                    if prompt::NAMES.contains(&name) {
                        continue;
                    }
                    let Some(id) = name.strip_prefix("output.") else {
                        here(format!(
                            "its prompt names `{{{name}}}`, which is no variable"
                        ));
                        continue;
                    };
                    match agent(id) {
                        Some(Some(true)) => {}
                        Some(Some(false)) => here(format!(
                            "its prompt names `{{{name}}}`, but that step keeps no answer"
                        )),
                        Some(None) => here(format!(
                            "its prompt names `{{{name}}}`, which is not an agent step"
                        )),
                        None => here(format!(
                            "its prompt names `{{{name}}}`, which is no earlier step"
                        )),
                    }
                }
                for gate in gates {
                    if !gate.fits(template.place) {
                        here(format!(
                            "the gate {} cannot hold {}",
                            gate.label(),
                            match template.place {
                                super::Place::Checkout => "in a checkout, which is never committed",
                                super::Place::Worktree => "on a worktree, which is committed",
                            }
                        ));
                    }
                }
                if gates.contains(&GateKind::CodeChanged)
                    && gates.contains(&GateKind::CodeUnchanged)
                {
                    here("its gates ask for the code both changed and unchanged".to_string());
                }
            }
            StepKind::Command { command, on_fail } => {
                if command.as_ref().is_some_and(|c| c.trim().is_empty()) {
                    here(
                        "its command is blank; leave it out to run the project's check".to_string(),
                    );
                }
                if agent(on_fail).flatten().is_none() {
                    here(format!(
                        "on failure it goes back to `{on_fail}`, which is no earlier agent step"
                    ));
                }
            }
            StepKind::Approval { of } => match agent(of) {
                Some(Some(true)) => {}
                Some(Some(false)) => here(format!("it approves `{of}`, which keeps no answer")),
                _ => here(format!(
                    "it approves `{of}`, which is no earlier agent step"
                )),
            },
        }
    }
    problems
}

/// A step id: what `{output.<id>}` and the steps that name another can say.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}
