//! Adding a project on a host, and the question asked before a host's service
//! is restarted onto this build.

use super::Shell;
use super::confirm::Confirm;
use crate::dialogs::{add_refusal, busy_body};
use crate::hosts::{self, HostFailure};
use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

/// The add-a-project-on-a-host form, while it is up. `Some` on the shell is
/// what puts it on screen.
///
/// Its fields are made with it and go with it, so the form always opens
/// empty and nothing typed into a cancelled one is found again.
pub(crate) struct HostProjectDraft {
    pub(crate) host: Entity<InputState>,
    pub(crate) folder: Entity<InputState>,
    /// What the last *Add* ran into. Cleared by the next keystroke, because
    /// the reader has started answering it.
    pub(crate) error: Option<String>,
    /// The attach under way, by [`HostFlow::attempt`]: attaching to the host
    /// and asking it about the folder.
    attempt: Option<u64>,
    _typing: [Subscription; 2],
}

impl HostProjectDraft {
    pub(crate) fn busy(&self) -> bool {
        self.attempt.is_some()
    }
}

/// What waits on a yes to a host's restart question.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    /// The session that was not attached, restarted.
    Session(u64),
    /// The form's *Add*, carried on.
    Add,
}

/// The restart questions on screen, one per host, and the count that tells
/// one attach of the form from the next.
#[derive(Default)]
pub(super) struct HostFlow {
    /// What each host's question answers for. `alive` is dropped with the
    /// question: its answer holds the one strong count, and the dialog drops
    /// the answer when it goes, however it was put away.
    asked: BTreeMap<HostName, Asked>,
    attempts: u64,
}

struct Asked {
    uids: Vec<u64>,
    add: bool,
    alive: Weak<()>,
}

impl HostFlow {
    /// Add `want` to the question on screen for `host`; or, when there is
    /// none, start one and hand back what keeps it alive, for the question's
    /// answer to hold.
    fn join(&mut self, host: &HostName, want: Want) -> Option<Rc<()>> {
        let live = self
            .asked
            .get(host)
            .is_some_and(|asked| asked.alive.strong_count() > 0);
        let fresh = (!live).then(|| {
            let alive = Rc::new(());
            let asked = Asked {
                uids: Vec::new(),
                add: false,
                alive: Rc::downgrade(&alive),
            };
            self.asked.insert(host.clone(), asked);
            alive
        });
        let asked = self.asked.get_mut(host)?;
        match want {
            Want::Session(uid) => asked.uids.push(uid),
            Want::Add => asked.add = true,
        }
        fresh
    }

    /// What `host`'s question answered for, once: its sessions, and whether
    /// the form's *Add* waits on it.
    fn take(&mut self, host: &HostName) -> Option<(Vec<u64>, bool)> {
        self.asked.remove(host).map(|asked| (asked.uids, asked.add))
    }

    /// A new attach of the form's.
    fn attempt(&mut self) -> u64 {
        self.attempts += 1;
        self.attempts
    }
}

/// Whether an attach's answer is for the form's attach under way: a form
/// cancelled and opened again has another, or none.
fn landed_on(current: Option<u64>, landed: u64) -> bool {
    current == Some(landed)
}

/// Whether a yes still has to restart the service: an earlier yes may have
/// put it on this build already, and restarting it again would stop what
/// that one started.
fn needs_restart<C>(attached: &Result<C, HostFailure>) -> bool {
    match attached {
        Err(HostFailure::Busy { .. }) => true,
        Ok(_) | Err(HostFailure::Missing(_) | HostFailure::Other(_)) => false,
    }
}

/// What a restart that failed says.
fn restart_failure(failure: HostFailure) -> String {
    match failure {
        HostFailure::Missing(why) | HostFailure::Other(why) => why,
        HostFailure::Busy { host, .. } => {
            format!("{host} still runs another build of the host service")
        }
    }
}

/// The question asked before `host`'s service is restarted, stopping `busy`.
fn restart_ask(host: &HostName, busy: &[HostSession]) -> Confirm {
    Confirm {
        id: "restart-host",
        title: format!("Restart the host service on {host}?").into(),
        description: busy_body(host, busy).into(),
        act: "Restart",
        ..Default::default()
    }
}

impl Shell {
    pub(crate) fn host_project(&self) -> Option<&HostProjectDraft> {
        self.host_project.as_ref()
    }

