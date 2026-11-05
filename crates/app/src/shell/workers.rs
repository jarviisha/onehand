//! The workers' region, which shows the lead on screen's workers in the
//! Workbench's place; and where a session is in the workspace. A worker is
//! shown in the agent pane as any session is, from its row under its lead.

use super::{FocusedPanel, Shell};
use gpui::{Context, Focusable as _, Window};
use gpui_component::dock::{DockLayout, DockPlacement};

/// Which panel the right dock holds: the Workbench, or the workers' region
/// in its place. The two never show at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RightPanel {
    Workbench,
    Workers,
}

/// Whether the open region closes as it follows the lead on screen: it
/// stays on one with workers and closes on anything else, a page included.
pub(super) fn region_closes(page: bool, has_workers: bool) -> bool {
    page || !has_workers
}

impl Shell {
    /// Where session `uid` is in the workspace: its project and its place.
    pub(super) fn locate(&self, uid: u64) -> Option<(usize, usize)> {
        self.window
            .workspace
            .roots
            .iter()
            .enumerate()
            .find_map(|(ri, root)| {
                root.sessions
                    .iter()
                    .position(|session| session.uid == uid)
                    .map(|si| (ri, si))
            })
    }
}

impl Shell {
    /// Whether the Workbench is on screen: its dock open, holding it.
    pub(super) fn workbench_open(&self, cx: &gpui::App) -> bool {
        self.right == RightPanel::Workbench && self.dock.read(cx).is_dock_open(DockPlacement::Right)
    }

    /// Whether the workers' region is on screen.
    pub(super) fn workers_open(&self, cx: &gpui::App) -> bool {
        self.right == RightPanel::Workers && self.dock.read(cx).is_dock_open(DockPlacement::Right)
    }

    /// Put `panel` in the right dock, which keeps its size and open state.
    fn mount_right(&mut self, panel: RightPanel, window: &mut Window, cx: &mut Context<Self>) {
        if self.right == panel {
            return;
        }
        self.right = panel;
        let layout = match panel {
            RightPanel::Workbench => DockLayout::tabs().panel(self.workbench.clone()),
            RightPanel::Workers => DockLayout::tabs().panel(self.workers_panel.clone()),
        };
        self.dock.update(cx, |dock, cx| {
            dock.set_dock(DockPlacement::Right, layout, window, cx)
        });
    }

    /// Show the region, at worker `at` picked when one is named, with the
    /// caret on the list when `caret`. In the Workbench's place: a caret in the
    /// Workbench goes to the lead's composer first, and the region takes
    /// none unless asked.
    pub(super) fn show_workers(
        &mut self,
        at: Option<u64>,
        caret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page_shown(cx) || self.chat.read(cx).lead_workers().is_empty() {
            return;
        }
        // Another panel filling the window would hide the region, and the
        // caret put in it with it.
        if let Some(panel) = self.app_maximized.filter(|p| *p != FocusedPanel::Workers) {
            self.toggle_maximize_panel(panel, window, cx);
        }
        if self.right == RightPanel::Workbench {
            self.workbench_under = self.workbench_open(cx);
            let had_focus = self.workbench.focus_handle(cx).contains_focused(window, cx);
            if had_focus {
                self.last_panel = FocusedPanel::Chat;
                self.chat
                    .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
            }
            self.stepped_aside = false;
            self.chat.update(cx, |pane, cx| pane.workers_shown(cx));
            self.mount_right(RightPanel::Workers, window, cx);
        }
        if !self.dock.read(cx).is_dock_open(DockPlacement::Right) {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
        if let Some(uid) = at {
            self.chat
                .update(cx, |pane, cx| pane.open_worker(uid, caret, window, cx));
        }
        cx.notify();
    }

    /// Give the Workbench its place back, open again if it was open when the
    /// region took it, else the dock closed. A caret in the region goes to
    /// the lead's composer before the region leaves.
    pub(super) fn hide_workers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right != RightPanel::Workers {
            return;
        }
        if self.dock.read(cx).is_dock_open(DockPlacement::Right) != self.workbench_under {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
        self.leave_workers(window, cx);
        cx.notify();
    }

    /// Put the Workbench back in the dock in the region's place, the dock
    /// left as it is; true when the region was there.
    pub(super) fn leave_workers(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.right != RightPanel::Workers {
            return false;
        }
        if self.app_maximized == Some(FocusedPanel::Workers) {
            self.set_app_maximized(None, cx);
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
        }
        if self
            .workers_panel
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
            self.last_panel = FocusedPanel::Chat;
            self.chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
        }
        self.mount_right(RightPanel::Workbench, window, cx);
        true
    }

    /// Say the workers' list's ages again while it shows: it is drawn by the
    /// pane, which nothing else wakes while every session stands still.
    pub(crate) fn tick_worker_ages(&self, cx: &mut Context<Self>) {
        if self.workers_open(cx) {
            self.chat.update(cx, |_, cx| cx.notify());
        }
    }

    /// The header's button and the strip's hide.
    pub(super) fn toggle_workers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.workers_open(cx) {
            true => self.hide_workers(window, cx),
            false => self.show_workers(None, false, window, cx),
        }
    }

    /// Run every frame: the region closes on a lead with no workers, on a
    /// page, and when its dock was closed by any other way.
    pub(super) fn follow_workers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right != RightPanel::Workers {
            return;
        }
        let has_workers = !self.chat.read(cx).lead_workers().is_empty();
        if !self.workers_open(cx) || region_closes(self.page_shown(cx), has_workers) {
            self.hide_workers(window, cx);
        }
    }
}

#[cfg(test)]
mod tests;
