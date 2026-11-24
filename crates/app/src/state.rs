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
    /// Where what each agent offered is kept: a file of its own in a test.
    pub facts_path: PathBuf,
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
    /// Recently opened storage directories, most-recent-first.
    pub recents: onehand_core::config::recents::RecentsDrive,
    /// Every project onehand keeps: one drive for every window, since a
    /// project is the same project in each window holding it. Read at boot,
    /// changed only through it, carried by `crate::projects`.
    pub projects: onehand_core::project::ProjectsDrive,
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
    /// what `crate::windows` is for.
    pub remote: crate::remote::RemoteBridge,
    /// The one thread every settings file is written on: off the UI thread,
    /// and in the order the changes were made.
    pub writer: onehand_core::saving::OrderedWriter,
    /// Each setting's value known saved and its newest change, so a failed
    /// write puts back only the settings no later write carries.
    pub config_saves: onehand_core::saving::Saves<&'static str, crate::saves::Put>,
    /// The events a test's sessions start on, taken by the next session
    /// started here before any agent is, in the order they were queued.
    #[cfg(test)]
    pub canned: std::collections::VecDeque<onehand_io::acp::Events>,
}

impl Global for Shared {}

impl Shared {
    pub fn from_config(cfg: AppConfig, config_path: PathBuf) -> Self {
        let facts = onehand_core::config::FactsFile::path();
        Self::with_recents(cfg, config_path, RecentsDrive::load(), facts)
    }

    /// The process's state for a test: the default config and no recents,
    /// so nothing of the person's own state is read.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        let dir = std::env::temp_dir().join("onehand-test-config.toml");
        let recents =
            std::env::temp_dir().join(format!("onehand-test-state-{}.toml", std::process::id()));
        let facts =
            std::env::temp_dir().join(format!("onehand-test-facts-{}.toml", std::process::id()));
        Self::with_recents(
            AppConfig::default(),
            dir,
            RecentsDrive::new(Default::default(), recents, Vec::new()),
            facts,
        )
    }

    fn with_recents(
        cfg: AppConfig,
        config_path: PathBuf,
        recents: RecentsDrive,
        facts_path: PathBuf,
    ) -> Self {
        let mut settings =
            onehand_core::config::drive::SettingsDrive::new(cfg.agents, cfg.appearance, cfg.hosts);
        settings.know_facts(onehand_core::config::FactsFile::load_from(&facts_path));
        Self {
            settings,
            on_machine: onehand_core::config::on_machine(std::env::var_os("PATH").as_deref()),
            config_path,
            facts_path,
            keymap: cfg.keymap,
            mono_family: None,
            writer: onehand_core::saving::OrderedWriter::spawn("onehand-settings"),
            config_saves: Default::default(),
            next_uid: 1,
            recents,
            projects: onehand_core::project::ProjectsDrive::new(
                Default::default(),
                onehand_core::project::file::store_dir(),
            ),
            // A runtime this process cannot start means no agent can ever run,
            // which is the whole app -- there is nothing useful to degrade to.
            acp: AcpRuntime::new(crate::control::Token::mint)
                .expect("failed to start the agent runtime"),
            hosts: crate::hosts::Hosts::new(),
            // Started separately, after the global exists: bringing a channel up
            // means reading a credential and spawning a poll, and neither of
            // those is something a constructor should be able to fail at.
            remote: crate::remote::RemoteBridge::off(),
            #[cfg(test)]
            canned: std::collections::VecDeque::new(),
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

#[cfg(test)]
mod tests {
    /// A test keeps what agents offered in a file of its own, never in the
    /// data root a run of onehand that started it points at.
    #[test]
    fn a_test_keeps_agent_facts_apart() {
        let shared = super::Shared::for_test();
        assert_ne!(shared.facts_path, onehand_core::config::FactsFile::path());
        assert!(shared.facts_path.starts_with(std::env::temp_dir()));
    }
}
