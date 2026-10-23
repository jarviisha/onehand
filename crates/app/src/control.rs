//! onehand's tools, carried out.
//!
//! What a tool may do and how a message is answered is core's `control`; the
//! socket's side of the conversation runs on the listener's runtime and sends
//! only a [`Call`] across, which [`perform`] carries out on the window in
//! front and answers with the text the caller reads.

use crate::instance::{Incoming, next_line};
use crate::state::Shared;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use gpui::{App, Window};
use onehand_core::chat::model::Link;
use onehand_core::config::ControlConfig;
use onehand_core::control::{self, Call, Conn, Handled, Peer, Standing};
use onehand_core::task::Group;
use onehand_core::workflow::Brief;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Component, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use tokio::io::{AsyncWriteExt as _, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod start;

pub(crate) use start::{Started, start_task};
use start::{start, template};

/// Whether a program outside onehand may use the tools. Read by the
/// listener on every message it answers, so it is shared with that runtime
/// rather than kept in a global only the UI thread reaches.
static OUTSIDE: AtomicBool = AtomicBool::new(false);

/// Put the config's answer in force, before anything listens.
pub(crate) fn boot(cfg: &ControlConfig) {
    OUTSIDE.store(cfg.outside, Ordering::Relaxed);
}

/// Whether outside control is on.
pub(crate) fn outside() -> bool {
    OUTSIDE.load(Ordering::Relaxed)
}

/// Turn outside control on or off, at once and for the next message of a
/// client already connected, then write it; a write that fails puts it back.
pub(crate) fn set_outside(on: bool, window: &mut Window, cx: &mut App) {
    let before = outside();
    if before == on {
        return;
    }
    put_outside(on, window, cx);
    crate::saves::write_config(
        "control-outside",
        "Saving outside control",
        move |cfg| cfg.control.outside = on,
        Some(crate::saves::Change {
            before,
            after: on,
            put: put_outside,
        }),
        window,
        cx,
    );
}

fn put_outside(on: bool, _: &mut Window, cx: &mut App) {
    OUTSIDE.store(on, Ordering::Relaxed);
    cx.refresh_windows();
}

/// What a connection may do now: read on every message it sends.
fn standing(peer: &Peer) -> Standing {
    match peer {
        Peer::Outside if outside() => Standing::On,
        Peer::Outside => Standing::Refused(
            "outside control is off: switch it on in Settings ▸ Connections".to_string(),
        ),
        Peer::Session(token) => {
            let sessions = sessions();
            match sessions.tokens.get(token) {
                None => Standing::Refused("no such session".to_string()),
                Some(Some(uid)) if sessions.on.contains(uid) => Standing::On,
                Some(Some(uid)) if sessions.only.contains_key(uid) => {
                    Standing::Only(sessions.only[uid].clone())
                }
                // Not claimed yet (a warm adapter's), or switched off.
                Some(_) => Standing::Off,
            }
        }
    }
}

/// Every session's standing, shared with the listener's runtime, which reads
/// it on every message a session's agent sends.
#[derive(Default)]
struct Sessions {
    /// Each token given out, and the session that holds it once one does.
    tokens: HashMap<String, Option<u64>>,
    /// The sessions switched on.
    on: HashSet<u64>,
    /// Where each token's connections are written, to tell them the tools
    /// changed.
    conns: HashMap<String, Vec<UnboundedSender<String>>>,
    /// The sessions each session's tools opened.
    opened: HashMap<u64, Vec<u64>>,
    /// The labels an issue each session's agent writes gets: its schedule's.
    labels: HashMap<u64, Vec<String>>,
    /// The tools each session not switched on may use: its schedule's.
    only: HashMap<u64, Vec<String>>,
}

static SESSIONS: LazyLock<Mutex<Sessions>> = LazyLock::new(Mutex::default);

fn sessions() -> std::sync::MutexGuard<'static, Sessions> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A session's token: what its agent's proxy says it speaks for. Minted when
/// the agent is started, which for a warm one is before any session holds
/// it, and let go with whatever holds it.
pub(crate) struct Token(String);

impl Token {
    pub(crate) fn mint() -> Self {
        let token = control::new_token();
        sessions().tokens.insert(token.clone(), None);
        Self(token)
    }

    /// Held by session `uid` from now on.
    pub(crate) fn claim(&self, uid: u64) {
        sessions().tokens.insert(self.0.clone(), Some(uid));
    }

    /// The `mcpServers` entry that gives an agent this token: onehand itself,
    /// as the proxy to its socket. Nothing when onehand cannot say where its
    /// own executable is.
    pub(crate) fn servers(&self) -> Vec<serde_json::Value> {
        let socket = onehand_core::instance::socket_path(&onehand_core::config::config_dir());
        std::env::current_exe()
            .map(|exe| vec![control::proxy_entry(&exe, &socket, &self.0)])
            .unwrap_or_default()
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        let mut sessions = sessions();
        sessions.tokens.remove(&self.0);
        sessions.conns.remove(&self.0);
    }
}

/// Whether session `uid` can be switched on at all: not when it is an
/// unattended run's, whose agent works with nobody watching.
pub(crate) fn offered(uid: u64, cx: &App) -> bool {
    crate::task::shown(uid, cx)
        .and_then(|shown| crate::task::task(&shown.task, cx))
        .is_none_or(|task| task.issue().is_none())
}

/// Whether session `uid`'s agent may use the tools.
pub(crate) fn switched_on(uid: u64) -> bool {
    sessions().on.contains(&uid)
}

/// Let session `uid`'s agent use the tools, or stop it, and tell every
/// connection it has that the list changed.
pub(crate) fn switch(uid: u64, on: bool) {
    let mut sessions = sessions();
    let changed = match on {
        true => sessions.on.insert(uid),
        false => sessions.on.remove(&uid),
    };
    if changed {
        tell_changed(&mut sessions, uid);
    }
}

/// Tell every connection of session `uid` that its tools changed, dropping
/// the ones whose other end has gone.
fn tell_changed(sessions: &mut Sessions, uid: u64) {
    let Sessions { tokens, conns, .. } = sessions;
    let note = control::tools_changed();
    for (token, held) in tokens.iter() {
        if *held != Some(uid) {
            continue;
        }
        if let Some(senders) = conns.get_mut(token) {
            senders.retain(|sender| sender.unbounded_send(note.clone()).is_ok());
        }
    }
}

/// Give session `uid` what its task was given: the tools it may use while
/// nobody switches it on, and the labels its issues get. A task given
/// nothing changes nothing.
pub(crate) fn grant(uid: u64, grant: &onehand_core::task::Grant) {
    let mut sessions = sessions();
    // An agent may have listed its tools before the session held its token
    // (a warm adapter's): told, it asks again.
    if !grant.tools.is_empty()
        && sessions.only.insert(uid, grant.tools.clone()).as_ref() != Some(&grant.tools)
    {
        tell_changed(&mut sessions, uid);
    }
    if !grant.labels.is_empty() {
        sessions.labels.insert(uid, grant.labels.clone());
    }
}

/// Session `uid` is closed: its switch and what its tools opened go with it.
pub(crate) fn forget(uid: u64) {
    let mut sessions = sessions();
    sessions.on.remove(&uid);
    sessions.opened.remove(&uid);
    sessions.labels.remove(&uid);
    sessions.only.remove(&uid);
}

/// Said when a call's answer cannot come, because the app is going.
const CLOSING: &str = "onehand is closing";

/// Carry one MCP conversation, `first` its first line, until the client hangs
/// up: each line answered by core, each call performed on the UI thread and
/// its answer written back before the next line is read.
pub(crate) async fn converse(
    first: String,
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
    tx: UnboundedSender<Incoming>,
) {
    // One writer for the replies and for what the session is told unasked,
    // so the two never interleave inside a line.
    let (out, mut lines) = futures::channel::mpsc::unbounded::<String>();
    tokio::spawn(async move {
        use futures::StreamExt as _;
        while let Some(line) = lines.next().await {
            if writer
                .write_all(format!("{line}\n").as_bytes())
                .await
                .is_err()
            {
                return;
            }
        }
    });
    let mut conn = Conn::default();
    let mut told = false;
    let mut line = Some(first);
    while let Some(text) = line.take() {
        let reply = match control::handle(&mut conn, &text, standing) {
            Handled::Reply(reply) => Some(reply),
            Handled::Quiet => None,
            Handled::Perform { id, call } => {
                let peer = conn.peer().cloned().unwrap_or(Peer::Outside);
                let (answer, answered) = oneshot::channel();
                let outcome = match tx.unbounded_send(Incoming::Call(peer, call, answer)) {
                    Ok(()) => answered.await.unwrap_or_else(|_| Err(CLOSING.to_string())),
                    Err(_) => Err(CLOSING.to_string()),
                };
                Some(control::answered(&id, outcome))
            }
        };
        // A session's connection, once it has said whose it is, is told when
        // that session's switch changes.
        if let (false, Some(Peer::Session(token))) = (told, conn.peer()) {
            told = true;
            sessions()
                .conns
                .entry(token.clone())
                .or_default()
                .push(out.clone());
        }
        if let Some(reply) = reply
            && out.unbounded_send(reply).is_err()
        {
            break;
        }
        line = next_line(&mut reader).await;
    }
    // For every clone, the one kept to tell the session too: the writer then
    // lets the socket go, so the proxy on the other end hears it close.
    out.close_channel();
}

/// Carry out `call` in the window in front, and answer what came of it in
/// the words the caller reads. Never from inside a shell's update: it runs
/// from the socket's own task.
pub(crate) fn perform(peer: Peer, call: Call, answer: Answer, cx: &mut App) {
    let mut answer = Some(answer);
    let outcome = perform_now(peer, call, &mut answer, cx);
    // Still here: the call was done, or refused, at once. A start that goes on
    // took it, and answers once its task has landed.
    if let Some(answer) = answer {
        let _ = answer.send(outcome);
    }
}

/// Where a call's answer goes.
pub(crate) type Answer = oneshot::Sender<Result<String, String>>;

/// How many sessions one session's tools may have open at once.
const OPENED_CAP: usize = 4;

fn perform_now(
    peer: Peer,
    call: Call,
    answer: &mut Option<Answer>,
    cx: &mut App,
) -> Result<String, String> {
    // A session's call lands in its own window, about its own project; an
    // outside program's in the window in front.
    let (handle, shell, caller) = match &peer {
        Peer::Outside => {
            let (handle, shell) = crate::state::front(cx).ok_or("no onehand window is open")?;
            (handle, shell, None)
        }
        Peer::Session(token) => {
            let uid = sessions()
                .tokens
                .get(token)
                .copied()
                .flatten()
                .ok_or("no such session")?;
            let windows: Vec<_> = Shared::global(cx)
                .windows
                .iter()
                .filter_map(|w| Some((w.handle, w.shell.upgrade()?)))
                .collect();
            windows
                .into_iter()
                .find_map(|(handle, shell)| {
                    let project = shell.read(cx).session_project(uid)?;
                    Some((handle, shell, Some((uid, project))))
                })
                .ok_or("this session is no longer open")?
        }
    };
    let projects: Vec<(String, PathBuf)> = shell
        .read(cx)
        .remote_roots()
        .into_iter()
        .map(|(path, name)| (name, path))
        .collect();
    match call {
        Call::StartWorkflow {
            project,
            workflow,
            title,
            brief,
        } => {
            let root = control::pick_project(&projects, &project)?.to_path_buf();
            let template = template(&workflow, cx)?;
            let brief = Brief {
                title: title.trim().to_string(),
                body: brief.unwrap_or_default().trim().to_string(),
                instructions: None,
            };
            let answer = answer.take();
            cx.spawn(async move |cx| {
                let outcome = start(handle, shell, root, template, brief, cx).await;
                if let Some(answer) = answer {
                    let _ = answer.send(outcome);
                }
            })
            .detach();
            Ok(String::new())
        }
        Call::StartIssue {
            project,
            issue,
            workflow,
        } => {
            let root = control::pick_project(&projects, &project)?.to_path_buf();
            let workflow = workflow
                .or_else(|| Some(crate::unattended::workflows(cx).0).filter(|id| !id.is_empty()))
                .ok_or("name a workflow; list_workflows gives them")?;
            let template = template(&workflow, cx)?;
            let issues = shell.read(cx).issues_file(&root);
            let answer = answer.take();
            cx.spawn(async move |cx| {
                let outcome = async {
                    let at = root.clone();
                    let (rows, ..) = cx
                        .background_executor()
                        .spawn(async move {
                            onehand_core::unattended::pickable_one_blocking(
                                crate::features::connectors(),
                                &at,
                                issues,
                                issue,
                            )
                        })
                        .await?;
                    let (tracker, row) = rows
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("no issue {issue} in this project"))?;
                    let brief = onehand_core::unattended::brief_supervised(&tracker, &row.issue);
                    start(handle, shell, root, template, brief, cx).await
                }
                .await;
                if let Some(answer) = answer {
                    let _ = answer.send(outcome);
                }
            })
            .detach();
            Ok(String::new())
        }
        Call::ListIssues { project } => {
            let file = issue_file(&shell, &projects, project, &caller, cx)?;
            let answer = answer.take();
            cx.spawn(async move |cx| {
                let read = cx
                    .background_executor()
                    .spawn(async move { onehand_core::issues::load_blocking(&file) })
                    .await;
                let outcome = read.map(|kept| {
                    let lines = kept
                        .listed()
                        .into_iter()
                        .filter(|issue| issue.open)
                        .map(|issue| format!("#{} — {}", issue.number, issue.title))
                        .collect();
                    control::listed(lines, "No open issue in this project")
                });
                if let Some(answer) = answer {
                    let _ = answer.send(outcome);
                }
            })
            .detach();
            Ok(String::new())
        }
        Call::CreateIssue {
            project,
            title,
            body,
        } => {
            let file = issue_file(&shell, &projects, project, &caller, cx)?;
            let draft = onehand_core::issues::Draft {
                title: title.trim().to_string(),
                body,
                labels: caller
                    .as_ref()
                    .and_then(|(uid, _)| sessions().labels.get(uid).cloned())
                    .unwrap_or_default(),
            };
            let answer = answer.take();
            cx.spawn(async move |cx| {
                use onehand_core::issues::Created;
                let shown = draft.title.clone();
                let written = cx
                    .background_executor()
                    .spawn(async move {
                        onehand_core::issues::create_unless_open_blocking(&file, draft)
                    })
                    .await;
                let outcome = match written {
                    Ok(Created::New(number)) => {
                        // Read again wherever the issues are on screen.
                        cx.update(|cx| shell.update(cx, |shell, cx| shell.refresh_worktree(cx)));
                        Ok(format!("Wrote issue #{number}: {shown}"))
                    }
                    Ok(Created::Already(number)) => {
                        Ok(format!("Issue #{number} is already open with that title"))
                    }
                    Err(why) => Err(why),
                };
                if let Some(answer) = answer {
                    let _ = answer.send(outcome);
                }
            })
            .detach();
            Ok(String::new())
        }
        Call::ListProjects => {
            let shell = shell.read(cx);
            let lines = projects
                .iter()
                .map(|(name, path)| {
                    let check = if shell.check_of(path).is_some() {
                        "yes"
                    } else {
                        "no"
                    };
                    format!("{name} — {} — check: {check}", path.display())
                })
                .collect();
            Ok(control::listed(lines, "This window has no project"))
        }
        Call::ListAgents => {
            let agents = &Shared::global(cx).agents;
            let lines = agents.iter().map(|spec| spec.name.clone()).collect();
            Ok(control::listed(
                lines,
                "No agent is set up; add one in Settings ▸ Agents",
            ))
        }
        Call::ListWorkflows => {
            let lines = crate::workflow::templates(cx)
                .iter()
                .map(|entry| match &entry.template {
                    Ok(template) => format!("{} — {}", template.id, template.name),
                    Err(why) => format!("{} — cannot be read: {why}", entry.name()),
                })
                .collect();
            Ok(control::listed(lines, "The library holds no workflow"))
        }
        Call::ListTasks { project } => {
            let roots = match &project {
                Some(named) => vec![control::pick_project(&projects, named)?.to_path_buf()],
                None => projects.iter().map(|(_, path)| path.clone()).collect(),
            };
            let lines = crate::task::rows(&roots, cx)
                .iter()
                .map(|row| {
                    let next: Vec<_> = crate::chat::pane::tasks_page::task_acts(row, false)
                        .into_iter()
                        .map(|act| act.label())
                        .collect();
                    let next = if next.is_empty() {
                        "nothing".to_string()
                    } else {
                        next.join(", ")
                    };
                    format!(
                        "{} — {} — {} — {} — next: {next}",
                        row.id,
                        row.title,
                        group_said(row.group),
                        row.at
                    )
                })
                .collect();
            Ok(control::listed(lines, "No task in this window's projects"))
        }
        Call::StopTask { task } => {
            let roots: Vec<PathBuf> = projects.into_iter().map(|(_, path)| path).collect();
            let row = crate::task::rows(&roots, cx)
                .into_iter()
                .find(|row| row.id == task)
                .ok_or_else(|| format!("no task {task} in this window's projects"))?;
            // A session stopping its own task would be an agent ending the
            // run that is watching it; that is the person's to do.
            let own = caller
                .as_ref()
                .and_then(|(uid, _)| crate::task::shown(*uid, cx))
                .is_some_and(|shown| shown.task == row.id);
            if own {
                return Err("that is this session's own task".to_string());
            }
            if !row.stoppable {
                return Err(format!(
                    "{} has nothing to stop: it is {}",
                    row.title,
                    group_said(row.group)
                ));
            }
            crate::task::stop_task(&row.id, cx);
            Ok(format!("Stopping {}", row.title))
        }
        Call::OpenFile { path } => {
            // Relative to the calling session's project, which is where its
            // agent works; an outside program has no project to start from.
            let path = match (&caller, path.is_absolute()) {
                (_, true) => path,
                (Some((_, project)), false) => project.join(path),
                (None, false) => return Err("give the file's absolute path".to_string()),
            };
            // Refused rather than resolved, so a path can never climb out of
            // the project it names by its first components.
            if path.components().any(|c| c == Component::ParentDir) {
                return Err("name the file without `..`".to_string());
            }
            let active = shell
                .read(cx)
                .on_screen()
                .ok_or("no project is on screen")?;
            if !path.starts_with(&active.1) {
                return Err(format!(
                    "{} is not in {}, the project on screen",
                    path.display(),
                    active.0
                ));
            }
            if !path.is_file() {
                return Err(format!("{} is not a file", path.display()));
            }
            crate::state::with_shell(handle, cx, |shell, window, cx| {
                shell.open_file(&path, window, cx)
            })
            .ok_or("the window closed")?;
            Ok(format!("Opened {} in the Workbench", path.display()))
        }
        Call::OpenSession {
            project,
            agent,
            prompt,
        } => {
            let dir = control::pick_project(&projects, &project)?.to_path_buf();
            let spec =
                onehand_core::config::agent_or_first(&Shared::global(cx).agents, Some(&agent))
                    .cloned()
                    .ok_or_else(|| format!("no agent named {agent}; list_agents names them"))?;
            let name = spec.name.clone();
            if let Some((uid, _)) = &caller {
                let shell = shell.read(cx);
                let open: Vec<u64> = sessions()
                    .opened
                    .get(uid)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|opened| shell.session_project(*opened).is_some())
                    .collect();
                if open.len() >= OPENED_CAP {
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
            let (opened, session) = shell
                .update(cx, |shell, cx| shell.run_unattended(dir, spec, cx))
                .ok_or("the session did not start")?;
            if let Some((uid, _)) = &caller {
                sessions().opened.entry(*uid).or_default().push(opened);
            }
            if let Some(prompt) = prompt {
                send_when_connected(session, prompt, cx);
            }
            Ok(format!(
                "Opened a session with {name} on {project}. It is in the rail, not shown"
            ))
        }
    }
}

/// The issue file of the project `named`, or of the calling session's when
/// none is named, or why there is none.
fn issue_file(
    shell: &gpui::Entity<crate::shell::Shell>,
    projects: &[(String, PathBuf)],
    named: Option<String>,
    caller: &Option<(u64, PathBuf)>,
    cx: &App,
) -> Result<PathBuf, String> {
    let root = match (named, caller) {
        (Some(named), _) => control::pick_project(projects, &named)?.to_path_buf(),
        (None, Some((_, project))) => project.clone(),
        (None, None) => return Err("name a project".to_string()),
    };
    shell
        .read(cx)
        .issues_file(&root)
        .ok_or_else(|| "this workspace keeps no issues of its own".to_string())
}

/// Send `prompt` as `session`'s first message once its agent is up, as a
/// person would after watching it connect; dropped if it never comes up.
fn send_when_connected(
    session: gpui::Entity<crate::chat::session::ChatSession>,
    prompt: String,
    cx: &mut App,
) {
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

/// A task's group, as a caller reads it.
fn group_said(group: Group) -> &'static str {
    match group {
        Group::Waiting => "waiting on a person",
        Group::Ended => "ended",
        Group::Running => "running",
        Group::Queued => "queued",
        Group::Finished => "finished",
    }
}

#[cfg(test)]
mod tests;
