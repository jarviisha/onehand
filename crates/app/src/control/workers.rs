//! `open_session` and `open_worker`, carried out: a session on a project of
//! the window, the person's own in the rail, or, for a session's agent, a
//! worker on a project or a worktree of one of their repositories.
//!
//! What may open, where, and what an open does next is
//! `onehand_core::control::drive`'s. This reads the folder off the UI thread,
//! gathers what the windows hold, starts and watches the worker's session,
//! and carries out the rest of what the drive answers.

use super::{Answer, carry, drive};
use crate::chat::session::ChatSession;
use crate::shell::Shell;
use gpui::{AnyWindowHandle, App, Entity, Global, Subscription, WeakEntity};
use onehand_core::chat::drive::{Code, Origin};
use onehand_core::config::AgentSpec;
use onehand_core::control::drive::{Begun, Effect, Facts, Input, Key, Open, World};
use onehand_core::control::{self, workers::resolve_blocking};
use std::collections::HashMap;
use std::path::PathBuf;

/// What the opens in flight need of the front end, by their key: where each
/// was asked, the agent it starts, where its answer goes, and the watch on
/// its session.
#[derive(Default)]
struct Opens {
    asked: HashMap<Key, Asked>,
    watches: HashMap<u64, Subscription>,
}

struct Asked {
    window: AnyWindowHandle,
    shell: WeakEntity<Shell>,
    /// The agent it starts, or why there is none.
    spec: Result<AgentSpec, String>,
    answer: Option<Answer>,
}

impl Global for Opens {}

/// Open a session on a project of the window, the person's own: it appears
/// in the rail, may share its project, and gets `prompt` once its agent is
/// up. What a session's agent opens this way counts under its cap.
pub(super) fn open_plain(
    caller: Option<&(u64, PathBuf)>,
    project: &str,
    agent: &str,
    prompt: Option<String>,
    shell: &Entity<Shell>,
    projects: &[(String, PathBuf)],
    cx: &mut App,
) -> Result<String, String> {
    let spec = agent_named(agent, cx)?;
    let dir = control::pick_project(projects, project)?.to_path_buf();
    let name = spec.name.clone();
    let lead = caller.map(|(lead, _)| *lead);
    let open = lead.map(|lead| still_open(lead, cx)).unwrap_or_default();
    drive().admit_plain(lead, &open)?;
    let (uid, session) = shell
        .update(cx, |shell, cx| {
            shell.run_unattended(dir, spec, Vec::new(), cx)
        })
        .ok_or("the session did not start")?;
    drive().opened_plain(lead, uid);
    if let Some(prompt) = prompt {
        // As a person would after watching it connect; dropped if it never
        // comes up.
        session.update(cx, |session, cx| {
            session.prompt_when_up(&prompt, Origin::Tools, cx)
        });
    }
    Ok(format!(
        "Opened a session with {name} on {project}. It is in the rail, not shown"
    ))
}

fn agent_named(agent: &str, cx: &App) -> Result<AgentSpec, String> {
    onehand_core::config::agent_or_first(
        &crate::state::Shared::global(cx).settings.agents,
        Some(agent),
    )
    .cloned()
    .ok_or_else(|| format!("no agent named {agent}; list_agents names them"))
}

/// Begin `open` for a session's agent: answered at once, or once its worker
/// is up or its open failed. An outside program is never offered the tool.
pub(super) fn open(
    open: Open,
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    projects: &[(String, PathBuf)],
    answer: &mut Option<Answer>,
    cx: &mut App,
) -> Result<String, String> {
    let spec = agent_named(&open.agent, cx)?;
    let mode = crate::unattended::worker_mode(&spec.name, cx);
    let dirs = open.dirs.clone();
    let begun = drive().open_worker(open, projects, mode)?;
    let (key, target, fx) = match begun {
        Begun::Said(said) => return Ok(said),
        Begun::Resolve { key, target, fx } => (key, target, fx),
    };
    let asked = Asked {
        window,
        shell: shell.downgrade(),
        spec: Ok(spec),
        answer: answer.take(),
    };
    resolve(key, target, fx, &dirs, asked, projects, cx);
    Ok(String::new())
}

