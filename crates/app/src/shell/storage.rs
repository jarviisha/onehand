use super::Shell;
use super::boot::open_or_focus;
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::InputState;
use gpui_component::notification::Notification;
use onehand_core::config::{WorkspaceConfig, WorkspaceLoad};
use onehand_core::workspace::drive::Effect;
use onehand_core::workspace::{self, Workspace};
use std::path::{Path, PathBuf};

impl Shell {
    /// Write the workspace once the user stops changing it: a dock's
    /// `LayoutChanged` comes on every frame of a drag, so each change only
    /// arms the drive's wait again, and the write comes once it runs out.
    pub(super) fn save_workspace_soon(&mut self, cx: &mut Context<Self>) {
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
        let Some(Effect::Write {
            pending,
            mut config,
            dir,
        }) = self.window.workspace.flush()
        else {
            return;
        };
        config.layout = self.dock_layout(cx);
        let result = Shared::global(cx).writer.write_within(
            move || config.save_to(&dir).map_err(|why| why.to_string()),
            CLOSE_WAIT,
        );
        if let Err(why) = &result {
            eprintln!("onehand: the workspace was not saved on closing: {why}");
        }
        self.window.workspace.flushed(pending, result);
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
        let was = self.window.workspace.unbind();
        self.workbench
            .update(cx, |panel, cx| panel.set_storage(None, cx));
        self.tell_issues_page(&onehand_workbench::Request::SetStorage(None), cx);
        self.set_window_identity(None, window, cx);
        // Forget the recent too, or the next launch reopens the workspace that
        // was just unbound -- `recent_workspaces[0]` takes precedence over
        // everything, including the CLI root.
        // Said once if it fails, unlike a touch: the workspace would come
        // back at the next launch.
        if let Some(dir) = was {
            cx.update_global::<Shared, _>(|shared, _| shared.recents.forget(&dir));
            let written = save_recents(cx);
            window
                .spawn(cx, async move |cx| {
                    if let Err(why) = written.await {
                        let _ = cx.update(|window, cx| {
                            window.push_notification(
                                crate::toast::failed("Forgetting the unbound workspace", why),
                                cx,
                            )
                        });
                    }
                })
                .detach();
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
        let fx = self.window.workspace.bind(dir.clone());
        self.workbench
            .update(cx, |panel, cx| panel.set_storage(Some(&dir), cx));
        self.tell_issues_page(&onehand_workbench::Request::SetStorage(Some(&dir)), cx);
        self.carry_workspace(fx, window, cx);
        self.set_window_identity(Some(dir.clone()), window, cx);
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents.touch(dir.clone());
            shared.recents.set_name(&dir, &self.window.workspace.name);
        });
        log_failed(save_recents(cx), cx);
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
        let dir = dir.map(workspace::canon_dir);
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
                        crate::toast::info(format!(
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
                Err(message) => say(
                    &shell,
                    crate::toast::failed("Using that folder", message),
                    cx,
                ),
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
                        crate::toast::error("Reading that folder's onehand-workspace.toml failed"),
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
                    crate::toast::warning(
                        "No workspace for that folder — New workspace… makes one",
                    ),
                    cx,
                ),
            }
        })
        .detach();
    }

    /// Every remembered workspace, most recent first, with what a list of them
    /// calls it.
    pub fn recents(&self, cx: &App) -> Vec<(PathBuf, String)> {
        let recents = &Shared::global(cx).recents;
        recents
            .recent_workspaces
            .iter()
            .map(|dir| (dir.clone(), recents.label_of(dir)))
            .collect()
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
                            });
                            log_failed(save_recents(cx), cx);
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
                                crate::toast::error(format!(
                                    "Reading the workspace in {} failed, and its entry was kept",
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

/// Write the recents as they stand now, on the ordered writer every recents
/// write goes through, so an older copy queued behind it cannot land over it.
pub(super) fn save_recents(
    cx: &App,
) -> impl std::future::Future<Output = Result<(), String>> + use<> {
    let recents = Shared::global(cx).recents.clone();
    Shared::global(cx)
        .writer
        .write(move || recents.save().map_err(|e| e.to_string()))
}

/// Log a recents write that fails: a convenience, not state the app depends
/// on, so nothing more.
pub(super) fn log_failed(
    written: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
    cx: &App,
) {
    cx.background_executor()
        .spawn(async move {
            if let Err(why) = written.await {
                eprintln!("onehand: failed to save recents: {why}");
            }
        })
        .detach();
}

/// Remember `dir`'s name among the recents, so the switcher reads it as the
/// rail does, and write them when that changed anything.
pub(super) fn remember_name(dir: &Path, name: &str, cx: &mut App) {
    if cx.update_global::<Shared, _>(|shared, _| shared.recents.set_name(dir, name)) {
        log_failed(save_recents(cx), cx);
    }
}
