//! How a strip of tabs fits the width it is given.
//!
//! The rule a strip of tabs that must never scroll follows, the editor's files
//! among them: a tab is as wide as its name, at most [`TAB_MAX_W`], and is
//! never squeezed to make room. Where the tabs at their own widths do not all
//! fit, the newest that do stay side by side, the current one always among
//! them, and the older ones go into one menu after them, so no tab is ever
//! pushed out of reach.

use crate::Named as _;
use crate::Quiet as _;
use crate::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ElementId, InteractiveElement as _, ParentElement as _, Rems, SharedString,
    StatefulInteractiveElement as _, Styled as _, rems,
};
use gpui_component::button::Button;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};
use std::cell::Cell;
use std::rc::Rc;

/// A tab never grows past this; a longer name truncates and says itself in
/// full on hover.
pub const TAB_MAX_W: Rems = rems(10.);
/// The room kept after the tabs for the menu of the ones left out: its count
/// and its caret.
const TAB_MORE_W: Rems = rems(4.5);

/// Which tabs a strip draws side by side, in their order; the rest go into
/// the menu after them.
#[derive(Clone, Debug, PartialEq)]
pub struct TabStrip {
    pub shown: Vec<usize>,
    pub left_out: Vec<usize>,
}

/// Lay out tabs `widths` wide, `gap` apart, in `avail`, `active` the current
/// one: every tab when all of them fit, otherwise the current one and then the
/// newest, as many as fit beside the menu, and the older ones left out.
pub fn tab_strip(widths: &[Rems], active: usize, avail: Rems, gap: Rems) -> TabStrip {
    let count = widths.len();
    let all = (0..count).collect::<Vec<_>>();
    let total: f32 =
        widths.iter().map(|w| w.0).sum::<f32>() + gap.0 * count.saturating_sub(1) as f32;
    if total <= avail.0 {
        return TabStrip {
            shown: all,
            left_out: Vec::new(),
        };
    }
    // The current file first, whatever room there is: it is never only in a
    // menu.
    let active = active.min(count - 1);
    let mut shown = vec![active];
    let mut used = widths[active].0 + gap.0 + TAB_MORE_W.0;
    for i in (0..count).rev().filter(|&i| i != active) {
        if used + widths[i].0 + gap.0 > avail.0 {
            break;
        }
        used += widths[i].0 + gap.0;
        shown.push(i);
    }
    shown.sort_unstable();
    let left_out = all.into_iter().filter(|i| !shown.contains(i)).collect();
    TabStrip { shown, left_out }
}

/// Each tab's width as last laid out, by what it shows, so a tab not on the
/// strip now is still known by the width it had there. One not drawn yet
/// counts at [`TAB_MAX_W`].
pub type TabWidths = Rc<std::cell::RefCell<std::collections::HashMap<SharedString, Rems>>>;

/// The width of the tab known as `key`, or [`TAB_MAX_W`] when it has not been
/// drawn yet.
pub fn tab_width(widths: &TabWidths, key: &SharedString) -> Rems {
    widths.borrow().get(key).copied().unwrap_or(TAB_MAX_W)
}

