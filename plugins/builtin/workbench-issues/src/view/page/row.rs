//! One row of the page's list.

use super::super::detail::priority::{priority, priority_ink, priority_name};
use super::{IssuesView, column};
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

/// A row's line: one line of the UI's text with room to aim the pointer, the
/// same for every row so the list reads as a table.
const ROW_H: Rems = rems(2.5);

/// The columns after the title, fixed so each lines up down the list: the
/// labels, the project and reference, the priority, and the end.
const LABELS_W: Rems = rems(7.);
const PROJECT_W: Rems = rems(9.);
const PRIORITY_W: Rems = rems(4.5);
const END_W: Rems = rems(10.);

/// The page's width, in rems, below which a row keeps only its title and its
/// end: the columns would leave the title too little to be read.
pub(super) const COLUMNS_MIN: f32 = 48.;

/// One row on one line: its state, the title, then in columns the labels,
/// the project (where the list mixes several) and reference, the priority,
/// and at its end how long ago it changed, or what its work needs.
pub(super) fn page_row(
    i: usize,
    last: bool,
    row: &Row<'_>,
    project: Option<SharedString>,
    picked: bool,
    columns: bool,
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
        .flex()
        .items_center()
        .justify_center()
        .role(gpui::accesskit::Role::Image)
        .aria_label(state)
        .tooltip(move |window, cx| Tooltip::new(state).build(window, cx))
        .child(icon.small().text_color(icon_ink));

    // Cut to the line; the whole of it on hover.
    let title = issue.title.clone();
    let title = div()
        .id(("issues-page-title", i))
        .flex_1()
        .min_w_0()
        .truncate()
        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
        .child(issue.title.clone());

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
        div().text_color(ink.warning).child(line)
    } else if running {
        div().text_color(theme.link).child("running")
    } else if row.earlier_attention {
        div()
            .text_color(ink.warning)
            .child("earlier task needs attention")
    } else {
        div().text_color(muted).child(onehand_core::age(
            onehand_core::issues::now(),
            issue.updated.max(issue.created),
        ))
    };
    let mono = theme.mono_font_family.clone();
    let radius = theme.radius_lg;

    let line = div()
        .h_flex()
        .items_center()
        .gap_3()
        .h(ROW_H)
        .child(mark)
        .child(title)
        .when(columns, |line| {
            line.child(
                div()
                    .h_flex()
                    .flex_none()
                    .w(LABELS_W)
                    .gap_1()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
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
            .child(
                div()
                    .h_flex()
                    .flex_none()
                    .w(PROJECT_W)
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .children(project.map(|project| div().min_w_0().truncate().child(project)))
                    .child(div().flex_none().font_family(mono).child(named)),
            )
            .child(
                div()
                    .flex_none()
                    .w(PRIORITY_W)
                    .truncate()
                    .text_xs()
                    .children(priority(&issue.body).map(|priority| {
                        div()
                            .text_color(priority_ink(&priority, cx))
                            .child(priority_name(&priority).to_string())
                    })),
            )
        })
        .child(end.flex_none().w(END_W).truncate().text_xs());

    column(div())
        .id(("issues-page-row", i))
        .relative()
        .v_flex()
        .px_3()
        // The rows stand in one hairline box: each its sides and foot, the
        // first its head, the ends rounded.
        .border_x_1()
        .border_b_1()
        .border_color(theme.border)
        .when(i == 0, |row| row.border_t_1().rounded_t(radius))
        .when(last, |row| row.rounded_b(radius))
        .cursor_pointer()
        .map(|div| {
            if picked {
                div.bg(theme.accent).text_color(theme.accent_foreground)
            } else {
                div.hover(|div| div.bg(theme.list_hover))
            }
        })
        // The picked row's edge: a line in the accent, so the row the open
        // issue belongs to is found at a glance down a long list.
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
                    .pt_1()
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
        .child(line)
        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.pick(root.clone(), number, window, cx)
        }))
        .into_any_element()
}
