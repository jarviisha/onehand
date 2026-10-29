//! `open_session`, carried out: a session on a project of the window, or,
//! for a session's agent, a worker on a worktree of one of their
//! repositories, with the further folders it may write.
//!
//! **One worker per folder.** A worker never shares its folder with another
//! session, nor with a process of the person's that already works in it: an
//! earlier run's worker left behind by a crash is the case this is for. The
//! folder is reserved on the UI thread in the same update that checks it,
//! which nothing else runs beside, so two opens in flight together cannot
//! both pass.

use super::{Answer, OPENED_CAP, sessions};
use crate::chat::session::ChatSession;
use crate::shell::Shell;
use gpui::{App, Entity};
use onehand_core::chat::model::Link;
use onehand_core::control;
use onehand_core::worktree::occupants::{self, Occupants};
use onehand_core::worktree::worker::{self, Repo};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The folders workers are opening or open on, each by its canonical path.
#[derive(Default)]
pub(super) struct Reservations {
    /// The worker holding each folder, once its session started.
    held: HashMap<PathBuf, Option<u64>>,
}

impl Reservations {
    /// Take `folder` for an open, unless an open or a worker holds it.
    pub(super) fn take(&mut self, folder: &Path) -> Result<(), String> {
        if self.held.contains_key(folder) {
            return Err(format!(
                "{} already has a worker opening or open on it",
                folder.display()
            ));
        }
        self.held.insert(folder.to_path_buf(), None);
        Ok(())
    }

    /// The open on `folder` started session `uid`.
    pub(super) fn bind(&mut self, folder: &Path, uid: u64) {
        self.held.insert(folder.to_path_buf(), Some(uid));
    }

    /// The open on `folder` failed.
    pub(super) fn release(&mut self, folder: &Path) {
        self.held.remove(folder);
    }

    /// Let go of every folder held by a worker `live` says is gone. A
    /// window closing ends its sessions without closing them one by one, so
    /// nothing else would.
    pub(super) fn prune(&mut self, live: impl Fn(u64) -> bool) {
        self.held.retain(|_, worker| worker.is_none_or(&live));
    }

    /// Session `uid` closed: the folder it held is free.
    pub(super) fn release_worker(&mut self, uid: u64) {
        self.held.retain(|_, worker| *worker != Some(uid));
    }
}

/// What `open_session` was asked.
pub(super) struct Open {
    pub(super) caller: Option<(u64, PathBuf)>,
    pub(super) project: String,
    pub(super) agent: String,
    pub(super) prompt: Option<String>,
    pub(super) dirs: Vec<PathBuf>,
}

/// Carry out `open`: at once for an outside program, and for a session's
/// agent once its folder has been checked off the UI thread.
pub(super) fn open(
    open: Open,
    shell: Entity<Shell>,
    projects: &[(String, PathBuf)],
    answer: &mut Option<Answer>,
    cx: &mut App,
) -> Result<String, String> {
    let spec = onehand_core::config::agent_or_first(
        &crate::state::Shared::global(cx).agents,
        Some(&open.agent),
    )
    .cloned()
    .ok_or_else(|| format!("no agent named {}; list_agents names them", open.agent))?;
    let named = control::pick_project(projects, &open.project).map(Path::to_path_buf);
    let Some(caller) = open.caller.clone() else {
        // An outside program opens on a project of the window, as it always
        // has; a worker's folders are for a session's agent.
        if !open.dirs.is_empty() {
            return Err("`dirs` is for a session's agent, not an outside program".to_string());
        }
        let dir = named?;
        let name = spec.name.clone();
        let (_, session) = start(&shell, dir, spec, Vec::new(), None, cx)?;
        return Ok(answered(&open, &name, &session, &[], cx));
    };
    let target = match named {
        Ok(dir) => Target::Project(dir),
        Err(_) if Path::new(&open.project).is_absolute() => {
            Target::Worktree(PathBuf::from(&open.project))
        }
        Err(why) => return Err(why),
    };
    let skip: Vec<u32> = std::iter::once(std::process::id())
        .chain(shell.read(cx).shell_pids(cx))
        .collect();
    let roots: Vec<PathBuf> = projects.iter().map(|(_, path)| path.clone()).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = open.dirs.clone();
    let shell = shell.downgrade();
    let answer = answer.take();
    cx.spawn(async move |cx| {
        let resolved = cx
            .background_executor()
            .spawn(async move { resolve_blocking(target, &roots, &dirs, home.as_deref(), &skip) })
            .await;
        let outcome = match resolved {
            // The window may have closed while the folder was read.
            Ok(resolved) => cx.update(|cx| match shell.upgrade() {
                Some(shell) => finish(&open, caller, resolved, spec, &shell, cx),
                None => Err("the window closed".to_string()),
            }),
            Err(why) => Err(why),
        };
        if let Some(answer) = answer {
            let _ = answer.send(outcome);
        }
    })
    .detach();
    Ok(String::new())
}

