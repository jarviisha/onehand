//! The windows this process has open, and the one way to reach them from
//! outside a window.
//!
//! Everything the app does to itself starts from a click and so already has
//! a shell in hand. What arrives from outside every window (a line over the
//! remote bridge, a tool's call, a task asking for its place, the clock) has
//! only a uid, a project or a window handle, and finds the shell it means
//! here. A window handle alone cannot answer that: it opens onto the overlay
//! root, not the shell underneath it.
//!
//! **Reaching a shell is where re-entrancy bites.** A shell being updated
//! cannot be updated again, nor read through its entity, without a panic. So
//! the walks a shell makes from inside its own update skip it
//! ([`others`]), and work that may be asked from inside some update runs once
//! that update is over ([`later`]).
//!
//! The list is a global of its own, so nothing outside this module walks it.

use crate::chat::session::ChatSession;
use crate::shell::Shell;
use gpui::{AnyWindowHandle, App, Entity, EntityId, Global, WeakEntity, WindowId};
use std::path::{Path, PathBuf};

/// A window this process has open.
struct OpenWindow {
    /// The workspace's storage directory, canonicalized. `None` for an
    /// unbound workspace, which can never be deduplicated because it has no
    /// identity.
    storage_dir: Option<PathBuf>,
    handle: AnyWindowHandle,
    /// This window's shell. Weak, so this list is never the reason a closed
    /// window's shell stays alive: a strong handle would make the moment
    /// between a close and its prune a leak, and a missed prune a bug.
    shell: WeakEntity<Shell>,
}

/// Every open window, in the order they were opened.
#[derive(Default)]
struct Windows(Vec<OpenWindow>);

impl Global for Windows {}

fn all(cx: &App) -> &[OpenWindow] {
    cx.try_global::<Windows>().map_or(&[], |w| &w.0)
}

/// File window `handle`, showing `shell` and the workspace kept in
/// `storage_dir`, once it is built.
pub(crate) fn register(
    storage_dir: Option<PathBuf>,
    handle: AnyWindowHandle,
    shell: WeakEntity<Shell>,
    cx: &mut App,
) {
    cx.default_global::<Windows>().0.push(OpenWindow {
        storage_dir,
        handle,
        shell,
    });
}

/// Window `closed` has gone. Left listed, opening its workspace again would
/// "focus" a dead handle and do nothing.
pub(crate) fn forget(closed: WindowId, cx: &mut App) {
    cx.default_global::<Windows>()
        .0
        .retain(|w| w.handle.window_id() != closed);
}

/// Window `handle`'s workspace is now kept in `dir` (canonicalized), or in
/// none.
pub(crate) fn bind_storage(handle: AnyWindowHandle, dir: Option<PathBuf>, cx: &mut App) {
    let windows = cx.default_global::<Windows>();
    if let Some(entry) = windows
        .0
        .iter_mut()
        .find(|w| w.handle.window_id() == handle.window_id())
    {
        entry.storage_dir = dir;
    }
}

/// The window already showing the workspace kept in `dir`, if any.
pub(crate) fn window_for(dir: &Path, cx: &App) -> Option<AnyWindowHandle> {
    all(cx)
        .iter()
        .find(|w| w.storage_dir.as_deref() == Some(dir))
        .map(|w| w.handle)
}

/// Every window whose shell is still alive, with it, in the order they were
/// opened. Reads no shell, so it is safe anywhere; a caller inside a shell's
/// own update that goes on to read the shells uses [`others`].
pub(crate) fn live(cx: &App) -> Vec<(AnyWindowHandle, Entity<Shell>)> {
    all(cx)
        .iter()
        .filter_map(|w| Some((w.handle, w.shell.upgrade()?)))
        .collect()
}

/// Every live window but the one whose shell is `me`: what a shell walks
/// from inside its own update, where reading itself through its entity
/// would panic. It reads itself as `self`.
pub(crate) fn others(me: EntityId, cx: &App) -> Vec<(AnyWindowHandle, Entity<Shell>)> {
    live(cx)
        .into_iter()
        .filter(|(_, shell)| shell.entity_id() != me)
        .collect()
}

/// Session `uid`, in whichever window holds it. Reads every shell: never
/// from inside one's own update, as the two below.
pub(crate) fn session(uid: u64, cx: &App) -> Option<Entity<ChatSession>> {
    live(cx)
        .into_iter()
        .find_map(|(_, shell)| shell.read(cx).session_entity(uid, cx))
}

/// The window holding session `uid`, and its shell.
pub(crate) fn holding_session(uid: u64, cx: &App) -> Option<(AnyWindowHandle, Entity<Shell>)> {
    live(cx)
        .into_iter()
        .find(|(_, shell)| shell.read(cx).session_project(uid).is_some())
}

/// The windows holding the project at `root`, in the order they were
/// opened, with their shells.
pub(crate) fn holding_root(root: &Path, cx: &App) -> Vec<(AnyWindowHandle, Entity<Shell>)> {
    live(cx)
        .into_iter()
        .filter(|(_, shell)| shell.read(cx).holds_root(root))
        .collect()
}

/// Run `f` on the shell of `window`, inside it: `None` when the window or
/// its shell has gone. Never from inside that shell's own update, which
/// would be updating it twice: use [`later`] where that can be.
pub(crate) fn with_shell<R>(
    window: AnyWindowHandle,
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut gpui::Window, &mut gpui::Context<Shell>) -> R,
) -> Option<R> {
    let shell = all(cx)
        .iter()
        .find(|w| w.handle == window)?
        .shell
        .upgrade()?;
    window
        .update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| f(shell, window, cx))
        })
        .ok()
}

/// [`with_shell`], once the update under way is over: for work that may be
/// asked from inside an update of the shell it reaches, or of another, and
/// would otherwise panic or be passed over. Nothing when the window has gone
/// by then.
pub(crate) fn later(
    window: AnyWindowHandle,
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut gpui::Window, &mut gpui::Context<Shell>) + 'static,
) {
    cx.defer(move |cx| {
        with_shell(window, cx, f);
    });
}

/// The window in front and its shell, or the first window when none is:
/// where something asked for from outside every window lands.
pub(crate) fn front(cx: &App) -> Option<(AnyWindowHandle, Entity<Shell>)> {
    let open: Vec<_> = all(cx).iter().map(|w| w.handle).collect();
    let front = onehand_core::instance::front(cx.active_window(), &open)?;
    live(cx).into_iter().find(|(handle, _)| *handle == front)
}

#[cfg(test)]
mod tests;
