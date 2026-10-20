//! A workflow open on the page of its own: its head across the page, then
//! three columns, the workflow's own fields, its steps, and the step open,
//! each scrolling apart. As the page narrows the first two stack into one,
//! and then the open step stands alone in their place.

use super::form::{editor_head, message_bar, steps_column, workflow_column};
use super::{WorkflowDraft, WorkflowsPage, click};
use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement, Rems,
    SharedString, StatefulInteractiveElement as _, Styled, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_plugin_host::back_link;
use std::path::PathBuf;

/// The workflow column's width: room for a field's hint to run a line or
/// two, and a pair of fields stacked rather than squeezed.
const INFO_W: Rems = rems(28.);

/// The least the steps column takes: a step's name, its kind and its ⋯.
const STEPS_MIN: Rems = rems(22.);

/// The open step's column: room for its prompt to be read at a line's
/// length, and its kind, id and label on one line.
const STEP_W: Rems = rems(26.);

/// How the editor stands in the width it is given.
pub(super) enum EditorLayout {
    /// The workflow, its steps and the open step side by side.
    Three,
    /// The workflow over its steps in one column, the open step beside.
    Stacked,
    /// One column: the steps, or the open step under a link back to them.
    OneAtATime,
}

/// The layout a page `width` wide holds.
pub(super) fn editor_layout(width: Rems) -> EditorLayout {
    if width.0 >= INFO_W.0 + STEPS_MIN.0 + STEP_W.0 {
        EditorLayout::Three
    } else if width.0 >= STEPS_MIN.0 + STEP_W.0 {
        EditorLayout::Stacked
    } else {
        EditorLayout::OneAtATime
    }
}

/// A column of the editor: its own scroll, padded, its content at its top.
fn column(id: &'static str, child: impl IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h_full()
        .min_h_0()
        .overflow_y_scroll()
        .child(div().px_6().py_4().child(child))
}

/// The head across the page: the way back to the cards over the editor's
/// head, its name, what it is, *Run…* and the ⋯.
fn head_row(handle: &Entity<WorkflowsPage>, head: Option<AnyElement>, cx: &App) -> AnyElement {
    let back = back_link("workflows-back", "Workflows", cx)
        .on_click(click(handle, |page, window, cx| {
            page.close_workflow_form(window, cx)
        }));
    div()
        .flex_none()
        .v_flex()
        .gap_3()
        .px_8()
        .pt(crate::tokens::BAR_H)
        .pb_4()
        .child(div().h_flex().child(back))
        .children(head)
        .into_any_element()
}

/// The editor on `draft`, laid out as `layout` says.
pub(super) fn editor(
    handle: &Entity<WorkflowsPage>,
    draft: &WorkflowDraft,
    layout: EditorLayout,
    writing: bool,
    choices: &[(PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    let edge = cx.theme().border;
    let info = || workflow_column(handle, draft, cx);
    let steps = || steps_column(handle, draft, cx);
    let step = || super::step::step_panel(handle, draft, draft.place, cx);
    let step_column = || {
        column("workflow-step", step())
            .flex_none()
            .w(STEP_W)
            .border_l_1()
            .border_color(edge)
    };
    let body = match layout {
        EditorLayout::Three => div()
            .h_flex()
            .child(
                column("workflow-info", info())
                    .flex_none()
                    .w(INFO_W)
                    .border_r_1()
                    .border_color(edge),
            )
            .child(column("workflow-steps", steps()).flex_1().min_w_0())
            .child(step_column()),
        EditorLayout::Stacked => div()
            .h_flex()
            .child(
                column(
                    "workflow-steps",
                    div().v_flex().gap_6().child(info()).child(steps()),
                )
                .flex_1()
                .min_w_0(),
            )
            .child(step_column()),
        EditorLayout::OneAtATime => match draft.open_step {
            Some(_) => {
                let back = back_link("workflow-steps-back", "Steps", cx)
                    .on_click(click(handle, |page, window, cx| page.escape(window, cx)));
                div().v_flex().child(column(
                    "workflow-step",
                    div()
                        .v_flex()
                        .gap_4()
                        .child(div().h_flex().child(back))
                        .child(step()),
                ))
            }
            None => div().v_flex().child(column(
                "workflow-steps",
                div().v_flex().gap_6().child(info()).child(steps()),
            )),
        },
    };
    div()
        .size_full()
        .v_flex()
        .child(head_row(
            handle,
            Some(editor_head(handle, draft, writing, choices, cx)),
            cx,
        ))
        .child(
            body.flex_1()
                .min_h_0()
                .w_full()
                .border_t_1()
                .border_color(edge),
        )
        .children(message_bar(draft, cx))
        .into_any_element()
}

/// What stands in for the editor when the file picked cannot be read: the
/// way back, then why, in a column read as prose.
pub(super) fn unreadable(handle: &Entity<WorkflowsPage>, view: AnyElement, cx: &App) -> AnyElement {
    div()
        .size_full()
        .v_flex()
        .child(head_row(handle, None, cx))
        .child(
            div()
                .h_flex()
                .justify_center()
                .px_8()
                .child(div().w_full().max_w(crate::tokens::PAGE_PROSE).child(view)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
