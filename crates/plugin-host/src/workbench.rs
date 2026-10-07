//! What a Workbench mode is, from the panel's side.
//!
//! A mode owns its state and its view; the panel owns neither. It keeps the
//! list, remembers which one is showing, draws the strip that switches between
//! them, and passes on the handful of things the shell asks of "the Workbench"
//! without knowing which mode answers.
//!
//! The trait lives here rather than in the GUI-free half of the plugin contract
//! because a mode's whole substance is a view: it hands back an `AnyView` and
//! takes a `Window`, neither of which that half is allowed to name.

use gpui::{AnyView, App, Pixels, Window};
use onehand_core::gitstat::GitStatus;
use onehand_core::task::Approval;
use onehand_core::task::work::IssueWork;
use onehand_plugin_api::WorkbenchModeSpec;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

/// One Workbench mode.
///
/// Everything past [`Self::spec`] and [`Self::view`] has a default that does
/// nothing, which is what keeps a mode down to the part of this that it
/// actually has an answer for — the file tree has no opinion about saving and
/// the quick editor none about a terminal's font size.
pub trait WorkbenchMode {
    /// The ID, the word on the strip, and the two facts the panel would
    /// otherwise have to work out by matching that ID against a list.
    fn spec(&self) -> WorkbenchModeSpec;

    /// The body, drawn while this mode is the one showing.
    fn view(&self) -> AnyView;

    /// Point this mode at a project root.
    ///
    /// Every mode here is per root, so this is not a request that one of them
    /// takes and the rest ignore — it is told to all of them, and the reply is
    /// nothing because there is nothing to decide.
    fn set_root(&mut self, _root: &Path, _cx: &mut App) {}

    /// Drop everything held for a root that has left the workspace.
    fn forget_root(&mut self, _root: &Path, _cx: &mut App) {}

