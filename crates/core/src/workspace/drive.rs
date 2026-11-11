//! What happens next for one window's workspace: its projects, the sessions
//! on each, what is selected, and the workspace file.
//!
//! **Decided here, carried out elsewhere.** A [`WorkspaceDrive`] owns the
//! window's [`Workspace`] tree and answers each change with [`Effect`]s: a
//! session to let go of ([`Effect::Close`]), a project's things to let go of
//! ([`Effect::Forgot`]), the selection to put on screen ([`Effect::Show`]),
//! and the workspace file to write ([`Effect::Write`]). It derefs to the tree
//! and hands out no mutable one, so the front end changes the tree only
//! through its methods.
//!
//! **A session's own life is its drive's.** Whether closing or restarting one
//! asks first is `SessionDrive`'s ([`crate::chat::SessionDrive::closing`]);
//! this drive says where a session sits, which goes with it, and what shows
//! after.
//!
//! **The file is written whole, in order, and put back by thing.** A change a
//! person makes shows at once and is written after every write before it; a
//! write that fails puts back each thing no later write carries, to its last
//! value known saved, and is said once ([`Signal::SaveFailed`]). That rule is
//! [`crate::saving::Saves`]'. A change made in a burst (a name typed, a size
//! dragged) is written once things have been still for [`SAVE_DEBOUNCE`]. An
//! unbound workspace writes nothing.

use super::{HostName, ProjectRoot, Workspace};
use crate::config::{AgentSpec, PanelLayout, WorkspaceConfig};
use crate::host::wire::HostSession;
use crate::saving::{Pending, Saves};
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;

mod git;
mod projects;
mod sessions;
pub use git::SweepProject;
pub use projects::Losses;

/// How long the workspace must sit still before a burst of changes is
/// written: long enough that a drag is one write rather than one per frame,
/// short enough that quitting right after a resize still saves it.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(600);

/// One thing in the workspace file a failed write puts back on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Thing {
    Name,
    Order,
    /// A project's pin, by its host (none for this machine) and path: a
    /// local project and a host's may share a path.
    Pin(Option<HostName>, PathBuf),
    /// A project's unattended runs switch, by its folder here.
    Unattended(PathBuf),
    /// A project's check command, by its folder here.
    Check(PathBuf),
}

/// A thing's value, as a failed write puts it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Name(String),
    /// Every project's place, as [`Workspace::root_order`] gives it.
    Order(Vec<(Option<HostName>, PathBuf)>),
    Pin {
        host: Option<HostName>,
        path: PathBuf,
        pinned: bool,
    },
    Unattended {
        path: PathBuf,
        on: bool,
    },
    Check {
        path: PathBuf,
        check: Option<String>,
    },
}

/// What a person is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// The workspace file could not be written: why. Said once, at the edge
    /// of a run of failures.
    SaveFailed(String),
    /// What was asked for cannot be done.
    Refused(Refusal),
    /// The project named was removed from the workspace.
    Removed(String),
    /// A fork of a conversation did not open, for why.
    ForkFailed(String),
    /// Ending the host session called `label` on `host` failed, for `why`;
    /// it is still there to attach to from the project page.
    EndOnHostFailed {
        host: HostName,
        label: String,
        why: String,
    },
}

/// Why a change was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A session needs a project to run in.
    NoProject,
    /// A session needs an agent to run.
    NoAgent,
}

/// A project that has left the window, on this machine or on `host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forgotten {
    pub path: PathBuf,
    pub host: Option<HostName>,
}

/// One write of the workspace file: the whole file as it stood, where it
/// goes, and which changes it carries. The front end puts its own part of
/// the file, the panels' layout, into `config` as it writes.
pub struct FileWrite {
    pub pending: Pending<Thing, Value>,
    pub config: WorkspaceConfig,
    pub dir: PathBuf,
}