/// The folder a session's agent named.
enum Target {
    /// A project of the window, by the path the window knows it by.
    Project(PathBuf),
    /// An absolute path that is no project of the window.
    Worktree(PathBuf),
}

/// What the background read of an open found.
struct Resolved {
    /// The folder, canonical: what is reserved.
    folder: PathBuf,
    /// The path the session is started on: the window's own for a folder
    /// that is one of its roots, else the canonical one.
    root: PathBuf,
    /// Every window root whose canonical path is `folder`.
    same: Vec<PathBuf>,
    /// What the worker may write besides its folder.
    dirs: Vec<PathBuf>,
    occupants: Occupants,
}

/// Check the folder and the further folders, and read what works in it.
/// Blocking.
fn resolve_blocking(
    target: Target,
    roots: &[PathBuf],
    dirs: &[PathBuf],
    home: Option<&Path>,
    skip: &[u32],
) -> Result<Resolved, String> {
    let repo = match &target {
        Target::Worktree(path) => worker::worktree_of_blocking(path, roots)?,
        Target::Project(path) => {
            let top =
                std::fs::canonicalize(path).map_err(|why| format!("{}: {why}", path.display()))?;
            // A project that is no checkout is its own folder and nothing more.
            worker::repo_of_blocking(&top)
                .map(|repo| Repo {
                    top: top.clone(),
                    ..repo
                })
                .unwrap_or_else(|_| Repo {
                    common: top.join(".git"),
                    top,
                })
        }
    };
    let writable = worker::writable_blocking(dirs, &repo, home)?;
    let same: Vec<PathBuf> = roots
        .iter()
        .filter(|root| std::fs::canonicalize(root).is_ok_and(|real| real == repo.top))
        .cloned()
        .collect();
    let root = match &target {
        Target::Project(path) => path.clone(),
        Target::Worktree(_) => same.first().cloned().unwrap_or_else(|| repo.top.clone()),
    };
    Ok(Resolved {
        occupants: occupants::occupants_blocking(&repo.top, skip),
        dirs: worker::worker_dirs(&repo, writable),
        folder: repo.top,
        root,
        same,
    })
}

/// Back on the UI thread: refuse a folder another session or a process
/// works in, or the caller over its cap; else reserve it and start.
fn finish(
    open: &Open,
    caller: (u64, PathBuf),
    resolved: Resolved,
    spec: onehand_core::config::AgentSpec,
    shell: &Entity<Shell>,
    cx: &mut App,
) -> Result<String, String> {
    let taken = shell.read(cx).roots_with_sessions();
    if let Some((root, _)) = taken
        .iter()
        .find(|(root, count)| *count > 0 && resolved.same.contains(root))
    {
        return Err(format!(
            "{} already has a session open on it; a worker never shares its folder",
            root.display()
        ));
    }
    let live: Vec<u64> = crate::state::Shared::global(cx)
        .windows
        .iter()
        .filter_map(|window| window.shell.upgrade())
        .flat_map(|shell| shell.read(cx).session_uids())
        .collect();
    let mut held = sessions();
    held.reserved.prune(|uid| live.contains(&uid));
    held.reserved.take(&resolved.folder)?;
    drop(held);
    let refused = match &resolved.occupants {
        Occupants::None => None,
        Occupants::Found(found) => Some(format!(
            "{} processes work in {}: {}; the person has to stop them before a worker opens there",
            found.len(),
            resolved.folder.display(),
            found.join(", ")
        )),
        Occupants::Unknown(unread) => Some(format!(
            "could not tell whether anything works in {}: {}",
            resolved.folder.display(),
            unread.join(", ")
        )),
    };
    if let Some(why) = refused {
        sessions().reserved.release(&resolved.folder);
        return Err(why);
    }
    let name = spec.name.clone();
    match start(
        shell,
        resolved.root,
        spec,
        resolved.dirs.clone(),
        Some(&caller),
        cx,
    ) {
        Ok((uid, session)) => {
            sessions().reserved.bind(&resolved.folder, uid);
            if let Some(prompt) = open.prompt.clone() {
                send_when_connected(session.clone(), prompt, cx);
            }
            Ok(answered(open, &name, &session, &resolved.dirs, cx))
        }
        Err(why) => {
            sessions().reserved.release(&resolved.folder);
            Err(why)
        }
    }
}

