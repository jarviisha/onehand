use gpui::{App, AppContext as _, Entity, Window};
use gpui_component::input::{InputState, TextareaState};
use onehand_core::workflow::{self as core, GateKind, Place, StepKind, StepSpec, Template};
use std::path::PathBuf;

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
        let (kind, prompt, gates, keep_answer, command, target) = match &spec.kind {
            StepKind::Agent {
                prompt,
                gates,
                keep_answer,
            } => (
                Kind::Agent,
                prompt.as_str(),
                gates.clone(),
                *keep_answer,
                "",
                "",
            ),
            StepKind::Command { command, on_fail } => (
                Kind::Command,
                "",
                Vec::new(),
                false,
                command.as_deref().unwrap_or_default(),
                on_fail.as_str(),
            ),
            StepKind::Approval { of } => (Kind::Approval, "", Vec::new(), false, "", of.as_str()),
            StepKind::Push => (Kind::Push, "", Vec::new(), false, "", ""),
            StepKind::PullRequest => (Kind::PullRequest, "", Vec::new(), false, "", ""),
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
            prompt: cx.new(|cx| TextareaState::new(window, cx).default_value(prompt)),
            gates,
            keep_answer,
            command: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Empty: the project's check command")
                    .default_value(command.to_string())
            }),
            wait: input(wait, window, cx),
            target: target.to_string(),
        }
    }

    fn to_spec(&self, cx: &App) -> StepSpec {
        let command = self.command.read(cx).value().trim().to_string();
        StepSpec {
            id: self.id.read(cx).value().trim().to_string(),
            label: self.label.read(cx).value().trim().to_string(),
            kind: match self.kind {
                Kind::Agent => StepKind::Agent {
                    prompt: self.prompt.read(cx).value().to_string(),
                    gates: self.gates.clone(),
                    keep_answer: self.keep_answer,
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

    /// Everything that keeps the form from being saved, as sentences.
    pub fn problems(&self, cx: &App) -> Vec<String> {
        match self.to_template(cx) {
            Ok(template) => core::validate(&template)
                .iter()
                .map(ToString::to_string)
                .collect(),
            Err(why) => vec![why],
        }
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
    }

    /// Move the step at `at` one place up, or down.
    pub fn move_step(&mut self, at: usize, up: bool) {
        let to = match up {
            true => at.checked_sub(1),
            false => Some(at + 1).filter(|to| *to < self.steps.len()),
        };
        if let Some(to) = to {
            self.steps.swap(at, to);
        }
    }
}
