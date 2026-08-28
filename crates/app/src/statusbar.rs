//! The window's status bar.
//!
//! One row along the bottom of the frame, under the rail *and* the dock, saying
//! what the window is currently pointed at: which project, what git says about
//! it, which agent is running and how it is doing, whether anything is unsaved,
//! and whether a panel is being read at something other than 100%.
//!
//! **It says what nothing else on screen says.** The conversation's own name and
//! what it is doing already sit in the agent pane's header, so neither is
//! repeated here. Everything in this row is either invisible while the rail is
//! hidden (the project, the branch, the agent) or invisible everywhere (the
//! unsaved count, a zoom factor). The terminal used to be here for the same
//! reason and has moved into that header, next to the Workbench button: the two
//! docks the conversation sits between are one decision, and the panel they take
//! their space from is where both belong.
//!
//! **The pointer is the contract.** A cell that shows the pointer and lights on
//! hover does something when pressed; a cell that does neither is a reading.
//! Half a row of each with no way to tell them apart is the failure this splits
//! [`cell`] and [`pressable`] to avoid.
//!
//! Sizes are rems, and the row is mounted outside every panel's zoom wrapper, so
//! it holds still while a panel scales — chrome that grew with the conversation
//! would be taking the conversation's space to say the same thing.

use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use crate::workbench::WorkbenchMode;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};

/// A project label is an anchor, not content: capped so a deep folder name
/// cannot push the rest of the row off the end.
const MAX_PROJECT_W: gpui::Rems = gpui::Rems(10.);

/// The branch line is the least important thing on the left, so it gives way
/// first — `fix/architecture-hardening-and-open-telemetry` is a real branch name
/// and would otherwise own the bar.
const MAX_GIT_W: gpui::Rems = gpui::Rems(11.);

/// The agent's name is a footnote about *how* the conversation runs; the mark
/// beside it is the part worth the width.
const MAX_AGENT_W: gpui::Rems = gpui::Rems(9.);

/// One cell: an icon column, a label, and enough padding to sit off its
/// neighbours.
///
/// Returned without hover or cursor styling. `hover` panics in debug if it is
/// set twice, so the pressable form is built on top of this rather than beside
/// it.
fn cell(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .min_w_0()
        .rounded(cx.theme().radius)
}

/// A cell that answers a press, and says so before it is pressed.
fn pressable(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().list_hover;
    cell(id, cx).cursor_pointer().hover(|row| row.bg(hover))
}

