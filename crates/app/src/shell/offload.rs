//! Offloading a project to a host and running it here again, and what the
//! window shows of who holds an offloaded project's code.

use super::Shell;
use crate::offload;
use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Task, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::{InputEvent, InputState};
use onehand_core::host::sshconf;
use onehand_core::workspace::HostName;
use std::collections::HashMap;
use std::path::PathBuf;

/// The offload form, while it is up. `Some` on the shell is what puts it on
/// screen.
pub(crate) struct OffloadDraft {
    /// The project offloaded, by its folder: an index could name another
    /// project once the send lands.
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    pub(crate) host: Entity<InputState>,
    /// What the last *Offload* ran into, until the next keystroke.
    pub(crate) error: Option<String>,
    /// The host the code is going to, while it goes.
    pub(crate) sending: Option<HostName>,
    /// The aliases of the SSH config, read once as the form opens.
    pub(crate) aliases: Vec<String>,
    _aliases: Task<()>,
    _typing: Subscription,
    _sending: Option<Task<()>>,
}

/// Why *Offload* cannot go yet: the host is unnamed, or `busy` sessions here
/// are mid-turn, which offloading would close.
pub(crate) fn offload_refusal(host: &str, busy: usize) -> Option<String> {
    match busy {
        _ if host.trim().is_empty() => Some("Name the host".into()),
        0 => None,
        1 => Some("Wait for the session here to finish".into()),
        n => Some(format!("Wait for {n} sessions here to finish")),
    }
}

impl Shell {
    pub(crate) fn offload_draft(&self) -> Option<&OffloadDraft> {
        self.offload_draft.as_ref()
    }

    /// How many of `root`'s sessions here are mid-turn.
    pub(crate) fn busy_here(&self, root: &std::path::Path, cx: &gpui::App) -> usize {
        let Some(root) = self
            .window
            .workspace
            .roots
            .iter()
            .find(|r| r.local_path() == Some(root))
        else {
            return 0;
        };
        let pane = self.chat.read(cx);
        root.sessions
            .iter()
            .filter(|s| pane.turn_in_flight(s.uid, cx))
            .count()
    }

