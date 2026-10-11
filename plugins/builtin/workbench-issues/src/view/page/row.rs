//! One row of the page's list.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement, Rems,
    SharedString, StatefulInteractiveElement as _, Styled, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::task::Group;
use onehand_core::task::work::list::Row;
use onehand_plugin_host::{AppIcon, action, status_ink};

/// How many labels a row draws before it counts the rest.
const ROW_LABELS: usize = 3;

/// One line of a row's title. The title always takes two of them, a short
/// one included, so every row is the same height and the list reads as a
/// column rather than a ragged stack.
const TITLE_LEADING: Rems = rems(1.25);

/// The two lines a title is clamped to.
const TITLE_H: Rems = rems(2.5);

/// One row: its state, the title, then one line of facts — the project when
/// the list mixes several, the reference, the labels — ending on how long ago
/// it changed, or what its work needs.
pub(super) fn page_row(
    i: usize,
    row: &Row<'_>,
    project: Option<SharedString>,
    picked: bool,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let ink = status_ink(cx);
    let issue = row.item.issue;
    let (root, number) = (row.item.root.to_path_buf(), issue.number);
    let running = row
        .item
        .work
        .is_some_and(|work| work.work.group == Group::Running);

    let (icon, icon_ink, state) = match (running, issue.open) {
        (true, _) => (Icon::new(IconName::LoaderCircle), theme.link, "Running"),
        (false, true) => (Icon::new(AppIcon::CircleDot), ink.success, "Open"),
        (false, false) => (Icon::new(IconName::CircleCheck), muted, "Closed"),
    };
    let mark = div()
        .id(("issues-page-state", i))
        .flex_none()
        .w_4()
        .h(TITLE_LEADING)
        .flex()
        .items_center()
        .justify_center()
        .role(gpui::accesskit::Role::Image)
        .aria_label(state)
        .tooltip(move |window, cx| Tooltip::new(state).build(window, cx))
        .child(icon.small().text_color(icon_ink));

    let named = issue
        .reference()
        .map_or_else(|| "Draft".to_string(), str::to_string);
    let more = issue.labels.len().saturating_sub(ROW_LABELS);
    // The end of the line says the one thing most worth reading there: where
    // a row that stopped matching went, then what needs the person, then a
    // run going, and only then how long ago the issue changed.
    let end = if let Some(left) = row.left.clone() {
        div().text_color(muted).child(left)
    } else if let Some(line) = row.line.clone().filter(|_| row.attention) {
        div().truncate().text_color(ink.warning).child(line)
    } else if running {
        div().text_color(theme.link).child("running")
    } else if row.earlier_attention {
        div()
            .truncate()
            .text_color(ink.warning)
            .child("earlier task needs attention")
    } else {
        div().text_color(muted).child(onehand_core::age(
            onehand_core::issues::now(),
            issue.updated.max(issue.created),
        ))
    };
    let meta = div()
        .h_flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_xs()
        .text_color(muted)
        .children(project.map(|project| div().flex_none().child(project)))
        .child(div().flex_none().child(named))
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .overflow_hidden()
                .children(
                    issue
                        .labels
                        .iter()
                        .take(ROW_LABELS)
                        .map(|label| super::super::list::chip(label.clone(), cx)),
                )
                .when(more > 0, |labels| {
                    labels.child(div().flex_none().child(format!("+{more}")))
                }),
        )
        .child(end.flex_none().max_w_1_2());

    div()
        .id(("issues-page-row", i))
        .relative()
        .v_flex()
        .w_full()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .cursor_pointer()
        .map(|div| {
            if picked {
                div.bg(theme.accent).text_color(theme.accent_foreground)
            } else {
                div.hover(|div| div.bg(theme.list_hover))
            }
        })
        // The picked row's edge: a line in the accent, so the row the issue
        // beside it belongs to is found at a glance down a long list.
        .when(picked, |row| {
            row.child(
                gpui::div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w_0p5()
                    .bg(theme.link),
            )
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
            div().h_flex().items_start().gap_2().child(mark).child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .h(TITLE_H)
                            .text_sm()
                            .line_height(TITLE_LEADING)
                            .line_clamp(2)
                            .child(issue.title.clone()),
                    )
                    .child(meta),
            ),
        )
        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.pick(root.clone(), number, window, cx)
        }))
        .into_any_element()
}
