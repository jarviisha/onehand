//! What a [`super::Drive`] is told, and what it answers with.

use crate::workflow::{ApprovalAt, CommandResult, Facts, Mark, Outcome, PrOpened, Seen};
use std::path::PathBuf;
use std::time::Duration;

/// What happened, as the driver is told it.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The session's transcript grew.
    Appended,
    TurnEnded,
    /// The agent parked a card for a person.
    AwaitingUser,
    Disconnected,
    /// The agent refused a pick, for this reason.
    PickRefused(String),
    /// The session changed in any way: a card may have been answered.
    Notified,
    /// The session went with the run still going: its window or the session
    /// was closed. The run is kept to be resumed, not ended.
    SessionGone,
    /// Picks were answered: plan the next.
    PicksAnswered,
    /// A person approved what they read at `at`.
    Approved {
        at: ApprovalAt,
    },
    /// A person sent what they read at `at` back with `note`.
    Revised {
        at: ApprovalAt,
        note: String,
    },
    /// A person pressed Stop.
    Stop,
    /// The marks asked for landed on this commit, or did not.
    Pinned(Result<String, String>),
    /// Where the step's work starts, or why it could not be read.
    Measured(Result<Mark, String>),
    /// The work after a turn, or why it could not be read.
    TurnRead(Result<Facts, String>),
    /// The session did not take the prompt.
    NotTaken,
    /// The step's command exited.
    CommandExited(CommandResult),
    /// A forge call landed.
    ForgeDone(Result<Option<PrOpened>, String>),
    /// A look at the pull request's status checks said something other than
    /// pending.
    ChecksSeen(Seen),
    /// The clock armed last ran out.
    ClockFired,
}

/// What to do, in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Pin these marks in the work, and report [`Input::Pinned`].
    Pin { dir: PathBuf, refs: Vec<String> },
    /// Read the work's [`Mark`], and report [`Input::Measured`].
    Measure { dir: PathBuf },
    /// Read the work since `from`, and report [`Input::TurnRead`].
    ReadFacts { dir: PathBuf, from: Mark },
    /// Send this prompt to the session; report [`Input::NotTaken`] if it
    /// does not take it.
    Submit(String),
    /// Cancel the session's turn.
    CancelTurn,
    /// Say this in the run's transcript.
    Note(String),
    /// Run this command in the work, and report [`Input::CommandExited`].
    RunCommand { dir: PathBuf, command: String },
    /// Call off the command or forge call running.
    CallOffCommand,
    /// Push `commit` as the run's branch, or with none open its pull
    /// request, and report [`Input::ForgeDone`].
    Forge { commit: Option<String> },
    /// Watch the pull request's status checks on `pushed`, at most `wait`,
    /// and report [`Input::ChecksSeen`].
    WatchChecks {
        pushed: Option<String>,
        wait: Duration,
    },
    /// Report [`Input::ClockFired`] after this long.
    ArmClock(Duration),
    /// Report this input once the event being handled is over.
    Defer(Input),
    /// Write the run, as [`Drive::run`] has it, into its task.
    Save,
    /// The run waits on a person, or on its status checks: its issue's slot
    /// is free meanwhile.
    Waiting,
    /// A person answered what the run waited on.
    Answered,
    /// The agent came up in the run's session for the first time.
    CameUp,
    /// Pause unattended runs: every later one would fail as this did.
    Pause { why: String, fix: String },
    /// What a run shows changed: draw it again.
    Refresh,
    /// The run is over, as `outcome`: keep it and let its place go.
    Finish { outcome: Outcome },
}
