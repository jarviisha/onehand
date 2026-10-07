//! What a start checks before it claims an issue, cuts a worktree or starts
//! an agent, and what it found.
//!
//! **It judges the configuration that will actually run.** A new start runs
//! what Settings says now; a Resume or a Retry runs the run's own setup, so a
//! setting changed since does not clear what blocks it. The app gathers the
//! facts (nothing here reads the disk or the network), and draws what comes
//! back where the start is asked for; the callers that claim and cut still do
//! that themselves, after this.

use crate::connector::PrState;
use crate::task::{Task, Working};
use crate::unattended::Slots;
use crate::workflow::{Failure, Place, StepKind, Template};

/// What kind of start is checked: each has its own configuration and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// From the launcher: the workflow picked, cut off `HEAD` when it works
    /// on a worktree.
    NewRun,
    /// An issue picked by hand or found by its label, on a new worktree.
    NewIssueRun,
    /// A run cut off, carried on where it stopped, on its own snapshot.
    Resume,
    /// A task's next run, on its last run's own snapshot and setup.
    Retry,
    /// A task's next run with what Settings say now: its own workflow at
    /// its newest version, judged as a new one is, on the configuration a
    /// new task of its kind would take.
    RetryCurrent,
    /// An issue task's next run answering the review on its open pull
    /// request, from its workflow's repair step, on its own snapshot and
    /// setup, in its own worktree brought up to the forge's branch.
    AnswerReview,
}

impl Kind {
    /// It runs the task's last run's own setup, so what Settings say now
    /// neither blocks it nor clears a block.
    fn keeps_own(self) -> bool {
        match self {
            Self::Resume | Self::Retry | Self::AnswerReview => true,
            Self::NewRun | Self::NewIssueRun | Self::RetryCurrent => false,
        }
    }

    /// Its workflow is judged anew, validation included: it is not one a
    /// run has already started on.
    fn judges_workflow(self) -> bool {
        match self {
            Self::NewRun | Self::NewIssueRun | Self::RetryCurrent => true,
            Self::Resume | Self::Retry | Self::AnswerReview => false,
        }
    }

    /// What a block of its own setup adds, said after the block.
    fn keeps(self) -> &'static str {
        match self {
            Self::Retry => {
                " The run keeps its own setup; Retry with current settings runs with what \
                 Settings say now."
            }
            Self::Resume | Self::AnswerReview => {
                " The run keeps its own setup, so changing Settings does not change it."
            }
            Self::NewRun | Self::NewIssueRun | Self::RetryCurrent => "",
        }
    }

    /// Where a block of its own setup is changed, when that is somewhere
    /// else: a Retry's by the retry that runs with what Settings say now.
    fn own_change(self) -> Option<Change> {
        match self {
            Self::Retry => Some(Change::RetryCurrent),
            Self::NewRun
            | Self::NewIssueRun
            | Self::Resume
            | Self::RetryCurrent
            | Self::AnswerReview => None,
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
pub enum Check {
    Workflow,
    Agent,
    Mode,
    CheckCommand,
    Place,
    Base,
    Forge,
    Issue,
    EarlierTask,
    Slot,
    PlaceTaken,
    Limits,
}

/// One thing the preflight found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    /// It stops the start; otherwise it is said and the start goes on.
    pub blocks: bool,
    pub text: String,
    /// Where it is changed, when that is somewhere else.
    pub change: Option<Change>,
    /// The task it offers instead, for an earlier task worth retrying.
    pub task: Option<String>,
}

/// The forge serving the project, and what its account said when last seen:
/// `None` until it has been asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forge {
    pub name: String,
    pub account: Option<Result<String, String>>,
}

/// The issue a start works, as a sentence names it, and its tasks with what
/// each is doing.
#[derive(Debug, Clone)]
pub struct IssueFacts {
    pub named: String,
    pub tasks: Vec<(Task, Option<Working>)>,
}

