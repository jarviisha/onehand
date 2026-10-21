use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::input::{InputState, TextareaState};
use onehand_core::workflow::{self as core, GateKind, Place, StepKind, StepSpec, Template};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The lines a step's prompt shows when it is short.
const PROMPT_MIN_ROWS: usize = 6;

/// The lines a step's prompt grows to before it scrolls.
const PROMPT_MAX_ROWS: usize = 24;

/// The template form's fields.
///
/// `file` is the template being changed, or `None` for a new one or a
/// duplicate; `original` is what it was when the form opened or was last
/// saved, which is what "not saved" is measured against.
pub struct WorkflowDraft {
    pub file: Option<PathBuf>,
    pub original: Template,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
    pub misses: Entity<InputState>,
    pub timeout: Entity<InputState>,
    pub place: Place,
    pub steps: Vec<StepDraft>,
    /// The one step open under its row, if any.
    pub open_step: Option<usize>,
    /// One onehand ships, shown read-only: it is changed by duplicating it.
    pub shipped: bool,
}

/// A step's kind as the form holds it: the kind alone, its fields kept apart
/// on [`StepDraft`] so switching kinds and back loses nothing typed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Agent,
    Command,
    Approval,
    Push,
    PullRequest,
    StatusChecks,
}

impl Kind {
    pub(super) const ALL: [Self; 6] = [
        Self::Agent,
        Self::Command,
        Self::Approval,
        Self::Push,
        Self::PullRequest,
        Self::StatusChecks,
    ];

    /// What a person calls it.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Agent => "Agent",
            Self::Command => "Command",
            Self::Approval => "Approval",
            Self::Push => "Push",
            Self::PullRequest => "Pull request",
            Self::StatusChecks => "Status checks",
        }
    }
}

/// One step's fields. The prompt, gates, command and the step another kind
/// points at are all kept whatever the kind, so switching kinds and back
/// loses nothing typed.
pub struct StepDraft {
    pub id: Entity<InputState>,
    pub label: Entity<InputState>,
    pub kind: Kind,
    pub prompt: Entity<TextareaState>,
    pub gates: Vec<GateKind>,
    pub keep_answer: bool,
    /// The step's mode and options, kept as the spec reads them: no field
    /// on this form edits them.
    pub mode: Option<String>,
    pub options: BTreeMap<String, String>,
    pub command: Entity<InputState>,
    /// How long status checks are waited on.
    pub wait: Entity<InputState>,
    /// The step a command or status checks go back to, or an approval
    /// approves, by id.
    pub target: String,
}

impl StepDraft {
    pub(super) fn new(spec: &StepSpec, window: &mut Window, cx: &mut App) -> Self {
        let input = |text: &str, window: &mut Window, cx: &mut App| {
            let text = text.to_string();
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        let mut wait = core::DEFAULT_WAIT;
        let (kind, prompt, gates, keep_answer, mode, options, command, target) = match &spec.kind {
            StepKind::Agent {
                prompt,
                gates,
                keep_answer,
                mode,
                options,
            } => (
                Kind::Agent,
                prompt.as_str(),
                gates.clone(),
                *keep_answer,
                mode.clone(),
                options.clone(),
                "",
                "",
            ),
            StepKind::Command { command, on_fail } => (
                Kind::Command,
                "",
                Vec::new(),
                false,
                None,
                BTreeMap::new(),
                command.as_deref().unwrap_or_default(),
                on_fail.as_str(),
            ),
            StepKind::Approval { of } => (
                Kind::Approval,
                "",
                Vec::new(),
                false,
                None,
                BTreeMap::new(),
                "",
                of.as_str(),
            ),
            StepKind::Push => (
                Kind::Push,
                "",
                Vec::new(),
                false,
                None,
                BTreeMap::new(),
                "",
                "",
            ),
            StepKind::PullRequest => (
                Kind::PullRequest,
                "",
                Vec::new(),
                false,
                None,
                BTreeMap::new(),
                "",
                "",
            ),
            StepKind::StatusChecks {
                on_fail,
                wait: waits,
            } => {
                wait = waits.as_str();
                (
                    Kind::StatusChecks,
                    "",
                    Vec::new(),
                    false,
                    None,
                    BTreeMap::new(),
                    "",
                    on_fail.as_str(),
                )
            }
        };
        let prompt = prompt.to_string();
        Self {
            id: input(&spec.id, window, cx),
            label: input(&spec.label, window, cx),
            kind,
            // A prompt is the step's whole instruction: it grows with what is
            // written, from a paragraph to most of a screen, and scrolls past
            // that. Only one step is open at a time, so its height pushes
            // nothing a person is reading.
            prompt: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(PROMPT_MIN_ROWS, PROMPT_MAX_ROWS)
                    .default_value(prompt)
            }),
            gates,
            keep_answer,
            mode,
            options,
            command: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Empty: the project's check command")
                    .default_value(command.to_string())
            }),
            wait: input(wait, window, cx),
            target: target.to_string(),
        }
    }

    pub(super) fn to_spec(&self, cx: &App) -> StepSpec {
        let command = self.command.read(cx).value().trim().to_string();
        StepSpec {
            id: self.id.read(cx).value().trim().to_string(),
            label: self.label.read(cx).value().trim().to_string(),
            kind: match self.kind {
                Kind::Agent => StepKind::Agent {
                    prompt: self.prompt.read(cx).value().to_string(),
                    gates: self.gates.clone(),
                    keep_answer: self.keep_answer,
                    mode: self.mode.clone(),
                    options: self.options.clone(),
                },
                Kind::Command => StepKind::Command {
                    command: (!command.is_empty()).then_some(command),
                    on_fail: self.target.clone(),
                },
                Kind::Approval => StepKind::Approval {
                    of: self.target.clone(),
                },
                Kind::Push => StepKind::Push,
                Kind::PullRequest => StepKind::PullRequest,
                Kind::StatusChecks => StepKind::StatusChecks {
                    on_fail: self.target.clone(),
                    wait: self.wait.read(cx).value().trim().to_string(),
                },
            },
        }
    }
}

