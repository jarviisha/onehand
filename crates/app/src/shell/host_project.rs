//! Adding a project on a host, what waits here on a host's busy sessions or
//! on a lost host being back, and the question asked before a host's service
//! is restarted onto this build.

use super::Shell;
use super::confirm::Confirm;
use crate::dialogs::{add_refusal, busy_body, clone_refusal, clone_target};
use crate::hosts::{self, HostFailure};
pub(crate) use browse::Browse;
use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Subscription, Task, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::host::client::HostConn;
use onehand_core::host::drive::Landing;
use onehand_core::host::drive::{needs_restart, restart_failure, said_by_wait};
use onehand_core::host::sshconf;
use onehand_core::host::waits::{Want, landed_on};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::path::{Path, PathBuf};

/// The add-a-project-on-a-host form, while it is up. `Some` on the shell is
/// what puts it on screen.
///
/// Its fields are made with it and go with it, so the form always opens
/// empty and nothing typed into a cancelled one is found again.
pub(crate) struct HostProjectDraft {
    /// Which way the project comes: a folder already there, or a clone.
    pub(crate) way: Way,
    pub(crate) host: Entity<InputState>,
    /// *Folder*, or in a clone *Into*: the folder the clone is made in.
    pub(crate) folder: Entity<InputState>,
    /// The repository a clone is of.
    pub(crate) repo: Entity<InputState>,
    /// What the last *Add* ran into. Cleared by the next keystroke, because
    /// the reader has started answering it.
    pub(crate) error: Option<String>,
    /// The attach under way, by [`WindowWaits::attempt`](onehand_core::host::waits::WindowWaits::attempt): attaching to the host
    /// and asking it about the folder, or waiting on the host's sessions.
    attempt: Option<u64>,
    /// The host the attach waits on, its busy sessions to go idle.
    pub(crate) waiting: Option<HostName>,
    /// The aliases of the SSH config, read once as the form opens.
    pub(crate) aliases: Vec<String>,
    /// The folder list under *Folder*, once *Browse* is pressed.
    pub(crate) browse: Option<Browse>,
    /// The listing under way; a newer one replaces it, dropping it, so only
    /// the newest lands.
    _browsing: Option<Task<()>>,
    _aliases: Task<()>,
    _typing: [Subscription; 3],
}

/// The two ways the form adds a project on a host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Way {
    /// A folder the host already has.
    Folder,
    /// A repository the host clones, its folder the project.
    Clone,
}

impl HostProjectDraft {
    pub(crate) fn busy(&self) -> bool {
        self.attempt.is_some()
    }

    /// Say why the form could not go on: one failure line at a time, so the
    /// folder list's own failure, likely the same attach, gives way to it.
    fn failed(&mut self, why: String) {
        if let Some(browse) = self.browse.as_mut() {
            browse.error = None;
        }
        self.error = Some(why);
    }
}

/// Whether `path` is a folder on `host`: `Ok` to add it.
async fn folder_found(
    host: &HostName,
    conn: HostConn,
    path: &Path,
    cx: &mut gpui::AsyncApp,
) -> Result<(), HostFailure> {
    let asking = cx.update(|cx| hosts::is_dir(cx, host, conn, path.to_path_buf()));
    match asking.await? {
        true => Ok(()),
        false => Err(HostFailure::Other(format!(
            "There is no folder {} on {host}",
            path.display()
        ))),
    }
}

/// Have `host` clone `repo` into `target`: `Ok` to add it. A folder already
/// there is asked about first, so it is told apart from git's other
/// refusals by the host's answer rather than by git's words.
async fn cloned(
    host: &HostName,
    conn: HostConn,
    repo: String,
    target: &Path,
    cx: &mut gpui::AsyncApp,
) -> Result<(), HostFailure> {
    let asking = cx.update(|cx| hosts::is_dir(cx, host, conn.clone(), target.to_path_buf()));
    if asking.await? {
        return Err(HostFailure::Other(format!(
            "There is already {} on {host}",
            target.display()
        )));
    }
    let cloning = cx.update(|cx| hosts::clone_repo(cx, conn, repo, target.to_path_buf()));
    cloning.await.map_err(HostFailure::Other)
}

