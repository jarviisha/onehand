//! How a strip of tabs fits the width it is given.
//!
//! The rule a strip of tabs that must never scroll follows, the editor's files
//! among them: a tab is at most [`TAB_MAX_W`] and truncates, the tabs share
//! what room there is, and a strip that cannot give each tab [`TAB_MIN`]
//! becomes one control naming the current tab and opening the others, so no
//! tab is ever pushed out of reach.

use gpui::{ParentElement as _, Styled as _};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme as _, Icon, IconName, StyledExt as _};

/// A tab never grows past this; a longer name truncates and says itself in
/// full on hover.
pub const TAB_MAX_W: gpui::Rems = gpui::rems(TAB_MAX);
const TAB_MAX: f32 = 10.;
/// Below this a tab's name is too short to tell one from another, so the strip
/// gives up tabs for a select.
const TAB_MIN: f32 = 5.;

/// How a strip is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabStrip {
    /// Every tab side by side, each this wide at most, in rems.
    Tabs(f32),
    /// One control naming the current tab and opening the others.
    Select,
}

/// Lay out `count` tabs, `gap` rems apart, in `avail` rems.
pub fn tab_strip(count: usize, avail: f32, gap: f32) -> TabStrip {
    if count == 0 {
        return TabStrip::Tabs(TAB_MAX);
    }
    let gaps = gap * (count - 1) as f32;
    let each = ((avail - gaps).max(0.) / count as f32).min(TAB_MAX);
    if each < TAB_MIN {
        TabStrip::Select
    } else {
        TabStrip::Tabs(each)
    }
}

/// Measure the width of the box this is put in, in rems, into `into`, and
/// redraw `view` when it changes: the strip reads it on the next frame. Put it
/// in a `relative` box that takes its width from its parent, never from what
/// it holds, or the measurement feeds itself.
pub fn measure_width<T: 'static>(
    into: std::rc::Rc<std::cell::Cell<f32>>,
    view: gpui::WeakEntity<T>,
) -> impl gpui::IntoElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let width = bounds.size.width / window.rem_size();
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

    #[test]
    fn a_roomy_strip_draws_each_tab_at_its_cap() {
        assert_eq!(tab_strip(3, 60., 0.5), TabStrip::Tabs(TAB_MAX));
        assert_eq!(tab_strip(0, 0., 0.5), TabStrip::Tabs(TAB_MAX));
    }

    #[test]
    fn tabs_share_a_tighter_strip_and_truncate() {
        assert_eq!(tab_strip(4, 24., 0.), TabStrip::Tabs(6.));
        assert_eq!(tab_strip(2, 10., 0.), TabStrip::Tabs(TAB_MIN));
        // The gaps between tabs come out of the room before it is shared.
        assert_eq!(tab_strip(4, 25.5, 0.5), TabStrip::Tabs(6.));
    }

    #[test]
    fn a_strip_that_cannot_hold_its_tabs_becomes_a_select() {
        assert_eq!(tab_strip(5, 24., 0.), TabStrip::Select);
        assert_eq!(tab_strip(1, 4., 0.), TabStrip::Select);
        assert_eq!(tab_strip(4, 20.5, 0.5), TabStrip::Select);
    }
}
