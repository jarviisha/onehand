//! Process-global state and the per-window workspace.
//!
//! [`Shared`] holds what is global to the process — the agent menu, the resolved
//! config path, the uid counter — while each window owns one [`Workspace`] and
//! everything hanging off it. The split is the
//! one `onehand-core` already models, so the tree logic is used as it is rather
//! than reinterpreted here.

use crate::acp::AcpRuntime;
use gpui::{App, Global};
use onehand_core::config::{AgentSpec, AppConfig, AppState, Appearance};
use onehand_core::gitstat::GitStatus;
use onehand_core::workspace::Workspace;
use std::collections::HashMap;
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
    /// The menu a new session spawns from. Agent definitions are global; each
    /// session keeps a clone of the spec it was spawned with.
    pub agents: Vec<AgentSpec>,
    /// Where `persist_agents` writes back to — the file the config came from.
    pub config_path: PathBuf,
    /// Light, dark, or following the desktop. Global rather than per window,
    /// because the theme it selects is itself one global: two windows cannot
    /// be drawn in two modes, so one of them holding a different answer would
    /// only be a lie about what is on screen.
    pub appearance: Appearance,
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
    pub recents: AppState,
    /// The tokio runtime every ACP adapter runs on (see [`crate::acp`]).
    pub acp: AcpRuntime,
    /// The channel a device outside this machine reaches the app through.
    ///
    /// Global rather than per window because a bot token identifies one bot: a
    /// second poll against the same token is two clients pulling from one
    /// queue, each seeing half the messages. What follows is that an incoming
    /// message belongs to no window in particular and has to find one, which is
    /// what [`OpenWindow::shell`] is for.
    pub remote: crate::remote::RemoteBridge,
    /// The user has left the machine.
    ///
    /// **The one thing the app cannot work out for itself.** Every rule about
    /// whether something is worth announcing asks the same question — is the
    /// user looking at what this is about — and answers it from the focused
    /// window and the conversation on screen. Both of those are still perfectly
    /// true in front of an empty chair: the window is focused, the transcript is
    /// showing, and the app concludes it has been read. This is where somebody
    /// says otherwise, and while it is set every announcement goes out as though
    /// nothing were on screen at all.
    ///
    /// Global, not per window, because it is a fact about the person rather than
    /// about a window — walking away from one window is walking away from all of
    /// them.
    ///
    /// **Not persisted, deliberately.** It describes right now, and a launch
    /// that came up believing the user was elsewhere would send a message about
    /// every turn to somebody sitting in front of it.
    pub away: bool,
    /// The task feeding [`Self::remote`]'s events into the app.
    ///
    /// Held rather than detached, for the reason every held task here is: it
    /// owns the receiving end, and dropping that is what tells the channel
    /// nobody is listening.
    pub _remote_pump: Option<gpui::Task<()>>,
}

impl Global for Shared {}

impl Shared {
    pub fn from_config(cfg: AppConfig, config_path: PathBuf) -> Self {
        Self {
            appearance: cfg.appearance,
            agents: cfg.agents,
            config_path,
            mono_family: None,
            next_uid: 1,
            windows: Vec::new(),
            recents: AppState::load(),
            // A runtime this process cannot start means no agent can ever run,
            // which is the whole app -- there is nothing useful to degrade to.
            acp: AcpRuntime::new().expect("failed to start the agent runtime"),
            // Started separately, after the global exists: bringing a channel up
            // means reading a credential and spawning a poll, and neither of
            // those is something a constructor should be able to fail at.
            remote: crate::remote::RemoteBridge::off(),
            away: false,
            _remote_pump: None,
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

/// One window's world: a workspace plus the per-root state derived from it.
pub struct WorkspaceWindow {
    pub workspace: Workspace,
    /// `git status` per root path. A non-repo root simply has no entry.
    pub git: HashMap<PathBuf, GitStatus>,
}

impl WorkspaceWindow {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            git: HashMap::new(),
        }
    }
}
