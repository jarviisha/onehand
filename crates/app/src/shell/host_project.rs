//! Adding a project on a host, what waits here on a host's busy sessions, and
//! the question asked before a host's service is restarted onto this build.

use super::Shell;
use super::confirm::Confirm;
use crate::dialogs::{add_refusal, busy_body};
use crate::hosts::{self, HostFailure};
use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    /// and asking it about the folder, or waiting on the host's sessions.
    attempt: Option<u64>,
    /// The host the attach waits on, its busy sessions to go idle.
    pub(crate) waiting: Option<HostName>,
    _typing: [Subscription; 2],
}

impl HostProjectDraft {
    pub(crate) fn busy(&self) -> bool {
        self.attempt.is_some()
    }
}

/// What waits on a host's busy sessions going idle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    /// The session that was not attached, restarted.
    Session(u64),
    /// The form's *Add*, on this attempt, carried on.
    Add(u64),
    /// The project page's list of the host's sessions, asked again.
    Page,
}

/// What waits here on each host kept waiting by its busy sessions, and the
/// count that tells one attach of the form from the next. The wait itself
/// is the process's, in [`hosts`]; this is the window's share of it.
#[derive(Default)]
pub(super) struct HostFlow {
    waiting: BTreeMap<HostName, Waiting>,
    attempts: u64,
}

/// The sessions to restart, the form's attempt to carry on and whether a
/// page waits, once a host's wait ends; and the listening to it, which
/// leaves the wait when this goes.
#[derive(Default)]
struct Waiting {
    uids: Vec<u64>,
    add: Option<u64>,
    page: bool,
    _listening: Option<gpui::Task<()>>,
}

impl HostFlow {
    /// Add `want` to what waits here on `host`: `true` when nothing did, so
    /// the end of the host's wait is still to be listened for.
    fn join(&mut self, host: &HostName, want: Want) -> bool {
        let fresh = !self.waiting.contains_key(host);
        let waiting = self.waiting.entry(host.clone()).or_default();
        match want {
            Want::Session(uid) if !waiting.uids.contains(&uid) => waiting.uids.push(uid),
            Want::Session(_) => {}
            Want::Page => waiting.page = true,
            Want::Add(attempt) => waiting.add = Some(attempt),
        }
        fresh
    }

    /// What waited here on `host`, once.
    fn take(&mut self, host: &HostName) -> Option<Waiting> {
        self.waiting.remove(host)
    }

    /// Hold the listening to `host`'s wait, for as long as something here
    /// waits on it.
    fn listen(&mut self, host: &HostName, listening: gpui::Task<()>) {
        if let Some(waiting) = self.waiting.get_mut(host) {
            waiting._listening = Some(listening);
        }
    }

    /// Keep only what `still` says waits yet; a host nothing here waits on
    /// any more is let go, and with it the listening, so a wait nobody
    /// anywhere listens to ends.
    fn prune(&mut self, still: impl Fn(&HostName, Want) -> bool) {
        self.waiting.retain(|host, waiting| {
            waiting.uids.retain(|uid| still(host, Want::Session(*uid)));
            waiting.add = waiting.add.filter(|add| still(host, Want::Add(*add)));
            waiting.page = waiting.page && still(host, Want::Page);
            !waiting.uids.is_empty() || waiting.add.is_some() || waiting.page
        });
    }

    fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// The form went away: no wait carries its *Add* on, so a form opened
    /// again is never committed by a wait that ended for this one.
    fn forget_add(&mut self) {
        for waiting in self.waiting.values_mut() {
            waiting.add = None;
        }
    }

    /// A new attach of the form's.
    fn attempt(&mut self) -> u64 {
        self.attempts += 1;
        self.attempts
    }
}

/// Let the form go from `held` as its host's wait ends, if that is still
/// the attempt it waits on: `true` when the form is the one that waited.
fn release(form: &mut Option<u64>, held: Option<u64>) -> bool {
    match held {
        Some(held) if landed_on(*form, held) => {
            *form = None;
            true
        }
        Some(_) | None => false,
    }
}

/// Whether an attach's answer is for the form's attach under way: a form
/// cancelled and opened again has another, or none.
fn landed_on(current: Option<u64>, landed: u64) -> bool {
    current == Some(landed)
}

/// Whether a yes still has to restart the service: the wait may have ended
/// since the question was asked, and restarting it again would stop what
/// that started.
fn needs_restart<C>(attached: &Result<C, HostFailure>) -> bool {
    match attached {
        Err(HostFailure::Busy { .. }) => true,
        Ok(_) | Err(HostFailure::Other(_)) => false,
    }
}