/// What answering a pull request review knows of it, read off the UI thread
/// before anything is claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFacts {
    /// The pull request on the task's branch as the forge says now, by its
    /// state and address; `None` when there is none, `Err` when it could not
    /// be read.
    pub pr: Result<Option<(PrState, String)>, String>,
    /// The workflow its task ran answers a review: its status checks send
    /// back to a step that repairs.
    pub answers: bool,
    /// The branch on the forge went its own way from the task's worktree, so
    /// bringing the worktree up to it is no fast-forward.
    pub diverged: bool,
    /// The issue is among the open ones a pick reads, which is what a review
    /// is answered through; `Err` when they could not be read.
    pub issue_open: Result<bool, String>,
}

/// The branch a detached `HEAD` reads as.
pub const DETACHED: &str = "(detached)";

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
    /// The modes the agent offers, when learned in this process from the
    /// spec it is configured with now; `None` when not known yet.
    pub offered: Option<Vec<String>>,
    /// The run has a check command to run.
    pub has_check: bool,
    /// The project is in git.
    pub in_git: bool,
    /// The branch checked out, [`DETACHED`] when there is none.
    pub checked_out: Option<String>,
    /// The forge the work goes to, `None` on a project no forge serves.
    pub forge: Option<Forge>,
    /// The issue it works, for an issue's start.
    pub issue: Option<IssueFacts>,
    /// The slots, for an issue's start.
    pub slots: Option<Slots>,
    /// The task whose place this start would queue behind.
    pub queued_behind: Option<String>,
    /// The pull request a review is answered on, for that start.
    pub review: Option<ReviewFacts>,
    /// A person's own session working in the checkout the start works in,
    /// by its name.
    pub shared_checkout: Option<String>,
}

