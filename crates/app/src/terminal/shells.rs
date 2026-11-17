//! The terminal dock's chrome: its bar (the open shell's name, then `+`,
//! maximize and hide) and, while it holds more than one shell, the list of
//! them down the box's right side.

use super::{TerminalPanel, TerminalPanelEvent};
use crate::controls::Named as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, FocusHandle, InteractiveElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt};

// The keys of a row in the list, keymap commands bound in `ShellRow`, the
// context only a row of the list carries.
gpui::actions!(terminal, [ShellRowUp, ShellRowDown, CloseShellRow]);

/// The most rows the list draws before it says how many more there are.
pub(super) const SHELL_LIST_CAP: usize = 30;

/// How many of `count` rows the list draws, and how many it leaves out.
pub(super) fn shown_rows(count: usize) -> (usize, usize) {
    let shown = count.min(SHELL_LIST_CAP);
    (shown, count - shown)
}

/// Whether the dock draws its list of shells: a lone shell has nothing to be
/// picked from, and its name already heads the bar.
pub(super) fn lists_shells(count: usize) -> bool {
    count > 1
}

/// What one row of the list says.
pub(super) struct ShellRow {
    pub(super) name: SharedString,
    /// A command is in the shell's foreground. One whose system will not say
    /// is not counted.
    pub(super) live: bool,
    pub(super) chosen: bool,
}

/// The rows for `shells`, each its program's name and whether it is busy, the
/// one at `active` chosen.
///
/// A shell is named by the PTY's program alone, which is the same word for
/// every shell a root has open, so three shells came out reading `zsh`.
/// **Numbered, and only where there is more than one**: a lone shell has
/// nothing to be told apart from, and `zsh 1` beside no `zsh 2` is a question
/// about where the rest went.
///
/// Not the project, which was tried and is the same word on every row for a
/// stronger reason than the shell is: this panel draws one root's shells and
/// only ever that root's, so the part of the name carrying the project was
/// constant by construction and first in line to be cut by the width cap.
///
/// Composed here and not in `PtyTab::label`, which the Neovim mode also reads:
/// that mode has one grid and no list, so a name built to separate siblings
/// would be a name with nothing to separate it from.
pub(super) fn shell_rows(shells: &[(SharedString, Option<bool>)], active: usize) -> Vec<ShellRow> {
    shells
        .iter()
        .enumerate()
        .map(|(i, (label, busy))| ShellRow {
            name: match shells.len() {
                1 => label.clone(),
                _ => format!("{label} {}", i + 1).into(),
            },
            live: *busy == Some(true),
            chosen: i == active,
        })
        .collect()
}

impl TerminalPanel {
    /// The dock's bar: the open shell's name, then `+`, maximize and hide.
    /// With no shell open there is no name, and the controls still stand,
    /// since the way out of the dock lives on this row.
    pub(super) fn top_bar(
        &self,
        name: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        onehand_ui::tab_bar(cx)
            .id("terminal-bar")
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .truncate()
                    .text_size(onehand_ui::tokens::CHROME_TEXT)
                    .children(name),
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
                    // ended, which the cross on every row is for. `Minus` and
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

    /// The shells down the box's right side, at a fixed width: the chosen one
    /// on the open tab's fill and scrolled into view, each row's close shown
    /// under the pointer and on the keyboard's row.
    ///
    /// Each row is a tab stop. Tab itself belongs to the PTY anywhere in this
    /// panel, so the keys that move here are the list's own commands (Up and
    /// Down step between rows, Delete closes one), and Enter or Space picks
    /// one, the caret going into its grid.
    pub(super) fn shell_list(
        &self,
        rows: Vec<ShellRow>,
        focus: Vec<FocusHandle>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let (secondary, ring, info, muted) = (
            theme.secondary,
            theme.ring,
            theme.info,
            theme.muted_foreground,
        );
        // The list stands on the terminal's grey, under which the list hover
        // of a white box sinks by day: a row is hovered as the rail's are.
        let hover = theme.sidebar_accent;
        let chosen_ink = onehand_ui::chosen_ink(cx);
        let mut list = div()
            .id("terminal-shell-list")
            .track_scroll(&self.tab_scroll)
            .flex_none()
            .w(onehand_ui::tokens::SHELL_LIST_W)
            .h_full()
            .v_flex()
            .overflow_y_scroll();
        let (shown, left_out) = shown_rows(rows.len());
        for (i, (row, handle)) in rows.into_iter().zip(focus).take(shown).enumerate() {
            let group = SharedString::from(format!("terminal-shell-{i}"));
            let name = row.name.clone();
            let close = onehand_ui::tab_close(("close-shell", i), "Close this shell", cx).on_click(
                cx.listener(
                    move |panel: &mut Self, _, window: &mut Window, cx: &mut Context<Self>| {
                        cx.stop_propagation();
                        panel.close_tab(i, window, cx);
                    },
                ),
            );
            list = list.child(
                onehand_ui::tree_row(("terminal-shell", i), 0, false, cx)
                    .group(group.clone())
                    .track_focus(&handle)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(move |el| el.border_color(ring))
                    .map(|el| match row.chosen {
                        true => el
                            .bg(secondary)
                            .text_color(chosen_ink)
                            .hover(move |el| el.bg(secondary)),
                        false => el.hover(move |el| el.bg(hover)),
                    })
                    .tooltip(onehand_ui::tip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(name.clone()).build(window, cx)
                    }))
                    // The caret goes to the grid picked: typing is what a shell
                    // is picked for, and the grid it leaves is no longer drawn,
                    // so focus left there would point at no node in the frame.
                    .on_click(cx.listener(move |panel: &mut Self, _, window, cx| {
                        panel.select_tab(i, cx);
                        panel.focus_active(window, cx);
                    }))
                    .key_context("ShellRow")
                    .on_action(
                        cx.listener(move |panel: &mut Self, _: &ShellRowUp, window, cx| {
                            panel.focus_row(i.checked_sub(1), window, cx)
                        }),
                    )
                    .on_action(cx.listener(
                        move |panel: &mut Self, _: &ShellRowDown, window, cx| {
                            panel.focus_row(Some(i + 1), window, cx)
                        },
                    ))
                    .on_action(cx.listener(
                        move |panel: &mut Self, _: &CloseShellRow, window, cx| {
                            panel.close_tab(i, window, cx)
                        },
                    ))
                    .child(onehand_ui::mark_slot(
                        Icon::new(IconName::SquareTerminal)
                            .small()
                            .when(!row.chosen, |icon| icon.text_color(muted)),
                    ))
                    .child(div().flex_1().min_w_0().truncate().child(row.name))
                    .when(row.live, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .size(crate::chat::pane::LIVE_DOT)
                                .rounded_full()
                                .bg(info),
                        )
                    })
                    // Inside the row, so `stop_propagation` is what keeps the
                    // press that closes a shell from also picking the one it
                    // closed.
                    .child(onehand_ui::hover_slot(group, false, close).focus(&handle)),
            );
        }
        // A list that silently stops is one the user thinks they have seen
        // all of. A shell past the cap that is chosen is still named in the
        // bar above.
        list.when(left_out > 0, |list| {
            list.child(
                div()
                    .pl(onehand_ui::tokens::ROW_INSET)
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{left_out} more")),
            )
        })
    }

    /// Put the keyboard on row `to` of the list, when there is one. Closing a
    /// row goes through [`Self::close_tab`], which hands the caret to the shell
    /// then chosen before the row leaves.
    fn focus_row(&mut self, to: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let handle = to.and_then(|to| {
            let set = self.active_set()?;
            Some(set.tabs.get(to)?.row.clone())
        });
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }
}