/// What a restart that failed says.
fn restart_failure(failure: HostFailure) -> String {
    match failure {
        HostFailure::Other(why) => why,
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
        description: busy_body(host, busy, onehand_core::chat::now_secs()).into(),
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
            waiting: None,
            _typing: typing,
        });
        cx.notify();
    }

    /// Put the form away. An attach under way goes on, and what it finds is
    /// dropped: it is about a form no longer there. So is the end of a wait
    /// it waits on, for the form.
    pub(crate) fn cancel_host_project(&mut self, cx: &mut Context<Self>) {
        self.host_flow.forget_add();
        if self.host_project.take().is_some() {
            self.prune_host_waits(cx);
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
            draft.waiting = None;
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
                self.host_flow.forget_add();
                self.open_host_root(host, path, window, cx);
            }
            Ok(false) => {
                draft.error = Some(format!("There is no folder {} on {host}", path.display()));
            }
            Err(HostFailure::Other(why)) => draft.error = Some(why),
            // The form stays busy on this attempt while the host waits, and
            // carries on with the same names once it is done.
            Err(HostFailure::Busy { host, .. }) => {
                draft.attempt = Some(attempt);
                draft.waiting = Some(host.clone());
                self.wait_on_host(host, Want::Add(attempt), window, cx);
            }
        }
        cx.notify();
    }

    /// Session `uid` was not attached, or with no `uid` the project page
    /// could not list, because `host` runs another build with sessions
    /// mid-turn: restart the session, or list the page again, once the host
    /// is done waiting.
    pub(super) fn host_busy(
        &mut self,
        uid: Option<u64>,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let want = uid.map_or(Want::Page, Want::Session);
        self.wait_on_host(host, want, window, cx);
    }

    /// Add `want` to what waits on `host` here, listening for the end of the
    /// host's wait when nothing here did yet.
    fn wait_on_host(
        &mut self,
        host: HostName,
        want: Want,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.host_flow.join(&host, want) {
            return;
        }
        let idle = hosts::idle(cx, &host);
        let listening = cx.spawn_in(window, {
            let host = host.clone();
            async move |shell, cx| {
                // `None`: an attach has landed since, and nothing waits.
                let end = match idle {
                    Some(idle) => idle.await,
                    None => Ok(()),
                };
                shell
                    .update_in(cx, |shell: &mut Self, window, cx| {
                        shell.host_idle(&host, end, window, cx)
                    })
                    .ok();
            }
        });
        self.host_flow.listen(&host, listening);
    }

    /// Let go of what no longer waits here on a host: a session closed or
    /// attached, a page left, a form put away.
    pub(super) fn prune_host_waits(&mut self, cx: &App) {
        if self.host_flow.is_empty() {
            return;
        }
        let pane = self.chat.read(cx);
        let page = pane.page_waits_on();
        let form = self
            .host_project
            .as_ref()
            .and_then(|draft| draft.waiting.as_ref().zip(draft.attempt));
        self.host_flow.prune(|host, want| match want {
            Want::Session(uid) => pane.host_wait(uid, cx).as_ref() == Some(host),
            Want::Add(attempt) => form == Some((host, attempt)),
            Want::Page => page == Some(host),
        });
    }

    /// `host`'s wait ended: on this build, carry on with what waited on it;
    /// out of reach, say so wherever it waited.
    fn host_idle(
        &mut self,
        host: &HostName,
        end: hosts::WaitEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(waiting) = self.host_flow.take(host) else {
            return;
        };
        let add = self.host_project.as_mut().is_some_and(|draft| {
            let released = release(&mut draft.attempt, waiting.add);
            if released {
                draft.waiting = None;
            }
            released
        });
        match end {
            Ok(()) => {
                for uid in waiting.uids {
                    self.restart_now(uid, true, window, cx);
                }
                // A page on a host lists again whatever waited: its sessions
                // are what the wait changed.
                self.chat.update(cx, |pane, cx| pane.list_host_sessions(cx));
                if add {
                    self.commit_host_project(window, cx);
                }
            }
            Err(why) => {
                self.chat.update(cx, |pane, cx| {
                    for uid in &waiting.uids {
                        pane.host_wait_failed(*uid, &why, cx);
                    }
                    if waiting.page {
                        pane.host_page_failed(host, &why, cx);
                    }
                });
                if let Some(draft) = self.host_project.as_mut().filter(|_| add) {
                    draft.error = Some(why);
                }
            }
        }
        cx.notify();
    }

    /// *Restart now*, wherever something waits on `host`: ask before its
    /// service is restarted onto this build, stopping its busy sessions.
    pub(crate) fn ask_restart_host(
        &mut self,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = restart_ask(&host, &hosts::busy(cx, &host));
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.restart_agreed(host.clone(), window, cx);
        });
    }

    /// The person said yes to restarting `host`'s service. Attached first,
    /// and restarted only if it is still busy. An attach that lands ends the
    /// host's wait, and what waited carries on from there.
    fn restart_agreed(&mut self, host: HostName, window: &mut Window, cx: &mut Context<Self>) {
        let attaching = hosts::conn(cx, &host);
        cx.spawn_in(window, async move |shell, cx| {
            let mut attached = attaching.await;
            if needs_restart(&attached) {
                let Ok(restarting) = cx.update(|_, cx| hosts::restart(cx, &host)) else {
                    return;
                };
                attached = restarting.await;
            }
            if let Err(failure) = attached {
                shell
                    .update_in(cx, |shell: &mut Self, window, cx| {
                        shell.restart_failed(&host, failure, window, cx)
                    })
                    .ok();
            }
        })
        .detach();
    }

    /// Say why `host`'s service could not be restarted, once: on the form
    /// when it waits on that host, else in a toast. Whatever waits goes on
    /// waiting.
    fn restart_failed(
        &mut self,
        host: &HostName,
        failure: HostFailure,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let why = restart_failure(failure);
        let form = self
            .host_project
            .as_mut()
            .filter(|draft| draft.waiting.as_ref() == Some(host));
        match form {
            Some(draft) => draft.error = Some(why),
            None => window
                .push_notification(crate::toast::failed("Restarting the host service", why), cx),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
