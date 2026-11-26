//! What a [`super::RunDrive`] is told, and what it answers with.
//!
//! **Each answered effect says what its late answer gets**, an answer that
//! comes after the run has moved on or finished: dropped, when the act left
//! nothing behind; recorded, when it did and must be known; or awaited, when
//! the run holds on until it lands. A finished run takes no answer at all:
//! its carrier lets it go, and what the run kept is the task's.

use super::{Lane, NotOpened};
use crate::connector::{Check, PullRequest};
use crate::workflow::{CommandResult, Facts, Mark, Outcome};
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
    /// The push landed, or why it did not.
    Pushed(Result<(), String>),
    /// The pull request on the run's branch, as the forge read it.
    PullRequestFound(Result<Option<PullRequest>, String>),
    /// The pull request was opened, with what reading it back found; or why
    /// it was not opened.
    PullRequestOpened(Result<Result<Option<PullRequest>, String>, String>),
    /// The failing checks' logs, by check name, or why each was not read.
    LogsRead(Vec<(String, Result<String, String>)>),
    /// The pull request was taken out of draft, or why not.
    MarkedReady(Result<(), String>),
    /// The run's forge cannot be called at all, for this reason: it has no
    /// branch of its own, or no connector of the name its setup keeps.
    NoForge(String),
    /// The clock armed last for `timer` ran out.
    Elapsed(Timer),
    /// The run's write landed, or why it did not.
    Saved(Result<(), String>),
    /// The run has room at the full step it waits before.
    RoomGiven,
    /// The session [`Effect::OpenSession`] asked for is open and bound to
    /// `lane`, or why it did not start.
    LaneOpened {
        lane: Lane,
        result: Result<(), NotOpened>,
    },
    /// Something landed in the session of `lane`, which is not the current
    /// one. Given with that lane's session, not the current one's.
    LaneAppended {
        lane: Lane,
    },
    /// The session of `lane`, which is not the current one, disconnected or
    /// was closed.
    LaneGone {
        lane: Lane,
    },
}

/// Whose session an [`Input`] is read with, and what becomes of it after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aim<'a> {
    /// The current lane's session.
    Current,
    /// A person's prompt in another lane: that lane's session.
    Speaks(&'a Lane),
    /// That lane's session went: it is forgotten once the run has been told.
    Gone(&'a Lane),
}

impl Input {
    /// Whose session this is read with, so a front end joins the right one.
    pub fn aim(&self) -> Aim<'_> {
        match self {
            Self::Appended
            | Self::TurnEnded
            | Self::AwaitingUser
            | Self::Disconnected
            | Self::NotTaken
            | Self::SessionGone
            | Self::PicksAnswered
            | Self::RoomGiven
            | Self::Notified
            | Self::PickRefused(_)
            | Self::Pinned(_)
            | Self::Measured(_)
            | Self::TurnRead(_)
            | Self::CommandExited(_)
            | Self::Pushed(_)
            | Self::PullRequestFound(_)
            | Self::PullRequestOpened(_)
            | Self::LogsRead(_)
            | Self::MarkedReady(_)
            | Self::NoForge(_)
            | Self::Elapsed(_)
            | Self::Saved(_)
            | Self::LaneOpened { .. } => Aim::Current,
            Self::LaneAppended { lane } => Aim::Speaks(lane),
            Self::LaneGone { lane } => Aim::Gone(lane),
        }
    }
}

/// What a run waits for on a clock. Each has one clock, which arming again
/// replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// The run's timeout.
    Timeout,
    /// The next look at the pull request's status checks.
    Checks,
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

