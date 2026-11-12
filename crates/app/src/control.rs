//! onehand's tools, carried out.
//!
//! What a tool may do and how a message is answered is core's `control`, and
//! what follows from a session's switch, its grant, its closing and a
//! worker's open is its drive's (`control::drive::ControlDrive`), held here.
//! The socket's side of the conversation runs on `onehand_io::socket` and
//! sends only a [`Call`] across, which [`perform`] carries out on the window
//! in front and answers with the text the caller reads.

use crate::state::Shared;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use gpui::{App, BorrowAppContext as _, Window};
use onehand_core::config::ControlConfig;
use onehand_core::control::drive::{ControlDrive, Effect};
use onehand_core::control::{self, Call, Peer, Standing};
use onehand_core::workflow::Brief;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

mod start;
mod workers;

pub(crate) use start::start_task;
use start::{start, template};
pub(crate) use workers::whose::{Worker, give_back, give_back_offered, lead_of, workers_of};

/// The tools' drive, held for the whole process: an agent's token is minted
/// and let go where no window is at hand, and the socket's runtime reads each
/// connection's standing on every message. Never held across anything else.
static CONTROL: LazyLock<Mutex<ControlDrive>> = LazyLock::new(Mutex::default);

pub(crate) fn drive() -> std::sync::MutexGuard<'static, ControlDrive> {
    CONTROL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where each token's connections are written, to tell them the tools
/// changed: the socket's half, which no drive holds.
static CONNS: LazyLock<Mutex<HashMap<String, Vec<UnboundedSender<String>>>>> =
    LazyLock::new(Mutex::default);

fn conns() -> std::sync::MutexGuard<'static, HashMap<String, Vec<UnboundedSender<String>>>> {
    CONNS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Put the config's answer in force, before anything listens.
pub(crate) fn boot(cfg: &ControlConfig) {
    drive().set_outside(cfg.outside);
}

/// Whether outside control is on.
pub(crate) fn outside() -> bool {
    drive().outside()
}

/// Turn outside control on or off, at once and for the next message of a
/// client already connected, then write it; a write that fails puts it back.
pub(crate) fn set_outside(on: bool, window: &mut Window, cx: &mut App) {
    let before = outside();
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.set_outside(before, on));
    crate::saves::carry_settings(fx, window, cx);
}

/// What a connection may do now: read on every message it sends.
fn standing(peer: &Peer) -> Standing {
    drive().standing(peer)
}

/// A session's token: what its agent's proxy says it speaks for. Minted when
/// the agent is started, which for a warm one is before any session holds
/// it, and let go with whatever holds it.
pub(crate) struct Token(String);

impl Token {
    pub(crate) fn mint() -> Self {
        Self(drive().mint())
    }

    /// Held by session `uid` from now on.
    pub(crate) fn claim(&self, uid: u64) {
        drive().claim(&self.0, uid);
    }
}

