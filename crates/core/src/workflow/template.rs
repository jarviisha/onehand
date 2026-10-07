//! A workflow: the steps a run takes, what each one asks
//! and what onehand checks before it moves on. Kept as TOML, one file each.

use serde::{Deserialize, Serialize};

/// The newest template layout this build reads and writes. A file carrying a
/// higher number was written by a newer onehand, and is refused rather than
/// read with whatever this build happens to understand of it.
pub const SCHEMA_VERSION: u32 = 1;

/// The steps of a workflow, in the order a run takes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub schema_version: u32,
    /// What names this template whatever it is called: a person's own file
    /// takes its file name when first saved, a shipped one `builtin:<name>`.
    /// Empty in a run kept by a build from before ids.
    #[serde(default)]
    pub id: String,
    /// How many times its file has been saved with something changed,
    /// counting from 1.
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub place: Place,
    /// How many turns may fail their gates in a stretch of steps before the
    /// run stops as exhausted.
    #[serde(default = "default_misses")]
    pub misses: u32,
    /// How long a run may spend working, waiting on a person not counted, in
    /// the form `"45m"`.
    #[serde(default = "default_timeout")]
    pub timeout: String,
    pub steps: Vec<StepSpec>,
}

fn default_version() -> u32 {
    1
}

fn default_misses() -> u32 {
    3
}

fn default_timeout() -> String {
    "45m".to_string()
}

impl Template {
    /// An empty template with the defaults filled in, to start a new one from.
    pub fn blank(name: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: String::new(),
            version: default_version(),
            name: name.to_string(),
            description: String::new(),
            place: Place::Checkout,
            misses: default_misses(),
            timeout: default_timeout(),
            steps: Vec::new(),
        }
    }

    /// Whether a run needs the project's check command: a command step names
    /// no command of its own.
    pub fn needs_check(&self) -> bool {
        self.steps.iter().any(|step| match &step.kind {
            StepKind::Command { command, .. } => command.is_none(),
            StepKind::Agent { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest
            | StepKind::StatusChecks { .. } => false,
        })
    }

    /// Whether `self` and `other` say the same, whatever their id and version.
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        let bare = |t: &Self| Self {
            id: String::new(),
            version: 0,
            ..t.clone()
        };
        bare(self) == bare(other)
    }

    /// Whether `self` is the workflow a run kept as `snapshot`, at whatever
    /// version: the same id, so a rename still finds it. A snapshot from
    /// before ids has none, and then a workflow of the same name is it.
    pub fn same_workflow(&self, snapshot: &Self) -> bool {
        match snapshot.id.is_empty() {
            true => self.name == snapshot.name,
            false => self.id == snapshot.id,
        }
    }

    /// The step that repairs what the forge's status checks found, and so
    /// answers a review of the pull request too, by id.
    pub fn repair_step(&self) -> Option<&str> {
        self.steps.iter().find_map(|step| match &step.kind {
            StepKind::StatusChecks { on_fail, .. } => Some(on_fail.as_str()),
            StepKind::Agent { .. }
            | StepKind::Command { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest => None,
        })
    }

    /// Where the step `id` is, if the template has one.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.steps.iter().position(|step| step.id == id)
    }
}

/// Where a run's steps work: in the checkout the project is open on, with
/// the work left uncommitted for the person who has it open, or on a branch
/// of its own in a new worktree, committed there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Checkout,
    Worktree,
}

impl Place {
    pub const ALL: [Self; 2] = [Self::Checkout, Self::Worktree];

    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Checkout => "In the checkout",
            Self::Worktree => "On a new worktree",
        }
    }
}

/// One step: an id other steps and prompts name it by, what a person reads,
/// and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepSpec {
    pub id: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: StepKind,
}

