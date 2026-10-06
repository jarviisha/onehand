//! One row of the page's list.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::task::work::list::Row;
use onehand_plugin_host::{action, status_ink};

/// How many labels a row draws before it counts the rest.
const ROW_LABELS: usize = 3;

/// One row: the title, then the project and how the issue is named, its
/// line of work, and its labels.
pub(super) fn page_row(
    i: usize,
    row: &Row<'_>,
    project: SharedString,
    picked: bool,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let warning = status_ink(cx).warning;
    let issue = row.item.issue;
    let (root, number) = (row.item.root.to_path_buf(), issue.number);
    let named = issue
        .reference()
        .map_or_else(|| "Draft".to_string(), str::to_string);
    let shown = issue
        .labels
        .iter()
        .take(ROW_LABELS)
        .cloned()
        .collect::<Vec<_>>();
    let more = issue.labels.len().saturating_sub(ROW_LABELS);
    // A row that stopped matching says where it went in place of its line.
    let line = row.line.clone().filter(|_| row.left.is_none()).map(|line| {
        div()
            .truncate()
            .text_color(if row.attention { warning } else { muted })
            .child(line)
    });
    div()
        .id(("issues-page-row", i))
        .v_flex()
        .gap_0p5()
        .w_full()
        .px_2()
        .py_1()
        .rounded(theme.radius)
        .cursor_pointer()
        .map(|div| {
            if picked {
                div.bg(theme.accent).text_color(theme.accent_foreground)
            } else {
                div.hover(|div| div.bg(theme.list_hover))
            }
        })
        .when(row.outside, |column| {
            column.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child("Outside current filters"))
                    .child(
                        action(("issues-page-clear", i))
                            .xsmall()
                            .ghost()
                            .label("Clear filters")
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                // The row under it picks the issue; this only
                                // clears the filters.
                                cx.stop_propagation();
                                view.clear_filters(window, cx)
                            })),
                    ),
            )
        })
        .child(
            div()
                .text_sm()
                .font_semibold()
                .line_clamp(2)
                .when(!issue.open, |title| title.text_color(muted).line_through())
                .child(issue.title.clone()),
        )
        .child(
            div()
                .h_flex()
                .gap_1()
                .min_w_0()
                .text_xs()
                .text_color(muted)
                .child(div().flex_none().child(project))
                .child("·")
                .child(div().flex_none().child(named)),
        )
        .child(
            div()
                .h_flex()
                .gap_1()
                .min_w_0()
                .text_xs()
                .children(line)
                .when(row.earlier_attention, |line| {
                    line.child(
                        div()
                            .flex_none()
                            .text_color(warning)
                            .child("earlier task needs attention"),
                    )
                })
                .children(
                    row.left
                        .clone()
                        .map(|left| div().flex_none().text_color(muted).child(left)),
                ),
        )
        .when(!shown.is_empty(), |column| {
            column.child(
                div()
                    .h_flex()
                    .gap_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .children(
                        shown
                            .into_iter()
                            .map(|label| super::super::list::chip(label, cx)),
                    )
                    .when(more > 0, |labels| {
                        labels.child(div().text_color(muted).child(format!("+{more}")))
                    }),
            )
        })
        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.pick(root.clone(), number, window, cx)
        }))
        .into_any_element()
}