impl onehand_io::acp::Pass for Token {
    /// The `mcpServers` entry that gives an agent this token: onehand itself,
    /// as the proxy to its socket. Nothing when onehand cannot say where its
    /// own executable is.
    fn servers(&self) -> Vec<serde_json::Value> {
        let socket = onehand_core::instance::socket_path(&onehand_core::config::config_dir());
        std::env::current_exe()
            .map(|exe| vec![control::proxy_entry(&exe, &socket, &self.0)])
            .unwrap_or_default()
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        drive().release(&self.0);
        conns().remove(&self.0);
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
    drive().switched_on(uid)
}

/// Let session `uid`'s agent use the tools, or stop it.
pub(crate) fn switch(uid: u64, on: bool, cx: &mut App) {
    let fx = drive().switch(uid, on);
    carry_later(fx, cx);
}

/// Give session `uid` what its task was given.
pub(crate) fn grant(uid: u64, grant: &onehand_core::task::Grant, cx: &mut App) {
    let fx = drive().grant(uid, grant);
    carry_later(fx, cx);
}

/// Session `uid` is closed: its switch and what its tools opened go with it.
pub(crate) fn forget(uid: u64, cx: &mut App) {
    let fx = drive().forget(uid);
    carry_later(fx, cx);
}

/// Carry `fx` once whatever asked has let go of its entities: an open's
/// failure closes a session and may drop a project.
fn carry_later(fx: Vec<Effect>, cx: &mut App) {
    if fx.is_empty() {
        return;
    }
    cx.defer(move |cx| carry(fx, cx));
}

/// Carry out the drive's effects, in order, each once; the drive's lock is
/// never held across one.
pub(crate) fn carry(fx: Vec<Effect>, cx: &mut App) {
    for effect in fx {
        workers::carry_one(effect, cx);
    }
}

/// Tell every connection of session `uid` that its tools changed, dropping
/// the ones whose other end has gone.
fn tell_changed(uid: u64) {
    let tokens = drive().tokens_of(uid);
    let note = control::tools_changed();
    let mut conns = conns();
    for token in tokens {
        if let Some(senders) = conns.get_mut(&token) {
            senders.retain(|sender| sender.unbounded_send(note.clone()).is_ok());
        }
    }
}

/// What the socket asks of the tools from its own thread: each connection's
/// standing, and where a session's connection is told its tools changed.
pub(crate) fn front() -> onehand_io::socket::Front {
    onehand_io::socket::Front {
        standing: std::sync::Arc::new(standing),
        joined: std::sync::Arc::new(|token: &str, out: UnboundedSender<String>| {
            conns().entry(token.to_string()).or_default().push(out);
        }),
    }
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
            let uid = drive().holder(token).ok_or("no such session")?;
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
    // Only the folders on this machine: a project on a host has no issue
    // file, worktree or check here for a tool to reach, and is refused by
    // name rather than said not to exist.
    let by_name = |roots: Vec<(PathBuf, String)>| -> Vec<(String, PathBuf)> {
        roots.into_iter().map(|(path, name)| (name, path)).collect()
    };
    let projects = by_name(shell.read(cx).remote_roots());
    if let Some(named) = call.project() {
        let hosted = by_name(shell.read(cx).host_roots());
        control::calls::hosted(&projects, &hosted, named)?;
    }
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
            let (root, file) = issue_place(&shell, &projects, project, &caller, cx)?;
            let draft = onehand_core::issues::Draft {
                title: title.trim().to_string(),
                body,
                labels: caller
                    .as_ref()
                    .map(|(uid, _)| drive().labels(*uid))
                    .unwrap_or_default(),
            };
            let answer = answer.take();
            let shown = draft.title.clone();
            // Through the file's drive, which every view of it shares: each
            // shows the issue as it lands.
            onehand_issues::write(
                &root,
                file,
                onehand_core::issues::drive::Change::CreateUnlessOpen(draft),
                crate::features::connectors(),
                move |written, _| {
                    use onehand_core::issues::drive::Applied;
                    let outcome = match written {
                        Ok(Applied::New(number)) => Ok(format!("Wrote issue #{number}: {shown}")),
                        Ok(Applied::Already(number)) => {
                            Ok(format!("Issue #{number} is already open with that title"))
                        }
                        Ok(Applied::Nothing | Applied::Named(_)) => {
                            Err("the issue was not written".to_string())
                        }
                        Err(why) => Err(why),
                    };
                    if let Some(answer) = answer {
                        let _ = answer.send(outcome);
                    }
                },
                cx,
            );
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
            let agents = &Shared::global(cx).settings.agents;
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
        Call::ReadWorkflow { workflow } => {
            onehand_core::workflow::store::to_text(&template(&workflow, cx)?)
        }
        Call::SaveWorkflow { text } => {
            let answer = answer.take();
            cx.spawn(async move |cx| {
                use onehand_core::workflow::store;
                let saved = cx
                    .background_executor()
                    .spawn(async move { store::save_text_blocking(&store::dir(), &text) })
                    .await;
                let outcome = saved.map(|(_, saved)| {
                    // Read again wherever the library is on screen.
                    cx.update(crate::workflow::reload_templates);
                    format!(
                        "Saved {} as {}, version {}",
                        saved.name, saved.id, saved.version
                    )
                });
                if let Some(answer) = answer {
                    let _ = answer.send(outcome);
                }
            })
            .detach();
            Ok(String::new())
        }
        Call::ListTasks { project } => {
            let roots = match &project {
                Some(named) => vec![control::pick_project(&projects, named)?.to_path_buf()],
                None => projects.iter().map(|(_, path)| path.clone()).collect(),
            };
            let lines = crate::task::rows(&roots, cx)
                .iter()
                .map(|row| {
                    let next: Vec<_> = crate::tasks_page::task_acts(row, false)
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
                        control::calls::group_said(row.group),
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
            let own = caller
                .as_ref()
                .and_then(|(uid, _)| crate::task::shown(*uid, cx))
                .is_some_and(|shown| shown.task == row.id);
            control::calls::may_stop(&row.title, row.group, own, row.stoppable)?;
            crate::task::stop_task(&row.id, cx);
            Ok(format!("Stopping {}", row.title))
        }
        Call::OpenFile { path } => {
            let on_screen = shell.read(cx).on_screen();
            let path = control::calls::file_to_open(
                path,
                caller.as_ref().map(|(_, project)| project.as_path()),
                on_screen
                    .as_ref()
                    .map(|(name, root)| (name.as_ref(), root.as_path())),
                Path::is_file,
            )?;
            crate::state::with_shell(handle, cx, |shell, window, cx| {
                shell.open_file(&path, window, cx)
            })
            .ok_or("the window closed")?;
            Ok(format!("Opened {} in the Workbench", path.display()))
        }
        Call::SessionState { session } => workers::turns::answer(caller, &session, cx),
        Call::SendPrompt {
            session,
            text,
            turn,
        } => workers::turns::send(caller, &session, &text, turn, cx),
        Call::CancelTurn { session } => workers::turns::cancel(caller, &session, cx),
        Call::OpenSession {
            project,
            agent,
            prompt,
        } => workers::open_plain(
            caller.as_ref(),
            &project,
            &agent,
            prompt,
            &shell,
            &projects,
            cx,
        ),
        Call::OpenWorker {
            project,
            agent,
            prompt,
            dirs,
            request,
            turn,
            model,
            name,
        } => workers::open(
            onehand_core::control::drive::Open {
                // Never listed to an outside program, so never called by one.
                lead: caller.ok_or("`open_worker` is for a session's agent")?.0,
                project,
                agent,
                prompt,
                dirs,
                request,
                turn,
                model,
                name,
            },
            handle,
            shell,
            &projects,
            answer,
            cx,
        ),
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
    issue_place(shell, projects, named, caller, cx).map(|(_, file)| file)
}

/// The project a call names, or the caller's own, and the file its issues
/// are kept in.
fn issue_place(
    shell: &gpui::Entity<crate::shell::Shell>,
    projects: &[(String, PathBuf)],
    named: Option<String>,
    caller: &Option<(u64, PathBuf)>,
    cx: &App,
) -> Result<(PathBuf, PathBuf), String> {
    let root = match (named, caller) {
        (Some(named), _) => control::pick_project(projects, &named)?.to_path_buf(),
        (None, Some((_, project))) => project.clone(),
        (None, None) => return Err("name a project".to_string()),
    };
    let file = shell
        .read(cx)
        .issues_file(&root)
        .ok_or_else(|| "this workspace keeps no issues of its own".to_string())?;
    Ok((root, file))
}

#[cfg(test)]
mod tests;
