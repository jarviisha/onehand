use super::Shell;
use super::boot::open_or_focus;
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity};
use gpui_component::WindowExt as _;
use gpui_component::input::InputState;
use gpui_component::notification::Notification;
use onehand_core::config::recents::{Effect as RecentsEffect, Input, Listed, Signal};
use onehand_core::config::{WorkspaceConfig, WorkspaceLoad};
use onehand_core::workspace::{self, Workspace};
use std::path::Path;

impl Shell {
    /// Write the workspace once the user stops changing it: a drag changes
    /// the layout frame after frame, so each change only arms the drive's
    /// wait again, and the write comes once it runs out.
    pub(super) fn save_workspace_soon(&mut self, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            self.saves_asked += 1;
        }
        let fx = self.window.workspace.save_soon();
        self.carry_workspace_quietly(fx, cx);
    }

    /// Write a burst's write now, if one waits: a window closing does this
    /// last, so the change made just before it is not lost with it.
    ///
    /// On the same ordered writer as every other write, so an older one still
    /// queued cannot land over it, and waited for, briefly: the window is
    /// going.
    pub(super) fn flush_workspace(&mut self, cx: &mut Context<Self>) {
        let fx = self.window.workspace.flush().into_iter().collect();
        self.carry_workspace_quietly(fx, cx);
    }

    /// Write `config` into `dir` at once, waiting briefly: what a closing
    /// window does last. Its answer goes back to the drive.
    pub(super) fn write_on_close(
        &mut self,
        write: onehand_core::workspace::drive::FileWrite,
        cx: &mut Context<Self>,
    ) {
        let onehand_core::workspace::drive::FileWrite {
            pending,
            mut config,
            dir,
        } = write;
        config.layout = self.dock_layout(cx);
        let result = Shared::global(cx).writer.write_within(
            move || config.save_to(&dir).map_err(|why| why.to_string()),
            CLOSE_WAIT,
        );
        if let Err(why) = &result {
            eprintln!("onehand: the workspace was not saved on closing: {why}");
        }
        let fx = self
            .window
            .workspace
            .handle(onehand_core::workspace::drive::Input::WrittenOnClose { pending, result });
        self.carry_workspace_quietly(fx, cx);
    }

    // ── Workspace settings ──────────────────────────────────────────────────

    /// Ask, then delete this window's workspace.
    pub fn confirm_delete_workspace(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let name = self.window.workspace.name.clone();
        let ask = super::confirm::Confirm {
            id: "delete-workspace",
            title: format!("Delete {name}?").into(),
            description: "Its projects stay on disk. Its issues kept in onehand go with it.".into(),
            act: "Delete",
            ..Default::default()
        };
        self.confirm(ask, window, cx, |shell, window, cx| {
            shell.delete_workspace(window, cx)
        });
    }

    /// Delete this window's workspace, as its drive answers.
    fn delete_workspace(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let fx = self.window.workspace.delete();
        self.carry_workspace(fx, window, cx);
    }

    /// Carry a deleted workspace out: its views write into `dir` no more,
    /// what is its own there goes once every write queued before has landed,
    /// on the same ordered writer, it leaves the list, and its window closes.
    pub(super) fn carry_deleted(
        &mut self,
        dir: std::path::PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.workbench
            .update(cx, |panel, cx| panel.set_storage(None, cx));
        self.tell_issues_page(&onehand_workbench::Request::SetStorage(None), cx);
        let removed = Shared::global(cx).writer.write({
            let dir = dir.clone();
            move || workspace::remove_workspace_blocking(&dir)
        });
        cx.spawn(async move |_, _| {
            if let Err(why) = removed.await {
                eprintln!("onehand: the deleted workspace's folder stayed: {why}");
            }
        })
        .detach();
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.recents.dropped(&dir));
        carry_recents(fx, cx);
        window.remove_window();
    }

    /// The conversation-rename field, for the dialog that edits it.
    pub fn rename_input(&self) -> &Entity<InputState> {
        &self.rename_input
    }

    /// Whether the conversation being renamed already carries a name the user
    /// set, which is the only case where *Use the automatic title* has anything
    /// to undo.
    pub fn rename_is_override(&self, cx: &App) -> bool {
        self.renaming
            .is_some_and(|uid| self.chat.read(cx).custom_title(uid, cx).is_some())
    }

    pub fn workspace_name_input(&self) -> &Entity<InputState> {
        &self.workspace_name
    }

    pub fn storage_dir(&self) -> &std::path::PathBuf {
        &self.window.workspace.storage_dir
    }

    /// Pick a project folder and open it as a *new* workspace in its own window.
    ///
    /// The workspace is written before the window opens, and a write that fails
    /// opens nothing: a workspace on screen that nothing on disk remembers would
    /// be gone at the next launch.
    ///
    /// It is written to `workspace::storage_for` and never into the project
    /// itself, and where that lands is stable for a given folder -- which is
    /// what makes the overwrite guard mean something here: creating a workspace
    /// on a folder that already has one opens it instead of starting a second
    /// one nothing distinguishes from the first.
    pub fn new_workspace(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(root) = pick_folder(cx).await else {
                return;
            };
            let dir = workspace::storage_for(&root);

            match workspace_in(&dir, cx).await {
                Ok(Some(workspace)) => {
                    cx.update(|cx| open_or_focus(workspace, cx));
                    say(
                        &shell,
                        crate::toast::info(format!(
                            "{} already has a workspace — opened it",
                            root.display()
                        )),
                        cx,
                    );
                }
                Ok(None) => {
                    let mut workspace = Workspace::seeded(root.clone());
                    // The folder's name, because a workspace per project is what
                    // this makes and every one of them being called "Workspace"
                    // leaves the rail's identity row naming nothing.
                    workspace.name = workspace::label_for(&root);
                    let written = cx
                        .background_executor()
                        .spawn({
                            let (dir, config) = (dir.clone(), workspace.to_config());
                            async move { config.save_to(&dir) }
                        })
                        .await;
                    match written {
                        Ok(()) => {
                            cx.update(|cx| open_or_focus(workspace, cx));
                        }
                        // Opening it anyway would be the ghost this exists to
                        // prevent: a workspace on screen that nothing on disk
                        // remembers.
                        Err(e) => say(
                            &shell,
                            crate::toast::failed(
                                "Creating the workspace",
                                format!("{} could not be written: {e}", dir.display()),
                            ),
                            cx,
                        ),
                    }
                }
                Err(message) => say(
                    &shell,
                    crate::toast::failed("Creating the workspace", message),
                    cx,
                ),
            }
        })
        .detach();
    }

    /// Every workspace onehand keeps, most recently opened first.
    pub fn recents(&self, cx: &App) -> Vec<Listed> {
        Shared::global(cx).recents.listed().to_vec()
    }

    /// Open a recents row: read its config off the UI loop, then funnel into
    /// the single open path so dedup-focus applies here too.
    /// Forget the badge on the conversation this window is showing.
    ///
    /// A pass-through, for the bridge: coming back from away has to clear it,
    /// and the bridge is where both ways of coming back meet.
    pub fn mark_active_seen(&mut self, cx: &mut Context<Self>) {
        self.chat.update(cx, |pane, cx| pane.mark_active_seen(cx));
    }

    pub fn open_recent(&mut self, dir: std::path::PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let loaded = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { WorkspaceConfig::load_from(&dir) }
                })
                .await;

            match loaded {
                WorkspaceLoad::Found(cfg) => {
                    cx.update(|cx| open_or_focus(Workspace::from_config(cfg, dir), cx));
                }
                // Gone for good: no such file. Stale, not an error the user
                // caused, so drop it rather than nag.
                WorkspaceLoad::Missing => {
                    shell
                        .update(cx, |_shell, cx| {
                            let fx = cx.update_global::<Shared, _>(|shared, _| {
                                shared.recents.dropped(&dir)
                            });
                            carry_recents(fx, cx);
                            cx.notify();
                        })
                        .ok();
                }
                // Present but unreadable -- an unmounted share, a permission
                // blip, a TOML typo. Every one of those is recoverable, and
                // forgetting the recent is not: say so and keep the row.
                WorkspaceLoad::Unreadable(why) => {
                    shell
                        .update_in(cx, |_, window, cx| {
                            window.push_notification(
                                crate::toast::failed(
                                    "Reading the workspace",
                                    format!(
                                        "{} cannot be read, and is kept as it is: {why}",
                                        dir.display()
                                    ),
                                ),
                                cx,
                            );
                            cx.notify();
                        })
                        .ok();
                }
            }
        })
        .detach();
    }
}

