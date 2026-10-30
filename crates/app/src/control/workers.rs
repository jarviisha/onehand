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
    pub(super) request: Option<String>,
    pub(super) turn: Option<u64>,
    pub(super) model: Option<String>,
    pub(super) name: Option<String>,
}

impl Open {
    /// The arguments its request is held to.
    fn asked(&self) -> requests::Asked {
        requests::Asked {
            project: self.project.clone(),
            agent: self.agent.clone(),
            prompt: self.prompt.clone(),
            dirs: self.dirs.clone(),
            turn: self.turn,
            model: self.model.clone(),
            name: self.name.clone(),
        }
    }
}

/// Carry out `open`: at once for an outside program, and for a session's
/// agent once its folder has been checked off the UI thread.
pub(super) fn open(
    open: Open,
    window: gpui::AnyWindowHandle,
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
        if let Some(why) = worker_only(&open) {
            return Err(why);
        }
        let dir = named?;
        let name = spec.name.clone();
        let (_, session) = start(&shell, dir, spec, Vec::new(), None, cx)?;
        return Ok(answered(&open, &name, &session, &[], cx));
    };
    let deadline = std::time::Instant::now() + opening::OPEN_DEADLINE;
    let request = open
        .request
        .clone()
        .ok_or("`request` is required: your id for this open, so a retry opens nothing twice")?;
    match (&open.prompt, open.turn) {
        (Some(_), None) => return Err("`turn` is required with a prompt".to_string()),
        (None, Some(_)) => return Err("`turn` goes with a prompt; there is none".to_string()),
        _ => {}
    }
    match sessions().requests.see(caller.0, &request, &open.asked())? {
        requests::Seen::New => {}
        requests::Seen::Opening(uid) => {
            return Ok(match uid {
                Some(uid) => format!(
                    "{} is still opening; call again with the same request",
                    control::open::handle(&super::RUN, uid)
                ),
                None => {
                    "the session is still opening; call again with the same request".to_string()
                }
            });
        }
        requests::Seen::Answered(answer) => return Ok(answer),
    }
    let mode = crate::unattended::worker_mode(&spec.name, cx);
    let target = match named {
        Ok(dir) => Target::Project(dir),
        Err(_) if Path::new(&open.project).is_absolute() => {
            Target::Worktree(PathBuf::from(&open.project))
        }
        Err(why) => {
            sessions().requests.failed(caller.0, &request);
            return Err(why);
        }
    };
    let skip: Vec<u32> = std::iter::once(std::process::id())
        .chain(shell.read(cx).shell_pids(cx))
        .collect();
    let roots: Vec<PathBuf> = projects.iter().map(|(_, path)| path.clone()).collect();
    // Every window's projects, to find a session on the folder or inside it.
    let everywhere: Vec<PathBuf> = crate::state::Shared::global(cx)
        .windows
        .iter()
        .filter_map(|window| window.shell.upgrade())
        .flat_map(|shell| shell.read(cx).roots_with_sessions())
        .map(|(path, _)| path)
        .collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = open.dirs.clone();
    let name = open.name.clone();
    let shell = shell.downgrade();
    let answer = answer.take();
    cx.spawn(async move |cx| {
        let resolved = cx
            .background_executor()
            .spawn(async move {
                resolve_blocking(target, &roots, &everywhere, &dirs, home.as_deref(), &skip)
            })
            .await;
        let started = match resolved {
            // The window may have closed while the folder was read.
            Ok(resolved) => cx.update(|cx| match shell.upgrade() {
                Some(strong) => finish(&caller, resolved, spec, name, &strong, cx),
                None => Err("the window closed".to_string()),
            }),
            Err(why) => Err(why),
        };
        let outcome = match started {
            Ok(started) => {
                sessions().requests.started(caller.0, &request, started.uid);
                let opening = opening::Opening {
                    lead: caller.0,
                    request: request.clone(),
                    uid: started.uid,
                    session: started.session,
                    window,
                    root: started.root,
                    added: started.added,
                    mode,
                    model: open.model,
                    prompt: open.prompt,
                    turn: open.turn,
                    dirs: started.dirs,
                    deadline,
                };
                opening::wait_open(opening, cx).await
            }
            Err(why) => {
                sessions().requests.failed(caller.0, &request);
                Err(why)
            }
        };
        if let Some(answer) = answer {
            let _ = answer.send(outcome);
        }
    })
    .detach();
    Ok(String::new())
}

