//! A fork's session: minted beside the conversation forked from, on its
//! project and its agent, and shown.

use super::Shell;
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Context, Window};
use onehand_core::config::AgentSpec;
use std::path::Path;

impl Shell {
    /// Start the session a fork the pane has ready waits for: on the project
    /// at `root`, running `spec` as the original did, shown at once.
    ///
    /// The agent is the original's own rather than looked up by name: a fork
    /// is the same conversation carried on, and an agent renamed meanwhile is
    /// still the one that holds it.
    pub(super) fn fork_session(
        &mut self,
        root: &Path,
        spec: AgentSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let (added, fx) = self.window.workspace.fork(root, spec, uid);
        // Before it is shown: showing is what spawns the adapter, and the
        // adapter has to be started as the fork. A fork that goes nowhere
        // lets its staged read go.
        self.chat.update(cx, |pane, _| match added {
            Ok(uid) => pane.fork_next(uid),
            Err(_) => pane.drop_fork(),
        });
        self.carry_workspace(fx, window, cx);
    }
}
