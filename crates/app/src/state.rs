//! Process-global state and the per-window workspace.
//!
//! [`Shared`] holds what is global to the process — the agent menu, the resolved
//! config path, the uid counter — while each window owns one [`Workspace`] and
//! everything hanging off it. The split is the
//! one `onehand-core` already models, so the tree logic is used as it is rather
//! than reinterpreted here.

use gpui::{App, Global};
use onehand_core::config::AppConfig;
use onehand_core::config::recents::RecentsDrive;
use onehand_core::gitstat::GitStatus;
use onehand_core::workspace::{ProjectRoot, Workspace};
use onehand_io::acp::AcpRuntime;
use std::path::PathBuf;

/// A window this process has open, tracked so that opening a workspace that is
/// already on screen focuses it instead of making a duplicate.
pub struct OpenWindow {
    /// The workspace's storage directory, canonicalized. `None` for an unbound
    /// workspace, which can never be deduplicated because it has no identity.
    pub storage_dir: Option<PathBuf>,
    pub handle: gpui::AnyWindowHandle,
    /// This window's shell.
    ///
    /// **The one way in from outside the window tree.** Everything the app does
    /// to itself starts from a click and therefore already has a shell in hand;
    /// a message arriving over the remote bridge starts from a global with no
    /// window in it at all, and it still has to find the session it names. The
    /// window handle alone cannot answer that — it opens onto the overlay root
    /// rather than onto the shell underneath it.
    ///
    /// Weak, so this list can never be the reason a closed window's shell stays
    /// alive. The prune on close removes the entry anyway; a strong handle would
    /// make the moment between the close and the prune a leak, and the moment
    /// after a bug that only shows up if the prune is ever missed.
    pub shell: gpui::WeakEntity<crate::shell::Shell>,
}

/// Global, process-wide state. One per `App`.
pub struct Shared {
    /// The agents a new session spawns from, the appearance, what each agent
    /// offered when it last came up and the checks of one: read through it,
    /// changed only through it. Agent definitions are global; each session
    /// keeps a clone of the spec it was spawned with.
    pub settings: onehand_core::config::drive::SettingsDrive,
    /// The built-in agents installed on this machine, looked for once at
    /// start, so a page that offers one never goes looking when it opens.
    pub on_machine: Vec<onehand_core::config::Preset>,
    /// Where `persist_agents` writes back to — the file the config came from.
    pub config_path: PathBuf,
    pub keymap: std::collections::BTreeMap<String, Vec<String>>,
    /// The monospace family the boot-time scan settled on, if it found one.
    ///
    /// Kept so it can be put back after a theme change: swapping the palette
    /// re-applies a whole theme config, and one that names a font family of its
    /// own would silently undo the scan — leaving every diff and command in the
    /// body face, which is exactly the failure the scan exists to prevent.
    pub mono_family: Option<String>,
    /// Process-wide unique session id salt. Global across windows: it is how an
    /// agent event finds its window.
    next_uid: u64,
    /// Every open window, for dedup-focus on open.
    pub windows: Vec<OpenWindow>,
    /// Recently opened storage directories, most-recent-first.
    pub recents: onehand_core::config::recents::RecentsDrive,
    /// The tokio runtime every ACP adapter runs on (see [`crate::acp`]).
    pub acp: AcpRuntime<crate::control::Token>,
    /// The one connection to each host, shared by every window's projects there.
    pub hosts: crate::hosts::Hosts,
    /// The channel a device outside this machine reaches the app through.
    ///
    /// Global rather than per window because a bot token identifies one bot: a
    /// second poll against the same token is two clients pulling from one
    /// queue, each seeing half the messages. What follows is that an incoming
    /// message belongs to no window in particular and has to find one, which is
    /// what [`OpenWindow::shell`] is for.
    pub remote: crate::remote::RemoteBridge,
    /// The task feeding [`Self::remote`]'s events into the app.
    ///
    /// Held rather than detached, for the reason every held task here is: it
    /// owns the receiving end, and dropping that is what tells the channel
    /// nobody is listening.
    pub _remote_pump: Option<gpui::Task<()>>,
    /// The one thread every settings file is written on: off the UI thread,
    /// and in the order the changes were made.
    pub writer: onehand_core::saving::OrderedWriter,
    /// Each setting's value known saved and its newest change, so a failed
    /// write puts back only the settings no later write carries.
    pub config_saves: onehand_core::saving::Saves<&'static str, crate::saves::Put>,
    /// The events a test's sessions start on, taken by the next session
    /// started here before any agent is: the last pushed goes first.
    #[cfg(test)]
    pub canned: Vec<onehand_io::acp::Events>,
}

