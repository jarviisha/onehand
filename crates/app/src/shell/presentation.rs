//! How the width beside the rail is shared between the conversation and the
//! Workbench.
//!
//! The conversation never drops under [`CHAT_MIN`] beside a dock. When the
//! window cannot hold it beside the narrowest useful Workbench, the Workbench
//! takes the whole area under a way back, rather than leaving a thin strip of
//! chat that nobody can read.

use super::{FocusedPanel, Shell};
use gpui::{Context, Focusable as _, Window, px};
use gpui_component::dock::DockPlacement;

/// The conversation's minimum beside a dock, in rems before its zoom.
const CHAT_MIN: f32 = 30.;
/// The narrowest useful Workbench, in rems.
const DOCK_MIN: f32 = 24.;
/// How much more room a split needs to come back than to hold, so a window
/// resting on the threshold does not flicker between the two.
const SPLIT_SLACK: f32 = 1.;

/// How the area beside the rail is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Presentation {
    /// The Workbench is closed or stepped aside; the conversation has it all.
    Conversation,
    /// Side by side, the Workbench drawn this many rems wide.
    Split(f32),
    /// The Workbench takes the whole area.
    WorkbenchFocus,
}

/// Choose the presentation for `avail` rems beside the rail.
///
/// `dock` is the width the person dragged the Workbench to. It is only ever
/// *read* here: when the window is too narrow for it the dock is drawn
/// narrower, down to [`DOCK_MIN`], and the dragged width comes back as soon as
/// there is room, so a narrow moment never overwrites the wide layout.
///
/// `zoom` is the conversation's: a zoomed conversation needs that much more
/// room to stay readable.
///
/// `was_split` gives the threshold its slack: a split already showing holds
/// until the dock would drop under [`DOCK_MIN`], one coming back needs
/// [`SPLIT_SLACK`] more.
fn presentation(
    avail: f32,
    dock: f32,
    workbench_open: bool,
    zoom: f32,
    was_split: bool,
) -> Presentation {
    let room = avail - CHAT_MIN * zoom;
    let needed = match was_split {
        true => DOCK_MIN,
        false => DOCK_MIN + SPLIT_SLACK,
    };
    if !workbench_open {
        Presentation::Conversation
    } else if room >= needed {
        Presentation::Split(dock.clamp(DOCK_MIN, room))
    } else {
        Presentation::WorkbenchFocus
    }
}

impl Shell {
    /// Share the area beside the rail for this frame, from the width the dock
    /// area measured on the last one.
    ///
    /// The dragged width is tracked here rather than read from the dock: a
    /// width the dock reports that is not the one this last drew is a drag,
    /// and only a drag changes what the person wants.
    pub(super) fn apply_presentation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A panel maximized over the window owns the dock area's zoom; once it
        // is restored, the area is shared again from scratch.
        if self.app_maximized.is_some() {
            self.presentation = Presentation::Conversation;
            self.workbench
                .update(cx, |panel, cx| panel.set_focused_area(false, cx));
            return;
        }
        let rem = f32::from(window.rem_size());
        let dock = self.dock.read(cx);
        let avail = f32::from(dock.bounds().size.width) / rem;
        // Nothing measured yet: the first frame has no bounds to share.
        if avail <= 0. {
            return;
        }
        let Some(right) = dock.right_dock().cloned() else {
            return;
        };
        let (size, open) = {
            let right = right.read(cx);
            (right.size(), right.is_open())
        };
        // Opened again by any other way in, the Workbench is no longer aside.
        if open {
            self.stepped_aside = false;
        }
        let zoom = self.chat.read(cx).zoom().factor();
        // A drag past the conversation's minimum is drawn at the most the
        // split allows, and kept as that, so the width wanted is one the
        // person saw.
        if open
            && let Some(drawn) = self.workbench_drawn
            && drawn != size
        {
            let most = (avail - CHAT_MIN * zoom).max(DOCK_MIN) * rem;
            self.window.workspace.layout.workbench_w = f32::from(size).min(most);
        }
        let wanted = self.window.workspace.layout.workbench_w / rem;
        let was_split = matches!(self.presentation, Presentation::Split(_));
        // A page puts the docks away and takes the conversation's place.
        let page = self.page_shown(cx);

        let mut open = open;
        // A caret put in the conversation while the Workbench has the area --
        // a session picked, the composer asked for -- is a request to see the
        // conversation, and drawn without it the caret would point at nothing.
        if open
            && self.presentation == Presentation::WorkbenchFocus
            && self.chat.read(cx).holds_caret(window, cx)
        {
            self.stepped_aside = true;
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
            open = false;
        }
        // Stepped aside, the split comes back by itself once there is room.
        if self.stepped_aside
            && !page
            && let Presentation::Split(_) = presentation(avail, wanted, true, zoom, false)
        {
            self.stepped_aside = false;
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
            open = true;
        }
        let next = presentation(avail, wanted, open && !page, zoom, was_split);
        let focus_before = self.presentation == Presentation::WorkbenchFocus;
        if let Presentation::Split(width) = next {
            let width = px(width * rem);
            if size != width {
                right.update(cx, |right, cx| right.set_size(width, window, cx));
            }
            self.workbench_drawn = Some(width);
        }
        let focus_now = next == Presentation::WorkbenchFocus;
        if focus_now != focus_before {
            // The conversation and the terminal leave the frame with the
            // Workbench taking the area, so a caret in either moves into the
            // Workbench first; left there, it would point at nothing and no
            // shortcut would reach the window.
            let leaving = self.chat.focus_handle(cx).contains_focused(window, cx)
                || self.terminal.focus_handle(cx).contains_focused(window, cx);
            if focus_now && leaving {
                self.last_panel = FocusedPanel::Workbench;
                let workbench = self.workbench.clone();
                window.defer(cx, move |window, cx| {
                    workbench.update(cx, |panel, cx| panel.focus_active(window, cx));
                });
            }
            let workbench = self.workbench.clone();
            self.dock.update(cx, |dock, cx| match focus_now {
                true => dock.set_zoomed_in(workbench, window, cx),
                false => dock.set_zoomed_out(window, cx),
            });
        }
        // Every frame, guarded inside: a flag pushed only on a change goes
        // stale across a maximize that restored straight into a split.
        self.workbench
            .update(cx, |panel, cx| panel.set_focused_area(focus_now, cx));
        self.presentation = next;
    }

    /// The Workbench steps aside so the conversation can be read: the dock
    /// closes without the Workbench being put away, so the header's button
    /// brings it straight back, and a wider window brings back the split.
    pub(super) fn step_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_workbench(window, cx);
        self.stepped_aside = true;
        self.last_panel = FocusedPanel::Chat;
        self.chat
            .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
    }

    /// Whether the Workbench has the area beside the rail.
    pub(super) fn workbench_focused(&self) -> bool {
        self.presentation == Presentation::WorkbenchFocus
    }

    /// Give the area back to the conversation now, rather than on the next
    /// frame, when the Workbench that had taken it is put away.
    pub(super) fn leave_workbench_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.presentation != Presentation::WorkbenchFocus {
            return;
        }
        self.presentation = Presentation::Conversation;
        self.dock
            .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
    }
}

#[cfg(test)]
mod tests;
