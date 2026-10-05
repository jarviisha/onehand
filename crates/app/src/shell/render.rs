use super::{
    CloseSession, FocusComposer, NewSession, NextSession, OpenNeovim, OpenSettings, PrevSession,
    RestartSession, RunWorkflow, SaveFile, ShowTasks, ToggleMarkdown, ToggleMaximize, ToggleRail,
    ToggleTerminal, ToggleWorkbench, ToggleWorkbenchVisibility, ZoomIn, ZoomOut, ZoomReset,
};
use super::{FocusedPanel, SelectSession, Shell, ZoomStep};
use crate::state::Shared;
use crate::workbench::{EDITOR_MODE, MARKDOWN_MODE};
use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div,
    px,
};
use gpui_component::{ResizableState, Root, StyledExt, h_resizable, resizable_panel};
use onehand_core::config::PanelLayout;

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A panel maximized in the app direction is the whole window: the rail
        // is not rendered at all rather than rendered at zero width, so
        // nothing of it can catch a click along the edge.
        let rail = (self.app_maximized.is_none() && !self.rail_hidden)
            .then(|| crate::rail::rail(self, &self.window, cx));
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
                // The drag emits per frame, so this goes through the same
                // debounce the dock's own resizing does rather than writing the
                // workspace file on every pixel.
                .on_resize(
                    cx.listener(|shell: &mut Self, _: &Entity<ResizableState>, _, cx| {
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
            .capture_key_up(
                cx.listener(|shell: &mut Self, event: &gpui::KeyUpEvent, _, cx| {
                    let overrides = &Shared::global(cx).keymap;
                    shell
                        .held_commands
                        .retain(|id| !crate::keymap::released(id, &event.keystroke.key, overrides));
                }),
            )
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
            .on_action(cx.listener(|shell: &mut Self, _: &ToggleRail, _, cx| {
                shell.toggle_rail(cx);
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
            .on_action(cx.listener(|shell: &mut Self, _: &ShowTasks, window, cx| {
                shell.show_tasks(None, window, cx);
            }))
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
        // a list nobody read. That is why the settings, agent-manager and Help
        // dialogs did nothing when clicked.
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .size_full()
            .relative()
            .child(frame)
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
                    .then(|| crate::dialogs::pick_issue(self, cx)),
            )
            // And the workflow launcher, opened from a menu entry or a key.
            .children(
                self.workflow_launcher
                    .is_some()
                    .then(|| crate::dialogs::run_workflow(self, window, cx)),
            )
            .children(
                self.settings_open
                    .then(|| crate::settings::dialog(self.settings_view.clone(), window, cx)),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}