impl WorkflowDraft {
    /// A form on `template`, kept in `file`.
    pub fn load(
        template: &Template,
        file: Option<PathBuf>,
        shipped: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let input = |text: String, window: &mut Window, cx: &mut App| {
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        Self {
            file,
            original: template.clone(),
            name: input(template.name.clone(), window, cx),
            description: input(template.description.clone(), window, cx),
            misses: input(template.misses.to_string(), window, cx),
            timeout: input(template.timeout.clone(), window, cx),
            place: template.place,
            steps: template
                .steps
                .iter()
                .map(|step| StepDraft::new(step, window, cx))
                .collect(),
            open_step: None,
            shipped,
        }
    }

    /// The template this form describes, or why the allowance of misses does
    /// not read as a number.
    pub fn to_template(&self, cx: &App) -> Result<Template, String> {
        let misses = self.misses.read(cx).value().trim().to_string();
        let misses = misses
            .parse()
            .map_err(|_| format!("the misses allowed, `{misses}`, is not a whole number"))?;
        Ok(Template {
            schema_version: core::SCHEMA_VERSION,
            id: self.original.id.clone(),
            version: self.original.version,
            name: self.name.read(cx).value().trim().to_string(),
            description: self.description.read(cx).value().trim().to_string(),
            place: self.place,
            misses,
            timeout: self.timeout.read(cx).value().trim().to_string(),
            steps: self.steps.iter().map(|step| step.to_spec(cx)).collect(),
        })
    }

    /// Everything that keeps the form from being saved, each with the step
    /// it is about, if one.
    pub fn problem_list(&self, cx: &App) -> Vec<(Option<usize>, String)> {
        match self.to_template(cx) {
            Ok(template) => core::validate(&template)
                .into_iter()
                .map(|problem| (problem.step, problem.said))
                .collect(),
            Err(why) => vec![(None, why)],
        }
    }

    /// Everything that keeps the form from being saved, as sentences.
    pub fn problems(&self, cx: &App) -> Vec<String> {
        self.problem_list(cx)
            .into_iter()
            .map(|(step, said)| match step {
                Some(at) => format!("Step {}: {said}", at + 1),
                // Said as a sentence on its own, so it starts as one.
                None => sentence(&said),
            })
            .collect()
    }

    /// Whether the form holds something not yet saved.
    pub fn dirty(&self, cx: &App) -> bool {
        self.to_template(cx).ok().as_ref() != Some(&self.original)
    }

    /// Add a step of `kind` at the end, with an id no other step has.
    pub fn add_step(&mut self, kind: Kind, window: &mut Window, cx: &mut App) {
        let taken: Vec<String> = self
            .steps
            .iter()
            .map(|step| step.id.read(cx).value().to_string())
            .collect();
        let id = (1..)
            .map(|n| format!("step-{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or_default();
        let spec = StepSpec {
            id,
            label: kind.label().to_string(),
            kind: match kind {
                Kind::Agent => StepKind::Agent {
                    prompt: String::new(),
                    gates: Vec::new(),
                    keep_answer: false,
                    mode: None,
                    options: BTreeMap::new(),
                },
                Kind::Command => StepKind::Command {
                    command: None,
                    on_fail: String::new(),
                },
                Kind::Approval => StepKind::Approval { of: String::new() },
                Kind::Push => StepKind::Push,
                Kind::PullRequest => StepKind::PullRequest,
                Kind::StatusChecks => StepKind::StatusChecks {
                    on_fail: String::new(),
                    wait: core::DEFAULT_WAIT.to_string(),
                },
            },
        };
        self.steps.push(StepDraft::new(&spec, window, cx));
        self.open_step = follow_open(self.open_step, StepEdit::Added, self.steps.len());
    }

    /// Remove the step at `at`.
    pub fn remove_step(&mut self, at: usize) {
        if at < self.steps.len() {
            self.steps.remove(at);
            self.open_step = follow_open(self.open_step, StepEdit::Removed(at), self.steps.len());
        }
    }

    /// Open the step at `at` under its row, closing the one open; or close it
    /// when it is the one open.
    pub fn toggle_step(&mut self, at: usize) {
        self.open_step = (self.open_step != Some(at)).then_some(at);
    }

    /// Move the step at `at` one place up, or down.
    pub fn move_step(&mut self, at: usize, up: bool) {
        let to = match up {
            true => at.checked_sub(1),
            false => Some(at + 1).filter(|to| *to < self.steps.len()),
        };
        if let Some(to) = to {
            self.steps.swap(at, to);
            self.open_step = follow_open(
                self.open_step,
                StepEdit::Moved { from: at, to },
                self.steps.len(),
            );
        }
    }
}

/// `said` with its first letter raised, as a sentence standing alone starts.
fn sentence(said: &str) -> String {
    let mut chars = said.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What a step's row says of it beside its name, in muted words: where a
/// failure sends the run back to, by the number of an earlier step, and that
/// an agent step keeps its answer.
pub(super) fn step_facts(at: usize, steps: &[StepSpec]) -> Vec<String> {
    let step = &steps[at];
    let earlier = |id: &str| steps[..at].iter().position(|s| s.id == id);
    let mut facts = Vec::new();
    match &step.kind {
        StepKind::Command { on_fail, .. } | StepKind::StatusChecks { on_fail, .. } => {
            if let Some(k) = earlier(on_fail) {
                facts.push(format!("on failure ↑ {}", k + 1));
            }
        }
        StepKind::Agent { keep_answer, .. } => {
            if *keep_answer {
                facts.push("keeps answer".to_string());
            }
        }
        StepKind::Approval { .. } | StepKind::Push | StepKind::PullRequest => {}
    }
    facts
}

/// Per step, the first problem said of it, so its row shows it shut.
pub(super) fn split_problems(
    problems: &[(Option<usize>, String)],
    steps: usize,
) -> Vec<Option<String>> {
    let mut first = vec![None; steps];
    for (step, said) in problems {
        if let Some(slot) = step.and_then(|at| first.get_mut(at)) {
            slot.get_or_insert_with(|| said.clone());
        }
    }
    first
}

/// A change to the steps' order or number, which the open one follows.
#[derive(Clone, Copy)]
pub(super) enum StepEdit {
    Added,
    Moved { from: usize, to: usize },
    Removed(usize),
}

/// Which step is open after `edit`, `len_after` steps then standing: a step
/// just added, the open one where it moved to, none when it was removed.
pub(super) fn follow_open(open: Option<usize>, edit: StepEdit, len_after: usize) -> Option<usize> {
    match edit {
        StepEdit::Added => len_after.checked_sub(1),
        StepEdit::Moved { from, to } => open.map(|at| match at {
            at if at == from => to,
            at if at == to => from,
            at => at,
        }),
        StepEdit::Removed(gone) => open.and_then(|at| match at.cmp(&gone) {
            std::cmp::Ordering::Less => Some(at),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(at - 1),
        }),
    }
}

#[cfg(test)]
mod tests;
