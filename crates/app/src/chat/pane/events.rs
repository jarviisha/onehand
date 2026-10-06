//! What the pane says to the window: the signal a session shows on the rail,
//! and the events by which the pane asks its owner for what it cannot do.

use gpui::SharedString;
use onehand_core::chat::Link;
use std::path::PathBuf;

/// What a session is doing, when that is something the rail should say.
///
/// Only states that want the user's eye. A session that is connected, idle and
/// already read carries **no** signal — that is what keeps a rail full of
/// healthy sessions a clean list of names rather than a wall of dots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionSignal {
    /// The adapter went away. `Ctrl+Shift+R` brings it back.
    Lost,
    /// A parked permission or question. The only signal that is about *the
    /// user*: nothing moves until they answer.
    AwaitingUser,
    /// A turn is in flight.
    Busy,
    /// A turn finished while this session was not being looked at.
    UnseenTurn,
}

impl SessionSignal {
    /// How badly each state wants the user's eye, lowest first.
    ///
    /// **The one place the order lives.** Three readers now — a session row
    /// reducing its own four facts, a project row reducing its sessions'
    /// signals, and the rail's flat session list sorting itself so the ones
    /// wanting an answer rise to the top — and an order written out twice is an
    /// order that will disagree with itself the first time someone edits one
    /// copy.
    pub(crate) fn rank(self) -> u8 {
        match self {
            // A dead adapter outranks a question nobody can answer any more.
            Self::Lost => 0,
            // A parked question outranks busy: only one of them moves on its
            // own.
            Self::AwaitingUser => 1,
            // What is happening now outranks what happened last turn.
            Self::Busy => 2,
            // The calmest of the four, and the only one about the past rather
            // than about now.
            Self::UnseenTurn => 3,
        }
    }

    /// Reduce a session's four independent facts to the one thing the rail
    /// draws.
    ///
    /// One mark, not one per condition: two marks side by side on a rail row
    /// are a code nobody learns. Which one survives is [`Self::rank`] — a
    /// judgement about which fact the user needs when several are true at once.
    ///
    /// Pure, and separate from the lookup, so the order is testable without a
    /// window: it is a rule about attention, and rules about attention are
    /// exactly what regresses silently.
    pub fn pick(link: Link, awaiting_user: bool, busy: bool, unseen: bool) -> Option<Self> {
        Self::most_urgent(
            [
                (link == Link::Lost).then_some(Self::Lost),
                awaiting_user.then_some(Self::AwaitingUser),
                busy.then_some(Self::Busy),
                unseen.then_some(Self::UnseenTurn),
            ]
            .into_iter()
            .flatten(),
        )
    }

    /// The signal a *group* of sessions carries — a project row's roll-up.
    ///
    /// Without it a collapsed project was silent about everything inside it:
    /// an agent could be waiting on an answer, or dead, with nothing on screen
    /// saying so until the user thought to expand that project. The same rank
    /// decides, so the mark on a project means exactly what the same mark means
    /// on the session it came from.
    pub fn most_urgent(signals: impl IntoIterator<Item = Self>) -> Option<Self> {
        signals.into_iter().min_by_key(|signal| signal.rank())
    }
}

/// What [`ChatPane::restart`] did, so the shell can say so.
pub enum Restart {
    /// The adapter is coming back up on the same conversation.
    Restarted,
    /// Nothing to restart -- no session, or one that never connected.
    Nothing,
}

/// The project-page menu's entries.
///
/// **Not a copy of the rail's project menu.** Two of that menu's entries are
/// missing here on purpose: *New session* is the primary button in the middle of
/// this very page, and *Open terminal* is a button at the end of the row the
/// menu hangs off. Repeating either would be the page offering the same thing
/// twice within an inch of itself.
#[derive(Clone, Copy)]
pub enum ProjectAction {
    /// Pin to the top of the rail, or take the pin off.
    TogglePin,
    /// Let its labelled issues be worked unattended, or stop that.
    ToggleUnattended,
    /// Open its open issues, to pick one to work now.
    PickIssue,
    /// Split it into a second checkout. Offered on repositories only.
    Worktree,
    /// Rename the branch checked out in it. Offered on repositories only, for
    /// the reason above and by the same fact: a project with no status from the
    /// last sweep has no branch to rename.
    RenameBranch,
    CopyPath,
    RefreshGit,
    /// Drop it from the workspace. The shell still guards this behind a second
    /// press while anything is running in it.
    Remove,
}