/// What a run asks of its session, in the order given. The link makes each
/// through the session's own drive, so none is a front end's to carry: a
/// line said after the prompt it announces, or a prompt sent twice, cannot
/// be written there.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Asked {
    /// Say this in the run's transcript.
    Note(String),
    /// Send this prompt. A session that does not take it says so at once,
    /// and the run is told [`Input::NotTaken`]: it is never late. `at` is
    /// how many effects the call had answered when the prompt was asked,
    /// which is where what a refusal says to do belongs among them.
    Prompt { text: String, at: usize },
    /// Cancel the session's turn. Asked only while one runs.
    Cancel,
    /// Call the session this, unless it has a name of its own: a lane's
    /// session, bound.
    Name(String),
}

/// What a front end is to do, in the order given.
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
    /// Run this command in the work, and report [`Input::CommandExited`].
    ///
    /// Awaited: a run stopped while it runs calls it off and ends only once
    /// it has exited, and its place is held until then.
    RunCommand {
        dir: PathBuf,
        command: String,
        /// How long it may run; `None` for no limit.
        limit: Option<std::time::Duration>,
    },
    /// Call off the step's command running. Not answered itself: the
    /// command's own answer says it is over. A forge call is not called off:
    /// it is awaited.
    CallOffCommand,
    /// Push `commit` as the run's branch, and report [`Input::Pushed`].
    ///
    /// Awaited, as a command is: code that went up is up, so a Stop waits
    /// for it.
    Push { commit: String },
    /// Read the pull request on the run's branch, and report
    /// [`Input::PullRequestFound`].
    ///
    /// Awaited while the pull request step decides whether to open one, so
    /// none is opened once Stop is pressed; dropped late while the status
    /// checks are looked at, since a look leaves nothing behind.
    FindPullRequest,
    /// Open the run's pull request as a draft, read it back, and report
    /// [`Input::PullRequestOpened`]. Awaited: once open, it is open.
    OpenPullRequest,
    /// Read the logs of these failing checks, and report
    /// [`Input::LogsRead`]. Dropped late.
    ReadLogs { checks: Vec<Check> },
    /// Take pull request `number` out of draft, and report
    /// [`Input::MarkedReady`]. Awaited: a Stop waits for it.
    MarkReady { number: u64 },
    /// Report [`Input::Elapsed`] for `timer` after `after`, in place of the
    /// clock armed for it before.
    ///
    /// Dropped late: the timeout looks at what is left of it again, so a
    /// fire that comes early only arms it anew, and a look is taken only
    /// while the run still waits on its checks.
    Arm { timer: Timer, after: Duration },
    /// Report this input once the event being handled is over. Dropped
    /// late, as the input it carries would be.
    Defer(Input),
    /// Write the run, as [`RunDrive::run`] has it, into its task, and
    /// report [`Input::Saved`].
    ///
    /// Recorded: every write is of the whole task, in order, so the next one
    /// holds what a failed one missed; a failure is said, once.
    Save,
    /// What a run shows changed: draw it again.
    Refresh,
    /// The session was changed here (a pick sent): tell whatever watches it.
    SessionChanged,
    /// Tell this, as the front end words it.
    Signal(Signal),
    /// The run waits before a full step: ask for room there for its task,
    /// `limit` runs at most holding it, and report [`Input::RoomGiven`] when
    /// it has some.
    ///
    /// Awaited: the run waits for nothing else. A run that ends meanwhile
    /// gives up its turn for room with the rest of what it held.
    AskRoom {
        key: crate::task::rooms::RoomKey,
        limit: u32,
    },
    /// The run has left the step it held room at, for another step or for
    /// good: give that room up. Not answered.
    LeaveRoom,
    /// The run is over, as `outcome`: keep it and let its place go.
    Finish { outcome: Outcome },
    /// Open a session of `agent` named `name`, unshown, for `lane`, and
    /// report [`Input::LaneOpened`]; from then on the run's calls come with
    /// it, since `lane` is already the current one. `agent` is empty when
    /// the run's setup names none, as for the run's first session.
    ///
    /// Awaited: the step's prompt waits in its lane for the session. Once
    /// the run has finished, the session opened is the carrier's to close.
    OpenSession {
        lane: Lane,
        agent: String,
        name: String,
    },
}
