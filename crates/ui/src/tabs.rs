//! Tabs and the bars that hold them: a dock's bar, a tab inside content, the
//! select that names one of several and opens the rest, and the cross that
//! closes one.

use crate::Named as _;
use crate::Quiet as _;
use crate::Short as _;
use crate::ShortIcon as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ElementId, InteractiveElement as _, ParentElement as _, Rems, SharedString,
    StatefulInteractiveElement as _, Styled as _, rems,
};
use gpui_component::button::Button;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Icon, IconName, StyledExt as _};
use std::cell::Cell;
use std::rc::Rc;

/// A tab never grows past this; a longer name truncates and says itself in
/// full on hover.
pub const TAB_MAX_W: Rems = rems(10.);
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

/// The most entries a tab select's menu lists before it says how many more
/// there are.
pub const TAB_MENU_CAP: usize = 30;

/// A strip's select of one of several (the Workbench's modes, when narrow; the
/// Editor's open file): a ghost button naming the current tab and the picker's caret, at a
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
            true => crate::chosen_ink(cx),
            false => theme.muted_foreground,
        })
        .when(chosen, |tab| tab.bg(crate::chosen_fill(cx)))
        .when(!chosen, |tab| tab.hover(|tab| tab.bg(theme.list_hover)))
        .tooltip(crate::tip(move |window, cx| {
            Tooltip::new(hint.clone()).build(window, cx)
        }))
        // `min_w_0` lets the name shrink far enough to ellipsize at all.
        .child(gpui::div().min_w_0().truncate().child(label))
}

/// A dock's bar of tabs: a strip with no fill, a line under it parting it from
/// what it heads, on the box it heads, its tabs ([`panel_tab_frame`]) parted from each other by
/// space, its controls at its end. The caller adds the tabs, then whatever
/// ends the bar.
pub fn tab_bar(cx: &App) -> gpui::Div {
    crate::strip(cx)
        .px_1()
        .gap_1()
        .border_b_1()
        .border_color(cx.theme().border)
}

/// One tab of a [`tab_bar`], for a tab whose press is a control of its own (a
/// button the keyboard reaches): a small control's height, the chosen one a
/// rounded fill (`secondary`) with its name in the content ink, the rest in
/// the bar's muted ink and inked under the pointer. The caller insets what it
/// holds and caps its width.
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
        .h(crate::tokens::CONTROL_H)
        .gap_1()
        .rounded(theme.radius)
        .text_size(crate::tokens::CHROME_TEXT)
        .map(|tab| match chosen {
            true => tab.bg(theme.secondary).text_color(crate::chosen_ink(cx)),
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
        .short_icon()
        .flex_none()
        .icon(Icon::new(IconName::Close))
        .named(hint)
}