/// What the front end carries out.
pub enum Effect {
    /// Let session `uid` go, and its agent with it.
    Close(u64),
    /// The offloaded folder at this path has left: its code is no longer
    /// checked for.
    ForgotOffload(PathBuf),
    /// The folder here at this path has left: let go of what is filed under
    /// its path for this machine's folder (its issues, its check field, its
    /// terminal's home, a form about it).
    ForgotHere(PathBuf),
    /// A project has left: let go of what every project has (its editor
    /// buffers, tree and shells, its place in the session lists). Its
    /// sessions were each said with [`Effect::Close`] first.
    Forgot(Forgotten),
    /// Put the active project's active session on screen.
    Show,
    /// The tree changed: whatever draws it draws it again.
    Changed,
    /// Put `value` back on screen: its write failed.
    Put(Value),
    /// Make `write`, after every write before it, and report
    /// [`Input::Written`] with its `pending`. A late answer is **recorded**:
    /// the write landed or it did not, whatever changed since.
    Write(Box<FileWrite>),
    /// What [`Effect::Write`] asks, as the window closes: written at once and
    /// waited for briefly, there being no later to hear the answer in, then
    /// reported as [`Input::WrittenOnClose`]. Its answer is **recorded**, and
    /// nothing is put back: the window is going, and should the close be
    /// called off the next change writes again.
    WriteOnClose(Box<FileWrite>),
    /// Report [`Input::Elapsed`] with `token` after `after`. A late answer is
    /// **dropped** by its token: a later change armed a later wait.
    Arm {
        token: u64,
        after: Duration,
    },
    /// Unattended runs were switched on for the project at `path`: look at it
    /// now, so a project that can never be worked says so now and not at the
    /// next tick.
    LookForRuns(PathBuf),
    /// Read the `git status` of each of `roots`, off the UI loop, and report
    /// [`Input::Swept`] with `token`. A late answer is **dropped** by its
    /// token when a later sweep supersedes it; what it read of a project that
    /// left meanwhile is dropped from it.
    Sweep {
        token: u64,
        roots: Vec<SweepProject>,
    },
    /// Read the projects' `git status` again, through [`WorkspaceDrive::sweep`]:
    /// the projects changed.
    Resweep,
    /// What was read of the projects' `git status` changed: whatever shows a
    /// branch or a change shows it again.
    GitRead,
    /// End host session `session` on `host` there, attaching as a person and
    /// asking a busy host now rather than waiting on it, and report
    /// [`Input::EndedOnHost`] with `label`, what it is called. Recorded: the
    /// session ended or did not, whatever changed since.
    EndOnHost {
        host: HostName,
        session: HostSession,
        label: String,
    },
    /// What runs on the hosts changed: whatever lists their sessions reads
    /// them again.
    ListHostSessions,
    /// The workspace in `dir` is known by `name`: remember it so.
    Remember {
        dir: PathBuf,
        name: String,
    },
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// The write [`Effect::Write`] asked for landed, or failed.
    Written {
        pending: Pending<Thing, Value>,
        result: Result<(), String>,
    },
    /// The write [`Effect::WriteOnClose`] asked for landed, or failed.
    WrittenOnClose {
        pending: Pending<Thing, Value>,
        result: Result<(), String>,
    },
    /// The wait [`Effect::Arm`] asked for ran out.
    Elapsed { token: u64 },
    /// What sweep `token` found: each repository's `git status`.
    Swept {
        token: u64,
        found: Vec<(PathBuf, crate::gitstat::GitStatus)>,
    },
    /// What [`Effect::EndOnHost`] for the host session called `label` on
    /// `host` came to.
    EndedOnHost {
        host: HostName,
        label: String,
        result: Result<(), String>,
    },
}

/// One window's workspace drive. See the module header.
pub struct WorkspaceDrive {
    workspace: Workspace,
    git: std::collections::HashMap<PathBuf, crate::gitstat::GitStatus>,
    /// The newest sweep's token.
    swept: u64,
    /// The projects the newest sweep reads.
    sweeping: Vec<SweepProject>,
    saves: Saves<Thing, Value>,
    /// The newest wait armed for a burst's write, while one waits.
    soon: Option<u64>,
    minted: u64,
    /// What each session added and not shown yet starts on.
    starts: std::collections::HashMap<u64, StartsOn>,
    /// The host session each session on a host watches, once known.
    watches: std::collections::HashMap<u64, HostSession>,
}

/// What a new session starts on, kept from when it is added until it is
/// first shown, which is what starts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartsOn {
    /// A fresh conversation.
    Fresh,
    /// The conversation kept in this folder, resumed.
    Resume(PathBuf),
    /// The host session named, read as its host keeps it.
    Watch(HostSession),
    /// The fork the front end has staged for it.
    Fork,
}

impl Deref for WorkspaceDrive {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
    }
}