/// Measure the tab this is put in into `widths` under `key`, and redraw `view`
/// when it changes. Put it in the tab, made `relative`.
pub fn measure_tab<T: 'static>(
    widths: TabWidths,
    key: SharedString,
    view: gpui::WeakEntity<T>,
) -> impl gpui::IntoElement {
    gpui::canvas(
        move |bounds, window, cx| {
            let width = rems(bounds.size.width / window.rem_size());
            if widths.borrow_mut().insert(key.clone(), width) != Some(width) {
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

/// The most tabs the menu of those left out lists before it says how many
/// more there are.
pub const TAB_MENU_CAP: usize = 30;

/// A strip's select of tabs too many for it (the Workbench's modes, when
/// narrow): a ghost button naming the current tab and the picker's caret, at a
/// strip's size and capped like a tab, saying `hint` in full on hover. The
/// caller hangs the menu of every tab on it.
pub fn tab_select(
    id: &'static str,
    label: gpui::SharedString,
    hint: gpui::SharedString,
    cx: &gpui::App,
) -> gpui_component::button::Button {
    crate::action(id)
        .quiet(cx)
        .short()
        .max_w(TAB_MAX_W)
        .tooltip(hint)
        .dropdown_caret(true)
        .child(gpui::div().min_w_0().truncate().child(label))
}

/// The control after the tabs that opens the ones left out: a ghost button
/// counting them, with the picker's caret, at a strip's size. The caller
/// hangs the menu on it.
pub fn tab_more(
    id: &'static str,
    left_out: usize,
    cx: &gpui::App,
) -> gpui_component::button::Button {
    crate::action(id)
        .quiet(cx)
        .short()
        .flex_none()
        .tooltip("The other open files")
        .dropdown_caret(true)
        .child(format!("{left_out} more"))
}

/// One tab: a flat label, the chosen one on the selected fill and in its ink,
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
        // Chrome, and said so here: neither strip that holds a tab sets a
        // size, so a tab took the panel's reading size and stood a step above
        // the buttons beside it.
        .text_size(crate::tokens::CHROME_TEXT)
        .rounded(theme.radius)
        .cursor_pointer()
        .text_color(match chosen {
            true => theme.accent_foreground,
            false => theme.muted_foreground,
        })
        .when(chosen, |tab| tab.bg(theme.accent))
        .when(!chosen, |tab| tab.hover(|tab| tab.bg(theme.list_hover)))
        .tooltip(crate::tip(move |window, cx| {
            Tooltip::new(hint.clone()).build(window, cx)
        }))
        // `min_w_0` lets the name shrink far enough to ellipsize at all.
        .child(gpui::div().min_w_0().truncate().child(label))
}

/// A dock's bar of tabs, as the library's tab panel draws one: a strip on
/// the tab bar's fill with a line under it, its tabs ([`panel_tab`]) flush
/// with its start and each other, its controls inset at its end. The caller
/// adds the tabs, then whatever ends the bar.
pub fn tab_bar(cx: &App) -> gpui::Div {
    crate::strip(cx)
        .px_0()
        .pr_1()
        .gap_0()
        .items_stretch()
        .bg(cx.theme().tab_bar)
        .border_b_1()
        .border_color(cx.theme().border)
}

/// One tab of a [`tab_bar`]: as tall as the bar, parted from the next by a
/// line, the chosen one on the tab's own fill in its ink with a stroke of the
/// primary along its top, the rest in the bar's muted ink and inked under
/// the pointer, truncating with its full `hint` on hover. The caller caps its
/// width, says what a press does, and adds what follows the name: a mark,
/// then [`tab_close`].
pub fn panel_tab(
    id: impl Into<ElementId>,
    label: SharedString,
    hint: SharedString,
    chosen: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    panel_tab_frame(id, chosen, cx)
        .pl_3()
        .pr_2()
        .cursor_pointer()
        .tooltip(crate::tip(move |window, cx| {
            Tooltip::new(hint.clone()).build(window, cx)
        }))
        .child(gpui::div().min_w_0().truncate().child(label))
}

/// A [`panel_tab`]'s frame alone, for a tab whose press is a control of its
/// own (a button the keyboard reaches): the bar's height, the line after it,
/// the chosen fill and stroke and the inks. The caller insets what it holds.
pub fn panel_tab_frame(
    id: impl Into<ElementId>,
    chosen: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    let lit = theme.tab_active_foreground;
    gpui::div()
        .id(id)
        .relative()
        .h_flex()
        .items_center()
        .flex_none()
        .h_full()
        .gap_1()
        .border_r_1()
        .border_color(theme.border)
        .text_size(crate::tokens::CHROME_TEXT)
        .map(|tab| match chosen {
            true => tab.bg(theme.tab_active).text_color(lit).child(
                gpui::div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(crate::tokens::TAB_MARK)
                    .bg(theme.primary),
            ),
            false => tab
                .text_color(theme.tab_foreground)
                .hover(move |tab| tab.text_color(lit)),
        })
}

/// The cross closing a tab, always shown, its glyph muted. The caller's press
/// should stop the click there, or it also picks the tab it closes.
pub fn tab_close(id: impl Into<ElementId>, hint: &'static str, cx: &App) -> Button {
    crate::action(id)
        .quiet_icon(false, cx)
        .small()
        .flex_none()
        .icon(Icon::new(IconName::Close))
        .named(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(widths: &[f32], active: usize, avail: f32) -> TabStrip {
        let widths: Vec<Rems> = widths.iter().map(|w| rems(*w)).collect();
        tab_strip(&widths, active, rems(avail), rems(0.5))
    }

    #[test]
    fn a_roomy_strip_draws_every_tab() {
        let s = strip(&[5., 6., 10.], 0, 60.);
        assert_eq!(s.shown, vec![0, 1, 2]);
        assert!(s.left_out.is_empty());
        assert!(strip(&[], 0, 0.).shown.is_empty());
        // Exactly the three widths and two gaps.
        assert_eq!(strip(&[5., 6., 10.], 0, 22.).shown.len(), 3);
    }

    #[test]
    fn tabs_count_at_their_own_width_not_the_cap() {
        // Six short tabs fit where six capped ones would not.
        assert_eq!(strip(&[4.; 6], 5, 26.5).shown.len(), 6);
    }

    #[test]
    fn a_tight_strip_keeps_the_newest_and_lists_the_older() {
        // Room for the current tab, one more and the menu: 5 + 0.5 + 5 + 0.5 + 4.5.
        let s = strip(&[5.; 5], 4, 15.5);
        assert_eq!(s.shown, vec![3, 4]);
        assert_eq!(s.left_out, vec![0, 1, 2]);
    }

    #[test]
    fn the_current_tab_is_never_only_in_the_menu() {
        let s = strip(&[5.; 5], 0, 15.5);
        assert_eq!(s.shown, vec![0, 4]);
        assert_eq!(s.left_out, vec![1, 2, 3]);
        // Too narrow for any tab beside the menu: the current one still shows.
        assert_eq!(strip(&[5.; 5], 2, 3.).shown, vec![2]);
    }
}
