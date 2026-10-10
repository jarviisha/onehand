//! How a strip of tabs fits the width it is given.
//!
//! The rule a strip of tabs that must never scroll follows, the editor's files
//! among them: a tab is at most [`TAB_MAX_W`] and truncates, the tabs share
//! what room there is, and a strip that cannot give each tab [`TAB_MIN`]
//! becomes one control naming the current tab and opening the others, so no
//! tab is ever pushed out of reach.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ElementId, InteractiveElement as _, ParentElement as _, Rems, SharedString,
    StatefulInteractiveElement as _, Styled as _, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};
use std::cell::Cell;
use std::rc::Rc;

/// A tab never grows past this; a longer name truncates and says itself in
/// full on hover.
pub const TAB_MAX_W: Rems = rems(10.);
/// Below this a tab's name is too short to tell one from another, so the strip
/// gives up tabs for a select.
const TAB_MIN_W: Rems = rems(5.);

/// How a strip is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabStrip {
    /// Every tab side by side, each this wide at most.
    Tabs(Rems),
    /// One control naming the current tab and opening the others.
    Select,
}

/// Lay out `count` tabs, `gap` apart, in `avail`.
pub fn tab_strip(count: usize, avail: Rems, gap: Rems) -> TabStrip {
    if count == 0 {
        return TabStrip::Tabs(TAB_MAX_W);
    }
    let gaps = gap.0 * (count - 1) as f32;
    let each = ((avail.0 - gaps).max(0.) / count as f32).min(TAB_MAX_W.0);
    if each < TAB_MIN_W.0 {
        TabStrip::Select
    } else {
        TabStrip::Tabs(rems(each))
    }
}

/// A width a view measures of one of its boxes, as last laid out, and reads on
/// the next frame.
pub type Measured = Rc<Cell<Rems>>;

/// A width not measured yet: wider than anything, so the first frame draws as
/// though there were room.
pub fn unmeasured() -> Measured {
    Rc::new(Cell::new(rems(f32::INFINITY)))
}

/// Measure the width of the box this is put in into `into`, and redraw `view`
/// when it changes: the view reads it on the next frame. Put it in a
/// `relative` box that takes its width from its parent, never from what it
/// holds, or the measurement feeds itself.
pub fn measure_width<T: 'static>(
    into: Measured,
    view: gpui::WeakEntity<T>,
) -> impl gpui::IntoElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let width = rems(bounds.size.width / window.rem_size());
            if into.replace(width) != width {
                // Prepaint runs inside the view's own frame; notifying from it
                // directly would update an entity already being updated.
                cx.defer(move |cx| {
                    let _ = view.update(cx, |_, cx| cx.notify());
                });
            }
        },
        |_, _: (), _, _| {},
    )
    .absolute()
    .size_full()
}

/// The most tabs a strip's select lists before it says how many it left out.
const TAB_MENU_CAP: usize = 30;

/// Which of `count` tabs a select lists: a run of at most [`TAB_MENU_CAP`]
/// that always holds the `active` one, and how many it leaves out.
pub fn tab_menu_rows(count: usize, active: usize) -> (std::ops::Range<usize>, usize) {
    let shown = count.min(TAB_MENU_CAP);
    let start = active.saturating_sub(shown / 2).min(count - shown);
    (start..start + shown, count - shown)
}

/// The control a strip too narrow for its tabs becomes: a ghost button naming
/// the current tab and a caret, capped like a tab, saying `hint` in full on
/// hover. The caller sizes it like the tabs it stands in for and hangs the
/// menu of every tab on it.
pub fn tab_select(
    id: &'static str,
    label: gpui::SharedString,
    hint: gpui::SharedString,
    cx: &gpui::App,
) -> gpui_component::button::Button {
    crate::action(id)
        .ghost()
        .max_w(TAB_MAX_W)
        .tooltip(hint)
        .child(
            gpui::div()
                .h_flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .child(gpui::div().min_w_0().truncate().child(label))
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size_3()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground),
                ),
        )
}

/// One tab: a flat label, the chosen one on the selected fill and in full ink,
/// the rest muted and taking the row hover, truncating with its full `hint` on
/// hover. The caller caps its width, says what a press does, and adds what
/// follows the name: a mark, then [`tab_close`].
pub fn tab_chip(
    id: impl Into<ElementId>,
    label: SharedString,
    hint: SharedString,
    chosen: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    gpui::div()
        .id(id)
        .h_flex()
        .items_center()
        .flex_none()
        .h_6()
        .pl_2()
        .gap_1()
        .rounded(theme.radius)
        .cursor_pointer()
        .text_color(match chosen {
            true => theme.foreground,
            false => theme.muted_foreground,
        })
        .when(chosen, |tab| tab.bg(theme.accent))
        .when(!chosen, |tab| tab.hover(|tab| tab.bg(theme.list_hover)))
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
        // `min_w_0` lets the name shrink far enough to ellipsize at all.
        .child(gpui::div().min_w_0().truncate().child(label))
}

/// The cross closing a tab, always shown, its glyph muted. The caller's press
/// should stop the click there, or it also picks the tab it closes.
pub fn tab_close(id: impl Into<ElementId>, hint: &'static str, cx: &App) -> Button {
    crate::action(id)
        .ghost()
        .xsmall()
        .flex_none()
        .icon(Icon::new(IconName::Close).text_color(cx.theme().muted_foreground))
        .tooltip(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_select_lists_a_capped_run_around_the_current_tab() {
        assert_eq!(tab_menu_rows(3, 2), (0..3, 0));
        assert_eq!(tab_menu_rows(100, 0), (0..TAB_MENU_CAP, 70));
        assert_eq!(tab_menu_rows(100, 50), (35..65, 70));
        assert_eq!(tab_menu_rows(100, 99), (70..100, 70));
    }

    fn strip(count: usize, avail: f32, gap: f32) -> TabStrip {
        tab_strip(count, rems(avail), rems(gap))
    }

    #[test]
    fn a_roomy_strip_draws_each_tab_at_its_cap() {
        assert_eq!(strip(3, 60., 0.5), TabStrip::Tabs(TAB_MAX_W));
        assert_eq!(strip(0, 0., 0.5), TabStrip::Tabs(TAB_MAX_W));
    }

    #[test]
    fn tabs_share_a_tighter_strip_and_truncate() {
        assert_eq!(strip(4, 24., 0.), TabStrip::Tabs(rems(6.)));
        assert_eq!(strip(2, 10., 0.), TabStrip::Tabs(TAB_MIN_W));
        // The gaps between tabs come out of the room before it is shared.
        assert_eq!(strip(4, 25.5, 0.5), TabStrip::Tabs(rems(6.)));
    }

    #[test]
    fn a_strip_that_cannot_hold_its_tabs_becomes_a_select() {
        assert_eq!(strip(5, 24., 0.), TabStrip::Select);
        assert_eq!(strip(1, 4., 0.), TabStrip::Select);
        assert_eq!(strip(4, 20.5, 0.5), TabStrip::Select);
    }
}