/// Start `spec` on `dir` off screen, within the caller's cap; the first
/// prompt of an outside program's open goes from here too.
fn start(
    shell: &Entity<Shell>,
    dir: PathBuf,
    spec: onehand_core::config::AgentSpec,
    dirs: Vec<PathBuf>,
    caller: Option<&(u64, PathBuf)>,
    cx: &mut App,
) -> Result<(u64, Entity<ChatSession>), String> {
    if let Some((uid, _)) = caller {
        let open: Vec<u64> = {
            let shell = shell.read(cx);
            sessions()
                .opened
                .get(uid)
                .into_iter()
                .flatten()
                .copied()
                .filter(|opened| shell.session_project(*opened).is_some())
                .collect()
        };
        if open.len() >= OPENED_CAP {
            let shell = shell.read(cx);
            let said: Vec<String> = open
                .iter()
                .map(|opened| {
                    let at = shell.session_project(*opened).unwrap_or_default();
                    format!("session {opened} on {}", at.display())
                })
                .collect();
            return Err(format!(
                "this session's tools already have {OPENED_CAP} sessions open: {}; \
                 one has to close first",
                said.join(", ")
            ));
        }
        sessions().opened.insert(*uid, open);
    }
    let started = shell
        .update(cx, |shell, cx| shell.run_unattended(dir, spec, dirs, cx))
        .ok_or("the session did not start")?;
    if let Some((uid, _)) = caller {
        sessions().opened.entry(*uid).or_default().push(started.0);
    }
    Ok(started)
}

/// What an open answers: what it opened, and the folders it may write.
fn answered(
    open: &Open,
    name: &str,
    session: &Entity<ChatSession>,
    dirs: &[PathBuf],
    cx: &mut App,
) -> String {
    if open.caller.is_none()
        && let Some(prompt) = open.prompt.clone()
    {
        send_when_connected(session.clone(), prompt, cx);
    }
    let mut said = format!(
        "Opened a session with {name} on {}. It is in the rail, not shown",
        open.project
    );
    if !dirs.is_empty() {
        let dirs: Vec<String> = dirs.iter().map(|dir| dir.display().to_string()).collect();
        said.push_str(&format!("\ndirs: {}", dirs.join(", ")));
    }
    said
}

/// Send `prompt` as `session`'s first message once its agent is up, as a
/// person would after watching it connect; dropped if it never comes up.
fn send_when_connected(session: Entity<ChatSession>, prompt: String, cx: &mut App) {
    // A warm adapter can be up before anything is watching for it.
    if session.read(cx).chat.link == Link::Connected {
        session.update(cx, |session, cx| session.submit(&prompt, &[], cx));
        return;
    }
    let held: Rc<RefCell<Option<gpui::Subscription>>> = Rc::default();
    let hold = held.clone();
    *held.borrow_mut() = Some(cx.observe(&session, move |session, cx| {
        let link = session.read(cx).chat.link;
        if link == Link::Connecting {
            return;
        }
        let (prompt, hold) = (prompt.clone(), hold.clone());
        // Deferred: the observer runs while the session's change is still
        // being flushed. Once only: dropping the subscription ends it.
        // The first change to land takes it; any queued behind it finds it gone.
        cx.defer(move |cx| {
            let first = hold.borrow_mut().take().is_some();
            if first && link == Link::Connected {
                session.update(cx, |session, cx| session.submit(&prompt, &[], cx));
            }
        });
    }));
}

pub(super) mod requests;

#[cfg(test)]
mod tests;