impl WorkspaceDrive {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            git: Default::default(),
            swept: 0,
            sweeping: Vec::new(),
            saves: Saves::default(),
            soon: None,
            minted: 0,
            starts: Default::default(),
            watches: Default::default(),
        }
    }

    /// The panels' layout, which the front end keeps in the workspace file
    /// beside the tree. View state: nothing here reads it.
    pub fn layout_mut(&mut self) -> &mut PanelLayout {
        &mut self.workspace.layout
    }

    /// The session on screen, if there is one.
    pub fn active_uid(&self) -> Option<u64> {
        self.workspace
            .active_root()
            .and_then(ProjectRoot::active_session)
            .map(|session| session.uid)
    }

    /// Where session `uid` sits: its project's index and its own.
    pub fn locate(&self, uid: u64) -> Option<(usize, usize)> {
        self.workspace
            .roots
            .iter()
            .enumerate()
            .find_map(|(ri, root)| {
                root.sessions
                    .iter()
                    .position(|session| session.uid == uid)
                    .map(|si| (ri, si))
            })
    }

    /// Write the workspace now: a change nothing puts back, such as a project
    /// added or a size dragged. Nothing when it is bound to no folder.
    #[must_use = "its effects are to be carried"]
    pub fn save(&mut self) -> Vec<Effect> {
        self.soon = None;
        self.write().into_iter().collect()
    }

    /// Write the workspace once it has been still for [`SAVE_DEBOUNCE`].
    #[must_use = "its effects are to be carried"]
    pub fn save_soon(&mut self) -> Vec<Effect> {
        if self.workspace.storage_dir.is_none() {
            return Vec::new();
        }
        self.minted += 1;
        self.soon = Some(self.minted);
        vec![Effect::Arm {
            token: self.minted,
            after: SAVE_DEBOUNCE,
        }]
    }

    /// The write a window closing makes last, if a burst's write still
    /// waits, so the change made just before it is not lost with it.
    #[must_use = "its effects are to be carried"]
    pub fn flush(&mut self) -> Option<Effect> {
        self.soon.take()?;
        self.snapshot().map(Effect::WriteOnClose)
    }

    /// Bind the workspace to the folder `dir`, and write it there.
    #[must_use = "its effects are to be carried"]
    pub fn bind(&mut self, dir: PathBuf) -> Vec<Effect> {
        self.workspace.storage_dir = Some(dir);
        self.save()
    }

    /// Bind the workspace to no folder: it writes nothing from now on, and a
    /// burst's write still waiting is dropped. Answers the folder it had.
    pub fn unbind(&mut self) -> Option<PathBuf> {
        self.soon = None;
        self.workspace.storage_dir.take()
    }

    /// Change `thing` from `before` to `after`, as already done to the tree,
    /// and write it now (`soon` false) or after the burst.
    fn change(&mut self, thing: Thing, before: Value, after: Value, soon: bool) -> Vec<Effect> {
        self.saves.change(thing, before, after);
        match soon {
            true => self.save_soon(),
            false => self.save(),
        }
    }

    fn write(&mut self) -> Option<Effect> {
        self.snapshot().map(Effect::Write)
    }

    /// The whole file as it now stands, and where it goes; none unbound.
    fn snapshot(&mut self) -> Option<Box<FileWrite>> {
        let dir = self.workspace.storage_dir.clone()?;
        Some(Box::new(FileWrite {
            pending: self.saves.write_all(),
            config: self.workspace.to_config(),
            dir,
        }))
    }

    /// What the front end reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Written { pending, result } => {
                let landed = result.is_ok();
                let mut fx = Vec::new();
                if let Some((why, back)) = self.saves.landed(pending, result) {
                    for value in back {
                        self.put(&value);
                        fx.push(Effect::Put(value));
                    }
                    fx.push(Effect::Signal(Signal::SaveFailed(why)));
                    fx.push(Effect::Changed);
                }
                // A name is remembered once its workspace holds it, never
                // while it could yet be put back.
                if let Some(dir) = self.workspace.storage_dir.clone().filter(|_| landed) {
                    fx.push(Effect::Remember {
                        dir,
                        name: self.workspace.name.clone(),
                    });
                }
                fx
            }
            Input::Elapsed { token } => match self.soon == Some(token) {
                true => self.save(),
                false => Vec::new(),
            },
            Input::Swept { token, found } => self.swept(token, found),
            Input::WrittenOnClose { pending, result } => {
                let _ = self.saves.landed(pending, result);
                Vec::new()
            }
            Input::EndedOnHost {
                host,
                label,
                result,
            } => match result {
                Ok(()) => vec![Effect::ListHostSessions],
                Err(why) => vec![Effect::Signal(Signal::EndOnHostFailed { host, label, why })],
            },
        }
    }

    /// Put `value` back into the tree.
    fn put(&mut self, value: &Value) {
        let ws = &mut self.workspace;
        match value {
            Value::Name(name) => ws.name = name.clone(),
            Value::Order(order) => ws.put_order(order),
            Value::Pin { host, path, pinned } => {
                if let Some(root) = ws
                    .roots
                    .iter_mut()
                    .find(|root| root.host == *host && root.path == *path)
                {
                    root.pinned = *pinned;
                }
            }
            Value::Unattended { path, on } => {
                if let Some(root) = local_mut(ws, path) {
                    root.unattended = *on;
                }
            }
            Value::Check { path, check } => {
                if let Some(root) = local_mut(ws, path) {
                    root.check = check.clone();
                }
            }
        }
    }

    /// Hold project `idx` at the top of the rail, or let it go; written now,
    /// and put back alone if its write fails. Only the drawing order moves.
    pub fn toggle_pin(&mut self, idx: usize) -> Vec<Effect> {
        self.workspace.toggle_pin(idx);
        let Some(root) = self.workspace.roots.get(idx) else {
            return Vec::new();
        };
        let (host, path, pinned) = (root.host.clone(), root.path.clone(), root.pinned);
        let mut fx = self.change(
            Thing::Pin(host.clone(), path.clone()),
            Value::Pin {
                host: host.clone(),
                path: path.clone(),
                pinned: !pinned,
            },
            Value::Pin { host, path, pinned },
            false,
        );
        fx.push(Effect::Changed);
        fx
    }

    /// Move the project drawn at `from` to `to`, display positions both, as
    /// [`Workspace::move_root`] does; written now, and put back to the order
    /// last saved if its write fails. A drop the pin line clamps back onto
    /// itself moved nothing, and writes nothing.
    pub fn move_root(&mut self, from: usize, to: usize) -> Vec<Effect> {
        let before = self.workspace.root_order();
        if self.workspace.move_root(from, to).is_none() {
            return Vec::new();
        }
        let after = self.workspace.root_order();
        let mut fx = self.change(
            Thing::Order,
            Value::Order(before),
            Value::Order(after),
            false,
        );
        fx.push(Effect::Changed);
        fx
    }

    /// Switch unattended runs on or off for project `idx`; written now, and
    /// put back alone if its write fails. A run's own project is not one
    /// anybody chose, and no run reaches a folder on a host or offloaded, so
    /// neither has the switch.
    pub fn toggle_unattended(&mut self, idx: usize) -> Vec<Effect> {
        if !self
            .workspace
            .roots
            .get(idx)
            .is_some_and(|root| !root.transient && root.runs_here())
        {
            return Vec::new();
        }
        self.workspace.toggle_unattended(idx);
        let root = &self.workspace.roots[idx];
        let (path, on) = (root.path.clone(), root.unattended);
        let mut fx = Vec::new();
        if on {
            fx.push(Effect::LookForRuns(path.clone()));
        }
        fx.extend(self.change(
            Thing::Unattended(path.clone()),
            Value::Unattended {
                path: path.clone(),
                on: !on,
            },
            Value::Unattended { path, on },
            false,
        ));
        fx.push(Effect::Changed);
        fx
    }

    /// Set the check command of the project at `path`, a folder here, to
    /// `typed`, none when blank; written once the typing stops, and put back
    /// alone if its write fails. Unchanged, nothing happens.
    pub fn set_check(&mut self, path: &std::path::Path, typed: &str) -> Vec<Effect> {
        let typed = typed.trim();
        let check = (!typed.is_empty()).then(|| typed.to_string());
        let Some(root) = local_mut(&mut self.workspace, path) else {
            return Vec::new();
        };
        if root.check == check {
            return Vec::new();
        }
        let before = std::mem::replace(&mut root.check, check.clone());
        let path = path.to_path_buf();
        let mut fx = self.change(
            Thing::Check(path.clone()),
            Value::Check {
                path: path.clone(),
                check: before,
            },
            Value::Check { path, check },
            true,
        );
        fx.push(Effect::Changed);
        fx
    }

    /// Name the workspace `name`, written once the typing stops; a name
    /// blank or unchanged changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn rename(&mut self, name: &str) -> Vec<Effect> {
        let name = name.trim();
        if name.is_empty() || name == self.workspace.name {
            return Vec::new();
        }
        let before = std::mem::replace(&mut self.workspace.name, name.to_string());
        let mut fx = self.change(
            Thing::Name,
            Value::Name(before),
            Value::Name(name.to_string()),
            true,
        );
        fx.push(Effect::Changed);
        fx
    }

    /// The spec a new session runs, refused when there is none.
    fn agent(spec: Option<AgentSpec>) -> Result<AgentSpec, Refusal> {
        spec.ok_or(Refusal::NoAgent)
    }
}

/// The project at `path`, a folder on this machine.
fn local_mut<'a>(ws: &'a mut Workspace, path: &std::path::Path) -> Option<&'a mut ProjectRoot> {
    ws.roots
        .iter_mut()
        .find(|root| root.local_path() == Some(path))
}

#[cfg(test)]
mod tests;
