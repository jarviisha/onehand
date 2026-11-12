//! What the tools keep about the workers a session's agent opens: the folders
//! reserved for them, the requests each lead made, and each worker's lead.
//!
//! **One worker per folder.** A worker never opens on a folder another
//! session is open on, nor where a process of the person's already works: an
//! earlier run's worker left behind by a crash is the case this is for. A
//! session opened later, by the person or with `open_session`, is not held
//! back: it is the person's own, as one opened by hand is. The folder is
//! reserved in the same call that checks it, so two opens in flight together
//! cannot both pass.
//!
//! **A retry is not a second worker.** A lead that lost the answer to an open
//! calls again with the same request id: the open still running says so, one
//! that finished answers the same again, and only one that failed is
//! forgotten and opened afresh. The same id with other arguments is a
//! mistake, not a retry, and is refused.

use crate::worktree::occupants::{self, Occupants};
use crate::worktree::worker::{self, Repo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

impl super::drive::ControlDrive {
    /// Whether one more `open_session` may go: a session's tools (`caller`)
    /// count under its cap, `open` being the sessions they opened still
    /// open; an outside program's do not.
    pub fn admit_plain(
        &mut self,
        caller: Option<u64>,
        open: &[(u64, PathBuf)],
    ) -> Result<(), String> {
        match caller {
            Some(lead) => self.admit(lead, open),
            None => Ok(()),
        }
    }

    /// `open_session` from `caller` opened session `uid`: counted when a
    /// session's tools opened it.
    pub fn opened_plain(&mut self, caller: Option<u64>, uid: u64) {
        if let Some(lead) = caller {
            self.opened_one(lead, uid);
        }
    }
}

/// The folders workers are opening or open on, each by its canonical path.
#[derive(Default)]
pub(crate) struct Reservations {
    /// The worker holding each folder, once its session started.
    held: HashMap<PathBuf, Option<u64>>,
}

impl Reservations {
    /// Take `folder` for an open, unless an open or a worker holds it.
    pub(crate) fn take(&mut self, folder: &Path) -> Result<(), String> {
        // A folder inside a held one, or holding one, is the same files.
        if let Some(held) = self
            .held
            .keys()
            .find(|held| held.starts_with(folder) || folder.starts_with(held))
        {
            return Err(format!(
                "{} already has a worker opening or open on it",
                held.display()
            ));
        }
        self.held.insert(folder.to_path_buf(), None);
        Ok(())
    }

    /// The open on `folder` started session `uid`.
    pub(crate) fn bind(&mut self, folder: &Path, uid: u64) {
        self.held.insert(folder.to_path_buf(), Some(uid));
    }

    /// The open on `folder` failed.
    pub(crate) fn release(&mut self, folder: &Path) {
        self.held.remove(folder);
    }

    /// Let go of every folder held by a worker `live` says is gone. A
    /// window closing ends its sessions without closing them one by one, so
    /// nothing else would.
    pub(crate) fn prune(&mut self, live: impl Fn(u64) -> bool) {
        self.held.retain(|_, worker| worker.is_none_or(&live));
    }

    /// Session `uid` closed: the folder it held is free.
    pub(crate) fn release_worker(&mut self, uid: u64) {
        self.held.retain(|_, worker| *worker != Some(uid));
    }
}

/// The arguments a request is held to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asked {
    pub(crate) project: String,
    pub(crate) agent: String,
    pub(crate) prompt: Option<String>,
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) turn: Option<u64>,
    pub(crate) model: Option<String>,
    pub(crate) name: Option<String>,
}

/// What a request has come to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seen {
    /// Never seen: open it.
    New,
    /// An open of it is running, on this session once one has started.
    Opening(Option<u64>),
    /// It opened, and this was its answer.
    Answered(String),
}

/// Every lead's requests, for the run of the app.
#[derive(Default)]
pub(crate) struct Requests {
    held: HashMap<(u64, String), (Asked, Seen)>,
}

impl Requests {
    /// What `lead`'s `request` has come to, recording it as opening when it
    /// is new.
    pub(crate) fn see(&mut self, lead: u64, request: &str, asked: &Asked) -> Result<Seen, String> {
        let key = (lead, request.to_string());
        match self.held.get(&key) {
            Some((before, _)) if before != asked => {
                Err(format!("request {request} was made with other arguments"))
            }
            Some((_, seen)) => Ok(seen.clone()),
            None => {
                self.held.insert(key, (asked.clone(), Seen::Opening(None)));
                Ok(Seen::New)
            }
        }
    }

    /// The open of `request` started session `uid`.
    pub(crate) fn started(&mut self, lead: u64, request: &str, uid: u64) {
        self.set(lead, request, Seen::Opening(Some(uid)));
    }

    /// The open of `request` finished with `answer`.
    pub(crate) fn answered(&mut self, lead: u64, request: &str, answer: String) {
        self.set(lead, request, Seen::Answered(answer));
    }

    /// The open of `request` failed: the next call opens afresh.
    pub(crate) fn failed(&mut self, lead: u64, request: &str) {
        self.held.remove(&(lead, request.to_string()));
    }

    /// `lead` closed: its requests go with it.
    pub(crate) fn forget_lead(&mut self, lead: u64) {
        self.held.retain(|(held, _), _| *held != lead);
    }

    /// The sessions `lead`'s opens in flight have started.
    pub(crate) fn opening_of(&self, lead: u64) -> Vec<u64> {
        self.held
            .iter()
            .filter_map(|((held, _), (_, seen))| match seen {
                Seen::Opening(Some(uid)) if *held == lead => Some(*uid),
                Seen::Opening(_) | Seen::New | Seen::Answered(_) => None,
            })
            .collect()
    }

