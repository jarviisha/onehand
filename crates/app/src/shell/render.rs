use super::{
    CloseSession, FocusComposer, NewSession, NextSession, OpenNeovim, OpenSettings, PrevSession,
    RestartSession, RunWorkflow, SaveFile, ShowSchedules, ShowTasks, ToggleMarkdown,
    ToggleMaximize, ToggleRail, ToggleTerminal, ToggleWorkbench, ToggleWorkbenchVisibility, ZoomIn,
    ZoomOut, ZoomReset,
};
use super::{FocusedPanel, SelectSession, Shell, ZoomStep};
use crate::state::Shared;
use crate::workbench::{EDITOR_MODE, MARKDOWN_MODE};
use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div,
    px,
};
use gpui_component::{
    ActiveTheme as _, ResizableState, Root, StyledExt, h_resizable, resizable_panel,
};
use onehand_core::config::PanelLayout;

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_presentation(window, cx);
        self.apply_terminal_height(window, cx);
        self.apply_rail_width(window, cx);
        self.sync_docks_open(cx);
        // A panel maximized in the app direction is the whole window: the rail
        // is not rendered at all rather than rendered at zero width, so
        // nothing of it can catch a click along the edge.
        let rail = (self.app_maximized.is_none() && !self.rail_hidden)
            .then(|| crate::rail::rail(self, window, cx));
        let dock = div().size_full().child(self.dock.clone());
        // With no rail there is no split to drag, so there is no split: an
        // `h_resizable` holding one panel would draw a handle against the
        // window's edge that resizes nothing.
        let body = match rail {
            None => dock.into_any_element(),
            Some(rail) => h_resizable("rail-split")
                .with_state(&self.rail_split)
                .child(
                    // `flex_none`: the panel sets `flex_grow: 1` on itself, and
                    // a rail that grows is a rail that takes whatever the dock
                    // is not using -- which is most of the window.
                    resizable_panel()
                        .size(px(self.window.workspace.layout.rail_w))
                        .size_range(px(PanelLayout::RAIL_MIN)..px(PanelLayout::RAIL_MAX))
                        .flex_none()
                        .child(rail),
                )
                .child(resizable_panel().child(dock))
                // Called once, when a drag ends: the one thing that changes the
                // width the person wants. Saved through the same debounce the
                // dock's own resizing goes through.
                .on_resize(
                    cx.listener(|shell: &mut Self, _: &Entity<ResizableState>, _, cx| {
                        shell.window.workspace.layout.rail_w = shell.rail_width(cx);
                        shell.rail_drawn = None;
                        shell.save_workspace_soon(cx);
                    }),
                )
                .into_any_element(),
        };
        // The rail and the dock are the whole frame now. `flex_1` + `min_h_0`
        // is left as it is because the body still has to be allowed to shrink
        // below its content rather than growing the window: both forms of it
        // are `size_full`, so this wrapper sets no direction of its own.
        let body = div().flex_1().min_h_0().w_full().child(body);
        let frame = div()
            .size_full()
            .v_flex()
            .key_context("Shell")
            .on_action(
                cx.listener(|shell: &mut Self, _: &OpenSettings, window, cx| {
                    shell.open_settings(window, cx);
                }),
            )
            .on_action(cx.listener(
                |shell: &mut Self, _: &ToggleWorkbenchVisibility, window, cx| {
                    shell.toggle_workbench(window, cx);
                },
            ))
            .on_action(cx.listener(|shell: &mut Self, _: &ToggleRail, window, cx| {
                shell.toggle_rail(window, cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleMarkdown, window, cx| {
                    shell.show_workbench(MARKDOWN_MODE, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleWorkbench, window, cx| {
                    shell.show_workbench(EDITOR_MODE, window, cx);
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &SaveFile, _, cx| {
                shell
                    .workbench
                    .update(cx, |panel, cx| panel.save_active(cx));
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleTerminal, window, cx| {
                    shell.show_terminal(window, cx);
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &OpenNeovim, window, cx| {
                shell.show_neovim(window, cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &FocusComposer, window, cx| {
                    shell.last_panel = FocusedPanel::Chat;
                    shell
                        .chat
                        .update(cx, |pane, cx| pane.focus_composer(window, cx));
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &NewSession, window, cx| {
                // Once per press: key repeat on a held chord would otherwise
                // start an agent per repeat.
                // The caret goes to the new composer: a key pressed from the
                // terminal is asking to type to the agent it just started.
                if shell.held_commands.insert(crate::keymap::NEW_SESSION)
                    && shell.spawn_session(0, None, window, cx).is_some()
                {
                    shell.last_panel = FocusedPanel::Chat;
                    shell
                        .chat
                        .update(cx, |pane, cx| pane.focus_composer(window, cx));
                }
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &RestartSession, window, cx| {
                    if shell.held_commands.insert(crate::keymap::RESTART) {
                        shell.restart_session(window, cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &CloseSession, window, cx| {
                    if shell.held_commands.insert(crate::keymap::CLOSE_SESSION) {
                        shell.close_active_session(window, cx);
                    }
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomIn, window, cx| {
                shell.zoom(ZoomStep::In, window, cx);
            }))
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomOut, window, cx| {
                shell.zoom(ZoomStep::Out, window, cx);
            }))
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomReset, window, cx| {
                shell.zoom(ZoomStep::Reset, window, cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &NextSession, window, cx| {
                    shell.cycle_session(true, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &PrevSession, window, cx| {
                    shell.cycle_session(false, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, action: &SelectSession, window, cx| {
                    shell.select_session(action.index, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &RunWorkflow, window, cx| {
                    shell.begin_workflow(window, cx);
                }),
            )
            .on_action(cx.listener(
                |shell: &mut Self, _: &crate::rail::FocusRailSearch, window, cx| {
                    shell.focus_rail_search(window, cx);
                },
            ))
            .on_action(
                cx.listener(|shell: &mut Self, _: &crate::rail::RailNext, window, cx| {
                    shell.rail_step(true, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &crate::rail::RailPrev, window, cx| {
                    shell.rail_step(false, window, cx);
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &ShowTasks, window, cx| {
                shell.show_tasks(None, window, cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &ShowSchedules, window, cx| {
                    shell.show_schedules(None, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleMaximize, window, cx| {
                    shell.toggle_maximize(window, cx);
                }),
            )
            // Commit on release of the actual shortcut modifiers, including remaps.
            .on_modifiers_changed(cx.listener(
                |shell: &mut Self, event: &gpui::ModifiersChangedEvent, _, cx| {
                    if shell
                        .tab_cycle
                        .as_ref()
                        .is_some_and(|cycle| !cycle.held(event.modifiers))
                    {
                        shell.end_cycle(cx);
                    }
                },
            ))
            .child(body);

        // `Root` stores dialogs, sheets and notifications; it does not draw
        // them. Its own `render` puts up the view, the tooltip overlay and the
        // native-menu overlay and stops -- every layer below is the app's to
        // mount, and until this existed `Dialog::trigger` opened a dialog into
        // a list nobody read, so every dialog did nothing when clicked.
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .size_full()
            .relative()
            // On the outermost element rather than the frame: a confirm a held
            // command opens takes focus into the dialog layer, a sibling of the
            // frame, and a release heard only inside the frame never cleared
            // the latch -- the same key then did nothing until pressed again.
            .capture_key_up(
                cx.listener(|shell: &mut Self, event: &gpui::KeyUpEvent, _, cx| {
                    let overrides = &Shared::global(cx).keymap;
                    shell
                        .held_commands
                        .retain(|id| !crate::keymap::released(id, &event.keystroke.key, overrides));
                }),
            )
            .child(frame)
            // The backdrop under the shell's own dialogs. The library dims
            // the window only under a dialog it opened itself, and these are
            // mounted here instead, so without this they stand on the app's
            // own surface and read as part of the page behind them.
            .children(self.dialog_up().then(|| {
                let dim = cx.theme().overlay;
                div().absolute().inset_0().bg(dim)
            }))
            // Above the frame, below the library's own layers: this one is
            // rendered by the shell rather than stored in `Root`, because it is
            // opened by a menu entry rather than by a control that can carry a
            // `Dialog::trigger`.
            .children(
                self.renaming
                    .map(|_| crate::dialogs::rename_session(self, cx)),
            )
            // Opened from the same kind of menu entry, and mounted here for the
            // same reason: there is no control left on screen to hang a
            // `Dialog::trigger` on by the time it is wanted.
            .children(
                self.worktree_draft
                    .is_some()
                    .then(|| crate::dialogs::new_worktree(self, cx)),
            )
            // And again for the same reason: the branch chip's menu is gone by
            // the time this is on screen.
            .children(
                self.branch_draft
                    .is_some()
                    .then(|| crate::dialogs::rename_branch(self, cx)),
            )
            // And the issue picker, opened from a project's menu the same way.
            .children(
                self.issue_picker
                    .is_some()
                    .then(|| crate::dialogs::pick_issue(self, window, cx)),
            )
            // And the workflow launcher, opened from a menu entry or a key.
            .children(
                self.workflow_launcher
                    .is_some()
                    .then(|| crate::dialogs::run_workflow(self, window, cx)),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

impl Shell {
    /// Whether one of the shell's own dialogs is up.
    fn dialog_up(&self) -> bool {
        self.renaming.is_some()
            || self.worktree_draft.is_some()
            || self.branch_draft.is_some()
            || self.issue_picker.is_some()
            || self.workflow_launcher.is_some()
    }

    /// Note what holds the keyboard as one of the shell's own dialogs opens.
    /// A dialog opened over another keeps the first one's, so closing both
    /// lands where the first was opened from.
    pub(super) fn remember_focus(&mut self, window: &Window, cx: &gpui::App) {
        if !self.dialog_up() {
            self.dialog_return = window.focused(cx).map(|focus| focus.downgrade());
        }
    }

    /// The keyboard left the frame. Once the shell's dialogs are all gone it
    /// goes back to what held it when the first one opened; and when that is
    /// gone too, or not drawn (which loses the keyboard once more and lands
    /// here again with nothing remembered), to the conversation, which every
    /// page and session keeps on screen.
    pub(super) fn focus_lost(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_up() {
            return;
        }
        match self.dialog_return.take().and_then(|back| back.upgrade()) {
            Some(focus) => focus.focus(window, cx),
            None => self
                .chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx)),
        }
    }
}
