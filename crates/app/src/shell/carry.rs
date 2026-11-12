//! The workspace drive's carrier: what the window's workspace drive decides,
//! carried out in this window. Each effect is carried once, in the order
//! given; what a write comes to goes back to the drive.

use super::Shell;
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::workspace::drive::{Effect, FileWrite, Forgotten, Input, Refusal, Signal, Value};
use std::path::PathBuf;

/// What a start is told when the project it names has left the workspace.
pub(crate) const PROJECT_GONE: &str = "That project is no longer in this workspace";

impl Shell {
    /// Carry out `fx`, in order.
    pub(super) fn carry_workspace(
        &mut self,
        fx: Vec<Effect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for effect in fx {
            match effect {
                Effect::Close(uid) => self.let_go_of(uid, cx),
                Effect::ForgotOffload(path) => crate::offload::forget(cx, &path),
                Effect::ForgotHere(path) => self.forgot_here(path, cx),
                Effect::Forgot(gone) => self.forgot(gone, cx),
                Effect::Show => self.show_active_session(window, cx),
                Effect::Changed => {
                    // The project page's menu says what the tree holds too.
                    self.sync_project_facts(cx);
                    cx.notify();
                }
                Effect::LookForRuns(path) => {
                    crate::unattended::check_now(self.project_for_runs(&path), cx)
                }
                Effect::Put(value) => self.put_back(value, window, cx),
                Effect::Write(write) => {
                    let FileWrite {
                        pending,
                        mut config,
                        dir,
                    } = *write;
                    config.layout = self.dock_layout(cx);
                    let written = Shared::global(cx)
                        .writer
                        .write(move || config.save_to(&dir).map_err(|why| why.to_string()));
                    cx.spawn_in(window, async move |shell, cx| {
                        let result = written.await;
                        let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                            let fx = shell
                                .window
                                .workspace
                                .handle(Input::Written { pending, result });
                            shell.carry_workspace(fx, window, cx);
                        });
                    })
                    .detach();
                }
                Effect::WriteOnClose(write) => self.write_on_close(*write, cx),
                Effect::Arm { token, after } => self.arm(token, after, cx),
                Effect::Sweep { token, roots } => self.sweep_git(token, roots, cx),
                Effect::Resweep => self.refresh_git(cx),
                Effect::GitRead => self.git_read(cx),
                Effect::Remember { dir, name } => {
                    let fx = cx
                        .update_global::<Shared, _>(|shared, _| shared.recents.named(&dir, &name));
                    super::storage::carry_recents(fx, None, cx);
                }
                Effect::EndOnHost {
                    host,
                    session,
                    label,
                } => self.carry_end_on_host(host, session, label, cx),
                Effect::ListHostSessions => {
                    self.chat.update(cx, |pane, cx| pane.list_host_sessions(cx))
                }
                Effect::ReadConversation(id) => self.read_conversation(id, window, cx),
                Effect::Signal(signal) => {
                    let toast = match signal {
                        Signal::SaveFailed(why) => {
                            crate::toast::failed("Saving the workspace", why)
                        }
                        Signal::Refused(Refusal::NoProject) => {
                            crate::toast::warning("Add a project before starting a session")
                        }
                        Signal::Refused(Refusal::NoAgent) => crate::toast::warning(
                            "Add an agent in Settings before starting a session",
                        ),
                        Signal::Refused(Refusal::NoSession) => {
                            crate::toast::warning("That task has no session open")
                        }
                        Signal::Refused(Refusal::ProjectGone) => {
                            crate::toast::warning(PROJECT_GONE)
                        }
                        Signal::Refused(Refusal::NotHere) => {
                            crate::toast::warning("Run check runs on a project on this machine")
                        }
                        Signal::Refused(Refusal::NoCheck) => {
                            crate::toast::warning("This project has no check command to run")
                        }
                        Signal::Removed(label) => crate::toast::info(format!("Removed {label}")),
                        Signal::ConversationNotSaved => {
                            crate::toast::warning("That session's conversation was not saved")
                        }
                        Signal::AgentGone(agent) => crate::toast::warning(format!(
                            "{agent} is no longer set up; add it in Settings ▸ Agents to open that conversation"
                        )),
                        Signal::FolderGone(root) => crate::toast::warning(format!(
                            "The folder that conversation ran in is gone: {}",
                            root.display()
                        )),
                        Signal::ForkFailed(why) => {
                            crate::toast::failed("Forking the conversation", why)
                        }
                        Signal::EndOnHostFailed { host, label, why } => {
                            crate::toast::failed(&format!("Ending {label} on {host}"), why)
                        }
                    };
                    window.push_notification(toast, cx);
                }
            }
        }
    }

    /// Carry out `fx` where no window is in hand: a burst's wait, or a sweep
    /// and what it read, at once; from the first effect that needs the
    /// window on, the rest in order once the window can be reached, after
    /// this update.
    pub(super) fn carry_workspace_quietly(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) {
        let mut fx = fx.into_iter();
        while let Some(effect) = fx.next() {
            match effect {
                Effect::Arm { token, after } => self.arm(token, after, cx),
                Effect::Sweep { token, roots } => self.sweep_git(token, roots, cx),
                Effect::GitRead => self.git_read(cx),
                Effect::WriteOnClose(write) => self.write_on_close(*write, cx),
                effect @ (Effect::Close(_)
                | Effect::ForgotOffload(_)
                | Effect::ForgotHere(_)
                | Effect::Forgot(_)
                | Effect::Show
                | Effect::Changed
                | Effect::Put(_)
                | Effect::Write(_)
                | Effect::LookForRuns(_)
                | Effect::Resweep
                | Effect::Remember { .. }
                | Effect::EndOnHost { .. }
                | Effect::ListHostSessions
                | Effect::ReadConversation(_)
                | Effect::Signal(_)) => {
                    let rest: Vec<Effect> = std::iter::once(effect).chain(fx).collect();
                    let (at, shell) = (self.chat.read(cx).window_handle(), cx.entity().downgrade());
                    cx.defer(move |cx| {
                        let _ = at.update(cx, |_, window, cx| {
                            shell.update(cx, |shell, cx| shell.carry_workspace(rest, window, cx))
                        });
                    });
                    return;
                }
            }
        }
    }

    /// Wait `after`, then tell the drive, in this window: it is told the
    /// window it is in, since a wait may be armed where none is in hand.
    fn arm(&mut self, token: u64, after: std::time::Duration, cx: &mut Context<Self>) {
        let at = self.chat.read(cx).window_handle();
        self._pending_save = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(after).await;
            let _ = at.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    let fx = shell.window.workspace.handle(Input::Elapsed { token });
                    shell.carry_workspace(fx, window, cx);
                })
            });
        }));
    }

    /// End host session `session` on `host`, called `label`, there: attached
    /// as a person, a busy host asked now rather than waited on, and what it
    /// came to told to the drive while the window is there to hear it.
    fn carry_end_on_host(
        &mut self,
        host: onehand_core::workspace::HostName,
        session: onehand_core::host::wire::HostSession,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let at = self.chat.read(cx).window_handle();
        let attaching = crate::hosts::conn(cx, &host);
        cx.spawn(async move |shell, cx| {
            let result = match attaching.await {
                Ok(conn) => {
                    let ending = cx.update(|cx| {
                        crate::hosts::on_runtime(
                            cx,
                            async move { conn.end_session(&session).await },
                        )
                    });
                    ending.await.map_err(|e| e.to_string())
                }
                Err(failure) => Err(failure.said()),
            };
            let input = Input::EndedOnHost {
                host,
                label,
                result,
            };
            let _ = at.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    let fx = shell.window.workspace.handle(input);
                    shell.carry_workspace(fx, window, cx);
                })
            });
        })
        .detach();
    }

    /// Let session `uid` go: the pane drops it, which ends its agent, and
    /// nothing the window keeps names it any more.
    fn let_go_of(&mut self, uid: u64, cx: &mut Context<Self>) {
        self.chat.update(cx, |pane, cx| pane.close(uid, cx));
        for order in self.mru.values_mut() {
            order.retain(|&seen| seen != uid);
        }
        // A cycle's frozen order can name a session that no longer exists.
        self.tab_cycle = None;
    }

    /// Let go of what every project has, one that has left.
    fn forgot(&mut self, gone: Forgotten, cx: &mut Context<Self>) {
        let Forgotten { path, host } = gone;
        let on_host = host.is_some();
        self.workbench
            .update(cx, |panel, cx| panel.forget_root(&path, on_host, cx));
        self.terminal
            .update(cx, |panel, cx| panel.forget_root(&path, host, cx));
        self.mru.remove(&path);
        self.folds.remove(&path);
        // A cycle's frozen order can name sessions that no longer exist.
        self.tab_cycle = None;
    }

    /// Let go of what is filed under the path of a folder here that left.
    fn forgot_here(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.issues_page.forget_root(&path, cx);
        // Its field goes with it: kept, the same folder added back found the
        // old command in a field its project no longer had.
        self.check_inputs.remove(&path);
        self.terminal_homes.forget(&path);
        // A form about it goes too: left up, it would run git on a folder
        // that is no longer a project here.
        self.worktree_draft.take_if(|draft| draft.root == path);
        self.branch_draft.take_if(|draft| draft.root == path);
    }

    /// Put a value whose write failed back on screen.
    fn put_back(&mut self, value: Value, window: &mut Window, cx: &mut Context<Self>) {
        match value {
            Value::Name(name) => self
                .workspace_name
                .update(cx, |input, cx| input.set_value(name, window, cx)),
            Value::Check { path, check } => {
                if let Some(input) = self.check_inputs.get(&path).cloned() {
                    let text = check.unwrap_or_default();
                    input.update(cx, |input, cx| input.set_value(text, window, cx));
                }
            }
            // Drawn from the tree, which the drive has put back.
            Value::Order(_) | Value::Pin { .. } | Value::Unattended { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests;
