//! The lock on a project's shells while its code is elsewhere: what is typed
//! into them is dropped, and a line under the grid says why.

use super::TerminalPanel;
use gpui::{AnyElement, App, Context, IntoElement as _};
use gpui_component::ActiveTheme as _;
use std::collections::HashMap;
use std::path::PathBuf;

impl TerminalPanel {
    /// Which folders here nothing may be typed into now, whole.
    pub fn set_locked(&mut self, locks: &HashMap<PathBuf, String>, cx: &mut Context<Self>) {
        self.locked = locks.clone();
        self.apply_locks();
        cx.notify();
    }

    /// Lock or open each shell here by its folder. A host's shells are the
    /// host's, never locked from here.
    pub(super) fn apply_locks(&self) {
        for ((host, root), set) in &self.shells {
            let shut = host.is_none() && self.locked.contains_key(root);
            for tab in &set.tabs {
                tab.shell.set_locked(shut);
            }
        }
    }

    /// The line under the project on screen's shells while they are locked.
    pub(super) fn locked_line(&self, cx: &App) -> Option<AnyElement> {
        let (None, root) = self.root.as_ref()? else {
            return None;
        };
        let why = self.locked.get(root)?;
        Some(onehand_ui::status_line(why.clone(), cx.theme().muted_foreground).into_any_element())
    }
}