    /// The lead whose open in flight started session `uid`.
    pub(crate) fn lead_opening(&self, uid: u64) -> Option<u64> {
        self.held
            .iter()
            .find_map(|((lead, _), (_, seen))| match seen {
                Seen::Opening(Some(started)) if *started == uid => Some(*lead),
                Seen::Opening(_) | Seen::New | Seen::Answered(_) => None,
            })
    }

    fn set(&mut self, lead: u64, request: &str, seen: Seen) {
        if let Some(entry) = self.held.get_mut(&(lead, request.to_string())) {
            entry.1 = seen;
        }
    }
}

/// A worker's lead, and what the lead last sent it: written once the open's
/// first prompt was accepted, never before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub lead: u64,
    pub turn: Option<u64>,
    pub text: Option<String>,
    /// The worker's prompts sent once the lead's last went.
    pub sent: usize,
    /// The folders it may write besides its own.
    pub dirs: Vec<PathBuf>,
}

/// One of a lead's workers, as the person's view draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    pub uid: u64,
    /// The last turn the lead sent it.
    pub turn: Option<u64>,
    /// Its prompts sent once the lead's last went; `None` while it is still
    /// opening, before any relation.
    pub sent: Option<usize>,
}

impl Worker {
    /// Whether the person holds it: a prompt went from them since the lead's
    /// last, `chat` being the worker's own conversation. A worker still
    /// opening has had nothing from its lead yet to be taken over from.
    pub fn held(&self, chat: &crate::chat::Chat) -> bool {
        self.sent.is_some_and(|sent| chat.prompted_since(sent))
    }
}

/// Offered while the person holds the worker and its lead is open and
/// switched on; with no such lead the worker stays the person's.
pub(crate) fn offered(
    relation: Option<&Relation>,
    prompts_sent: usize,
    lead_on: bool,
    lead_open: bool,
) -> bool {
    relation.is_some_and(|relation| prompts_sent > relation.sent) && lead_on && lead_open
}

/// What a handle is to the caller.
pub(crate) enum Whose {
    /// Its session is gone.
    Closed,
    /// A worker the caller opened, open.
    Mine,
    /// Another lead's worker, or a session no lead opened.
    NotYours,
}

/// What session `uid` is to `caller`: from its relation while it lasts, the
/// lead it had once it closed (`closed`), and whether its session is `open`.
pub(crate) fn whose(
    relation: Option<&Relation>,
    closed: Option<u64>,
    caller: u64,
    open: bool,
) -> Whose {
    match relation.filter(|relation| relation.lead == caller) {
        Some(_) if open => Whose::Mine,
        Some(_) => Whose::Closed,
        None if closed == Some(caller) && !open => Whose::Closed,
        None => Whose::NotYours,
    }
}

/// The folder a session's agent named.
pub enum Target {
    /// A project of the window, by the path the window knows it by.
    Project(PathBuf),
    /// An absolute path that is no project of the window.
    Worktree(PathBuf),
}

/// What the background read of an open found.
pub struct Resolved {
    /// The folder, canonical: what is reserved.
    pub(crate) folder: PathBuf,
    /// The path the session is started on: the window's own for a folder
    /// that is one of its roots, else the canonical one.
    pub(crate) root: PathBuf,
    /// Every window root whose canonical path is `folder`.
    pub(crate) same: Vec<PathBuf>,
    /// What the worker may write besides its folder.
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) occupants: Occupants,
    /// Every window's project, by the path the window knows it by and its
    /// canonical one.
    pub(crate) canonical: Vec<(PathBuf, PathBuf)>,
}

/// Check the folder and the further folders, and read what works in it.
/// Blocking.
pub fn resolve_blocking(
    target: Target,
    roots: &[PathBuf],
    everywhere: &[PathBuf],
    dirs: &[PathBuf],
    home: Option<&Path>,
    skip: &[u32],
) -> Result<Resolved, String> {
    let repo = match &target {
        Target::Worktree(path) => worker::worktree_of_blocking(path, roots)?,
        Target::Project(path) => {
            let top =
                std::fs::canonicalize(path).map_err(|why| format!("{}: {why}", path.display()))?;
            // A project that is no checkout is its own folder and nothing
            // more; git failing to say refuses the open in its words.
            match worker::repo_of_blocking(&top)? {
                Some(repo) => Repo {
                    top: top.clone(),
                    ..repo
                },
                None => Repo {
                    common: top.join(".git"),
                    top,
                },
            }
        }
    };
    let writable = worker::writable_blocking(dirs, &repo, home)?;
    let same: Vec<PathBuf> = roots
        .iter()
        .filter(|root| std::fs::canonicalize(root).is_ok_and(|real| real == repo.top))
        .cloned()
        .collect();
    let canonical = everywhere
        .iter()
        .filter_map(|root| Some((root.clone(), std::fs::canonicalize(root).ok()?)))
        .collect();
    let root = match &target {
        Target::Project(path) => path.clone(),
        Target::Worktree(_) => same.first().cloned().unwrap_or_else(|| repo.top.clone()),
    };
    Ok(Resolved {
        canonical,
        occupants: occupants::occupants_blocking(&repo.top, skip, std::process::id()),
        dirs: worker::worker_dirs(&repo, writable),
        folder: repo.top,
        root,
        same,
    })
}

/// The roots of `roots` (as the window knows it, canonical, sessions open)
/// with a session on `folder` or on a folder inside it: a worker would share
/// their files. A root holding `folder` does not count: a lead in the main
/// checkout with its workers' worktrees beneath it is the usual layout.
pub(crate) fn sessions_in(folder: &Path, roots: &[(PathBuf, PathBuf, usize)]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|(_, real, count)| *count > 0 && real.starts_with(folder))
        .map(|(path, _, _)| path.clone())
        .collect()
}

#[cfg(test)]
mod tests;
