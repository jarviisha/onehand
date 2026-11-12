//! What a Workbench mode is, apart from how it is mounted: its name, what it
//! declares about itself, where it is offered, and the requests that travel
//! between it and the panel. The mount (the trait a mode implements and how
//! it asks the panel for things) is each front end's.

use crate::disk::Disk;
use crate::gitstat::GitStatus;
use crate::task::work::IssueWork;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModeId(&'static str);

impl ModeId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ModeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The key context a Workbench mode takes while it is on screen, unless it says
/// otherwise. It is the app's own name for the panel, and it is what `Ctrl+S`
/// is bound against.
pub const WORKBENCH_KEY_CONTEXT: &str = "Workbench";

/// The key context a mode hosting a live PTY takes instead.
///
/// It has to be the terminal's own name and not one of the mode's: `Ctrl+S` is
/// bound so that a program inside a PTY keeps it, and that binding names this
/// context. A mode that invented a second name for the same fact would have the
/// quick editor's save fire over the top of the program's own.
pub const TERMINAL_KEY_CONTEXT: &str = "Terminal";

/// What a Workbench mode declares about itself.
///
/// The two facts past the name are stated by the mode rather than worked out by
/// the panel from the mode's ID. A panel branching on IDs it has to know by
/// heart is a panel that cannot host a mode it was not compiled against, and
/// every such branch is one more place a fourth mode has to be remembered in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkbenchModeSpec {
    pub id: ModeId,
    /// The word on the mode strip.
    pub label: &'static str,
    /// The key context the panel takes while this mode shows.
    pub key_context: &'static str,
    /// Whether this mode's body is scaled by the panel's rem base.
    ///
    /// False for a measured glyph grid: it is sized from a shaped glyph rather
    /// than from the rem base around it, so scaling the box stretches the
    /// container while the cell stays put and every column lands past its own
    /// character. Such a mode takes its reading size some other way.
    pub rem_zoom: bool,
    /// Whether this mode is offered on a project whose files are on a host.
    ///
    /// Only a mode that reads and writes through the [`Disk`] it is handed
    /// can be: one that reads this machine's disk itself, or starts a program
    /// here, would show or change the wrong files.
    pub on_host: bool,
}

impl WorkbenchModeSpec {
    /// A mode drawn as an ordinary element tree: it scales with the rem base
    /// and leaves the panel's own key context alone.
    pub const fn element(id: ModeId, label: &'static str) -> Self {
        Self {
            id,
            label,
            key_context: WORKBENCH_KEY_CONTEXT,
            rem_zoom: true,
            on_host: false,
        }
    }

    /// A mode hosting a live terminal grid.
    pub const fn terminal_grid(id: ModeId, label: &'static str) -> Self {
        Self {
            id,
            label,
            key_context: TERMINAL_KEY_CONTEXT,
            rem_zoom: false,
            on_host: false,
        }
    }

    /// The same mode, offered on a host's project too.
    pub const fn on_host(self) -> Self {
        Self {
            on_host: true,
            ..self
        }
    }

    /// Whether this mode is offered on a project, `on_host` when its files
    /// are on a host.
    pub const fn offered(self, on_host: bool) -> bool {
        !on_host || self.on_host
    }
}

/// Whether the Workbench should attach again to the host a project on screen
/// is shown from.
#[derive(Debug, PartialEq, Eq)]
pub enum Again {
    /// The attach is up, or the project is this machine's: nothing to do.
    No,
    /// The attach has ended: attach again.
    Attach,
    /// The host closed it because another machine attached. That machine
    /// holds the host now, and taking it back would end its sessions in turn,
    /// so this stands as a failure, said in these words.
    Taken(String),
}

/// Whether to attach again to the host `disk` is on.
pub fn again(disk: &Disk) -> Again {
    if !disk.is_closed() {
        return Again::No;
    }
    match disk.host().filter(|_| disk.taken()) {
        Some(host) => Again::Taken(format!("{host} was attached from another machine")),
        None => Again::Attach,
    }
}

/// Which mode the Workbench shows, among its modes in strip order: the one
/// picked, unless the project on screen does not offer it, and never another
/// while the panel says why it has nothing for the project.
///
/// A model whose commands answer the place that has just come to show, if
/// any, which is the one to tell it shows; it has no effects of its own, and
/// the front end holds the modes themselves.
pub struct Showing {
    specs: Vec<WorkbenchModeSpec>,
    active: usize,
    on_host: bool,
    refused: bool,
}