impl Global for Shared {}

impl Shared {
    pub fn from_config(cfg: AppConfig, config_path: PathBuf) -> Self {
        Self::with_recents(cfg, config_path, RecentsDrive::load())
    }

    /// The process's state for a test: the default config and no recents,
    /// so nothing of the person's own state is read.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        let dir = std::env::temp_dir().join("onehand-test-config.toml");
        let recents =
            std::env::temp_dir().join(format!("onehand-test-state-{}.toml", std::process::id()));
        Self::with_recents(
            AppConfig::default(),
            dir,
            RecentsDrive::new(Default::default(), recents),
        )
    }

    fn with_recents(cfg: AppConfig, config_path: PathBuf, recents: RecentsDrive) -> Self {
        Self {
            settings: onehand_core::config::drive::SettingsDrive::new(cfg.agents, cfg.appearance),
            on_machine: onehand_core::config::on_machine(std::env::var_os("PATH").as_deref()),
            config_path,
            keymap: cfg.keymap,
            mono_family: None,
            writer: onehand_core::saving::OrderedWriter::spawn("onehand-settings"),
            config_saves: Default::default(),
            next_uid: 1,
            windows: Vec::new(),
            recents,
            // A runtime this process cannot start means no agent can ever run,
            // which is the whole app -- there is nothing useful to degrade to.
            acp: AcpRuntime::new(crate::control::Token::mint)
                .expect("failed to start the agent runtime"),
            hosts: crate::hosts::Hosts::new(),
            // Started separately, after the global exists: bringing a channel up
            // means reading a credential and spawning a poll, and neither of
            // those is something a constructor should be able to fail at.
            remote: crate::remote::RemoteBridge::off(),
            _remote_pump: None,
            #[cfg(test)]
            canned: Vec::new(),
        }
    }

    /// Hand out the next session uid.
    pub fn next_uid(&mut self) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        uid
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// The window already showing `dir`, if any.
    pub fn window_for(&self, dir: &PathBuf) -> Option<gpui::AnyWindowHandle> {
        self.windows
            .iter()
            .find(|w| w.storage_dir.as_ref() == Some(dir))
            .map(|w| w.handle)
    }
}

/// One window's world: its workspace, changed only through its drive.
pub struct WorkspaceWindow {
    pub workspace: onehand_core::workspace::WorkspaceDrive,
}

impl WorkspaceWindow {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace: onehand_core::workspace::WorkspaceDrive::new(workspace),
        }
    }

    /// `root`'s `git status`, as the last sweep read it.
    pub(crate) fn git_of(&self, root: &ProjectRoot) -> Option<&GitStatus> {
        self.workspace.git_of(root)
    }
}

/// Run `f` on the shell of `window`, inside it: `None` when the window or its
/// shell has gone. The way in for work that starts outside a window — a task
/// asking for its place, a tool's call — and never from inside that shell's
/// own update, which would be updating it twice.
pub(crate) fn with_shell<R>(
    window: gpui::AnyWindowHandle,
    cx: &mut App,
    f: impl FnOnce(
        &mut crate::shell::Shell,
        &mut gpui::Window,
        &mut gpui::Context<crate::shell::Shell>,
    ) -> R,
) -> Option<R> {
    let shell = cx
        .try_global::<Shared>()?
        .windows
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

/// The window in front and its shell, or the first window when none is:
/// where something asked for from outside every window lands.
pub(crate) fn front(
    cx: &App,
) -> Option<(gpui::AnyWindowHandle, gpui::Entity<crate::shell::Shell>)> {
    let windows = &Shared::global(cx).windows;
    let open: Vec<_> = windows.iter().map(|w| w.handle).collect();
    let front = onehand_core::instance::front(cx.active_window(), &open)?;
    windows
        .iter()
        .find(|w| w.handle == front)
        .and_then(|w| Some((w.handle, w.shell.upgrade()?)))
}