/// Lead `lead`'s conversation, kept in `archive`, was opened again: the
/// workers it still held come back, read off the UI thread first.
pub(crate) fn bring_back(lead: u64, archive: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let kept = cx
            .background_executor()
            .spawn(async move { onehand_core::control::kept::read_blocking(&archive) })
            .await;
        if kept == onehand_core::control::kept::Kept::default() {
            return;
        }
        cx.update(|cx| {
            let Some((window, shell)) = crate::windows::holding_session(lead, cx) else {
                return;
            };
            let projects: Vec<(String, PathBuf)> = shell
                .read(cx)
                .remote_roots()
                .into_iter()
                .map(|(path, name)| (name, path))
                .collect();
            let agents: HashMap<String, String> = kept
                .workers
                .iter()
                .map(|worker| (worker.handle.clone(), worker.agent.clone()))
                .collect();
            let begun = drive().back(lead, kept, &projects);
            for begun in begun {
                let (key, target, fx) = match begun {
                    Begun::Resolve { key, target, fx } => (key, target, fx),
                    // Nobody asked: nothing is answered.
                    Begun::Said(_) => continue,
                };
                let agent = agents.get(&key.1).cloned().unwrap_or_default();
                let spec = agent_named(&agent, cx)
                    .map_err(|_| format!("its agent {agent} is no longer in the agent list"));
                let asked = Asked {
                    window,
                    shell: shell.downgrade(),
                    spec,
                    answer: None,
                };
                resolve(key, target, fx, &[], asked, &projects, cx);
            }
        });
    })
    .detach();
}

/// Read open `key`'s folder off the UI thread, as `asked`, the further
/// folders being `dirs` and the window's projects `projects`, and carry
/// `fx` now.
fn resolve(
    key: Key,
    target: onehand_core::control::workers::Target,
    fx: Vec<Effect>,
    dirs: &[PathBuf],
    asked: Asked,
    projects: &[(String, PathBuf)],
    cx: &mut App,
) {
    let lead = key.0;
    let dirs = dirs.to_vec();
    let skip: Vec<u32> = std::iter::once(std::process::id())
        .chain(
            asked
                .shell
                .upgrade()
                .map(|shell| shell.read(cx).shell_pids(cx))
                .unwrap_or_default(),
        )
        .collect();
    let roots: Vec<PathBuf> = projects.iter().map(|(_, path)| path.clone()).collect();
    // Every window's projects, to find a session on the folder or inside it.
    let everywhere: Vec<PathBuf> = counts(cx).into_iter().map(|(path, _)| path).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    cx.default_global::<Opens>()
        .asked
        .insert(key.clone(), asked);
    carry(fx, cx);
    cx.spawn(async move |cx| {
        let resolved = cx
            .background_executor()
            .spawn(async move {
                resolve_blocking(target, &roots, &everywhere, &dirs, home.as_deref(), &skip)
            })
            .await;
        cx.update(|cx| {
            let world = World {
                lead_here: session_here(lead, cx),
                counts: counts(cx),
                live: live(cx),
                opened: still_open(lead, cx),
            };
            let fx = drive().handle(Input::Resolved {
                key,
                resolved,
                world,
            });
            carry(fx, cx);
        });
    })
    .detach();
}