impl StepSpec {
    /// What the step does, in a line: its kind, then what it checks or runs
    /// and where it leads.
    pub fn summary(&self) -> String {
        match &self.kind {
            StepKind::Agent {
                gates, keep_answer, ..
            } => {
                let mut parts = vec!["Agent".to_string()];
                if !gates.is_empty() {
                    let gates: Vec<&str> = gates.iter().map(|gate| gate.label()).collect();
                    parts.push(gates.join(", "));
                }
                if *keep_answer {
                    parts.push("keeps its answer".to_string());
                }
                parts.join(" · ")
            }
            StepKind::Command { command, on_fail } => format!(
                "Command · {} · back to {on_fail} on failure",
                command
                    .as_deref()
                    .map_or_else(|| "the project's check".to_string(), |c| format!("`{c}`"))
            ),
            StepKind::Approval { of } => format!("Approval of {of}"),
            StepKind::Push => "Push · the commit the check passed on".to_string(),
            StepKind::PullRequest => "Pull request · opened as a draft".to_string(),
            StepKind::StatusChecks { on_fail, wait } => {
                format!("Status checks · at most {wait} · back to {on_fail} on failure")
            }
        }
    }
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Prompt the agent, then check the turn against `gates`. `keep_answer`
    /// keeps what the turn answered, for later prompts and an approval.
    Agent {
        prompt: String,
        #[serde(default)]
        gates: Vec<GateKind>,
        #[serde(default)]
        keep_answer: bool,
    },
    /// Run a command in the work, onehand itself; `None` runs the project's
    /// check command. A failure goes back to the step `on_fail`, carrying
    /// what the command printed.
    Command {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        on_fail: String,
    },
    /// Wait for a person to approve the answer the step `of` kept, or send
    /// it back with a note.
    Approval { of: String },
    /// Put the commit the last command passed on onto the forge, as the
    /// run's branch. onehand pushes, never the agent, so what lands is what
    /// was checked.
    Push,
    /// Open a draft pull request from the run's branch, or take the one
    /// already open there.
    PullRequest,
    /// Wait for the forge's status checks on the pull request's head, at most
    /// `wait`. All passing takes it out of draft; a failure or a conflict goes
    /// back to the step `on_fail`, carrying what failed.
    StatusChecks {
        on_fail: String,
        #[serde(default = "default_wait")]
        wait: String,
    },
}

/// How long status checks are waited on when a step does not say.
pub const DEFAULT_WAIT: &str = "1h";

fn default_wait() -> String {
    DEFAULT_WAIT.to_string()
}

impl StepKind {
    /// The step what this one checks goes back to when it fails, by id.
    pub(crate) fn sends_back_to(&self) -> Option<&str> {
        match self {
            Self::Command { on_fail, .. } | Self::StatusChecks { on_fail, .. } => Some(on_fail),
            Self::Agent { .. } | Self::Approval { .. } | Self::Push | Self::PullRequest => None,
        }
    }

    /// What the step does, as a person deciding whether to start it reads
    /// it: *the agent changes the code*, *onehand runs the check command*.
    pub(crate) fn does(&self) -> String {
        match self {
            Self::Agent { gates, .. } => {
                let changes = gates.iter().any(|gate| match gate {
                    GateKind::CodeChanged | GateKind::Committed => true,
                    GateKind::Answered | GateKind::CodeUnchanged | GateKind::Uncommitted => false,
                });
                let answers = gates.contains(&GateKind::Answered);
                match (changes, answers) {
                    (true, _) => "the agent changes the code",
                    (false, true) => "the agent answers",
                    (false, false) => "the agent works",
                }
                .to_string()
            }
            Self::Command { command: None, .. } => "onehand runs the check command".to_string(),
            Self::Command {
                command: Some(command),
                ..
            } => format!("onehand runs {command}"),
            Self::Approval { .. } => "a person is asked to approve again".to_string(),
            Self::Push => "onehand pushes the commit the check passed on".to_string(),
            Self::PullRequest => "onehand opens a draft pull request".to_string(),
            Self::StatusChecks { .. } => "onehand waits for the forge's status checks".to_string(),
        }
    }

    /// Whether onehand does the step on the forge itself.
    pub(crate) fn on_forge(&self) -> bool {
        match self {
            Self::Push | Self::PullRequest | Self::StatusChecks { .. } => true,
            Self::Agent { .. } | Self::Command { .. } | Self::Approval { .. } => false,
        }
    }
}

/// A condition onehand checks itself after an agent turn, against the work
/// as git reads it and the answer as the transcript holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// The turn ended with an answer.
    Answered,
    /// The turn changed no code and committed nothing.
    CodeUnchanged,
    /// The turn changed the code.
    CodeChanged,
    /// Every change is committed, and there is at least one new commit.
    Committed,
    /// No commit landed: the work stays uncommitted.
    Uncommitted,
}

impl GateKind {
    pub const ALL: [Self; 5] = [
        Self::Answered,
        Self::CodeUnchanged,
        Self::CodeChanged,
        Self::Committed,
        Self::Uncommitted,
    ];

    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Answered => "Answered",
            Self::CodeUnchanged => "Code unchanged",
            Self::CodeChanged => "Code changed",
            Self::Committed => "Committed",
            Self::Uncommitted => "Uncommitted",
        }
    }

    /// Whether the gate can hold where the work is: a checkout's work is
    /// never committed, a worktree's always is.
    pub fn fits(self, place: Place) -> bool {
        match (self, place) {
            (Self::Committed, Place::Checkout) | (Self::Uncommitted, Place::Worktree) => false,
            (Self::Answered | Self::CodeUnchanged | Self::CodeChanged, _)
            | (Self::Committed, Place::Worktree)
            | (Self::Uncommitted, Place::Checkout) => true,
        }
    }
}