    /// Open the offload form for root `idx`, a folder here whose sessions run
    /// here.
    pub(crate) fn begin_offload(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self
            .window
            .workspace
            .roots
            .get(idx)
            .filter(|r| r.runs_here())
        else {
            return;
        };
        if self.offload_draft.is_some() || root.transient {
            return;
        }
        let (path, label) = (root.path.clone(), root.label.clone());
        self.remember_focus(window, cx);
        let host = cx.new(|cx| InputState::new(window, cx));
        let typing = cx.subscribe_in(
            &host,
            window,
            |shell: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(draft) = shell.offload_draft.as_mut() {
                        draft.error = None;
                    }
                    cx.notify();
                }
            },
        );
        let reading = cx.background_executor().spawn(async {
            sshconf::ssh_dir().map_or_else(Vec::new, |dir| sshconf::aliases(&dir))
        });
        let aliases = cx.spawn(async move |shell, cx| {
            let aliases = reading.await;
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                if let Some(draft) = shell.offload_draft.as_mut() {
                    draft.aliases = aliases;
                    cx.notify();
                }
            });
        });
        host.focus_handle(cx).focus(window, cx);
        self.offload_draft = Some(OffloadDraft {
            root: path,
            label,
            host,
            error: None,
            sending: None,
            aliases: Vec::new(),
            _aliases: aliases,
            _typing: typing,
            _sending: None,
        });
        cx.notify();
    }

    /// A host picked from the menu beside the form's *Host*.
    pub(crate) fn pick_offload_host(
        &mut self,
        host: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.offload_draft.as_mut() {
            draft.error = None;
            draft
                .host
                .update(cx, |input, cx| input.set_value(host, window, cx));
            cx.notify();
        }
    }

    /// Put the form away. A send under way is dropped with it; what it had
    /// copied stays in the mirror, and the project stays here.
    pub(crate) fn cancel_offload(&mut self, cx: &mut Context<Self>) {
        if self.offload_draft.take().is_some() {
            cx.notify();
        }
    }

    /// Send the project's code to the host the form names and, once it is
    /// there, run the project's new sessions on it. Its sessions here close;
    /// their conversations stay among its past ones.
    pub(crate) fn commit_offload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.offload_draft.as_ref().filter(|d| d.sending.is_none()) else {
            return;
        };
        let host = draft.host.read(cx).value().trim().to_string();
        let path = draft.root.clone();
        if offload_refusal(&host, self.busy_here(&path, cx)).is_some() {
            return;
        }
        let host = HostName(host);
        let sending = cx.spawn_in(window, {
            let (path, host) = (path.clone(), host.clone());
            async move |shell, cx| {
                let went = offload::offload_to(cx, path.clone(), host.clone()).await;
                let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                    shell.offload_landed(path, host, went, window, cx)
                });
            }
        });
        if let Some(draft) = self.offload_draft.as_mut() {
            draft.error = None;
            draft.sending = Some(host);
            draft._sending = Some(sending);
        }
        cx.notify();
    }

    fn offload_landed(
        &mut self,
        path: PathBuf,
        host: HostName,
        went: Result<(PathBuf, onehand_core::host::offload::OffloadState), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.offload_draft.as_mut().filter(|d| d.root == path) else {
            return;
        };
        draft.sending = None;
        let (mirror, state) = match went {
            Ok(went) => went,
            Err(why) => {
                draft.error = Some(why);
                cx.notify();
                return;
            }
        };
        // A turn started while the code went would be cut off by the close.
        if let Some(why) = offload_refusal(&host.0, self.busy_here(&path, cx)) {
            if let Some(draft) = self.offload_draft.as_mut() {
                draft.error = Some(why);
            }
            cx.notify();
            return;
        }
        self.offload_draft = None;
        let Some(idx) = self
            .window
            .workspace
            .roots
            .iter()
            .position(|r| r.local_path() == Some(path.as_path()))
        else {
            return;
        };
        let here: Vec<u64> = self.window.workspace.roots[idx]
            .sessions
            .iter()
            .map(|s| s.uid)
            .collect();
        for uid in here {
            self.drop_session(uid, cx);
        }
        self.window.workspace.roots[idx].offload = Some((host.clone(), mirror.clone()));
        offload::track(cx, path, host, Some(state));
        self.save_workspace_soon(cx);
        self.show_active_session(window, cx);
        self.sync_offload(cx);
        cx.notify();
    }

    /// Run root `idx`'s new sessions here again, while this machine holds its
    /// code. Its sessions on the host are let go here, and run on there until
    /// they end; the mirror stays, so a later offload sends only what changed.
    pub(crate) fn run_here_again(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.window.workspace.roots.get(idx) else {
            return;
        };
        let Some((host, _)) = root.offload.clone() else {
            return;
        };
        let path = root.path.clone();
        if !offload::get(cx, &path).is_some_and(offload::Offload::here) {
            window.push_notification(
                crate::toast::warning(format!(
                    "{host} has the code; it runs here again once its sessions finish"
                )),
                cx,
            );
            return;
        }
        let there: Vec<u64> = root.sessions.iter().map(|s| s.uid).collect();
        for uid in there {
            self.drop_session(uid, cx);
        }
        self.window.workspace.roots[idx].offload = None;
        offload::forget(cx, &path);
        self.save_workspace_soon(cx);
        self.show_active_session(window, cx);
        self.sync_offload(cx);
        cx.notify();
    }

    /// *Take the host's code*, on a project whose copy back stopped: asked
    /// first, naming where this machine's versions of what changed go.
    pub(crate) fn ask_take_host_code(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(o) = offload::get(cx, &path) else {
            return;
        };
        let host = o.host.clone();
        let into = offload::backup_dir(&path);
        let ask = super::confirm::Confirm {
            id: "take-host-code",
            title: format!("Take {host}'s code over these changes?").into(),
            description: format!(
                "This machine's versions of what changed are copied to {} first; then \
                 {host}'s code is brought back over them.",
                into.display()
            )
            .into(),
            act: "Take the host's code",
            ..Default::default()
        };
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.take_host_code(path.clone(), host.clone(), into.clone(), window, cx)
        });
    }

    fn take_host_code(
        &mut self,
        path: PathBuf,
        host: HostName,
        into: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |_, cx| {
            let took = offload::take_over(cx, path, into.clone()).await;
            // A failure stands on the project's page, where it was pressed.
            let Ok(taken) = took else {
                return;
            };
            if !taken {
                let _ = cx.update(|window, cx| {
                    window.push_notification(
                        crate::toast::info(format!(
                            "{host} had given the code back already; nothing was taken, and \
                             what changed here stays"
                        )),
                        cx,
                    );
                });
                return;
            }
            let at = into.clone();
            let backed = cx
                .background_executor()
                .spawn(async move { at.is_dir() })
                .await;
            let _ = cx.update(|window, cx| {
                let toast = match backed {
                    true => crate::toast::success(format!(
                        "Took {host}'s code; this machine's versions are in {}",
                        into.display()
                    )),
                    false => crate::toast::success(format!(
                        "Took {host}'s code; nothing here had changed any more"
                    )),
                };
                window.push_notification(toast, cx);
            });
        })
        .detach();
    }

    /// *Send to <host>* on `path`, a file or folder of an offloaded project:
    /// copied to the same place in its copy on the host, whoever holds the
    /// code, and how it went said in a toast.
    pub(crate) fn send_to_host(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((folder, host, mirror)) = self.window.workspace.roots.iter().find_map(|r| {
            let (host, mirror) = r.offload.clone()?;
            path.starts_with(&r.path)
                .then(|| (r.path.clone(), host, mirror))
        }) else {
            return;
        };
        let rel = path.strip_prefix(&folder).unwrap_or(&path).to_path_buf();
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let addr = host.addr();
        let sending = cx.background_executor().spawn(async move {
            onehand_core::host::offload::send_path_blocking(&addr, &folder, &mirror, &rel)
        });
        cx.spawn_in(window, async move |_, cx| {
            let sent = sending.await;
            let _ = cx.update(|window, cx| {
                let toast = match sent {
                    Ok(()) => crate::toast::success(format!("Sent {name} to {host}")),
                    Err(why) => crate::toast::failed(&format!("Sending {name} to {host}"), why),
                };
                window.push_notification(toast, cx);
            });
        })
        .detach();
    }

    /// Keep every offloaded root of this window, and tell the Workbench and
    /// the terminal which folders are locked; only when that changed.
    pub(super) fn sync_offload(&mut self, cx: &mut Context<Self>) {
        let kept: Vec<_> = self
            .window
            .workspace
            .roots
            .iter()
            .filter_map(|r| Some((r.path.clone(), r.offload.clone()?.0)))
            .collect();
        let offloaded: HashMap<PathBuf, String> = kept
            .iter()
            .map(|(path, host)| (path.clone(), host.0.clone()))
            .collect();
        self.workbench
            .update(cx, |panel, cx| panel.set_offloaded(&offloaded, cx));
        for (path, host) in kept {
            offload::track(cx, path, host, None);
        }
        let locks: HashMap<PathBuf, String> = offload::lock_lines(cx)
            .into_iter()
            // By the folder, whichever window offloaded it: one folder here
            // holds one copy of its code.
            .filter(|(path, _)| {
                self.window
                    .workspace
                    .roots
                    .iter()
                    .any(|r| r.local_path() == Some(path.as_path()))
            })
            .collect();
        if locks != self.offload_locks {
            self.workbench
                .update(cx, |panel, cx| panel.set_locked(&locks, cx));
            self.terminal
                .update(cx, |panel, cx| panel.set_locked(&locks, cx));
            self.offload_locks = locks;
        }
        // The page's menu offers *Run here again* by who holds the code, and
        // its box says who does.
        self.sync_project_facts(cx);
        self.chat.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}