/// Why an outside program's `open` is refused: every argument it named
/// that only a session's agent may, or `None` when it named none.
fn worker_only(open: &Open) -> Option<String> {
    let named: Vec<String> = [
        ("dirs", !open.dirs.is_empty()),
        ("request", open.request.is_some()),
        ("turn", open.turn.is_some()),
        ("model", open.model.is_some()),
        ("name", open.name.is_some()),
    ]
    .iter()
    .filter(|(_, named)| *named)
    .map(|(name, _)| format!("`{name}`"))
    .collect();
    (!named.is_empty()).then(|| {
        format!(
            "{} {} for a session's agent, not an outside program",
            named.join(", "),
            if named.len() == 1 { "is" } else { "are" }
        )
    })
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
    /// Every window's project, by the path the window knows it by and its
    /// canonical one.
    canonical: Vec<(PathBuf, PathBuf)>,
}

/// Check the folder and the further folders, and read what works in it.
/// Blocking.
fn resolve_blocking(
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

/// A worker's session, started and not yet up.
struct Started {
    uid: u64,
    session: gpui::WeakEntity<ChatSession>,
    root: PathBuf,
    /// Whether the open added `root` to the window.
    added: bool,
    dirs: Vec<PathBuf>,
}

/// Back on the UI thread: refuse a folder another session or a process
/// works in, or the caller over its cap; else reserve it and start.
fn finish(
    caller: &(u64, PathBuf),
    resolved: Resolved,
    spec: onehand_core::config::AgentSpec,
    name: Option<String>,
    shell: &Entity<Shell>,
    cx: &mut App,
) -> Result<Started, String> {
    // The lead may have gone while the folder was read: nothing is started
    // for nobody, nor counted under a cap a forgotten lead no longer has.
    if !opening::lead_here(caller.0, cx) {
        return Err("the lead went before its worker was ready".to_string());
    }
    let counts: Vec<(PathBuf, usize)> = crate::state::Shared::global(cx)
        .windows
        .iter()
        .filter_map(|window| window.shell.upgrade())
        .flat_map(|shell| shell.read(cx).roots_with_sessions())
        .collect();
    let roots: Vec<(PathBuf, PathBuf, usize)> = resolved
        .canonical
        .iter()
        .map(|(path, real)| {
            let count = counts
                .iter()
                .filter(|(root, _)| root == path)
                .map(|(_, count)| count)
                .sum();
            (path.clone(), real.clone(), count)
        })
        .collect();
    if let Some(root) = sessions_in(&resolved.folder, &roots).first() {
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
    let added = resolved.same.is_empty();
    match start(
        shell,
        resolved.root.clone(),
        spec,
        resolved.dirs.clone(),
        Some(caller),
        cx,
    ) {
        Ok((uid, session)) => {
            sessions().reserved.bind(&resolved.folder, uid);
            // Named for what it works on, else for its folder, never for its
            // prompt, which is the lead's instructions. No conversation file
            // exists yet; the first turn's end writes the given name with the
            // rest.
            let folder = resolved.root.file_name().map_or_else(
                || resolved.root.display().to_string(),
                |last| last.to_string_lossy().into_owned(),
            );
            session.update(cx, |session, cx| {
                session.chat.stand_in_title = Some(folder);
                session.chat.custom_title = name;
                cx.notify();
            });
            Ok(Started {
                uid,
                session: session.downgrade(),
                root: resolved.root,
                added,
                dirs: resolved.dirs,
            })
        }
        Err(why) => {
            sessions().reserved.release(&resolved.folder);
            Err(why)
        }
    }
}

/// The roots of `roots` (as the window knows it, canonical, sessions open)
/// with a session on `folder` or on a folder inside it: a worker would share
/// their files. A root holding `folder` does not count: a lead in the main
/// checkout with its workers' worktrees beneath it is the usual layout.
fn sessions_in(folder: &Path, roots: &[(PathBuf, PathBuf, usize)]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|(_, real, count)| *count > 0 && real.starts_with(folder))
        .map(|(path, _, _)| path.clone())
        .collect()
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

mod opening;
pub(super) mod requests;
pub(super) mod state;
pub(super) mod turns;
pub(super) mod whose;

#[cfg(test)]
mod tests;
