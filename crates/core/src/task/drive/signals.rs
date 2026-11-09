//! What a [`super::RunDrive`] is told, and what it answers with.
//!
//! **Each answered effect says what its late answer gets**, an answer that
//! comes after the run has moved on or finished: dropped, when the act left
//! nothing behind; recorded, when it did and must be known; or awaited, when
//! the run holds on until it lands. A finished run takes no answer at all:
//! its carrier lets it go, and what the run kept is the task's.

use crate::workflow::{CommandResult, Facts, Mark, Outcome, PrOpened, Seen};
use std::path::PathBuf;
use std::time::Duration;

/// What happened, as the drive is told it.
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
    /// The run's write landed, or why it did not.
    Saved(Result<(), String>),
}

/// What a person, or the log, is told about the run: the words and where
/// they go are the front end's.
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// The run's file could not be written, for this reason; said once until
    /// a write lands again.
    SaveFailed(String),
    /// A step's mark could not be pinned, for this reason. The run goes on
    /// without it: gates read the work, not the pinned commits.
    PinFailed(String),
}

/// What to do, in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Pin these marks in the work, and report [`Input::Pinned`].
    ///
    /// Awaited: what is asked meanwhile, an ending included, waits for it,
    /// so no run ends with a pin in flight.
    Pin { dir: PathBuf, refs: Vec<String> },
    /// Read the work's [`Mark`], and report [`Input::Measured`].
    ///
    /// Dropped late: a read leaves nothing behind.
    Measure { dir: PathBuf },
    /// Read the work since `from`, and report [`Input::TurnRead`].
    ///
    /// Dropped late: a read leaves nothing behind, and a turn ending while
    /// a mark is pinned is not judged.
    ReadFacts { dir: PathBuf, from: Mark },
    /// Send this prompt to the session; report [`Input::NotTaken`] if it
    /// does not take it, which the session says at once: it is never late.
    Submit(String),
    /// Cancel the session's turn.
    CancelTurn,
    /// Say this in the run's transcript.
    Note(String),
    /// Run this command in the work, and report [`Input::CommandExited`].
    ///
    /// Awaited: a run stopped while it runs calls it off and ends only once
    /// it has exited, and its place is held until then.
    RunCommand { dir: PathBuf, command: String },
    /// Call off the command or forge call running. Not answered itself: the
    /// call's own answer says it is over.
    CallOffCommand,
    /// Push `commit` as the run's branch, or with none open its pull
    /// request, and report [`Input::ForgeDone`].
    ///
    /// Awaited, as a command is: code that went up is up, so a Stop waits
    /// for it, and no pull request is opened once it is pressed.
    Forge { commit: Option<String> },
    /// Watch the pull request's status checks on `pushed`, at most `wait`,
    /// and report [`Input::ChecksSeen`].
    ///
    /// Dropped late: the watch goes with the run. A draft taken out of draft
    /// just before is left so; the pull request's state is the forge's, read
    /// again wherever it is shown.
    WatchChecks {
        pushed: Option<String>,
        wait: Duration,
    },
    /// Report [`Input::ClockFired`] after this long, in place of the clock
    /// armed before.
    ///
    /// Dropped late: a fire looks at what is left of the timeout again, so
    /// one that comes early only arms the clock anew.
    ArmClock(Duration),
    /// Report this input once the event being handled is over. Dropped
    /// late, as the input it carries would be.
    Defer(Input),
    /// Write the run, as [`RunDrive::run`] has it, into its task, and
    /// report [`Input::Saved`].
    ///
    /// Recorded: every write is of the whole task, in order, so the next one
    /// holds what a failed one missed; a failure is said, once.
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
    /// The session was changed here (a pick sent): tell whatever watches it.
    SessionChanged,
    /// Tell this, as the front end words it.
    Signal(Signal),
    /// The run is over, as `outcome`: keep it and let its place go.
    Finish { outcome: Outcome },
}
