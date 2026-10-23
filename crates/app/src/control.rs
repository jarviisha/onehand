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
use std::cell::RefCell;
use std::path::{Component, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncWriteExt as _, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

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
    }
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
    let mut conn = Conn::default();
    let mut line = Some(first);
    while let Some(text) = line.take() {
        let out = match control::handle(&mut conn, &text, standing) {
            Handled::Reply(reply) => Some(reply),
            Handled::Quiet => None,
            Handled::Perform { id, call } => {
                let (answer, answered) = oneshot::channel();
                let outcome = match tx.unbounded_send(Incoming::Call(call, answer)) {
                    Ok(()) => answered.await.unwrap_or_else(|_| Err(CLOSING.to_string())),
                    Err(_) => Err(CLOSING.to_string()),
                };
                Some(control::answered(&id, outcome))
            }
        };
        if let Some(out) = out
            && writer
                .write_all(format!("{out}\n").as_bytes())
                .await
                .is_err()
        {
            return;
        }
        line = next_line(&mut reader).await;
    }
}

/// Carry out `call` in the window in front, and say what came of it in the
/// words the caller reads. Never from inside a shell's update: it runs from
/// the socket's own task.
pub(crate) fn perform(call: Call, cx: &mut App) -> Result<String, String> {
    let (handle, shell) = crate::state::front(cx).ok_or("no onehand window is open")?;
    let projects: Vec<(String, PathBuf)> = shell
        .read(cx)
        .remote_roots()
        .into_iter()
        .map(|(path, name)| (name, path))
        .collect();
    match call {
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
            if !path.is_absolute() {
                return Err("give the file's absolute path".to_string());
            }
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
            let (_, session) = shell
                .update(cx, |shell, cx| shell.run_unattended(dir, spec, cx))
                .ok_or("the session did not start")?;
            if let Some(prompt) = prompt {
                send_when_connected(session, prompt, cx);
            }
            Ok(format!(
                "Opened a session with {name} on {project}. It is in the rail, not shown"
            ))
        }
    }
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