/// Carry one of the drive's effects.
pub(super) fn carry_one(effect: Effect, cx: &mut App) {
    match effect {
        Effect::Start {
            key,
            root,
            dirs,
            folder: _,
            name: _,
            resume: Some(conversation),
        } => come_back(key, root, dirs, conversation, cx),
        Effect::Start {
            key,
            root,
            dirs,
            folder,
            name,
            resume: None,
        } => {
            let started = asked_of(&key, cx)
                .ok_or_else(|| "the window closed".to_string())
                .and_then(|(shell, spec)| Ok((shell, spec?)))
                .and_then(|(shell, spec)| {
                    shell
                        .update(cx, |shell, cx| shell.run_unattended(root, spec, dirs, cx))
                        .ok_or_else(|| "the session did not start".to_string())
                })
                .map(|(uid, session)| {
                    // No conversation file exists yet; the first turn's end
                    // writes the given name with the rest.
                    session.update(cx, |session, cx| {
                        session.drive(cx, |chat, _| {
                            chat.name_for_folder(folder, name);
                            ((), Vec::new())
                        })
                    });
                    uid
                });
            let fx = drive().handle(Input::Started { key, started });
            carry(fx, cx);
        }
        Effect::Watch(uid) => {
            if let Some(session) = session_of(uid, cx) {
                let watch = cx.observe(&session, move |_, cx| wake(uid, cx));
                cx.default_global::<Opens>().watches.insert(uid, watch);
            }
            wake(uid, cx);
        }
        Effect::Unwatch(uid) => {
            cx.default_global::<Opens>().watches.remove(&uid);
        }
        Effect::Pick { uid, pick } => {
            if let Some(session) = session_of(uid, cx) {
                session.update(cx, |session, cx| {
                    session.drive(cx, |chat, _| ((), chat.pick(&pick)))
                });
            }
        }
        Effect::Begin { uid, prompt } => {
            let facts = session_of(uid, cx).map(|session| begin(uid, &session, prompt, cx));
            // Read before the drive is taken: its lock is not reentrant.
            let lead_here = session_here_of_lead(uid, cx);
            let fx = match facts {
                Some(facts) => drive().handle(Input::Begun { uid, facts }),
                None => drive().handle(Input::Woke {
                    uid,
                    chat: None,
                    lead_here,
                }),
            };
            carry(fx, cx);
        }
        Effect::Arm { token, wait } => {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(wait).await;
                cx.update(|cx| {
                    let fx = drive().handle(Input::Elapsed { token });
                    carry(fx, cx);
                });
            })
            .detach();
        }
        Effect::Drop {
            key,
            uid,
            root,
            added,
        } => {
            // The window holding the session, else the one the open was
            // asked in, which may still hold the project it added.
            let window = window_of(uid, cx).or_else(|| {
                cx.default_global::<Opens>()
                    .asked
                    .get(&key)
                    .map(|asked| asked.window)
            });
            if let Some(window) = window {
                crate::windows::with_shell(window, cx, |shell, window, cx| {
                    shell.drop_opened(uid, &root, added, window, cx)
                });
            }
        }
        Effect::Answer { key, outcome } => {
            let asked = cx.default_global::<Opens>().asked.remove(&key);
            if let Some(answer) = asked.and_then(|asked| asked.answer) {
                let _ = answer.send(outcome);
            }
        }
        // The person's view reads the relation: nothing in a session changed.
        Effect::Changed => cx.refresh_windows(),
        Effect::ToolsChanged(uid) => super::tell_changed(uid),
        Effect::Keep {
            lead,
            kept,
            dir,
            moved_from,
            last,
        } => super::keeping::keep(lead, kept, dir, moved_from, last, cx),
        Effect::Send { uid, turn, text } => turns::send_now(uid, turn, text, cx),
        Effect::TurnAnswer { uid, outcome } => turns::answered(uid, outcome, cx),
    }
}

/// Worker `uid`'s session changed: the drive decides the open's next step.
fn wake(uid: u64, cx: &mut App) {
    let lead_here = session_here_of_lead(uid, cx);
    let session = session_of(uid, cx);
    let fx = {
        let chat = session.as_ref().map(|session| session.read(cx).chat());
        drive().handle(Input::Woke {
            uid,
            chat: chat.map(|chat| &**chat),
            lead_here,
        })
    };
    carry(fx, cx);
}