/// Everything `facts` says about a start of `kind`: blocks first, the
/// workflow's own problems first among them, then what only informs.
pub fn preflight(kind: Kind, facts: &Facts) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut say = |check, blocks, text: String, change| {
        found.push(Finding {
            check,
            blocks,
            text,
            change,
            task: None,
        })
    };
    let (own, keeps, own_change) = (kind.keeps_own(), kind.keeps(), kind.own_change());
    let issue_kind = kind == Kind::NewIssueRun;

    // The workflow.
    let template = match &facts.workflow {
        Ok(template) => Some(template),
        Err(why) => {
            say(Check::Workflow, true, capital(why), None);
            None
        }
    };
    if let Some(template) = template.filter(|_| kind.judges_workflow()) {
        if let Some(why) = unfit_for_issue(template).filter(|_| issue_kind) {
            say(Check::Workflow, true, capital(&why), None);
        }
        for problem in crate::workflow::validate(template) {
            say(
                Check::Workflow,
                true,
                format!("The workflow cannot run: {problem}"),
                Some(Change::At(WORKFLOWS_PAGE)),
            );
        }
    }

    // The agent and its mode.
    match (&facts.agent, facts.agent_configured) {
        (None, _) => say(
            Check::Agent,
            true,
            "No agent is configured to run it.".to_string(),
            Some(Change::At("Settings ▸ Agents")),
        ),
        (Some(name), false) => say(
            Check::Agent,
            true,
            format!("The agent `{name}` is no longer configured.{keeps}"),
            own_change.or(Some(Change::At("Settings ▸ Agents"))),
        ),
        (Some(_), true) => {}
    }
    if let Some(mode) = facts.mode.as_deref().filter(|m| !m.trim().is_empty()) {
        let change = match own {
            true => own_change,
            false => Some(Change::At("unattended.mode, in the config file")),
        };
        match &facts.offered {
            Some(offered) => {
                if let Some(why) = mode_refused(mode, offered) {
                    say(
                        Check::Mode,
                        true,
                        format!("{}.{keeps}", capital(&why)),
                        change,
                    );
                }
            }
            None => say(
                Check::Mode,
                false,
                format!(
                    "Mode `{mode}` is not known yet: the agent has not come up since onehand \
                     started or since it was edited, and it is checked when it does."
                ),
                None,
            ),
        }
    }

    // The check command.
    if let Some(template) = template {
        let commands = template
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Command { .. }));
        if template.needs_check() && !facts.has_check {
            say(
                Check::CheckCommand,
                true,
                format!(
                    "The workflow `{}` runs the project's check command, and there is \
                     none.{keeps}",
                    template.name
                ),
                own_change.or(Some(Change::At(PROJECT_PAGE))),
            );
        } else if !commands {
            say(
                Check::CheckCommand,
                false,
                "The workflow runs no command: nothing verifies the work.".to_string(),
                None,
            );
        }
    }

    // Where it works, and what the branch is cut off.
    let detached = facts.checked_out.as_deref() == Some(DETACHED);
    let worktree = template.is_some_and(|t| t.place == Place::Worktree);
    match kind {
        Kind::NewRun if worktree && !facts.in_git => say(
            Check::Place,
            true,
            "The project is not in git, so no worktree can be cut for it.".to_string(),
            None,
        ),
        Kind::NewRun if worktree => say(
            Check::Base,
            false,
            "The branch is cut off `HEAD`.".to_string(),
            None,
        ),
        // Only a workflow that reads is known to work in the checkout.
        Kind::NewRun if template.is_some() => {
            if let Some(session) = &facts.shared_checkout {
                say(
                    Check::Place,
                    false,
                    format!(
                        "Your session “{session}” works in this checkout too: the run edits \
                         the same files, and neither sees the other's edits coming."
                    ),
                    None,
                );
            }
        }
        Kind::NewIssueRun => match (&facts.forge, detached, &facts.checked_out) {
            (Some(_), _, _) => say(
                Check::Base,
                false,
                "The branch is cut off the default branch on `origin`, fetched first.".to_string(),
                None,
            ),
            (None, true, _) => say(
                Check::Place,
                true,
                "HEAD is detached and no forge serves the project, so there is no branch to \
                 cut the worktree from."
                    .to_string(),
                None,
            ),
            (None, false, Some(branch)) => say(
                Check::Base,
                false,
                format!("The branch is cut off `{branch}`, the branch checked out."),
                None,
            ),
            (None, false, None) => {}
        },
        Kind::NewRun | Kind::Resume | Kind::Retry | Kind::RetryCurrent | Kind::AnswerReview => {}
    }

    // The forge: an issue's run asks it for the default branch whatever its
    // steps; any other run needs it only for forge steps.
    let forge_steps = template.is_some_and(|t| {
        t.steps.iter().any(|step| match step.kind {
            StepKind::Push | StepKind::PullRequest | StepKind::StatusChecks { .. } => true,
            StepKind::Agent { .. } | StepKind::Command { .. } | StepKind::Approval { .. } => false,
        })
    });
    let refused = match &facts.forge {
        Some(Forge {
            name,
            account: Some(Err(why)),
        }) if issue_kind || forge_steps => Some((name, why)),
        _ => None,
    };
    if let Some((name, why)) = refused {
        say(
            Check::Forge,
            true,
            format!("{name} cannot be used: {why}"),
            Some(Change::At(CONNECTIONS)),
        );
    }
    // Answering a review on no forge is refused below, and never told twice.
    if forge_steps && facts.forge.is_none() && kind != Kind::AnswerReview {
        say(
            Check::Forge,
            false,
            "No forge serves the project: the forge steps pass at once, and the branch is \
             the result."
                .to_string(),
            None,
        );
    }

    // The limits the run is held to, from the snapshot that will run.
    if let Some(template) = template {
        say(
            Check::Limits,
            false,
            format!(
                "Limits: {} of work, waiting on a person not counted; {} failed {} in a \
                 stretch of steps before it stops.",
                template.timeout,
                template.misses,
                if template.misses == 1 {
                    "turn"
                } else {
                    "turns"
                }
            ),
            None,
        );
    }

    // The pull request a review is answered on.
    if let Some(review) = facts.review.as_ref().filter(|_| kind == Kind::AnswerReview) {
        if let Some(why) = review_refused(review, facts.forge.is_some()) {
            say(Check::Issue, true, capital(&why), None);
        }
    }

    // The issue and its earlier tasks.
    if let Some(issue) = facts.issue.as_ref().filter(|_| issue_kind) {
        if issue.tasks.iter().any(|(_, working)| working.is_some()) {
            say(Check::Issue, true, already_working(&issue.named), None);
        } else if let Some((task, said)) = earlier_ended(&issue.tasks) {
            found.push(Finding {
                check: Check::EarlierTask,
                blocks: false,
                text: format!(
                    "The issue's last task ended: {said}. Starting makes a second task \
                     beside it; retry that one to carry its work on."
                ),
                change: None,
                task: Some(task),
            });
        }
    }

    // The slots and the place.
    let mut say = |check, blocks, text: String| {
        found.push(Finding {
            check,
            blocks,
            text,
            change: None,
            task: None,
        })
    };
    if let Some(why) = facts.slots.as_ref().and_then(Slots::full) {
        say(Check::Slot, true, why);
    }
    if let Some(ahead) = &facts.queued_behind {
        say(
            Check::PlaceTaken,
            false,
            format!("The place is taken: the run will queue behind {ahead}."),
        );
    }

    found.sort_by_key(|f| (!f.blocks, f.check));
    found
}