    /// Open the add-a-project-on-a-host form, the caret in its *Host* field.
    pub(crate) fn begin_host_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host_project.is_some() {
            return;
        }
        self.remember_focus(window, cx);
        let host = cx.new(|cx| InputState::new(window, cx));
        let folder = cx.new(|cx| InputState::new(window, cx));
        // Every keystroke redraws: *Add* refuses until both fields say enough.
        let typing = [&host, &folder].map(|input| {
            cx.subscribe_in(
                input,
                window,
                |shell: &mut Self, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        if let Some(draft) = shell.host_project.as_mut() {
                            draft.error = None;
                        }
                        cx.notify();
                    }
                },
            )
        });
        host.focus_handle(cx).focus(window, cx);
        self.host_project = Some(HostProjectDraft {
            host,
            folder,
            error: None,
            attempt: None,
            _typing: typing,
        });
        cx.notify();
    }

    /// Put the form away. An attach under way goes on, and what it finds is
    /// dropped: it is about a form no longer there.
    pub(crate) fn cancel_host_project(&mut self, cx: &mut Context<Self>) {
        if self.host_project.take().is_some() {
            cx.notify();
        }
    }

    /// Attach to the host the form names, ask it whether the folder is
    /// there, and add the project when it is.
    ///
    /// The form stays up until the host answers, so what went wrong is shown
    /// against the names that caused it.
    pub(crate) fn commit_host_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.host_project.as_ref() else {
            return;
        };
        if draft.busy() {
            return;
        }
        let host = draft.host.read(cx).value().trim().to_string();
        let folder = draft.folder.read(cx).value().trim().to_string();
        if add_refusal(&host, &folder).is_some() {
            return;
        }
        let attempt = self.host_flow.attempt();
        if let Some(draft) = self.host_project.as_mut() {
            draft.error = None;
            draft.attempt = Some(attempt);
        }
        cx.notify();
        let (host, path) = (HostName(host), PathBuf::from(folder));
        let attaching = hosts::conn(cx, &host);
        cx.spawn_in(window, async move |shell, cx| {
            let found = match attaching.await {
                Ok(conn) => {
                    let asking = cx.update(|_, cx| hosts::is_dir(cx, &host, conn, path.clone()));
                    match asking {
                        Ok(asking) => asking.await,
                        Err(_) => return,
                    }
                }
                Err(failure) => Err(failure),
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    shell.host_project_found(attempt, host, path, found, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// What the host said about the folder the form names, on attach
    /// `attempt`.
    fn host_project_found(
        &mut self,
        attempt: u64,
        host: HostName,
        path: PathBuf,
        found: Result<bool, HostFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only the attach the form on screen is waiting on: a form cancelled
        // and opened again since has started another, or none.
        let Some(draft) = self
            .host_project
            .as_mut()
            .filter(|draft| landed_on(draft.attempt, attempt))
        else {
            return;
        };
        draft.attempt = None;
        match found {
            Ok(true) => {
                self.host_project = None;
                self.open_host_root(host, path, window, cx);
            }
            Ok(false) => {
                draft.error = Some(format!("There is no folder {} on {host}", path.display()));
            }
            Err(HostFailure::Missing(why) | HostFailure::Other(why)) => draft.error = Some(why),
            // Closes nothing: the form waits under the question, and a yes
            // carries on with the same names.
            Err(HostFailure::Busy { host, busy }) => {
                self.offer_restart(host, &busy, Want::Add, window, cx);
            }
        }
        cx.notify();
    }

    /// Session `uid` was not attached because `host` runs another build with
    /// `busy` mid-turn: ask whether to restart its service, and on a yes,
    /// restart it and then the session.
    pub(super) fn ask_host_restart(
        &mut self,
        uid: u64,
        host: HostName,
        busy: &[HostSession],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.offer_restart(host, busy, Want::Session(uid), window, cx);
    }

    /// Ask whether to restart `host`'s service for `want`, or add `want` to
    /// the question already asking it: one yes answers for every session
    /// and form that waits on that host.
    fn offer_restart(
        &mut self,
        host: HostName,
        busy: &[HostSession],
        want: Want,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(alive) = self.host_flow.join(&host, want) else {
            return;
        };
        let ask = restart_ask(&host, busy);
        self.confirm(ask, window, cx, move |shell, window, cx| {
            // Held here, so the question counts as asked while it is up.
            let _ = &alive;
            shell.restart_agreed(&host, window, cx);
        });
    }

    /// The person said yes to restarting `host`'s service. Attached first,
    /// and restarted only if it is still busy: an earlier yes may have
    /// restarted it already.
    fn restart_agreed(&mut self, host: &HostName, window: &mut Window, cx: &mut Context<Self>) {
        let Some((uids, add)) = self.host_flow.take(host) else {
            return;
        };
        let attaching = hosts::conn(cx, host);
        let host = host.clone();
        cx.spawn_in(window, async move |shell, cx| {
            let mut attached = attaching.await;
            if needs_restart(&attached) {
                let Ok(restarting) = cx.update(|_, cx| hosts::restart(cx, &host)) else {
                    return;
                };
                attached = restarting.await;
            }
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    shell.restart_landed(uids, add, attached.map(drop), window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The service is on this build, or could not be put there: carry on
    /// with what waited on it, or say why not.
    fn restart_landed(
        &mut self,
        uids: Vec<u64>,
        add: bool,
        done: Result<(), HostFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match done {
            Ok(()) => {
                for uid in uids {
                    self.restart_now(uid, window, cx);
                }
                if add {
                    self.commit_host_project(window, cx);
                }
            }
            // Said once: on the form, when it waits on this and is still up,
            // else in a toast.
            Err(failure) => {
                let why = restart_failure(failure);
                match self.host_project.as_mut().filter(|_| add) {
                    Some(draft) => draft.error = Some(why),
                    None => window.push_notification(
                        crate::toast::failed("Restarting the host service", why),
                        cx,
                    ),
                }
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