/// Send worker `uid` its first prompt, if it has one, and what its answer
/// reads.
fn begin(uid: u64, session: &Entity<ChatSession>, prompt: Option<String>, cx: &mut App) -> Facts {
    let intake = prompt.map(|prompt| {
        session
            .update(cx, |session, cx| {
                session.prompt(&prompt, &[], Origin::Tools, Code::Here, cx)
            })
            .intake
    });
    let name = window_of(uid, cx)
        .and_then(|window| {
            crate::windows::with_shell(window, cx, |shell, _, cx| shell.session_label(uid, cx))
        })
        .unwrap_or_default();
    let chat = &session.read(cx).chat();
    Facts {
        intake,
        sent: chat.prompts_sent,
        mode: control::state::mode_said(chat),
        model: control::state::model_said(chat),
        name,
    }
}

/// Start worker `key`, coming back, on `root` with `dirs`, on the
/// conversation `conversation` names, found off the UI thread; its agent
/// connects once it is shown or called on.
fn come_back(key: Key, root: PathBuf, dirs: Vec<PathBuf>, conversation: String, cx: &mut App) {
    cx.spawn(async move |cx| {
        let found = cx
            .background_executor()
            .spawn(async move {
                onehand_core::chat::find_conversation(
                    &onehand_core::chat::conversations_dir(),
                    &conversation,
                )
            })
            .await;
        cx.update(|cx| {
            let started = found
                .ok_or_else(|| "its conversation is gone".to_string())
                .and_then(|(_, meta)| {
                    let (shell, spec) =
                        asked_of(&key, cx).ok_or_else(|| "the window closed".to_string())?;
                    let spec = spec?;
                    shell
                        .update(cx, |shell, cx| {
                            shell.add_unshown_resuming(
                                root,
                                spec,
                                dirs,
                                meta.dir,
                                meta.session_id,
                                cx,
                            )
                        })
                        .ok_or_else(|| "the session did not start".to_string())
                });
            // No call waits on a worker coming back: what was asked goes now.
            cx.default_global::<Opens>().asked.remove(&key);
            let fx = drive().handle(Input::Started { key, started });
            carry(fx, cx);
        });
    })
    .detach();
}

/// Where open `key` was asked, and the agent it starts or why none.
fn asked_of(key: &Key, cx: &mut App) -> Option<(Entity<Shell>, Result<AgentSpec, String>)> {
    let asked = cx.default_global::<Opens>().asked.get(key)?;
    Some((asked.shell.upgrade()?, asked.spec.clone()))
}

/// Session `uid`, in whichever window holds it.
pub(super) fn session_of(uid: u64, cx: &App) -> Option<Entity<ChatSession>> {
    crate::windows::session(uid, cx)
}

fn window_of(uid: u64, cx: &App) -> Option<AnyWindowHandle> {
    crate::windows::holding_session(uid, cx).map(|(window, _)| window)
}

/// Whether session `uid` is still a session of some window.
pub(super) fn session_here(uid: u64, cx: &App) -> bool {
    window_of(uid, cx).is_some()
}

/// Whether the lead of worker `uid` is still a session of some window.
pub(super) fn session_here_of_lead(uid: u64, cx: &App) -> bool {
    let lead = drive().lead_of(uid);
    lead.is_some_and(|lead| session_here(lead, cx))
}

/// Every window's project roots on this machine, with their sessions.
fn counts(cx: &App) -> Vec<(PathBuf, usize)> {
    crate::windows::live(cx)
        .into_iter()
        .flat_map(|(_, shell)| shell.read(cx).roots_with_sessions())
        .collect()
}

/// Every session open in any window.
fn live(cx: &App) -> Vec<u64> {
    crate::windows::live(cx)
        .into_iter()
        .flat_map(|(_, shell)| shell.read(cx).session_uids())
        .collect()
}

/// The sessions `lead`'s tools opened that are still open, with their
/// projects.
fn still_open(lead: u64, cx: &App) -> Vec<(u64, PathBuf)> {
    drive()
        .opened_by(lead)
        .into_iter()
        .filter_map(|uid| {
            let (_, shell) = crate::windows::holding_session(uid, cx)?;
            let at = shell.read(cx).session_project(uid)?;
            Some((uid, at))
        })
        .collect()
}

pub(super) mod turns;
pub(super) mod whose;

#[cfg(test)]
mod tests;
