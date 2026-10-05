//! What keeps a template from being saved or run. Checked once, when it is
//! saved and again before a run starts, so a run never meets a step that
//! names nothing or a prompt variable with no value.

use super::prompt;
use super::template::{GateKind, StepKind, StepSpec, Template, SCHEMA_VERSION};

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
    let commits = template.steps.iter().any(|step| match &step.kind {
        StepKind::Agent { gates, .. } => gates.contains(&GateKind::Committed),
        StepKind::Command { .. }
        | StepKind::Approval { .. }
        | StepKind::Push
        | StepKind::PullRequest
        | StepKind::StatusChecks { .. } => false,
    });
    if template.place == super::Place::Worktree && !template.steps.is_empty() && !commits {
        whole(
            "it works on a worktree, and no agent step has the gate Committed, so the branch \
             could end with nothing on it"
                .to_string(),
        );
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
        let label = step.label.trim();
        if label.is_empty() {
            here("it has no label".to_string());
        } else if earlier.iter().any(|other| other.label.trim() == label) {
            here(format!("its label `{label}` is used by an earlier step"));
        }
        let later = &steps[at + 1..];
        // An earlier agent step by id, and whether it keeps its answer.
        let agent = |id: &str| {
            earlier
                .iter()
                .find(|other| other.id == id)
                .map(|other| match &other.kind {
                    StepKind::Agent { keep_answer, .. } => Some(*keep_answer),
                    StepKind::Command { .. }
                    | StepKind::Approval { .. }
                    | StepKind::Push
                    | StepKind::PullRequest
                    | StepKind::StatusChecks { .. } => None,
                })
        };
        // Whether an earlier step is of the kind `is`.
        let before = |is: fn(&StepKind) -> bool| earlier.iter().any(|other| is(&other.kind));
        if step.kind.on_forge() && template.place != super::Place::Worktree {
            here(
                "it works on the forge, which only a workflow on a worktree has a branch for"
                    .to_string(),
            );
        }
        match &step.kind {
            StepKind::Agent { prompt, gates, .. } => {
                if prompt.trim().is_empty() {
                    here("its prompt is empty".to_string());
                }
                for name in prompt::refs(prompt) {
                    // A variable only a later step sending this one back
                    // fills in is always empty without one.
                    let unfed = SENT_BACK.iter().find(|(var, _)| *var == name).filter(|_| {
                        !later
                            .iter()
                            .any(|other| fills_on_send_back(other, &step.id) == Some(name))
                    });
                    if let Some((_, sender)) = unfed {
                        here(format!(
                            "its prompt names `{{{name}}}`, but no later {sender} sends it back \
                             here, so it is always empty"
                        ));
                    }
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
            StepKind::Push => {
                if !before(|kind| matches!(kind, StepKind::Command { .. })) {
                    here(
                        "it pushes the commit a command passed on, and no earlier step is a \
                         command"
                            .to_string(),
                    );
                }
            }
            StepKind::PullRequest => {
                if !before(|kind| *kind == StepKind::Push) {
                    here("it opens a pull request, and no earlier step pushes".to_string());
                }
            }
            StepKind::StatusChecks { on_fail, wait } => {
                if !before(|kind| *kind == StepKind::PullRequest) {
                    here(
                        "it waits on a pull request's checks, and no earlier step opens one"
                            .to_string(),
                    );
                }
                if agent(on_fail).flatten().is_none() {
                    here(format!(
                        "on failure it goes back to `{on_fail}`, which is no earlier agent step"
                    ));
                }
                if crate::unattended::parse_every(wait).is_none() {
                    here(format!(
                        "its wait `{wait}` is not a duration such as 45m, 2h or 90s"
                    ));
                }
            }
        }
    }
    problems
}

/// The variables a step sending another back fills in, beside what that
/// step is called.
const SENT_BACK: [(&str, &str); 2] = [
    ("check_output", "command or status checks step"),
    ("revise", "approval"),
];

/// The variable `step` fills in when it sends the step `id` back, if it does.
fn fills_on_send_back(step: &StepSpec, id: &str) -> Option<&'static str> {
    match &step.kind {
        StepKind::Command { on_fail, .. } | StepKind::StatusChecks { on_fail, .. } => {
            (on_fail == id).then_some("check_output")
        }
        StepKind::Approval { of } => (of == id).then_some("revise"),
        StepKind::Agent { .. } | StepKind::Push | StepKind::PullRequest => None,
    }
}

/// A step id: what `{output.<id>}` and the steps that name another can say.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}