/// Why a review is not answered, in the words the label path uses where it
/// has them; `None` when it can be.
fn review_refused(review: &ReviewFacts, forge: bool) -> Option<String> {
    if !forge {
        return Some(
            "no forge serves the project, so there is no pull request review to answer".to_string(),
        );
    }
    let (state, url) = match &review.pr {
        Err(why) => return Some(format!("the pull request could not be read: {why}")),
        Ok(None) => return Some("there is no pull request on the task's branch".to_string()),
        Ok(Some(pr)) => pr,
    };
    match state {
        PrState::Closed => Some(crate::unattended::review_closed(url)),
        PrState::Merged => Some(format!(
            "its pull request {url} was merged: there is no review left to answer"
        )),
        PrState::Open if !review.answers => Some(crate::unattended::review_unanswerable(url)),
        PrState::Open if review.diverged => Some(crate::unattended::review_diverged()),
        PrState::Open => match &review.issue_open {
            Ok(true) => None,
            Ok(false) => {
                Some("the issue is closed, or not among the open issues a pick reads".to_string())
            }
            Err(why) => Some(format!("the issue could not be read: {why}")),
        },
    }
}

/// Why `template` cannot work an issue: it works in the checkout, and an
/// issue's run is cut a worktree of its own, where a checkout workflow leaves
/// its work uncommitted where nobody looks.
pub fn unfit_for_issue(template: &Template) -> Option<String> {
    (template.place != Place::Worktree).then(|| {
        format!(
            "the workflow `{}` works in the checkout, and an issue is worked on a worktree \
             of its own",
            template.name
        )
    })
}

/// Why the issue `named` is not started again: a run already works on it.
pub fn already_working(named: &str) -> String {
    format!("A run is already working on issue {named}.")
}

/// What a run fails on when what `check` would have blocked is found only
/// once it runs: a configuration failure is a preflight block found late,
/// so the way out offered is the one that changes the configuration.
pub fn found_late(check: Check) -> Failure {
    match check {
        Check::Workflow | Check::Agent | Check::Mode | Check::CheckCommand => {
            Failure::Configuration
        }
        Check::Forge => Failure::Forge,
        Check::Place
        | Check::Base
        | Check::Issue
        | Check::EarlierTask
        | Check::Slot
        | Check::PlaceTaken
        | Check::Limits => Failure::Other,
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

/// The issue's newest task, when it ended on something nobody chose and
/// nothing works on it now: its id, and how it ended.
fn earlier_ended(tasks: &[(Task, Option<Working>)]) -> Option<(String, String)> {
    let (task, working) = tasks.iter().max_by_key(|(task, _)| task.recency())?;
    if working.is_some() || !task.group(*working).needs_attention() {
        return None;
    }
    let said = crate::task::work::Work::of(task, None, None).said;
    Some((task.id.clone(), lower(&said)))
}

/// What a new task's report says of the issue's last one, when it ended on
/// something nobody chose: the run started anyway, beside it.
pub fn earlier_note(tasks: &[(Task, Option<Working>)]) -> Option<String> {
    let (_, said) = earlier_ended(tasks)?;
    Some(format!(
        "An earlier task on this issue ended: {said}. Its worktree is kept."
    ))
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn lower(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests;