impl Showing {
    /// The modes, in strip order; the first shows. There is at least one.
    pub fn new(specs: Vec<WorkbenchModeSpec>) -> Self {
        assert!(!specs.is_empty(), "the Workbench has no modes");
        Self {
            specs,
            active: 0,
            on_host: false,
            refused: false,
        }
    }

    /// The place of the mode showing.
    pub fn active(&self) -> usize {
        self.active
    }

    /// Whether the project on screen is a host's folder.
    pub fn on_host(&self) -> bool {
        self.on_host
    }

    /// Whether `spec` is offered on the project on screen.
    pub fn offers(&self, spec: WorkbenchModeSpec) -> bool {
        spec.offered(self.on_host)
    }

    /// Another project is on screen, a host's folder when `on_host`: the
    /// first mode offered there shows when the one showing is not.
    pub fn set_root(&mut self, on_host: bool) -> Option<usize> {
        self.on_host = on_host;
        if self.offers(self.specs[self.active]) {
            return None;
        }
        let place = self.specs.iter().position(|spec| self.offers(*spec))?;
        self.active = place;
        Some(place)
    }

    /// A file is to be opened: the modes offered are asked in strip order
    /// whether each takes it (`took`, given its place), and the first that
    /// does shows. Nothing is asked while a refusal stands.
    pub fn opened(&mut self, mut took: impl FnMut(usize) -> bool) -> Option<usize> {
        if self.refused {
            return None;
        }
        let on_host = self.on_host;
        let place = (0..self.specs.len())
            .find(|&place| self.specs[place].offered(on_host) && took(place))?;
        self.active = place;
        Some(place)
    }

    /// The mode `id` is picked: it shows, the one showing included, so
    /// picking it again asks it again. Refused while a refusal stands, or
    /// where it is not offered.
    pub fn pick(&mut self, id: ModeId) -> Option<usize> {
        if self.refused {
            return None;
        }
        let place = self
            .specs
            .iter()
            .position(|spec| spec.id == id && self.offers(*spec))?;
        self.active = place;
        Some(place)
    }

    /// The panel says why it has nothing for the project (`refused`), or no
    /// longer.
    pub fn refuse(&mut self, refused: bool) {
        self.refused = refused;
    }
}

/// Something asked of the Workbench without naming which mode answers it.
///
/// One vocabulary in **both** directions. The panel broadcasts these down; a
/// mode sends one back up through the way its front end gives it to ask the
/// panel, when a click inside it means something the panel owns, or another
/// mode does — a row in the file tree opens a file, which is the quick
/// editor's business and not the tree's. What is the app's own to answer, as
/// an issue's work is, does not travel here: a view with such a thing to say
/// speaks to the shell directly.
///
/// This enum is what keeps a mode's trait from growing a method per shell
/// feature. Every mode matches it whole, naming what it has no answer to, so a
/// new request is a compile error in each mode until it is decided there
/// rather than a silent fall-through.
pub enum Request<'a> {
    /// Open a file in whichever mode edits files, and switch to it.
    ///
    /// Only ever travels **upward**: it is what a mode raises, and the panel
    /// puts it to the modes through the mount's own open, which has what
    /// building a buffer needs.
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
    /// The reading size for a mode that is measured rather than laid out, in
    /// pixels.
    ///
    /// A font size and not the panel's zoom factor, because the only mode that
    /// answers this is a glyph grid: it is sized from a shaped glyph, so the
    /// rem base the other modes scale by would stretch the box around it while
    /// the cell stayed put.
    SetFontSize(f32),
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
    /// The project folders nothing here may write now, each with the line
    /// that says why: an offloaded project while its host holds the code.
    /// Broadcast whenever the set changes, whole, so a folder left out is
    /// one that may be written again.
    Locked(&'a HashMap<PathBuf, String>),
    /// The offloaded project folders, each with the host its sessions run
    /// on. Broadcast whenever the set changes, whole.
    Offloaded(&'a HashMap<PathBuf, String>),
    /// Copy this file, or this folder with everything under it, ignored
    /// files included, to the same place in its offloaded project's copy on
    /// the host. Only ever travels **upward**: the shell knows the host and
    /// the copy, and says how it went.
    SendToHost(&'a Path),
}

#[cfg(test)]
mod tests;
