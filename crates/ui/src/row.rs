//! A list row's surface, a tree's row, and the slot a row's mark stands in.

use crate::tokens;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// One mark's slot: an icon, a state's mark or a dot, centred in a column one
/// icon wide. Every mark down a column sits in one, so the marks share a
/// centre line whatever each is drawn at, and the labels after them start on
/// one edge.
///
/// A control may stand in one too, wider than the slot: it stays centred on
/// the column's line, its hit area spilling past the slot on both sides.
pub fn mark_slot(mark: impl IntoElement) -> Div {
    div()
        .w_4()
        .flex_none()
        .h_flex()
        .items_center()
        .justify_center()
        .child(mark)
}

/// A tree's row, as Files and Markdown draw theirs: a list row at
/// [`tokens::TREE_ROW_H`] a pitch, its fill [`tokens::ROW_GUTTER`] off both of
/// the dock's side edges and [`tokens::TREE_ROW_ABOVE`] and
/// [`tokens::TREE_ROW_BELOW`] off the rows around it, so a hovered fill never
/// touches the picked one beside it; its content inset [`tokens::ROW_INSET`] and stepped in
/// [`tokens::TREE_INDENT`] per level, the picked row on the selected fill.
/// It stretches itself across the tree's column, less the gutter, rather than
/// taking a width: a full width and a margin would overrun the column.
/// The caller adds its mark in a [`mark_slot`], its name, and its marks in
/// one trailing slot.
pub fn tree_row(
    id: impl Into<ElementId>,
    depth: usize,
    picked: bool,
    cx: &App,
) -> gpui::Stateful<Div> {
    row(picked, cx)
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .self_stretch()
        .mx(tokens::ROW_GUTTER)
        .mt(tokens::TREE_ROW_ABOVE)
        .mb(tokens::TREE_ROW_BELOW)
        .min_h(tokens::TREE_ROW_FILL)
        .h(tokens::TREE_ROW_FILL)
        .text_size(tokens::CHROME_TEXT)
        .pl(gpui::rems(
            tokens::ROW_INSET.0 - tokens::ROW_GUTTER.0 + depth as f32 * tokens::TREE_INDENT.0,
        ))
        .pr(gpui::rems(tokens::ROW_INSET.0 - tokens::ROW_GUTTER.0))
}

/// A list row's surface: at least [`tokens::ROW_H`] tall, inset `px_2`, at
/// the theme's corner, answering the pointer. The hover fills it with
/// `list_hover` and leaves its inks; the picked row stands on `accent` in
/// that fill's paired ink, a quieter ink on it taken through [`middle_ink`](crate::middle_ink)
/// under that pair. The caller lays it out, a line or a line over a second,
/// and adds the ring where the keyboard is.
pub fn row(picked: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let hover = theme.list_hover;
    div()
        .min_h(tokens::ROW_H)
        .px_2()
        .rounded(theme.radius)
        .cursor_pointer()
        .map(|row| match picked {
            true => row.bg(theme.accent).text_color(theme.accent_foreground),
            false => row.hover(move |row| row.bg(hover)),
        })
}