/// What the pane asks the shell for. Kept tiny on purpose: the chat's job is
/// the conversation, and routing a file into the Workbench is the shell's.
pub enum ChatPaneEvent {
    OpenFile(PathBuf),
    /// A turn finished, so the agent has probably touched the working tree.
    ///
    /// Announced rather than acted on: the chat has no idea a file tree or a
    /// git status exists, and the two things that go stale here belong to two
    /// other panels. Which session it was does not matter — an agent writes to
    /// the root, and the panels are per root.
    WorkTreeTouched,
    /// An agent was just spawned for a session — a first connect, a
    /// restart, a resume. Announced so the shell can tell whatever keeps
    /// count of what the running agent has loaded; every path that starts one
    /// passes through here, which no caller of those paths can promise.
    AgentStarted,
    /// The rail is hidden and the user asked for it back.
    ///
    /// Announced rather than acted on for the usual reason: the rail is the
    /// window's chrome, and a dock panel has no business reaching outside the
    /// dock to draw it.
    ShowRail,
    /// The user pressed the Workbench button in the conversation's header.
    ///
    /// Announced rather than acted on for the same reason as the rail: the
    /// Workbench is a dock, the dock is the window's arrangement, and the panel
    /// sitting in the middle of it does not get to rearrange the window. It
    /// also does not know which mode the Workbench would come back on, and two
    /// places deciding that would drift apart.
    ///
    /// **Open or closed, and not the third state the key has.** A key has one
    /// binding to serve every case, so an open-but-unfocused panel is focused
    /// rather than closed -- there is nothing else to reach it with. A button
    /// can see the dock, and the caret when it is pressed is almost always back
    /// in the composer, so the third state made the first press do nothing a
    /// presser could see.
    ToggleWorkbench,
    /// The user pressed the terminal button in the conversation's header.
    ///
    /// Announced rather than acted on for exactly the reasons above, and open
    /// or closed for the same one -- with one condition of its own: an open
    /// dock holding no shell is opened *into* rather than closed, because that
    /// is what closing the last tab leaves and the press there means "start
    /// one". The key is `` Ctrl+` ``, unshifted, because the shifted form
    /// cannot be typed.
    ToggleTerminal,
    /// Restart the agent on the conversation showing.
    ///
    /// Announced rather than done here even though the pane owns the adapter:
    /// a restart mid-turn has to be confirmed, and the confirmation is a
    /// notification the shell raises. Doing half of it here would mean two
    /// places deciding when a turn may be thrown away.
    Restart,
    /// Close the session showing, and with it its agent.
    ///
    /// The pane can drop a conversation but not the workspace row that names
    /// it, and the mid-turn guard belongs with the same one that guards the
    /// rail's ✕.
    CloseSession,
    /// Something done to the project the pane is standing on with no session.
    ///
    /// One variant carrying an action rather than five of its own, because they
    /// are one sentence with a word swapped: do this to the *selected* project.
    /// The shell answers every one of them by reaching for the same root, and
    /// the page that offers them is only ever drawn for that root — it is what
    /// shows when the selected project has nothing running in it.
    Project(ProjectAction),
    /// Rename the conversation showing.
    ///
    /// Announced rather than done here because the rename is a dialog, and a
    /// dialog belongs to the window: the shell already owns the field, the
    /// trigger-less dialog it lives in and the reset-to-derived-title rule, and
    /// the rail's own *Rename…* goes to the same place.
    Rename,
    /// Delete the conversation showing — the directory on disk, and with it the
    /// session that is writing to it.
    ///
    /// The pane cannot do this alone and must not try. While the session is
    /// alive its mark says the transcript so far is already on disk, so the very
    /// next turn would write the file back holding only what came after: a
    /// delete that does not stay deleted. Ending the session is what settles
    /// that, and the session is a row in the workspace tree — the shell's.
    DeleteConversation(PathBuf),
    /// Start a session on the project the pane is standing in — the project
    /// page's *New session*, and every past conversation listed under it.
    ///
    /// `agent` names the agent to run it, which is the archive's own when a
    /// past conversation was picked and nobody's when the button was; `resume`
    /// is the archive to open it on.
    ///
    /// Announced rather than done here because a session is a row in the
    /// workspace tree: the pane holds conversations, the shell holds the tree,
    /// and a pane that minted its own sessions would be the second place
    /// deciding which project one belongs to.
    StartSession {
        agent: Option<SharedString>,
        resume: Option<PathBuf>,
    },
    /// A conversation could not be written to disk.
    ///
    /// Announced rather than shown here because the pane has no window to show
    /// it on, and because this is the one layer of the app's state the user
    /// cannot produce again: a workspace or a config that fails to save can be
    /// re-entered, a conversation that fails to save is gone. Every other write
    /// in the app says so when it fails; this one used to fail in silence.
    ArchiveFailed(String),
    /// A conversation the user asked to delete is still there, and why.
    ///
    /// Its own variant rather than a second use of the one above, because the
    /// two need opposite sentences: one says work was not kept, this one says
    /// work was not thrown away. Reporting a refusal as a failure to save would
    /// send the reader looking for the wrong thing entirely.
    ConversationNotDeleted(String),
    /// Open issue `number` of the project at `root` — a row of the workspace
    /// page. Selecting a project and opening the Workbench are both the
    /// shell's.
    OpenIssue {
        root: PathBuf,
        number: u64,
    },
    /// Show session `uid`, in whichever window holds it — a row of the
    /// workspace page, for an unattended run's session or one of this window's.
    ShowSession {
        uid: u64,
        window: gpui::AnyWindowHandle,
    },
    /// Select the project at this root — a row of the workspace page.
    ShowProject(PathBuf),
    /// Resume `archive` on `agent` in the project at `root` — a recent
    /// conversation on the workspace page.
    ResumeIn {
        root: PathBuf,
        agent: SharedString,
        archive: PathBuf,
    },
    /// The run on session `uid` waits for approval, and it is
    /// given: go on. Announced, because the run is the shell's and the strip
    /// that offers this only draws it.
    ContinueWorkflow(u64),
    /// Send what that run waits on back, with a `note` on what to change.
    ReviseWorkflow {
        uid: u64,
        note: String,
    },
    /// Stop the run on session `uid`.
    StopWorkflow(u64),
    /// Resume the interrupted task `id` — a row of the Tasks page.
    ResumeTask(String),
    /// Let the ended task `id` go; it is kept as history.
    DismissTask(String),
    /// Run task `id` again, asking first where from.
    RetryTask(String),
    /// Stop task `id`, queued or running.
    StopTask(String),
    /// Show the Tasks page, narrowed to one project or not.
    ShowTasks(Option<PathBuf>),
    /// Run the check command of the project on screen, as a task.
    RunCheck,
}