/// The bar. `use<>` for the same reason the rail needs it: everything returned
/// is owned, so it must not capture the borrows the caller needs back.
pub fn status_bar(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let (background, border, muted) = (theme.background, theme.border, theme.muted_foreground);
    let warning = crate::theme::status_ink(cx).warning;

    let root_idx = window_state.workspace.active_root;
    let root = window_state.workspace.active_root();
    let git = root.and_then(|root| window_state.git.get(&root.path));
    let session = root.and_then(|root| root.active_session());
    let facts = shell.panel_facts(cx);
    let zoomed = shell.zoomed_panels(cx);

    div()
        .h_flex()
        .items_center()
        .gap_2()
        .flex_none()
        .w_full()
        .h_6()
        .px_2()
        // A hairline, not a shadow and not a second fill: the bar is the same
        // surface as everything above it, held apart by one line.
        .border_t_1()
        .border_color(border)
        .bg(background)
        .text_xs()
        .text_color(muted)
        // A project with no root selected still gets a line rather than an empty
        // strip -- a bar that is sometimes blank reads as broken rather than as
        // empty.
        .child(match root {
            None => div().px_1().child("No project").into_any_element(),
            Some(root) => {
                let path = SharedString::from(root.path.display().to_string());
                let label = SharedString::from(root.label.clone());
                pressable("project", cx)
                    .child(Icon::new(IconName::Folder).size_3())
                    .child(div().max_w(MAX_PROJECT_W).truncate().child(label))
                    .on_click(
                        cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.copy_root_path(root_idx, window, cx);
                        }),
                    )
                    .tooltip(move |window, cx| {
                        let path = path.clone();
                        Tooltip::element(move |_, _| {
                            div()
                                .v_flex()
                                .gap_0p5()
                                .child(path.clone())
                                .child("Click to copy the path")
                        })
                        .build(window, cx)
                    })
                    .into_any_element()
            }
        })
        // Only for a root that is a repository. A non-repo project gets no cell
        // at all rather than a cell saying it has no branch, which is a fact
        // about git rather than about the project.
        .children(git.map(|git| {
            let label = SharedString::from(git.label());
            let changed = git.changed;
            pressable("git", cx)
                .child(Icon::new(IconName::Network).size_3())
                .child(div().max_w(MAX_GIT_W).truncate().child(label))
                .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                    shell.refresh_git(cx);
                }))
                .tooltip(move |window, cx| {
                    Tooltip::element(move |_, _| {
                        div()
                            .v_flex()
                            .gap_0p5()
                            .when(changed > 0, |col| {
                                col.child(format!(
                                    "{changed} changed {}",
                                    if changed == 1 { "file" } else { "files" }
                                ))
                            })
                            .child("Click to re-read Git status")
                    })
                    .build(window, cx)
                })
        }))
        // The agent, and the one state worth a mark. Drawn through the rail's
        // own mark so the four shapes are decided in one place: a spinner here
        // and a dot there for the same condition is a code with two spellings.
        .children(session.map(|session| {
            let agent = SharedString::from(session.title().to_string());
            let signal = shell.session_row(session.uid, cx).signal;
            cell("agent", cx)
                .children(signal.map(|signal| crate::rail::signal_mark(signal, cx)))
                .child(div().max_w(MAX_AGENT_W).truncate().child(agent))
        }))
        // Everything after this sits at the right end.
        .child(div().flex_1())
        // Unsaved work is a standing condition, not news, so it is the one thing
        // here drawn in a colour: a toast would fade and leave the user believing
        // the buffer is on disk.
        .when(facts.unsaved > 0, |bar| {
            let unsaved = facts.unsaved;
            bar.child(
                pressable("unsaved", cx)
                    .text_color(warning)
                    .child(Icon::new(IconName::Replace).size_3())
                    .child(format!(
                        "{unsaved} unsaved {}",
                        if unsaved == 1 { "file" } else { "files" }
                    ))
                    .on_click(
                        cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.show_workbench(WorkbenchMode::Editor, window, cx);
                        }),
                    )
                    .tooltip(|window, cx| {
                        Tooltip::new("Open the editor — Ctrl+S saves the active file")
                            .build(window, cx)
                    }),
            )
        })
        // The terminal is *not* here any more. It used to be the one route to a
        // closed bottom dock, and it now sits in the conversation's header
        // beside the Workbench button -- the two docks the conversation is
        // between, offered from the panel that took their space, rather than one
        // in each of two pieces of chrome. The live-shell dot went with it.
        //
        // One cell per panel that is *not* at 100%, which is normally none and
        // occasionally one. Read from the panels themselves rather than from
        // whichever holds focus: focus moves without telling the window, so a
        // focus-derived reading would sit here showing another panel's factor
        // with nothing to say it had gone stale.
        .children(zoomed.into_iter().map(|(panel, factor)| {
            let percent = (factor * 100.).round() as i32;
            let name = panel.label();
            pressable(("zoom", panel as usize), cx)
                .child(format!("{name} {percent}%"))
                .on_click(
                    cx.listener(move |shell: &mut Shell, _: &ClickEvent, _, cx| {
                        shell.reset_zoom(panel, cx);
                    }),
                )
                .tooltip(move |window, cx| {
                    Tooltip::new(format!("Click to put {name} back to 100%")).build(window, cx)
                })
        }))
}
