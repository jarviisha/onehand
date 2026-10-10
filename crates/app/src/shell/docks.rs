use super::{FocusedPanel, Shell, ZoomStep};
use crate::workbench::NEOVIM_MODE;
use gpui::{App, Context, Focusable as _, Window, px};
use gpui_component::dock::{DockItem, DockPlacement};
use onehand_core::config::PanelLayout;
use std::path::Path;

impl Shell {
    /// The panel a panel-scoped command addresses: whichever holds focus, else
    /// the last one a panel command was aimed at.
    pub fn focused_panel(&self, window: &Window, cx: &App) -> FocusedPanel {
        if self.terminal.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Terminal
        } else if self.workbench.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Workbench
        } else if self.chat.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Chat
        } else {
            self.last_panel
        }
    }

    /// Tell the conversation header whether the active root has a shell alive.
    ///
    /// One fact and one reader: the terminal button's dot, which is the only
    /// thing on screen that can say a child process outlived a closed dock. The
    /// push is guarded on the pane's side, because this runs on every chunk a
    /// build prints into the terminal.
    pub(super) fn sync_terminal_live(&mut self, cx: &mut Context<Self>) {
        let live = self.terminal.read(cx).has_shell();
        self.chat
            .update(cx, |pane, cx| pane.set_terminal_live(live, cx));
    }

    /// Tell the conversation header which docks are open, so their buttons can
    /// show it. Guarded on the pane's side; run on every frame the window draws,
    /// because a dock opens from more places than any one hook sees.
    pub(super) fn sync_docks_open(&mut self, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        let open = crate::chat::DocksOpen {
            terminal: dock.is_dock_open(DockPlacement::Bottom, cx),
            workbench: dock.is_dock_open(DockPlacement::Right, cx),
        };
        self.chat
            .update(cx, |pane, cx| pane.set_docks_open(open, cx));
    }

    /// Read the dock's current geometry back out.
    ///
    /// Four facts, not `DockArea::dump`. `dump` produces a whole
    /// `DockAreaState`, but *restoring* one rebuilds every panel through
    /// gpui-component's process-global `PanelRegistry` — and onehand's panels
    /// are per window and held by the shell, so the shell would be left holding
    /// handles to orphans, and one global registry could not tell two windows'
    /// panels apart anyway. The arrangement here is fixed by design, so what a
    /// user actually changes is these values (see
    /// `onehand_core::config::PanelLayout`).
    ///
    /// The rail's width comes from the split it lives in rather than from the
    /// dock, because the rail is not in the dock -- but it is the same fact and
    /// belongs in the same snapshot.
    pub(super) fn dock_layout(&self, cx: &App) -> PanelLayout {
        let dock = self.dock.read(cx);
        let fallback = self.window.workspace.layout;
        let right = dock.right_dock().map(|d| d.read(cx));
        let bottom = dock.bottom_dock().map(|d| d.read(cx));
        PanelLayout {
            // The width the person wants, never one a narrow window drew.
            workbench_w: fallback.workbench_w,
            workbench_open: right.is_some_and(|d| d.is_open()) || self.stepped_aside,
            terminal_h: bottom.map_or(fallback.terminal_h, |d| f32::from(d.size())),
            terminal_open: bottom.is_some_and(|d| d.is_open()),
            rail_w: self.rail_width(cx),
        }
    }

    /// How wide the rail is right now, as the split has it.
    ///
    /// The saved width is only ever the answer when the live one cannot be:
    /// the split seeds every panel at its own floor before the first prepaint
    /// measures anything, so a size outside the range the rail could have been
    /// dragged to is that gap rather than a width the user chose.
    ///
    /// Two readers, and they want different things from it -- the snapshot
    /// wants a number to write down, the rail wants to know how much of a name
    /// a row can carry this frame -- so the check and the fallback are here
    /// rather than at each of them.
    pub fn rail_width(&self, cx: &App) -> f32 {
        self.rail_split
            .read(cx)
            .sizes()
            .first()
            .map(|w| f32::from(*w))
            .filter(|w| (PanelLayout::RAIL_MIN..=PanelLayout::RAIL_MAX).contains(w))
            .unwrap_or(self.window.workspace.layout.rail_w)
    }

    /// Open the bottom terminal on a named root.
    ///
    /// Selection first for the same reason `new_session_in` does it: the
    /// terminal is a tab per root, so opening it without switching would put
    /// the user in a shell in one project while every other panel says another.
    pub fn open_terminal_in(
        &mut self,
        root_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root(root_idx, window, cx);
        // Never the closing direction: this was asked for by name, and a menu
        // entry called *Open terminal* that closes the terminal is a bug that
        // reads as one.
        if !self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx) {
            self.show_terminal(window, cx);
        }
    }

    /// Open and focus a mode. Repeating its shortcut never hides the panel.
    pub fn show_workbench(
        &mut self,
        mode: onehand_plugin_api::PluginId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The Workbench and the terminal are one project's, and the workspace
        // page stands on none.
        if self.page_shown(cx) {
            return;
        }
        self.last_panel = FocusedPanel::Workbench;
        self.stepped_aside = false;
        let open = self.dock.read(cx).is_dock_open(DockPlacement::Right, cx);
        self.workbench
            .update(cx, |panel, cx| panel.set_mode(mode, cx));
        if !open {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
        self.workbench
            .update(cx, |panel, cx| panel.focus_active(window, cx));
        cx.notify();
    }

    /// Toggle visibility independently of focus, preserving the selected mode.
    pub(super) fn toggle_workbench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock.read(cx).is_dock_open(DockPlacement::Right, cx) {
            self.hide_workbench(window, cx);
        } else {
            let mode = self.workbench.read(cx).mode();
            self.show_workbench(mode, window, cx);
        }
    }

    /// Close the dock and recover focus if its focused content disappears.
    pub(super) fn hide_workbench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stepped_aside = false;
        self.leave_workbench_focus(window, cx);
        let had_focus = self.workbench.focus_handle(cx).contains_focused(window, cx);
        // A maximized panel cannot be left blown up over a dock that is no
        // longer open, and the way back out of that is the button that just
        // went away with it.
        if self.app_maximized == Some(FocusedPanel::Workbench) {
            self.set_app_maximized(None, cx);
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
        }
        self.dock.update(cx, |dock, cx| {
            if dock.is_dock_open(DockPlacement::Right, cx) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
        // The caret may be inside the panel going off screen, and a closed dock
        // draws none of its content -- so leaving focus there leaves the window
        // pointing at an element no frame contains, which is a window no
        // shortcut reaches.
        if had_focus {
            self.last_panel = FocusedPanel::Chat;
            self.chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
        }
        cx.notify();
    }

    /// Toggle the bottom terminal regardless of focus. A shell is spawned on
    /// first open and never at boot (see [`crate::terminal`]).
    pub fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page_shown(cx) {
            return;
        }
        self.last_panel = FocusedPanel::Terminal;
        // The terminal sits under the conversation, so while the Workbench has
        // the area, asking for it brings the conversation back first -- and
        // the press then opens the terminal, never closes it.
        let stepped = self.workbench_fills_area();
        if stepped {
            self.step_aside(window, cx);
        }
        let open = self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
        // **An open dock with nothing in it is not a dock to close.** Closing
        // the last tab's ✕ leaves exactly that, and the panel it leaves offers
        // *New terminal* -- so a press here means "open one", which is what
        // falling through does. Closed instead, the one gesture that reaches an
        // empty terminal took it off screen, and the way back up asked for a
        // shell the user had just been offered.
        if open && !stepped && self.terminal.read(cx).has_shell() {
            self.set_terminal_visible(false, window, cx);
            return;
        }
        self.set_terminal_visible(true, window, cx);
        self.terminal.update(cx, |panel, cx| {
            if !panel.has_shell() {
                panel.open_shell(window, cx);
            }
            panel.focus_active(window, cx);
        });
        cx.notify();
    }

    /// Open Neovim on the active project, as the Workbench's third mode.
    ///
    /// Spawned here rather than by the mode switch, so that clicking the mode
    /// strip stays a view change: the key is the request to *start* an editor,
    /// and a tab that launched a process when clicked would be the one control
    /// in the panel that does something irreversible-looking.
    ///
    /// Three-state after that, like the other two Workbench keys and for the
    /// same reason: closing the dock puts the editor aside rather than ending
    /// it — the panel entity outlives the dock, so the PTY, its scrollback and
    /// whatever is unsaved in the buffer are all still there on the next press.
    pub fn show_neovim(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused before the editor is started, not only before the dock
        // opens: on the workspace page there is no project to start it in.
        if self.page_shown(cx) {
            return;
        }
        self.workbench.update(cx, |panel, cx| panel.start_child(cx));
        self.show_workbench(NEOVIM_MODE, window, cx);
    }

    /// Put the terminal on screen, or take it off.
    ///
    /// **Mounted and unmounted, not opened and closed.** A *closed* bottom dock
    /// still draws a strip of title bar, on the library's own reasoning that the
    /// button to reopen it lives there — and this terminal has no such button to
    /// put on it. What was left was a bare band of chrome across the bottom of
    /// every window, in every project, naming nothing and reopening nothing.
    /// With no bottom dock at all there is nothing to draw, and the way back is
    /// the key and the terminal button in the conversation's header.
    pub(super) fn set_terminal_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if visible == self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx) {
            return;
        }
        // Asked while the panel is still in the frame, because a handle that is
        // no longer drawn cannot answer it. Every route that takes the terminal
        // off screen arrives here -- the key, the header button, the project
        // menu, a project handing its dock over -- so the caret is given back
        // once rather than at each of them.
        if !visible && self.terminal.focus_handle(cx).contains_focused(window, cx) {
            self.chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
        }
        if visible {
            // The panel entity is the same one every time, so the shells, their
            // PTYs and their scrollback all survive being taken off screen --
            // only the dock around them is rebuilt.
            let item = DockItem::panel(std::sync::Arc::new(self.terminal.clone()));
            let height = px(self.window.workspace.layout.terminal_h);
            self.dock.update(cx, |area, cx| {
                area.set_bottom_dock(item, Some(height), true, window, cx);
                cx.notify();
            });
            cx.notify();
            return;
        }
        // The height has to be read back before the dock holding it goes, or
        // every reopen comes up at the built-in default and the drag is lost.
        // It lands in the workspace's own layout, which is what the saved
        // arrangement falls back to while there is no dock to ask.
        if let Some(height) = self
            .dock
            .read(cx)
            .bottom_dock()
            .map(|dock| f32::from(dock.read(cx).size()))
        {
            self.window.workspace.layout.terminal_h = height;
        }
        // A maximized panel cannot be unmounted out from under the zoom: the
        // dock area would be left blown up over something that is no longer
        // mounted, and the key that undoes it is the same key that got here.
        if self.app_maximized == Some(FocusedPanel::Terminal) {
            self.set_app_maximized(None, cx);
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
        }
        self.dock.update(cx, |area, cx| {
            area.remove_bottom_dock(window, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Hand the terminal dock over from the project being left to the one
    /// arriving.
    ///
    /// Called from the one place the selection actually moves, and it does both
    /// halves there: it files the live state under the root that owns it, then
    /// puts the dock into whatever the incoming root left it in. Doing it at the
    /// handover rather than at each of the four controls that can toggle the
    /// dock is what keeps this from being four places to forget — the dock is
    /// read, never assumed.
    ///
    /// A session switch inside one project is not a handover: the dock on screen
    /// is already this root's, so the live state is filed and nothing moves.
    /// Restoring there would fight a user who had just opened it.
    pub(super) fn follow_terminal_dock(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live = self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
        let leaving = self.terminal_root.replace(root.to_path_buf());
        if leaving.as_deref() == Some(root) {
            self.terminal_open.insert(root.to_path_buf(), live);
        } else {
            if let Some(leaving) = leaving {
                self.terminal_open.insert(leaving, live);
            }
            // A project the terminal has never been opened in gets it closed.
            // The shells that were on screen a moment ago belong to the project
            // just left and do not come along, so an inherited open dock would
            // greet the new one with an empty panel where they had been.
            let wanted = self.terminal_open.get(root).copied().unwrap_or(false);
            self.set_terminal_visible(wanted, window, cx);
        }
        // Whichever branch ran, and this is the whole rule: a dock that is on
        // screen has a shell in it. The session-switch branch is an `else` now
        // rather than an early return precisely so this covers it too -- there
        // the dock was already showing and was already this root's, so the one
        // way to reach it with nothing inside is to have closed the last shell,
        // and a panel drawn over nothing is what this exists to prevent.
        self.fill_open_terminal(window, cx);
    }

    /// Start a shell where the dock is open on a root that has none.
    ///
    /// **An open dock is a request for a terminal, and the only thing that ever
    /// answered it was the key.** A launch restoring a saved layout mounts the
    /// panel and stops there, so the first thing a user who left the terminal
    /// open saw on the next launch was an empty panel asking them to press
    /// *New terminal* — a question whose answer they had already given by
    /// leaving it open.
    ///
    /// The rule it holds is one sentence — **a terminal dock on screen has a
    /// shell in it** — and it is the panel being *drawn* that earns the shell,
    /// not any particular way of getting there. Arriving at a project, coming
    /// back to one, a launch restoring a layout, or moving between sessions in
    /// a project whose last shell was closed: all of them are the panel about
    /// to be drawn, so all of them go through here.
    ///
    /// This is not the rule that keeps shells lazy, which is about roots nobody
    /// is looking at: this runs for the arriving root alone and only where that
    /// root's dock is open, so a workspace of a dozen projects still starts at
    /// most one shell, in the project on screen, because its dock is showing.
    ///
    /// Called from the handover and nowhere else, for the reason the handover
    /// itself is: the dock is *read* rather than assumed, so this cannot drift
    /// from the four controls that toggle it.
    fn fill_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx) {
            return;
        }
        self.terminal.update(cx, |panel, cx| {
            if !panel.has_shell() {
                panel.open_shell(window, cx);
            }
        });
    }

    /// Blow the focused panel up to the whole frame, or put it back.
    ///
    /// The rail goes with it -- that is what makes this the *app* direction
    /// rather than the content one, which each panel's tab bar offers as a
    /// button and which leaves the rail in place. Restoring is the same key,
    /// whichever panel is focused: a maximized panel is the only thing on
    /// screen, so there is nothing else the key could mean.
    pub(super) fn toggle_maximize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.focused_panel(window, cx);
        self.toggle_maximize_panel(panel, window, cx);
    }

    /// The same thing for a named panel rather than the focused one.
    ///
    /// Split out because a *control* cannot use the focused-panel rule the key
    /// uses: the button lives in the terminal's own strip and says so, while
    /// the caret at the moment it is pressed may be anywhere -- so routing it
    /// through the key's path would blow up the conversation because that is
    /// where the user happened to be typing.
    ///
    /// Restoring is still whatever is maximized, whoever asks: a maximized
    /// panel is the only thing on screen, so there is nothing else the request
    /// could mean.
    pub(super) fn toggle_maximize_panel(
        &mut self,
        panel: FocusedPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.app_maximized.is_some() {
            self.set_app_maximized(None, cx);
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
            cx.notify();
            return;
        }
        // The rail goes with everything else but the panel, so a caret in it
        // moves into the panel that stays.
        let rail_had_focus = self.rail.holds_focus(window, cx);
        self.dock.update(cx, |dock, cx| match panel {
            FocusedPanel::Chat => dock.set_zoomed_in(self.chat.clone(), window, cx),
            FocusedPanel::Workbench => dock.set_zoomed_in(self.workbench.clone(), window, cx),
            FocusedPanel::Terminal => dock.set_zoomed_in(self.terminal.clone(), window, cx),
        });
        if rail_had_focus {
            match panel {
                FocusedPanel::Chat => self
                    .chat
                    .update(cx, |pane, cx| pane.reclaim_focus(window, cx)),
                FocusedPanel::Workbench => self.workbench.focus_handle(cx).focus(window, cx),
                FocusedPanel::Terminal => self.terminal.focus_handle(cx).focus(window, cx),
            }
        }
        self.set_app_maximized(Some(panel), cx);
        cx.notify();
    }

    /// Write the field, and tell the two panels that draw a control toggling it
    /// and so have to know which way round it currently is.
    ///
    /// One setter because the field moves from several places -- the key, each
    /// strip's button, unmounting a maximized terminal, hiding a maximized
    /// Workbench -- and a push left off one of them is a button whose icon says
    /// the opposite of what it does. Guarded inside each panel, because this is
    /// called on paths that run whether or not anything changed and a panel
    /// notifying redraws the window.
    fn set_app_maximized(&mut self, panel: Option<FocusedPanel>, cx: &mut Context<Self>) {
        self.app_maximized = panel;
        let terminal = panel == Some(FocusedPanel::Terminal);
        self.terminal.update(cx, |terminal_panel, cx| {
            terminal_panel.set_maximized(terminal, cx);
        });
        let workbench = panel == Some(FocusedPanel::Workbench);
        self.workbench.update(cx, |workbench_panel, cx| {
            workbench_panel.set_maximized(workbench, cx);
        });
    }

    /// Step the focused panel's zoom.
    ///
    /// The target is the panel that holds focus, falling back to the last one
    /// a panel command addressed -- so zooming right after a shortcut opened a
    /// panel hits that panel, not whatever was under the mouse.
    pub(super) fn zoom(&mut self, step: ZoomStep, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.focused_panel(window, cx);
        self.zoom_panel(panel, step, cx);
    }

    /// Step one named panel's zoom.
    fn zoom_panel(&mut self, panel: FocusedPanel, step: ZoomStep, cx: &mut Context<Self>) {
        match panel {
            FocusedPanel::Chat => self.chat.update(cx, |pane, cx| {
                step.apply(pane.zoom_mut());
                cx.notify();
            }),
            // Handed the whole value, not a `&mut` to the field: the Workbench's
            // Neovim mode is a measured grid, so a step there has to be pushed
            // into the view as a font size as well as recorded.
            FocusedPanel::Workbench => self.workbench.update(cx, |panel, cx| {
                let mut zoom = panel.zoom();
                step.apply(&mut zoom);
                panel.set_zoom(zoom, cx);
            }),
            FocusedPanel::Terminal => self.terminal.update(cx, |panel, cx| {
                let mut zoom = panel.zoom();
                step.apply(&mut zoom);
                panel.set_zoom(zoom, cx);
            }),
        }
        cx.notify();
    }

    /// Whether the rail is on screen.
    pub(crate) fn rail_shown(&self) -> bool {
        !self.rail_hidden && self.app_maximized.is_none()
    }

    pub(crate) fn rail_state(&self) -> &crate::rail::RailState {
        &self.rail
    }

    pub(crate) fn rail_state_mut(&mut self) -> &mut crate::rail::RailState {
        &mut self.rail
    }

    /// The window's workspace and what is known about its projects, for the
    /// rail to draw.
    pub(crate) fn workspace_window(&self) -> &crate::state::WorkspaceWindow {
        &self.window
    }

    /// Show or hide the rail.
    ///
    /// The chat pane is told, because it is what offers the way back: with the
    /// rail gone the key is the only route to it, and a key nobody has been
    /// told about is not a route.
    pub fn toggle_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hidden = !self.rail_hidden;
        if hidden && self.rail.holds_focus(window, cx) {
            self.chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
        }
        self.rail_hidden = hidden;
        self.chat
            .update(cx, |pane, cx| pane.set_rail_hidden(hidden, cx));
        cx.notify();
    }

    /// Bring the rail back, whatever asked for it: a panel filling the window
    /// hides it as surely as hiding it does.
    pub(crate) fn show_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_maximized.is_some() {
            self.toggle_maximize(window, cx);
        }
        if !self.rail_hidden {
            return;
        }
        self.toggle_rail(window, cx);
    }

    /// Open issue `number` of the project at `root`: that project selected,
    /// and the Workbench on its Issues mode with the issue showing.
    pub(super) fn open_issue(
        &mut self,
        root: &Path,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(idx) = self
            .window
            .workspace
            .roots
            .iter()
            .position(|r| r.path == root)
        else {
            return;
        };
        self.select_root(idx, window, cx);
        self.show_workbench(crate::workbench::ISSUES_MODE, window, cx);
        self.workbench
            .update(cx, |panel, cx| panel.show_issue(number, cx));
    }
}
