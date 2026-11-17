//! The terminal dock's bar: the shells as its tabs, then `+`, maximize and
//! hide.

use super::{TerminalPanel, TerminalPanelEvent};
use crate::controls::Named as _;
use gpui::{
    Context, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    Window, div,
};
use gpui_component::{IconName, StyledExt};

impl TerminalPanel {
    pub(super) fn tab_bar(
        &self,
        labels: Vec<SharedString>,
        active: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        // The dock's tab bar: the shells as its tabs, the chosen one
        // on the open tab's fill.
        onehand_ui::tab_bar(cx)
            .id("terminal-tabs")
            // The tabs live in a box of their own that scrolls sideways,
            // and that box is what gives way, so `+` and the controls at
            // the end stay on screen at any count.
            .child(
                div()
                    .id("terminal-tab-list")
                    .track_scroll(&self.tab_scroll)
                    .h_flex()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .children(labels.into_iter().enumerate().map(|(i, label)| {
                        onehand_ui::panel_tab(
                            ("terminal-tab", i),
                            label.clone(),
                            label,
                            i == active,
                            cx,
                        )
                        .max_w(onehand_ui::TAB_MAX_W)
                        .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                            panel.select_tab(i, cx);
                        }))
                        // Inside the tab, so `stop_propagation` is what
                        // keeps the press that closes a shell from also
                        // selecting the one it closed.
                        .child(
                            onehand_ui::tab_close(("close-shell", i), "Close this shell", cx)
                                .on_click(cx.listener(
                                move |panel: &mut Self,
                                      _,
                                      window: &mut Window,
                                      cx: &mut Context<Self>| {
                                    cx.stop_propagation();
                                    panel.close_tab(i, window, cx);
                                },
                            )),
                        )
                    })),
            )
            // Muted, like the conversation header's own controls: a
            // ghost button in full ink would be the brightest thing in
            // the panel. The hover fill brings it back on the one about
            // to be pressed.
            .child(
                div().h_flex().items_center().px_1().child(
                    onehand_ui::strip_control("add-shell", IconName::Plus, false, cx)
                        .named("New shell in this project")
                        .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                            panel.open_shell(window, cx);
                        })),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    // The icon is the state, not the action: the one situation
                    // where it matters is the panel filling the frame and the
                    // user looking for the way back.
                    .child({
                        let full = self.maximized;
                        onehand_ui::strip_control(
                            "maximize-terminal",
                            match full {
                                true => IconName::Minimize,
                                false => IconName::Maximize,
                            },
                            false,
                            cx,
                        )
                        .named_with_action(
                            match full {
                                true => "Back to the dock",
                                false => "Fill the window",
                            },
                            &crate::shell::ToggleMaximize,
                            None,
                        )
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(TerminalPanelEvent::ToggleMaximize);
                        }))
                    })
                    // A minus, as the Workbench's: the shells are not being
                    // ended, which the cross on every tab is for. `Minus` and
                    // never `Dash`, whose stroke is written as literal black and
                    // ignores `text_color`.
                    .child(
                        onehand_ui::strip_control("hide-terminal", IconName::Minus, false, cx)
                            .named_with_action(
                                "Hide the terminal",
                                &crate::shell::ToggleTerminal,
                                None,
                            )
                            .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                cx.emit(TerminalPanelEvent::Hide);
                            })),
                    ),
            )
    }
}
