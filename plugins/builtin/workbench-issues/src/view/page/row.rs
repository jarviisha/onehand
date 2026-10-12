//! One row of the page's list, and the names of its columns.

use super::super::detail::moment;
use super::super::detail::priority::{priority, priority_icon, priority_name};
use super::super::list::chip;
use super::{IssuesView, column};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement, Rems,
    SharedString, StatefulInteractiveElement as _, Styled, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::issues::LabelKind;
use onehand_core::task::Group;
use onehand_core::task::work::list::Row;
use onehand_plugin_host::{action, status_ink};

/// How many labels a row draws before it counts the rest.
const ROW_LABELS: usize = 2;

/// A row's inset: enough round one line of text to aim the pointer at, the
/// same for every row so the list reads as a table.
const ROW_PAD_X: Rems = rems(0.875);
const ROW_PAD_Y: Rems = rems(0.5625);

/// The columns after the title, fixed so each lines up down the list and
/// only the title gives way: two labels and a count, and an age.
const LABELS_W: Rems = rems(13.125);
const UPDATED_W: Rems = rems(2.75);

/// The widest what an issue's work needs gets beside its title, so a long
/// sentence leaves the title room to be read.
const NOTE_MAX: Rems = rems(14.);

/// The page's width, in rems, below which a row keeps only its title and its
/// age: the labels would leave the title too little to be read.
pub(super) const COLUMNS_MIN: f32 = 36.;

/// Where a row stands among the others, and how it is drawn there.
pub(super) struct Place {
    /// It opens the box: no column names stand over it.
    pub(super) first: bool,
    /// It closes the box.
    pub(super) last: bool,
    /// Its issue is the one picked.
    pub(super) picked: bool,
    /// The page holds the keyboard, so the picked row is where it is.
    pub(super) focused: bool,
    /// The labels' column is drawn.
    pub(super) columns: bool,
}

/// One row on one line: the reference, the title, the priority's mark and
/// what its work needs, then the labels and how long ago it changed.
pub(super) fn page_row(
    i: usize,
    place: Place,
    row: &Row<'_>,
    project: Option<SharedString>,
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

    // The reference leads the title in machine type, the project before it
    // while the list mixes several.
    let reference = issue
        .reference()
        .map_or_else(|| "Draft".to_string(), str::to_string);
    let reference = match project {
        Some(project) => format!("{project} {reference}"),
        None => reference,
    };
    let title = issue.title.clone();
    let title = div()
        .id(("issues-page-title", i))
        .flex_initial()
        .min_w_0()
        .truncate()
        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
        .child(issue.title.clone());
    let priority = priority(&issue.body).map(|said| {
        let name = priority_name(&said).to_string();
        let said_name = format!("Priority: {name}");
        div()
            .id(("issues-page-priority", i))
            .flex_none()
            .role(gpui::accesskit::Role::Image)
            .aria_label(said_name.clone())
            .tooltip(move |window, cx| Tooltip::new(said_name.clone()).build(window, cx))
            .child(Icon::new(priority_icon(&said)).xsmall().text_color(muted))
    });
    // What is most worth reading after the title: where a row that stopped
    // matching went, then what needs the person, then a run going.
    let note = if let Some(left) = row.left.clone() {
        Some(div().text_color(muted).child(left))
    } else if let Some(line) = row.line.clone().filter(|_| row.attention) {
        Some(div().text_color(ink.warning).child(line))
    } else if running {
        Some(div().text_color(theme.link).child("running"))
    } else if row.earlier_attention {
        Some(
            div()
                .text_color(ink.warning)
                .child("earlier task needs attention"),
        )
    } else {
        None
    };
    let changed = issue.changed();
    let updated = div()
        .id(("issues-page-updated", i))
        .flex_none()
        .w(UPDATED_W)
        .h_flex()
        .justify_end()
        .text_xs()
        .text_color(muted)
        .tooltip(move |window, cx| Tooltip::new(moment(changed)).build(window, cx))
        .child(onehand_core::age(onehand_core::issues::now(), changed));

    let more = issue.labels.len().saturating_sub(ROW_LABELS);
    let hidden = issue
        .labels
        .iter()
        .skip(ROW_LABELS)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    // A neutral label is drawn on the bubble, which the picked row's fill
    // swallows; there it takes the well instead.
    let well = theme.muted;
    let labels = div()
        .h_flex()
        .flex_none()
        .w(LABELS_W)
        .gap_1()
        .overflow_hidden()
        .children(issue.labels.iter().take(ROW_LABELS).map(|label| {
            let neutral = LabelKind::of(label) == LabelKind::Category;
            chip(label.clone(), cx)
                .flex_initial()
                .min_w_0()
                .truncate()
                .when(place.picked && neutral, |chip| chip.bg(well))
        }))
        .when(more > 0, |labels| {
            labels.child(
                div()
                    .id(("issues-page-more-labels", i))
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .tooltip(move |window, cx| Tooltip::new(hidden.clone()).build(window, cx))
                    .child(format!("+{more}")),
            )
        });

    let radius = theme.radius_lg;
    let line = div()
        .h_flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .font_family(theme.mono_font_family.clone())
                        .child(reference),
                )
                .child(title)
                .children(priority)
                .children(note.map(|note| note.flex_none().max_w(NOTE_MAX).truncate().text_xs())),
        )
        .when(place.columns, |line| line.child(labels))
        .child(updated);

    column(div())
        .id(("issues-page-row", i))
        .relative()
        .v_flex()
        .px(ROW_PAD_X)
        .py(ROW_PAD_Y)
        // The rows stand in one hairline box: each its sides and foot, the
        // first its head unless the columns' names stand over it, the ends
        // rounded.
        .border_x_1()
        .border_b_1()
        .border_color(theme.border)
        .when(place.first, |row| row.border_t_1().rounded_t(radius))
        .when(place.last, |row| row.rounded_b(radius))
        .cursor_pointer()
        .map(|div| {
            if place.picked {
                div.bg(theme.accent).text_color(theme.accent_foreground)
            } else {
                div.hover(|div| div.bg(theme.list_hover))
            }
        })
        // Where the keyboard is: the arrow keys move the picked row, so while
        // the page holds them it carries the ring a focused control does.
        .when(place.picked && place.focused, |row| {
            row.child(
                gpui::div()
                    .absolute()
                    .inset_0()
                    .border_1()
                    .border_color(theme.ring)
                    .when(place.first, |ring| ring.rounded_t(radius))
                    .when(place.last, |ring| ring.rounded_b(radius)),
            )
        })
        .when(row.outside, |column| {
            column.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .pb_1()
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

/// What each column of the rows holds, over them, in the same box and at the
/// same widths, so a column says what it is without a person guessing.
pub(super) fn columns_head(columns: bool, cx: &mut Context<IssuesView>) -> AnyElement {
    let theme = cx.theme();
    column(div())
        .h_flex()
        .items_center()
        .gap_3()
        .px(ROW_PAD_X)
        .py_1p5()
        .border_1()
        .border_color(theme.border)
        .rounded_t(theme.radius_lg)
        .bg(theme.muted)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(div().flex_1().min_w_0().child("Title"))
        .when(columns, |head| {
            head.child(div().flex_none().w(LABELS_W).child("Labels"))
        })
        .child(
            div()
                .flex_none()
                .w(UPDATED_W)
                .h_flex()
                .justify_end()
                .child("Updated"),
        )
        .into_any_element()
}