/// How long a closing window waits for its last workspace write.
const CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Run the native folder picker off the UI loop.
pub(super) async fn pick_folder(cx: &mut gpui::AsyncApp) -> Option<std::path::PathBuf> {
    cx.background_executor()
        .spawn(async {
            rfd::FileDialog::new()
                .pick_folder()
                .map(workspace::canon_dir)
        })
        .await
}

/// Read what a storage folder already holds, off the UI loop.
///
/// **The overwrite guard, in one place**, because it is one rule wherever a
/// workspace is about to be written: `Ok(Some)` is a workspace already there,
/// to be opened rather than replaced; `Ok(None)` a folder free to write into;
/// and `Err` a config that exists and could not be read. That last one must
/// never read as an empty folder, or a workspace config with one bad character
/// is a workspace deleted by a folder picker -- and the sentence saying so
/// lives here too, so the app cannot come to refuse the same thing in two
/// different words.
async fn workspace_in(dir: &Path, cx: &mut gpui::AsyncApp) -> Result<Option<Workspace>, String> {
    let loaded = cx
        .background_executor()
        .spawn({
            let dir = dir.to_path_buf();
            async move { WorkspaceConfig::load_from(&dir) }
        })
        .await;
    match loaded {
        WorkspaceLoad::Found(cfg) => Ok(Some(Workspace::from_config(cfg, dir.to_path_buf()))),
        WorkspaceLoad::Missing => Ok(None),
        WorkspaceLoad::Unreadable(_) => Err(format!(
            "{} already holds a workspace config that cannot be read — nothing was changed",
            dir.display()
        )),
    }
}

/// Put a line on the window that asked, if it is still there.
fn say(shell: &gpui::WeakEntity<Shell>, note: Notification, cx: &mut gpui::AsyncApp) {
    shell
        .update_in(cx, |_, window, cx| {
            window.push_notification(note, cx);
            cx.notify();
        })
        .ok();
}

/// Carry out the workspace list's effects, in order, each once: a write on
/// the ordered writer every settings file goes through, its answer reported
/// back, and what the drive says.
pub(super) fn carry_recents(fx: Vec<RecentsEffect>, cx: &mut App) {
    for effect in fx {
        match effect {
            RecentsEffect::Write {
                pending,
                state,
                path,
            } => {
                let written = Shared::global(cx)
                    .writer
                    .write(move || state.save_to(&path).map_err(|e| e.to_string()));
                cx.spawn(async move |cx| {
                    let result = written.await;
                    cx.update(|cx| {
                        let fx = cx.update_global::<Shared, _>(|shared, _| {
                            shared.recents.handle(Input::Written { pending, result })
                        });
                        carry_recents(fx, cx);
                    });
                })
                .detach();
            }
            RecentsEffect::Signal(Signal::WriteFailed(why)) => {
                eprintln!("onehand: failed to save recents: {why}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
