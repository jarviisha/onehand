//! One column of the board: its key, its parts and the box they are drawn in.

use crate::page_parts::{page_card, ruled};
use gpui::{
    App, InteractiveElement, ParentElement, Rems, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::board::BOARD_SHOWN;

/// How wide each column stands: a task's title and its line of facts with
/// room for its `⋯`, so a board of four steps fits a wide pane and a longer
/// one scrolls sideways rather than squeezing every title.
pub(super) const COLUMN_W: Rems = rems(17.);

/// How far the scrollbar's frame reaches back through the card's own inset
/// (`pr_3`), so its thumb runs down the card's edge and never over a row.
const SCROLLBAR_PULL: Rems = rems(-0.75);

/// Which column a scroll handle is kept for.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) enum ColumnKey {
    Backlog,
    /// A column of the board, by its id.
    Board(String),
    Done,
}

impl ColumnKey {
    /// The id of the element scrolling this column's rows.
    fn scroll_id(&self) -> SharedString {
        let key = match self {
            Self::Backlog => "backlog".to_string(),
            Self::Board(id) => id.clone(),
            Self::Done => "Done".to_string(),
        };
        format!("board-column-{key}").into()
    }

    /// The id of the scrollbar beside [`Self::scroll_id`]'s rows.
    fn bar_id(&self) -> SharedString {
        format!("{}-bar", self.scroll_id()).into()
    }
}

/// What a column is drawn from.
pub(super) struct ColumnParts {
    pub(super) title: SharedString,
    pub(super) count: usize,
    pub(super) rows: Vec<gpui::AnyElement>,
    /// Said where there is nothing to list.
    pub(super) note: Option<String>,
    /// At the head's end.
    pub(super) control: Option<gpui::AnyElement>,
    pub(super) key: ColumnKey,
    /// Where the rows stand scrolled.
    pub(super) scroll: ScrollHandle,
}

/// A column: a box with its name and count, `control` at its head's end,
/// `rows` capped at [`BOARD_SHOWN`] (said under them when it bites), and
/// `note` where there is nothing to list. As tall as its rows, up to the
/// board's height; past it the rows scroll under `scroll`, the head and the
/// cap's line kept in place.
pub(super) fn column(parts: ColumnParts, cx: &App) -> gpui::Div {
    let ColumnParts {
        title,
        count,
        rows,
        note,
        control,
        key,
        scroll,
    } = parts;
    let muted = cx.theme().muted_foreground;
    let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
    let hidden = count.saturating_sub(BOARD_SHOWN);
    // A column with rows to scroll keeps the wheel; one that fits hands it
    // on to the board, which moves sideways. Its own axis only, so a
    // sideways swipe over it moves the board too.
    let wheel_scroll = scroll.clone();
    let list = div()
        .id(key.scroll_id())
        .min_h_0()
        .overflow_y_scroll()
        .restrict_scroll_to_axis()
        .track_scroll(&scroll)
        // The card's own inset, given back to the rows: the frame below
        // reaches through it, so the thumb runs down the card's edge in that
        // lane and never over a row.
        .pr_3()
        .on_scroll_wheel(move |event, window, cx| {
            let delta = event.delta.pixel_delta(window.line_height());
            if wheel_scroll.max_offset().y > gpui::px(0.) && delta.y != gpui::px(0.) {
                cx.stop_propagation();
            }
        })
        .child(ruled(rows, cx));
    page_card(title, Some(count), control, cx)
        .debug_selector(|| format!("{}-box", key.scroll_id()))
        .flex_none()
        .w(COLUMN_W)
        .max_h_full()
        .min_h_0()
        .children(note.map(muted_line))
        .child(
            div()
                .relative()
                .v_flex()
                .min_h_0()
                .mr(SCROLLBAR_PULL)
                .child(list)
                .child(onehand_ui::scrollbar(&scroll).id(key.bar_id())),
        )
        .children((hidden > 0).then(|| muted_line(format!("{hidden} more not shown"))))
}

/// What a board column's head says: its name, and how full it is when it
/// holds a limit, and that its workflow is gone when it is.
pub(crate) fn column_head(column: &onehand_core::board::Column, gone: bool) -> String {
    let mut head = match column.limit {
        Some(limit) => format!("{} · {} of {limit}", column.name, column.entries.len()),
        None => column.name.clone(),
    };
    if gone {
        head.push_str(" · its workflow is gone");
    }
    head
}
