//! The rows the workspace page lists issues, sessions and conversations
//! with.

use gpui::{App, InteractiveElement, IntoElement, ParentElement, Styled, div};
use gpui_component::{ActiveTheme, StyledExt};

/// One issue as a pressable row: how it is named — the forge's number, or
/// *Draft* — in its slot in the middle ink, then the title, a few labels
/// tinted by kind, and a muted word at the end saying where it lives or whose
/// it is.
///
/// One builder for every list of issues, so an issue reads the same
/// wherever it is listed.
pub(crate) fn issue_row(
    id: impl Into<gpui::ElementId>,
    shown: String,
    title: String,
    labels: &[String],
    trailing: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let muted = cx.theme().muted_foreground;
    row_shell(id, shown, title, cx)
        // A few labels, not all: the row is for telling issues apart, and the
        // title is what does most of that.
        .children(
            labels
                .iter()
                .take(3)
                .map(|label| onehand_ui::label_badge(label.clone(), cx)),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(trailing),
        )
}

/// A row in the issue row's shape with something other than a number at its
/// head — a session's mark, a conversation's age — and no labels. What the
/// workspace page lists that is not an issue.
pub(crate) fn page_row(
    id: impl Into<gpui::ElementId>,
    lead: impl IntoElement,
    title: String,
    trailing: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let muted = cx.theme().muted_foreground;
    row_shell(id, lead, title, cx).child(
        div()
            .flex_none()
            .text_xs()
            .text_color(muted)
            .child(trailing),
    )
}

/// What both row shapes share: a list row's surface, its head in a slot of
/// its own in the middle ink, and its truncated title.
fn row_shell(
    id: impl Into<gpui::ElementId>,
    lead: impl IntoElement,
    title: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    onehand_ui::row(false, cx)
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_none()
                .w(crate::page_parts::ROW_LEAD_W)
                .whitespace_nowrap()
                .text_xs()
                .text_color(crate::theme::meta_ink(cx))
                .child(lead),
        )
        .child(div().flex_1().min_w_0().truncate().child(title))
}
