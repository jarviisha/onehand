//! Adding a project on a host, and the question asked before a host's service
//! is restarted onto this build.

use super::Shell;
use super::confirm::Confirm;
use crate::dialogs::{add_refusal, busy_body};
use crate::hosts::{self, HostFailure};
use futures::FutureExt as _;
use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
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
    /// Attaching to the host and asking it about the folder.
    pub(crate) busy: bool,
    _typing: [Subscription; 2],
}

/// What a failed attach says once the form is gone.
fn said(failure: HostFailure) -> String {
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
            busy: false,
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
    /// there, and add the project when it is; with `fresh`, restart the
    /// host's service onto this build first, as the person agreed to.
    ///
    /// The form stays up until the host answers, so what went wrong is shown
    /// against the names that caused it.
    pub(crate) fn commit_host_project(
        &mut self,
        fresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.host_project.as_mut() else {
            return;
        };
        if draft.busy {
            return;
        }
        let host = draft.host.read(cx).value().trim().to_string();
        let folder = draft.folder.read(cx).value().trim().to_string();
        if add_refusal(&host, &folder).is_some() {
            return;
        }
        draft.error = None;
        draft.busy = true;
        cx.notify();
        let (host, path) = (HostName(host), PathBuf::from(folder));
        let attaching = match fresh {
            true => hosts::restart(cx, &host).boxed_local(),
            false => hosts::conn(cx, &host).boxed_local(),
        };
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
                    shell.host_project_found(host, path, found, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// What the host said about the folder the form names.
    fn host_project_found(
        &mut self,
        host: HostName,
        path: PathBuf,
        found: Result<bool, HostFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only the form this was started from: one cancelled and opened
        // again since is not waiting on it.
        let Some(draft) = self.host_project.as_mut().filter(|draft| draft.busy) else {
            return;
        };
        draft.busy = false;
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
                self.confirm(
                    restart_ask(&host, &busy),
                    window,
                    cx,
                    |shell, window, cx| shell.commit_host_project(true, window, cx),
                );
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
        let ask = restart_ask(&host, busy);
        self.confirm(ask, window, cx, move |_, window, cx| {
            let restarting = hosts::restart(cx, &host);
            cx.spawn_in(window, async move |shell, cx| {
                let restarted = restarting.await;
                shell
                    .update_in(cx, |shell: &mut Self, window, cx| match restarted {
                        Ok(_) => shell.restart_now(uid, window, cx),
                        Err(failure) => window.push_notification(
                            crate::toast::failed("Restarting the host service", said(failure)),
                            cx,
                        ),
                    })
                    .ok();
            })
            .detach();
        });
    }
}
