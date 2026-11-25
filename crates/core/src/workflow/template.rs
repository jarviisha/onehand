//! A workflow: the steps a run takes, what each one asks
//! and what onehand checks before it moves on. Kept as TOML, one file each.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// The newest template layout this build reads and writes. A file carrying a
/// higher number was written by a newer onehand, and is refused rather than
/// read with whatever this build happens to understand of it.
pub(crate) const SCHEMA_VERSION: u32 = 2;

/// The id of the one-step workflow a schedule's prompt runs as. No library
/// file takes it: a person's takes its file name, a shipped one `builtin:`.
pub(crate) const PROMPT_ID: &str = "schedule:prompt";

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
    /// The schema a file of this template must carry: 2 once a step names
    /// its agent, else 1, so a file nothing new touches stays readable by an
    /// older onehand.
    pub fn schema_version_needed(&self) -> u32 {
        if self.steps.iter().any(|step| step.agent().is_some()) {
            2
        } else {
            1
        }
    }

    /// An empty template with the defaults filled in, to start a new one from.
    pub fn blank(name: &str) -> Self {
        Self {
            schema_version: 1,
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

    /// Whether it is the one-step workflow a schedule's prompt runs as: a
    /// prompt to an agent rather than a workflow, so its run is not called a
    /// workflow, and its one step is not named apart from it.
    pub fn is_prompt(&self) -> bool {
        self.id == PROMPT_ID
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

    /// Why `self` cannot run in a project with no check command, when
    /// `has_check` says it has none and a step needs one. Where to set one is
    /// the caller's to say: a run that keeps its own setup is not fixed there.
    pub fn lacks_check(&self, has_check: bool) -> Option<String> {
        (self.needs_check() && !has_check).then(|| {
            format!(
                "the workflow `{}` runs the project's check command, and the project has none",
                self.name
            )
        })
    }

    /// How long a run of `self` may work. A timeout that does not read, which
    /// validation refuses, so only a file edited by hand carries one, gives
    /// what a new workflow is given.
    pub fn timeout_limit(&self) -> std::time::Duration {
        parse_every(&self.timeout)
            .or_else(|| parse_every(&default_timeout()))
            .expect("the default timeout reads")
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
    /// The agent the step names, on an agent step that does.
    pub fn agent(&self) -> Option<&str> {
        match &self.kind {
            StepKind::Agent { agent, .. } => agent.as_deref(),
            StepKind::Command { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest
            | StepKind::StatusChecks { .. } => None,
        }
    }

    /// What the step does, in a line: its kind, then what it checks or runs
    /// and where it leads.
    pub fn summary(&self) -> String {
        match &self.kind {
            StepKind::Agent {
                gates,
                keep_answer,
                agent,
                ..
            } => {
                let mut parts = vec!["Agent".to_string()];
                if let Some(agent) = agent {
                    parts[0] = format!("Agent on {agent}");
                }
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
        /// The adapter's mode id to set before the step's first prompt, or
        /// `None` to leave the run's starting mode as it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Config option id to the choice value to set before the step's
        /// first prompt, exactly as the agent sends them.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
        /// The agent of the agent list this step runs on, or `None` for the
        /// run's own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
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
            Self::Agent { gates, agent, .. } => {
                let changes = gates.iter().any(|gate| match gate {
                    GateKind::CodeChanged | GateKind::Committed => true,
                    GateKind::Answered | GateKind::CodeUnchanged | GateKind::Uncommitted => false,
                });
                let answers = gates.contains(&GateKind::Answered);
                let who = match agent {
                    Some(name) => name.as_str(),
                    None => "the agent",
                };
                match (changes, answers) {
                    (true, _) => format!("{who} changes the code"),
                    (false, true) => format!("{who} answers"),
                    (false, false) => format!("{who} works"),
                }
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

/// `"30m"`, `"2h"`, `"90s"` as a duration.
///
/// One number and one unit, nothing else. A zero is refused rather than read
/// as "continuously": an interval of nothing is a tick loop, and a timeout of
/// nothing cancels every run before its prompt goes in.
pub fn parse_every(text: &str) -> Option<Duration> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let count: u64 = text[..text.len() - unit.len_utf8()].parse().ok()?;
    let secs = match unit {
        's' => count,
        'm' => count.checked_mul(60)?,
        'h' => count.checked_mul(3600)?,
        _ => return None,
    };
    (secs > 0).then(|| Duration::from_secs(secs))
}

#[cfg(test)]
mod every_tests {
    use super::*;

    #[test]
    fn an_interval_is_a_number_and_a_unit() {
        assert_eq!(parse_every("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_every("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_every(" 90s "), Some(Duration::from_secs(90)));
        assert_eq!(parse_every("soon"), None);
        assert_eq!(parse_every("0m"), None);
        assert_eq!(parse_every("m"), None);
        assert_eq!(parse_every(""), None);
    }
}
