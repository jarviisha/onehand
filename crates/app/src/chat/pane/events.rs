//! What the pane says to the window: the signal a session shows on the rail,
//! and the events by which the pane asks its owner for what it cannot do.

use gpui::SharedString;
use std::path::PathBuf;

pub use onehand_core::chat::SessionSignal;

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
    /// The workers' button in the header, or the hide on the region's strip:
    /// the region takes the Workbench's place in the window's dock, or gives
    /// it back.
    ToggleWorkers,
    /// The region's maximize, which the shell carries out as it does a
    /// dock's.
    MaximizeWorkers,
    /// *Close* in a worker's block: its session closed as the rail closes
    /// one, by the shell that owns the row.
    CloseWorker(u64),
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
    /// Open Settings on its Agents page — the no-agent state's way to where an
    /// agent is set up. Settings is the shell's, as every page is.
    OpenAgentSettings,
    /// Add a built-in agent found on this machine — the no-agent state's offer.
    /// The agent list is the shell's to change and to save.
    AddFoundAgent(onehand_core::config::Preset),
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
    /// A rename's write failed, its name put back: why.
    RenameFailed(String),
    /// The agent refused a pick, put back to what it last confirmed: why.
    PickRefused(String),
    /// The adapter would not stop a background item: why.
    StopRefused(String),
    /// A conversation is being forked: a session on the project at `root`,
    /// running `spec`, the agent of the conversation forked from. The fork
    /// itself is waiting in the pane for that session.
    Fork {
        root: PathBuf,
        spec: onehand_core::config::AgentSpec,
    },
    /// A fork could not be made, and why. `uid` is the session started for it,
    /// to end, when one was.
    ForkFailed {
        uid: Option<u64>,
        why: String,
    },
    /// Stop the run on session `uid`.
    StopWorkflow(u64),
    /// Show the Tasks page, narrowed to one project or not.
    ShowTasks(Option<PathBuf>),
    /// Show the Schedules page, narrowed to one project or not.
    ShowSchedules(Option<PathBuf>),
    /// Run the check command of the project on screen, as a task.
    RunCheck,
}
