//! One issue of the page's list, as a row on one line.

use super::super::detail::moment;
use super::super::detail::priority::{priority, priority_icon, priority_name};
use super::super::list::chip;
use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement, Rems,
    SharedString, StatefulInteractiveElement as _, Styled, div, rems,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::LabelKind;
use onehand_core::task::Group;
use onehand_core::task::work::list::Row;
use onehand_ui::Quiet as _;
use onehand_ui::Short as _;
use onehand_ui::{AppIcon, action, meta_ink, middle_ink, row as row_surface, status_ink};

/// How many labels a row draws before it counts the rest.
const ROW_LABELS: usize = 3;

/// The reference's slot: room for a five-digit number or *Draft*, so every
/// title starts on one edge. A longer reference widens its own row only.
const REFERENCE_W: Rems = rems(3.);

/// The project's slot at the row's end, so the labels before it and the age
/// after it line up down the list whatever the name.
const PROJECT_W: Rems = rems(9.);

/// The age's slot, room for *11mo*, so every age ends on the row's edge.
const UPDATED_W: Rems = rems(2.5);

/// A row: a step over a list's row, holding a mark and badges on one line
/// with room above and below them.
const ISSUE_ROW_H: Rems = rems(2.5);

/// The widest what an issue's work needs gets beside its title, so a long
/// sentence leaves the title room to be read.
const NOTE_MAX: Rems = rems(14.);

/// The labels' column: as wide on every row, so the head names it, and no
/// wider, so three long ones leave the title room; what runs over is cut.
const LABELS_W: Rems = rems(12.);

/// The page's width, in rems, below which a row keeps only its title and its
/// age: the labels and the project would leave the title too little to be
/// read.
pub(super) const COLUMNS_MIN: f32 = 46.;

/// Where a row stands among the others, and how it is drawn there.
pub(super) struct Place {
    /// Its issue is the one picked: the arrow keys move it, and its fill
    /// alone says where they are, with no ring over it.
    pub(super) picked: bool,
    /// The page is wide enough for the labels and the project.
    pub(super) columns: bool,
}

