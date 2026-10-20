//! The terminal dock's rules, as plain decisions the shell carries out: which
//! project's state is on screen and what each left, and what the terminal key
//! does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Whether the terminal dock was open, per project root, and which root the
/// dock on screen belongs to.
///
/// **The terminal is per project all the way down, so its dock is too.** Its
/// tabs, its shells and its working directory are all keyed by root, and none
/// of them come along when the selection moves — so a dock that stayed open
/// across a switch was showing the new project an empty panel where the old
/// project's shells had been, which reads as the terminal having lost them
/// rather than as their having been left behind.
///
/// A root nobody has opened it in is closed, not "however it was left in the
/// project before". Inheriting would reproduce the same empty panel on every
/// project newly arrived at, which is the thing this exists to stop.
///
/// The dock is read *live* at each handover — the state is changed by a key,
/// by the conversation's header, by a rail menu entry and by the dock's own
/// chrome, and a memory updated at each of those has four places to forget.
/// Knowing whose state is on screen is enough to file it correctly at the one
/// moment it matters, which is the switch itself.
#[derive(Debug, Default)]
pub(super) struct TerminalHomes {
    on_screen: Option<PathBuf>,
    open: HashMap<PathBuf, bool>,
}

impl TerminalHomes {
    /// The state a launch restores: the one saved answer belongs to the root
    /// about to be on screen and no other, so the first handover does not read
    /// the restored dock as another project's and close it on the way in.
    pub(super) fn seeded(root: Option<PathBuf>, open: bool) -> Self {
        Self {
            open: root.iter().map(|root| (root.clone(), open)).collect(),
            on_screen: root,
        }
    }

    /// `root` arrives with the dock `live`: what is on screen is filed under
    /// the root it belongs to, and the dock `root` wants is returned — `None`
    /// when `root` is the one on screen already, as between two sessions of
    /// one project, where restoring would fight a person who just opened it.
    pub(super) fn arrive(&mut self, root: &Path, live: bool) -> Option<bool> {
        let leaving = self.on_screen.replace(root.to_path_buf());
        if let Some(leaving) = leaving.as_ref() {
            self.open.insert(leaving.clone(), live);
        }
        if leaving.as_deref() == Some(root) {
            return None;
        }
        Some(self.open.get(root).copied().unwrap_or(false))
    }

    /// A page puts the dock away: what is on screen is filed and its root let
    /// go, so the next arrival is a handover that restores it.
    pub(super) fn put_away(&mut self, live: bool) {
        if let Some(root) = self.on_screen.take() {
            self.open.insert(root, live);
        }
    }

    /// `root` left the workspace. The dock on screen no longer belongs to it,
    /// or the next handover would file the live state under a project that is
    /// gone and hand a re-added one a state it never chose.
    pub(super) fn forget(&mut self, root: &Path) {
        self.open.remove(root);
        if self.on_screen.as_deref() == Some(root) {
            self.on_screen = None;
        }
    }
}

/// What a press of the terminal key does.
#[derive(Debug, PartialEq)]
pub(super) enum TerminalPress {
    /// A page shows: the terminal is one project's, and a page stands on none.
    Nothing,
    Close,
    /// Put the dock on screen, first giving the area back to the conversation
    /// the terminal sits under, and start a shell where it has none.
    Open {
        step_aside: bool,
        spawn: bool,
    },
}

/// The terminal key, pressed with a page showing or not, the Workbench filling
/// the area or not, the dock `open` or not and with a shell in it or not.
///
/// **An open dock with nothing in it is not a dock to close.** Closing the
/// last tab's ✕ leaves exactly that, and the panel it leaves offers *New
/// terminal* — so a press means "open one". And while the Workbench has the
/// area, the press brings the conversation back and opens, never closes.
pub(super) fn terminal_press(
    page: bool,
    workbench_fills: bool,
    open: bool,
    has_shell: bool,
) -> TerminalPress {
    if page {
        return TerminalPress::Nothing;
    }
    if open && !workbench_fills && has_shell {
        return TerminalPress::Close;
    }
    TerminalPress::Open {
        step_aside: workbench_fills,
        spawn: !has_shell,
    }
}

#[cfg(test)]
mod tests;