/// The question asked before `host`'s service is restarted, stopping `busy`.
fn restart_ask(host: &HostName, busy: &[HostSession], terms: usize) -> Confirm {
    Confirm {
        id: "restart-host",
        title: format!("Restart the host service on {host}?").into(),
        description: busy_body(host, busy, terms, onehand_core::chat::now_secs()).into(),
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
        let repo = cx.new(|cx| InputState::new(window, cx));
        // Every keystroke redraws: *Add* refuses until the fields say enough.
        // One typed into *Host* drops the folder list, another host's.
        let typing = [(&host, true), (&folder, false), (&repo, false)].map(|(input, is_host)| {
            cx.subscribe_in(
                input,
                window,
                move |shell: &mut Self, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        if let Some(draft) = shell.host_project.as_mut() {
                            draft.error = None;
                            if is_host {
                                draft.drop_browse();
                            }
                        }
                        cx.notify();
                    }
                },
            )
        });
        // A file read, so off the UI thread; the menu beside *Host* waits
        // for it.
        let reading = cx.background_executor().spawn(async {
            sshconf::ssh_dir().map_or_else(Vec::new, |dir| sshconf::aliases(&dir))
        });
        let aliases = cx.spawn(async move |shell, cx| {
            let aliases = reading.await;
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                if let Some(draft) = shell.host_project.as_mut() {
                    draft.aliases = aliases;
                    cx.notify();
                }
            });
        });
        host.focus_handle(cx).focus(window, cx);
        self.host_project = Some(HostProjectDraft {
            way: Way::Folder,
            host,
            folder,
            repo,
            error: None,
            attempt: None,
            waiting: None,
            aliases: Vec::new(),
            browse: None,
            _browsing: None,
            _aliases: aliases,
            _typing: typing,
        });
        cx.notify();
    }

    /// Put the form away. An attach under way goes on, and what it finds is
    /// dropped: it is about a form no longer there. So is the end of a wait
    /// it waits on, for the form.
    pub(crate) fn cancel_host_project(&mut self, cx: &mut Context<Self>) {
        self.host_waits.forget_add();
        if self.host_project.take().is_some() {
            self.prune_host_waits(cx);
            cx.notify();
        }
    }

    /// Attach to the host the form names, then ask it whether the folder is
    /// there, or have it clone the repository, and add the project when that
    /// goes.
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
        let repo = draft.repo.read(cx).value().trim().to_string();
        // What is added: the folder, or the one the clone makes in it.
        let (path, clone) = match draft.way {
            Way::Folder if add_refusal(&host, &folder).is_none() => (PathBuf::from(folder), None),
            Way::Clone if clone_refusal(&host, &repo, &folder).is_none() => {
                let Some(target) = clone_target(&repo, Path::new(&folder)) else {
                    return;
                };
                (target, Some(repo))
            }
            Way::Folder | Way::Clone => return,
        };
        let attempt = self.host_waits.attempt();
        if let Some(draft) = self.host_project.as_mut() {
            draft.error = None;
            draft.attempt = Some(attempt);
            draft.waiting = None;
        }
        cx.notify();
        let host = HostName(host);
        let attaching = hosts::conn(cx, &host);
        cx.spawn_in(window, async move |shell, cx| {
            let found = match (attaching.await, clone) {
                (Ok(conn), None) => folder_found(&host, conn, &path, cx).await,
                (Ok(conn), Some(repo)) => cloned(&host, conn, repo, &path, cx).await,
                (Err(failure), _) => Err(failure),
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    shell.host_project_found(attempt, host, path, found, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// The hosts of this window's projects, each once, in the rail's order.
    pub(crate) fn used_hosts(&self) -> Vec<HostName> {
        let mut used: Vec<HostName> = Vec::new();
        for host in self
            .window
            .workspace
            .roots
            .iter()
            .filter_map(|root| root.session_host().map(|(host, _)| host))
        {
            if !used.contains(host) {
                used.push(host.clone());
            }
        }
        used
    }

    /// The way picked at the top of the form. *Host* and *Folder* stay as
    /// they are; the failure was the other way's. Not while an attach is
    /// under way, which carries on with the way it started.
    pub(crate) fn pick_way(&mut self, way: Way, cx: &mut Context<Self>) {
        let Some(draft) = self
            .host_project
            .as_mut()
            .filter(|draft| !draft.busy() && draft.waiting.is_none())
        else {
            return;
        };
        draft.way = way;
        draft.error = None;
        cx.notify();
    }

    /// A host picked from the menu beside *Host*.
    pub(crate) fn pick_host(&mut self, host: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.host_project.as_mut() else {
            return;
        };
        // Setting a field's value says nothing to its subscription, so what
        // typing would clear is cleared here: the list was another host's,
        // and the failure was about the names before.
        draft.drop_browse();
        draft.error = None;
        draft
            .host
            .update(cx, |input, cx| input.set_value(host, window, cx));
        cx.notify();
    }

    /// What the host said about the folder the form names, or the clone it
    /// made, on attach `attempt`: `Ok` adds `path`.
    fn host_project_found(
        &mut self,
        attempt: u64,
        host: HostName,
        path: PathBuf,
        found: Result<(), HostFailure>,
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
            Ok(()) => {
                self.host_project = None;
                self.host_waits.forget_add();
                self.open_host_root(host, path, window, cx);
            }
            Err(HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why)) => {
                draft.failed(why)
            }
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
    /// mid-turn or terminals open: restart the session, or list the page again, once the host
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

    /// Session `uid` is out of reach of `host`, or with no `uid` the project
    /// page shows what it kept of it: restart the session, or list the page
    /// again, once the host is attached again.
    pub(super) fn host_lost(
        &mut self,
        uid: Option<u64>,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let want = uid.map_or(Want::PageBack, Want::Back);
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
        if !self.host_waits.wait(&host, want) {
            return;
        }
        let back = want.back();
        let listening = match back {
            true => {
                let lost = hosts::back(cx, &host);
                cx.spawn_in(window, {
                    let host = host.clone();
                    async move |shell, cx| {
                        // `None`: the host is not lost, attached since or
                        // taken, which the end reads.
                        let end = match lost {
                            Some(lost) => lost.await,
                            None => Ok(()),
                        };
                        shell
                            .update_in(cx, |shell: &mut Self, window, cx| {
                                shell.host_back(&host, end, window, cx)
                            })
                            .ok();
                    }
                })
            }
            false => {
                let idle = hosts::idle(cx, &host);
                cx.spawn_in(window, {
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
                })
            }
        };
        self.host_listening.insert((host, back), listening);
    }

    /// The wait for lost `host` to be back ended. Attached again, or found
    /// running another build, each session that waited is restarted and the
    /// page lists again: on a busy host, that is what puts them on its busy
    /// wait. Taken by another machine, or refused, nothing attaches again by
    /// itself: the sessions end on why, and the page says it.
    fn host_back(
        &mut self,
        host: &HostName,
        end: hosts::WaitEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host_listening.remove(&(host.clone(), true));
        let (state, busy) = (hosts::state(cx, host), hosts::found_busy(cx, host));
        let landed = self.host_waits.back_ended(host, &state, busy, end);
        self.carry_landed(host, landed, window, cx);
    }

    /// Carry out what a wait on `host` ending comes to, step by step.
    fn carry_landed(
        &mut self,
        host: &HostName,
        landed: Vec<Landing>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for step in landed {
            match step {
                Landing::Restart { uid, after_service } => {
                    self.restart_told(uid, after_service, after_service, window, cx)
                }
                Landing::List => self.chat.update(cx, |pane, cx| pane.list_host_sessions(cx)),
                Landing::Add => self.commit_host_project(window, cx),
                Landing::BackFailed { uid, why } => self
                    .chat
                    .update(cx, |pane, cx| pane.host_back_failed(uid, &why, cx)),
                Landing::WaitFailed { uid, why } => self
                    .chat
                    .update(cx, |pane, cx| pane.host_wait_failed(uid, &why, cx)),
                Landing::PageUnreached(why) => self
                    .chat
                    .update(cx, |pane, cx| pane.host_page_unreached(host, &why, cx)),
                Landing::PageFailed => self
                    .chat
                    .update(cx, |pane, cx| pane.host_page_failed(host, cx)),
                Landing::AddFailed(why) => {
                    if let Some(draft) = self.host_project.as_mut() {
                        draft.failed(why);
                    }
                }
            }
        }
        cx.notify();
    }

    /// Let go of what no longer waits here on a host: a session closed or
    /// attached, a page left, a form put away.
    pub(super) fn prune_host_waits(&mut self, cx: &App) {
        if self.host_waits.is_empty() {
            return;
        }
        let pane = self.chat.read(cx);
        let page = pane.page_waits_on();
        let away = pane.page_away_on();
        let form = self
            .host_project
            .as_ref()
            .and_then(|draft| draft.waiting.as_ref().zip(draft.attempt));
        let gone = self.host_waits.prune(|host, want| match want {
            Want::Session(uid) => pane.host_wait(uid, cx).as_ref() == Some(host),
            Want::Add(attempt) => form == Some((host, attempt)),
            Want::Page => page == Some(host),
            Want::Back(uid) => pane.host_lost(uid, cx).as_ref() == Some(host),
            Want::PageBack => away == Some(host),
        });
        // A host nothing here waits on is no longer listened to, so a wait
        // nobody anywhere listens to ends.
        for listened in gone {
            self.host_listening.remove(&listened);
        }
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
        self.host_listening.remove(&(host.clone(), false));
        let form = self.host_project.as_ref().and_then(|draft| draft.attempt);
        let Some((landed, released)) = self.host_waits.idle_ended(host, form, end) else {
            return;
        };
        // The form carried on is no longer waiting on the host.
        if let (true, Some(draft)) = (released, self.host_project.as_mut()) {
            draft.attempt = None;
            draft.waiting = None;
        }
        self.carry_landed(host, landed, window, cx);
    }

    /// *Restart now*, wherever something waits on `host`: ask before its
    /// service is restarted onto this build, stopping its busy sessions.
    pub(crate) fn ask_restart_host(
        &mut self,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (busy, terms) = hosts::busy(cx, &host);
        let ask = restart_ask(&host, &busy, terms);
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.restart_agreed(host.clone(), window, cx);
        });
    }

    /// The person said yes to restarting `host`'s service. Attached first,
    /// and restarted only if it is still busy. An attach that lands ends the
    /// host's wait, and what waited carries on from there; one that cannot
    /// reach the host ends it on why.
    fn restart_agreed(&mut self, host: HostName, window: &mut Window, cx: &mut Context<Self>) {
        let waited = hosts::waited(cx, &host);
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
                        shell.restart_failed(&host, waited, failure, window, cx)
                    })
                    .ok();
            }
        })
        .detach();
    }

    /// Say why `host`'s service could not be restarted, once. A host out of
    /// reach ended the wait on it with that reason, and every place that
    /// waited says it already; one still busy goes on waiting, and is said
    /// on the form when it waits on that host, else in a toast, as is any
    /// failure with nothing waiting.
    fn restart_failed(
        &mut self,
        host: &HostName,
        waited: bool,
        failure: HostFailure,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if said_by_wait(waited, &failure) {
            return;
        }
        let why = restart_failure(failure);
        let form = self
            .host_project
            .as_mut()
            .filter(|draft| draft.waiting.as_ref() == Some(host));
        match form {
            Some(draft) => draft.failed(why),
            None => window
                .push_notification(crate::toast::failed("Restarting the host service", why), cx),
        }
        cx.notify();
    }
}

mod browse;
#[cfg(test)]
mod tests;