/// One issue on one line: whether it is open, the reference, the title, the
/// priority's mark and what its work needs; at the end its labels, its
/// project and how long ago it changed.
pub(super) fn issue_row(
    i: usize,
    place: Place,
    row: &Row<'_>,
    project: SharedString,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let theme = cx.theme();
    // Every quieter ink on the picked row is taken under the selected fill's
    // own ink, so it stays a step quieter there rather than sinking into it.
    let muted = match place.picked {
        true => middle_ink(theme.accent_foreground, theme.muted_foreground),
        false => theme.muted_foreground,
    };
    let ink = status_ink(cx);
    let issue = row.item.issue;
    let (root, number) = (row.item.root.to_path_buf(), issue.number);
    let running = row
        .item
        .work
        .is_some_and(|work| work.work.group == Group::Running);

    // The reference leads the title in machine type and the middle ink:
    // context for the title, not a name of its own. On the picked row the step
    // is taken under the selected fill's ink, so it stays a step there.
    let reference = issue
        .reference()
        .map_or_else(|| "Draft".to_string(), str::to_string);
    let reference_ink = match place.picked {
        true => muted,
        false => meta_ink(cx),
    };
    let project = div()
        .flex_none()
        .w(PROJECT_W)
        .h_flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .child(Icon::new(IconName::Folder).xsmall())
        .child(div().min_w_0().truncate().child(project));
    // Whether it is open, in the hue of its state, the one coloured mark on
    // the line: open in the success ink, closed in the closed ink.
    let (glyph, hue) = match issue.open {
        true => (Icon::from(AppIcon::CircleDot), ink.success),
        false => (Icon::new(IconName::CircleCheck), ink.closed),
    };
    let state = Icon::new(glyph).small().text_color(hue);
    let title = issue.title.clone();
    // The title is what the row is, so it takes weight, as the age does; the
    // reference, the note and the project are context, in the regular weight
    // and the quieter inks.
    let title = div()
        .id(("issues-page-title", i))
        .flex_initial()
        .min_w_0()
        .truncate()
        .font_medium()
        .tooltip(onehand_ui::tip(move |window, cx| {
            Tooltip::new(title.clone()).build(window, cx)
        }))
        .child(issue.title.clone());
    let priority = priority(&issue.body).map(|said| {
        let name = priority_name(&said).to_string();
        let said_name = format!("Priority: {name}");
        div()
            .id(("issues-page-priority", i))
            .flex_none()
            .role(gpui::accesskit::Role::Image)
            .aria_label(said_name.clone())
            .tooltip(onehand_ui::tip(move |window, cx| {
                Tooltip::new(said_name.clone()).build(window, cx)
            }))
            .child(Icon::new(priority_icon(&said)).xsmall().text_color(muted))
    });
    // What is most worth reading after the title: where a row that stopped
    // matching went, then what needs the person, then a run going.
    let note = if let Some(left) = row.left.clone() {
        Some(div().text_color(muted).child(left))
    } else if let Some(line) = row.line.clone().filter(|_| row.attention) {
        Some(div().text_color(ink.warning).child(line))
    } else if running {
        Some(div().text_color(theme.info).child("running"))
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
    let age_ink = match place.picked {
        true => theme.accent_foreground,
        false => meta_ink(cx),
    };
    let updated = div()
        .id(("issues-page-updated", i))
        .flex_none()
        .w(UPDATED_W)
        .h_flex()
        .justify_end()
        // Weighted as the title is, and a step over the muted ink: the middle
        // one, or on the picked row that fill's own ink. How fresh an issue
        // is is read down the column as the titles are.
        .font_medium()
        .text_xs()
        .text_color(age_ink)
        .tooltip(onehand_ui::tip(move |window, cx| {
            Tooltip::new(moment(changed)).build(window, cx)
        }))
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
        .overflow_hidden()
        .gap_1()
        .children(issue.labels.iter().take(ROW_LABELS).map(|label| {
            let neutral = LabelKind::of(label) == LabelKind::Category;
            chip(label.clone(), cx)
                .flex_initial()
                .min_w_0()
                .truncate()
                // Its edge stays: it is drawn from the label's ink, which the
                // well does not change.
                .when(place.picked && neutral, |chip| chip.bg(well))
        }))
        .when(more > 0, |labels| {
            labels.child(
                div()
                    .id(("issues-page-more-labels", i))
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .tooltip(onehand_ui::tip(move |window, cx| {
                        Tooltip::new(hidden.clone()).build(window, cx)
                    }))
                    .child(format!("+{more}")),
            )
        });

    // The whole row at the meta size, the reference and the title with it:
    // many issues read down the list at a glance, and the title stands out by
    // its ink, not its size.
    let line = div()
        .h(ISSUE_ROW_H)
        .text_xs()
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
                .child(div().flex_none().child(state))
                .child(
                    div()
                        .flex_none()
                        .min_w(REFERENCE_W)
                        .text_color(reference_ink)
                        .font_family(theme.mono_font_family.clone())
                        .child(reference),
                )
                .child(title)
                .children(priority)
                .children(note.map(|note| note.flex_none().max_w(NOTE_MAX).truncate().text_xs())),
        )
        .when(place.columns, |line| line.child(labels).child(project))
        .child(updated);

    row_surface(place.picked, cx)
        .id(("issues-page-row", i))
        .v_flex()
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
                            .short()
                            .quiet(cx)
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

/// The table's head over the rows: each column's name in the meta ink, in
/// the slots the rows draw theirs in, so every name stands over its column.
pub(super) fn head_row(columns: bool, cx: &mut Context<IssuesView>) -> AnyElement {
    let name = |text: &'static str| div().flex_none().child(text);
    div()
        // The row's own inset, so the names start where the rows' content does.
        .px_2()
        .h(ISSUE_ROW_H)
        .text_xs()
        .text_color(meta_ink(cx))
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
                // The state mark's slot, held empty.
                .child(
                    div()
                        .flex_none()
                        .invisible()
                        .child(Icon::new(IconName::CircleCheck).small()),
                )
                .child(div().flex_none().min_w(REFERENCE_W).child("Issue"))
                .child(name("Title")),
        )
        .when(columns, |head| {
            head.child(name("Labels").w(LABELS_W))
                .child(name("Project").w(PROJECT_W))
        })
        .child(name("Updated").w(UPDATED_W).h_flex().justify_end())
        .into_any_element()
}
