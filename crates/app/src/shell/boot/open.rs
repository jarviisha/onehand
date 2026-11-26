//! Opening a workspace's window: the seed a first launch makes, the one
//! path every open goes through so a workspace already on screen is brought
//! forward rather than doubled, and the window itself.

use super::super::{APP_ID, Shell};
use crate::state::Shared;
use gpui::{App, AppContext, BorrowAppContext, Focusable as _};
use gpui_component::Root;
use onehand_core::workspace::{Launch, Workspace};

/// Seed the first workspace: the positional CLI argument is the project root,
/// else the current directory.
pub fn seed_workspace() -> Workspace {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    Workspace::seeded(root)
}

/// Open `workspace` in a new window -- unless a window already shows that
/// storage directory, in which case focus it. Storage dirs are canonicalized on
/// the way in, so symlink and `..` aliases deduplicate.
pub fn open_or_focus(workspace: Workspace, cx: &mut App) {
    open(Launch::Found(workspace), cx);
}

/// [`open_or_focus`] for what a launch opens: a seed, which nothing on disk
/// holds yet, is written once its window is up.
pub(super) fn open(launch: Launch, cx: &mut App) {
    let (workspace, write) = match launch {
        Launch::Found(workspace) => (workspace, false),
        Launch::Seeded(workspace) => (workspace, true),
    };
    let dir = workspace.storage_dir.clone();
    if let Some(handle) = crate::windows::window_for(&dir, cx) {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        return;
    }
    let name = workspace.name.clone();
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.recents.opened(dir, &name));
    super::super::storage::carry_recents(fx, cx);
    open_window(workspace, write, cx);
}

/// The smallest the window may be dragged to: the rail's minimum beside the
/// conversation's, and a transcript still a few turns tall. Pixels, because it
/// is a bound measured against the screen, not something drawn inside a zoom.
pub(in crate::shell) const WINDOW_MIN: gpui::Size<gpui::Pixels> =
    gpui::size(gpui::px(720.), gpui::px(480.));

/// Open a window for `workspace` and register it, writing the workspace once
/// its window exists when `write` says to, so a write that fails is said there.
fn open_window(workspace: Workspace, write: bool, cx: &mut App) {
    let storage_dir = workspace.storage_dir.clone();
    let name = workspace.name.clone();
    cx.spawn(async move |cx| {
        let options = gpui::WindowOptions {
            // The window's identity to the desktop, and the only reason the app
            // has an icon at all.
            //
            // Nothing about a window carries a picture on Linux. The compositor
            // gets one by matching what the window calls itself against an
            // installed desktop entry -- Wayland compares this string to the
            // entry's file name, X11 compares the `WM_CLASS` it becomes to
            // `StartupWMClass`. Left unset, the window answers with nothing to
            // match, so every entry on disk is unreachable and the result is
            // the generic placeholder no matter how many icons are installed.
            app_id: Some(APP_ID.into()),
            window_min_size: Some(WINDOW_MIN),
            // The title bar is the app's, holding its menus, so the window asks
            // the compositor to leave its decoration to the app; one that will
            // not keeps its own, and the library's title bar draws no window
            // controls of its own beside it.
            #[cfg(target_os = "linux")]
            window_decorations: Some(gpui::WindowDecorations::Client),
            ..gpui_component::TitleBar::window_options()
        };
        // Filled in by the window builder below and read out after it, because
        // the shell does not exist until then and the registry entry needs it.
        let mut built: Option<gpui::WeakEntity<Shell>> = None;
        let handle = cx
            .open_window(options, |window, cx| {
                let shell = cx.new(|cx| {
                    let mut shell = Shell::new(workspace, window, cx);
                    shell.refresh_git(cx);
                    // Point every panel at the seeded root before the first
                    // frame. Sessions are not persisted, so there is never one
                    // to connect here -- what this settles is which project the
                    // Workbench, the terminal and the empty chat are about,
                    // which was nothing at all until the first rail click.
                    shell.show_active_session(window, cx);
                    if write {
                        let fx = shell.window.workspace.save();
                        shell.carry_workspace(fx, window, cx);
                    }
                    // The empty project also needs a live focus path, or no
                    // app shortcut (including Settings) can reach the shell.
                    shell.chat.focus_handle(cx).focus(window, cx);
                    shell
                });
                built = Some(shell.downgrade());
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open window");

        cx.update(|cx| {
            // A window with no shell is not a thing this can build, so there is
            // nothing to degrade to and nothing worth reporting -- but the
            // registry is what deduplicates windows, so an entry is filed either
            // way rather than the whole window being dropped from it.
            let shell = built.expect("the window was built without a shell");
            crate::windows::register(storage_dir.clone(), handle.into(), shell.clone(), cx);
            // A row that names no project gets one, now the window is there
            // to take it.
            if let Some(shell) = shell.upgrade() {
                crate::projects::place_rows(handle.into(), shell, cx);
            }
            // What reached the socket before any window waited for this one.
            crate::instance::window_up(cx);
            let fx = cx
                .update_global::<Shared, _>(|shared, _| shared.recents.named(&storage_dir, &name));
            super::super::storage::carry_recents(fx, cx);
        });
    })
    .detach();
}