    /// Open a file, and say whether this mode was the one that edits files.
    ///
    /// A method rather than a broadcast [`Request`] for the reason
    /// [`Self::focus`] is: building a buffer needs a `Window`, and a request
    /// can arrive from a path that has none. [`Request::OpenFile`] is the same
    /// thing travelling the other way — what a mode raises through its [`Ask`]
    /// when a click inside it means a file should be opened.
    fn open_file(&mut self, _path: &Path, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// Put the caret where this mode's work happens, and say whether there was
    /// anywhere to put it.
    ///
    /// A method rather than a [`Request`] because it is the one thing a mode is
    /// asked that needs a `Window`, and requests arrive from paths that have
    /// none — a `git status` sweep landing, a turn ending. `false` is the
    /// honest answer from a mode that is clicked rather than typed into, and
    /// the panel then takes focus itself.
    fn focus(&self, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// Answer a [`Request`], and say whether this mode was the one that did.
    ///
    /// Broadcast to every mode in the panel's order, because the shell asks the
    /// *Workbench* to save, to start, to reap — it has no business knowing
    /// which mode owns a buffer or a child process. The `bool` is what lets the
    /// panel act on the answer where the answer matters: a mode that answers
    /// [`Request::Reap`] is one whose child has just stopped existing, which is
    /// what decides whether the caret has to be moved out of it.
    fn handle(&mut self, _request: &Request<'_>, _cx: &mut App) -> bool {
        false
    }

    /// How many of `root`'s open files hold edits that closing it would throw
    /// away, which the shell asks before it removes a project.
    fn unsaved(&self, _root: &Path, _cx: &App) -> usize {
        0
    }
}

/// Something asked of the Workbench without naming which mode answers it.
///
/// One vocabulary in **both** directions. The panel broadcasts these down; a
/// mode sends one back up through its [`Ask`] when a click inside it means
/// something the panel owns — a row in the file tree is the case that exists
/// today, since opening a file is the quick editor's business and not the
/// tree's.
///
/// This enum is what keeps the trait above from growing a method per shell
/// feature. A mode writes an arm for what it recognises and falls through the
/// rest, which is the same bargain the trait's own defaults strike: an eighth
/// request must not become an edit in four crates that have no answer to it.
pub enum Request<'a> {
    /// Open a file in whichever mode edits files, and switch to it.
    ///
    /// Only ever travels **upward**: it is what a mode raises through its
    /// [`Ask`], and the panel puts it to the modes through
    /// [`WorkbenchMode::open_file`], which has the `Window` that building a
    /// buffer needs.
    OpenFile(&'a Path),
    /// Write the active buffer. `Ctrl+S`, which is bound so that a program in a
    /// PTY keeps it — so this only ever reaches a mode drawing an element tree.
    Save,
    /// Start whatever child process this mode is a front end for.
    ///
    /// Separate from becoming the active mode on purpose: switching to a mode
    /// is a view change and must not launch anything, or the strip becomes a
    /// row of buttons one of which spawns a process.
    Start,
    /// Re-read whatever this mode listed off the disk, because a turn has ended
    /// and the agent has been writing the whole time.
    Rescan,
    /// This mode has just become the one on screen.
    ///
    /// The one request that is **not** broadcast: it goes to the arriving mode
    /// alone, since every other mode's answer would be a lie. A mode whose
    /// listing costs a walk of the whole project uses this to decide the walk
    /// is worth paying for.
    Shown,
    /// Collect a child that has exited.
    ///
    /// Answered `true` by a mode that had one, which is what tells the panel to
    /// move the caret out of a view that is about to stop existing.
    Reap,
    /// The workspace's `git status`, for whoever draws change badges.
    SetGit(&'a HashMap<PathBuf, GitStatus>),
    /// Where this window's workspace keeps what it persists, or `None` for a
    /// workspace bound to no storage — which keeps nothing.
    ///
    /// Told once when the panel is built and again whenever the binding
    /// changes, and only a mode that keeps files of its own answers it.
    SetStorage(Option<&'a Path>),
    /// The reading size for a mode that is measured rather than laid out.
    ///
    /// A font size and not the panel's zoom factor, because the only mode that
    /// answers this is a glyph grid: it is sized from a shaped glyph, so the
    /// rem base the other modes scale by would stretch the box around it while
    /// the cell stayed put.
    SetFontSize(Pixels),
    /// Select the active project's issue with this number, for whichever mode
    /// lists issues. Sent after the project is chosen, so it may arrive before
    /// that project's issues have been read.
    ShowIssue(u64),
    /// When the agent of the session on screen started, or `None` while no
    /// session is showing. Broadcast whenever that changes — a project or
    /// session switch, a first connect, a restart — so a mode that changes
    /// what an agent loads at start can tell which of its changes the running
    /// one has not seen.
    AgentStarted(Option<Instant>),
    /// The conversations this window has a live session on, by the agent's
    /// session id. Broadcast whenever the set changes, so a mode that keeps
    /// record of which conversation took something up can tell whether that
    /// one is still going.
    LiveConversations(&'a [String]),
    /// Restart the agent of the session on screen. Only ever travels
    /// **upward**: the shell owns sessions, and a mode asking for this has
    /// changed something an agent reads only when it starts.
    RestartAgent,
    /// Start a session on project `root`, in the checkout it is open on, with
    /// `prompt` as its first message, and say on issue `number` which session
    /// took it up once the agent has named it. Only ever travels **upward**,
    /// for the reason [`Self::RestartAgent`] does.
    WorkIssueHere {
        root: &'a Path,
        number: u64,
        prompt: &'a str,
    },
    /// Put the conversation the agent named `session` on screen: the live
    /// session holding it, or the saved one reopened on the project it ran
    /// in. Only ever travels **upward**.
    OpenConversation(&'a str),
    /// The work on every issue a project in this window keeps itself that a
    /// task works, one summary per issue, and the projects a run may be
    /// started on (git repositories that are not a run's own worktree).
    /// Broadcast whenever a task starts, moves on a step or ends, or the
    /// projects change, so a mode listing issues can say where each one's
    /// work stands and offer a run only where one can start.
    IssueWork {
        works: &'a [IssueWork],
        offered: &'a [PathBuf],
    },
    /// Let a person choose a workflow and work issue `number` of project
    /// `root` with it, on a worktree of its own, as a task. Only ever travels
    /// **upward**.
    RunIssueWorkflow { root: &'a Path, number: u64 },
    /// Put issue `number` of project `root` on the Issues page, where there
    /// is room to work it, its filters left as they are. Only ever travels
    /// **upward**.
    OpenInIssues { root: &'a Path, number: u64 },
    /// Put issue `number` of project `root` on the Issues page with what its
    /// work waits for approval on open to review. Only ever travels
    /// **upward**.
    ReviewInIssues { root: &'a Path, number: u64 },
    /// Approve what a run waits on, as read: refused by the run unless it
    /// still waits where it was read. Only ever travels **upward**.
    ApproveTask(&'a Approval),
    /// Send what a run waits on, as read, back with `note`: refused the same
    /// way. Only ever travels **upward**.
    ReviseTask {
        approval: &'a Approval,
        note: &'a str,
    },
    /// Put task `id` on screen, in the Tasks page's detail. Only ever travels
    /// **upward**.
    OpenTask(&'a str),
    /// Put the session task `id` runs in on screen. Only ever travels
    /// **upward**.
    OpenTaskSession(&'a str),
    /// Carry task `id` on where it was. Only ever travels **upward**.
    ResumeTask(&'a str),
    /// Choose how to run task `id` again. Only ever travels **upward**.
    RetryTask(&'a str),
    /// Run task `id` again with what Settings say now, asking first. Only
    /// ever travels **upward**.
    RetryTaskCurrent(&'a str),
    /// Answer the review on issue task `id`'s open pull request, as putting
    /// the trigger label back does. Only ever travels **upward**.
    AnswerReview(&'a str),
    /// Stop task `id`. Only ever travels **upward**.
    StopTask(&'a str),
}

/// How a mode reaches back into the panel.
///
/// Handed to a mode when it is built rather than set afterwards, so there is no
/// window in which a mode is on screen holding nothing to ask through.
pub type Ask = Rc<dyn Fn(&Request<'_>, &mut Window, &mut App)>;
