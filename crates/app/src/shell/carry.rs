//! The workspace drive's carrier: what the window's workspace drive decides,
//! carried out in this window. Each effect is carried once, in the order
//! given; what a write comes to goes back to the drive.

use super::Shell;
use crate::state::Shared;
use gpui::{Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::workspace::drive::{Effect, Forgotten, Input, Refusal, Signal, Value};

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
                Effect::Write {
                    pending,
                    mut config,
                    dir,
                } => {
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
                Effect::Arm { token, after } => self.arm(token, after, cx),
                Effect::Sweep { token, roots } => self.sweep_git(token, roots, cx),
                Effect::Resweep => self.refresh_git(cx),
                Effect::GitRead => self.git_read(cx),
                Effect::Remember { dir, name } => super::storage::remember_name(&dir, &name, cx),
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
                        Signal::Removed(label) => crate::toast::info(format!("Removed {label}")),
                    };
                    window.push_notification(toast, cx);
                }
            }
        }
    }

    /// Carry out `fx` where no window is in hand: a burst's wait, or a
    /// sweep and what it read.
    pub(super) fn carry_workspace_quietly(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in fx {
            match effect {
                Effect::Arm { token, after } => self.arm(token, after, cx),
                Effect::Sweep { token, roots } => self.sweep_git(token, roots, cx),
                Effect::GitRead => self.git_read(cx),
                _ => debug_assert!(false, "only a wait or a sweep is carried without a window"),
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

    /// Let go of everything the window keeps for a project that has left.
    fn forgot(&mut self, gone: Forgotten, cx: &mut Context<Self>) {
        let Forgotten {
            path,
            host,
            offloaded,
        } = gone;
        // Its code is no longer checked for here; offloaded again, it comes
        // back as any does.
        if offloaded {
            crate::offload::forget(cx, &path);
        }
        // The editor's buffers, the tree and the shells go with a host's
        // folder too, or they come back under a later project on the same path.
        let on_host = host.is_some();
        self.workbench
            .update(cx, |panel, cx| panel.forget_root(&path, on_host, cx));
        self.terminal
            .update(cx, |panel, cx| panel.forget_root(&path, host, cx));
        // A folder on a host was never given the rest, and what is filed
        // under its path belongs to a folder of the same path here.
        if !on_host {
            self.issues_page.forget_root(&path, cx);
            // Its field goes with it: kept, the same folder added back found the
            // old command in a field its project no longer had.
            self.check_inputs.remove(&path);
            self.terminal_homes.forget(&path);
        }
        self.mru.remove(&path);
        self.folds.remove(&path);
        // A cycle's frozen order can name sessions that no longer exist.
        self.tab_cycle = None;
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
