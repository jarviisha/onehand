use super::boot::open_or_focus;
use super::{SAVE_DEBOUNCE, Shell};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::InputState;
use gpui_component::notification::Notification;
use onehand_core::config::{WorkspaceConfig, WorkspaceLoad};
use onehand_core::workspace::{self, Workspace};
use std::path::{Path, PathBuf};

impl Shell {
    /// Write the workspace once the user stops changing it.
    ///
    /// Two callers, one problem: the dock emits `LayoutChanged` on **every
    /// frame of a drag**, and the rename field emits `Change` on **every
    /// keystroke**. Either one writing on the event is a file write per frame
    /// or per character, on the UI thread. Each call replaces
    /// the pending task, and dropping a `Task` cancels it — so the write only
    /// happens after things have been still for [`SAVE_DEBOUNCE`]. The
    /// debounce *is* the replacement; there is no flag to keep in step.
    pub(super) fn save_workspace_soon(&mut self, cx: &mut Context<Self>) {
        // Nothing to save to, so nothing to schedule. An unbound workspace
        // persists nothing at all -- that is what binding a folder is for.
        if self.window.workspace.storage_dir.is_none() {
            return;
        }
        self._pending_save = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                // `save_workspace` reads the dock itself, so there is one place
                // that knows how to turn the live arrangement into the saved
                // one.
                shell.save_workspace(window, cx);
                shell._pending_save = None;
            });
        }));
    }

    // ── Workspace settings ──────────────────────────────────────────────────

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

    pub fn storage_dir(&self) -> Option<&std::path::PathBuf> {
        self.window.workspace.storage_dir.as_ref()
    }

    /// Drop the storage binding. An unbound workspace persists nothing.
    pub fn unbind_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.window.workspace.storage_dir.take();
        self.workbench
            .update(cx, |panel, cx| panel.set_storage(None, cx));
        self.set_window_identity(None, window, cx);
        // Forget the recent too, or the next launch reopens the workspace that
        // was just unbound -- `recent_workspaces[0]` takes precedence over
        // everything, including the CLI root.
        if let Some(dir) = was {
            cx.update_global::<Shared, _>(|shared, _| {
                shared.recents.forget(&dir);
                if let Err(e) = shared.recents.save() {
                    eprintln!("onehand: failed to save recents: {e}");
                }
            });
        }
        cx.notify();
    }

    /// Point this workspace at a storage folder and write it there.
    ///
    /// One function for all four writes because they are one fact arriving, and
    /// splitting them is what let them drift: binding used to set
    /// `workspace.storage_dir` and nothing else, leaving this window's registry
    /// entry saying `None` forever -- so opening the very same workspace from
    /// recents made a *second* window for it, which is the one thing the
    /// registry exists to prevent.
    fn bind_storage(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.storage_dir = Some(dir.clone());
        self.workbench
            .update(cx, |panel, cx| panel.set_storage(Some(&dir), cx));
        if self.settings_open {
            self.workspace_note_wanted = true;
        }
        self.save_workspace(window, cx);
        self.set_window_identity(Some(dir.clone()), window, cx);
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents.touch(dir);
            // Recents are a convenience, not state the app depends on:
            // a failed write is logged, never surfaced or fatal.
            if let Err(e) = shared.recents.save() {
                eprintln!("onehand: failed to save recents: {e}");
            }
        });
        cx.notify();
    }

    /// Update what the process-wide registry thinks this window is showing.
    ///
    /// Canonicalized on the way in, exactly as `open_or_focus` does, so a
    /// symlinked or `..`-laden path binds to the same identity it would be
    /// looked up by.
    fn set_window_identity(
        &mut self,
        dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = dir.map(|d| std::fs::canonicalize(&d).unwrap_or(d));
        let handle = window.window_handle();
        cx.update_global::<Shared, _>(|shared, _| {
            if let Some(entry) = shared
                .windows
                .iter_mut()
                .find(|w| w.handle.window_id() == handle.window_id())
            {
                entry.storage_dir = dir;
            }
        });
    }

    /// Bind this workspace to a storage folder, chosen with the native picker.
    ///
    /// **Overwrite guard:** a folder that already holds another workspace's
    /// `onehand-workspace.toml` is never overwritten -- that workspace is opened
    /// (or its window focused) and this one is left as it was. Binding a folder
    /// is a choice about *this* workspace; it must not be a way to lose another
    /// one.
    pub fn pick_storage_dir(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };

            match workspace_in(&dir, cx).await {
                // Opening it is the guard doing its job, but from the picker it
                // is also the one outcome with nothing on screen to show for it
                // -- a folder already open in a window just re-focuses that
                // window, which reads exactly like a control that did nothing.
                Ok(Some(workspace)) => {
                    cx.update(|cx| open_or_focus(workspace, cx));
                    say(
                        &shell,
                        Notification::info(format!(
                            "{} already holds a workspace — opened it, and this one is unchanged",
                            dir.display()
                        )),
                        cx,
                    );
                }
                Ok(None) => {
                    shell
                        .update_in(cx, |shell, window, cx| {
                            shell.bind_storage(dir.clone(), window, cx);
                        })
                        .ok();
                }
                Err(message) => say(&shell, Notification::error(message), cx),
            }
        })
        .detach();
    }

    /// Pick a project folder and open it as a *new* workspace in its own window.
    ///
    /// The workspace is bound and written before the window opens. An unbound
    /// workspace persists nothing, so one made this way was remembered nowhere:
    /// the next launch could not reopen it, and the folder it was created in did
    /// not open it either -- picking that folder answered "no workspace here",
    /// which is what a folder with nothing written into it truthfully is.
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
                        Notification::info(format!(
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
                    workspace.storage_dir = Some(dir.clone());
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
                            Notification::error(format!(
                                "Workspace not created — {} could not be written: {e}",
                                dir.display()
                            )),
                            cx,
                        ),
                    }
                }
                Err(message) => say(&shell, Notification::error(message), cx),
            }
        })
        .detach();
    }

    /// Pick a folder and open the workspace it belongs to.
    ///
    /// **Two folders answer, in order**: the one picked, for a workspace bound
    /// to a folder by hand, and then the storage a workspace created *from* that
    /// folder would have been written to. The second is what makes the obvious
    /// gesture work -- a workspace made by *New workspace…* keeps its config in
    /// the per-user data root, so the folder a user thinks of as "the
    /// workspace", and the only one they can find in a picker, is the project
    /// itself.
    pub fn open_workspace(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            let picked = workspace_in(&dir, cx).await;
            let found = match picked {
                Ok(Some(workspace)) => Some(workspace),
                Ok(None) => workspace_in(&workspace::storage_for(&dir), cx)
                    .await
                    .ok()
                    .flatten(),
                // "I could not read what is here" sends the user to fix the file
                // they meant to open, where "there is nothing here" below sends
                // them to a different folder. Reading on to the derived storage
                // would answer the second question when the first one was asked.
                Err(_) => {
                    say(
                        &shell,
                        Notification::error(
                            "That folder's onehand-workspace.toml could not be read",
                        ),
                        cx,
                    );
                    return;
                }
            };

            match found {
                Some(workspace) => {
                    cx.update(|cx| open_or_focus(workspace, cx));
                }
                None => say(
                    &shell,
                    Notification::warning(
                        "No workspace for that folder — New workspace… makes one",
                    ),
                    cx,
                ),
            }
        })
        .detach();
    }

    /// Recently opened storage directories, most-recent-first.
    pub fn recents(&self, cx: &App) -> Vec<std::path::PathBuf> {
        Shared::global(cx).recents.recent_workspaces.clone()
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
                            cx.update_global::<Shared, _>(|shared, _| {
                                shared.recents.forget(&dir);
                                let _ = shared.recents.save();
                            });
                            cx.notify();
                        })
                        .ok();
                }
                // Present but unreadable -- an unmounted share, a permission
                // blip, a TOML typo. Every one of those is recoverable, and
                // forgetting the recent is not: say so and keep the row.
                WorkspaceLoad::Unreadable => {
                    shell
                        .update_in(cx, |_, window, cx| {
                            window.push_notification(
                                Notification::error(format!(
                                    "Could not read the workspace in {} — the entry was kept",
                                    dir.display()
                                )),
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

    /// Write the workspace back to its storage folder, if it has one.
    pub(super) fn save_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.window.workspace.storage_dir.clone() else {
            return;
        };
        // Every write picks the arrangement up here rather than at each call
        // site. Otherwise the *first* save after binding a storage folder --
        // which is a save the user triggered by choosing the folder, not by
        // touching a panel -- would write the arrangement this window started
        // with instead of the one on screen, and the next launch would restore
        // that.
        let layout = self.dock_layout(cx);
        self.window.workspace.layout = layout;

        let saved = self.window.workspace.to_config().save_to(&dir);
        let noted = std::mem::take(&mut self.workspace_note_wanted);
        self.report_write("Workspace", saved, noted, window, cx);
        cx.notify();
    }
}

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
        WorkspaceLoad::Unreadable => Err(format!(
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
