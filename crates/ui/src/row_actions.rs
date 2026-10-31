//! A row's actions, shown only while they are wanted, in a slot that holds
//! its width so nothing beside it moves when they appear; and the ⋯ menu and
//! right-click menu that offer them, one list for both.

use crate::Named as _;
use crate::Quiet as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled as _, Window, div,
};
use gpui_component::menu::PopupMenu;
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme as _, IconName, Sizable as _};
use std::rc::Rc;

/// What a press on a row's act runs.
pub type Press = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One thing a row offers from its ⋯ and from a right-click on it.
#[derive(Clone)]
pub struct RowAct {
    label: SharedString,
    danger: bool,
    press: Press,
}

impl RowAct {
    pub fn new(label: impl Into<SharedString>, press: Press) -> Self {
        Self {
            label: label.into(),
            danger: false,
            press,
        }
    }

    /// A removal, in the danger ink and after a separator.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// `acts` as rows of `menu`, a removal parted from the rest by a separator.
pub fn row_menu_items(mut menu: PopupMenu, acts: &[RowAct], cx: &App) -> PopupMenu {
    let danger = cx.theme().danger;
    for (i, act) in acts.iter().enumerate() {
        let press = act.press.clone();
        let label = act.label.clone();
        if act.danger && i > 0 {
            menu = menu.separator();
        }
        let item = match act.danger {
            true => crate::menu_row(move |_, _| div().text_color(danger).child(label.clone())),
            false => crate::menu_item(label),
        };
        menu = menu.item(item.on_click(move |event, window, cx| press(event, window, cx)));
    }
    menu
}

/// A row's ⋯: a quiet ellipsis at a row control's size, named `name`,
/// opening `acts` below it; `key` is unique to the row. The same acts go on
/// the row's right-click ([`row_menu_items`]), so the two never offer
/// different things.
pub fn row_more(
    key: impl Into<SharedString>,
    name: &'static str,
    acts: Rc<[RowAct]>,
    cx: &App,
) -> Popover {
    let key: SharedString = key.into();
    crate::menu_below(
        ElementId::Name(format!("{key}-menu").into()),
        crate::action(ElementId::Name(key))
            .quiet_icon(false, cx)
            .small()
            .icon(IconName::Ellipsis)
            .named(name),
        move |menu, _, cx| row_menu_items(menu, &acts, cx),
    )
}

/// A row's actions, in a trailing slot that keeps its width: hidden until
/// the pointer is on the row named `group`, unless `shown` (the selected row,
/// one whose next action is earned, or one whose action is still running), so
/// nothing beside it moves when it appears.
///
/// A hidden slot is not painted, so its controls are no tab stops: a row the
/// keyboard should reach names its own focus handle with [`HoverSlot::focus`].
pub fn hover_slot(
    group: impl Into<SharedString>,
    shown: bool,
    child: impl IntoElement,
) -> HoverSlot {
    HoverSlot {
        group: group.into(),
        shown,
        focus: None,
        child: child.into_any_element(),
    }
}

/// What [`hover_slot`] draws. Resolved as it is drawn, because whether the
/// keyboard is on the row is known to the window, not to the caller.
#[derive(IntoElement)]
pub struct HoverSlot {
    group: SharedString,
    shown: bool,
    focus: Option<FocusHandle>,
    child: AnyElement,
}

impl HoverSlot {
    /// The row's own focus handle, a tab stop: the slot also shows while the
    /// keyboard holds the row or one of the slot's controls, so the row's
    /// actions are reached without the pointer. Only for the keyboard: a
    /// click that focused the row leaves the slot to the pointer.
    pub fn focus(mut self, row: &FocusHandle) -> Self {
        self.focus = Some(row.clone());
        self
    }
}

impl RenderOnce for HoverSlot {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let keyed = self.focus.is_some_and(|row| {
            window.last_input_was_keyboard() && row.contains_focused(window, cx)
        });
        div()
            .flex_none()
            .when(!(self.shown || keyed), |slot| {
                slot.invisible().group_hover(self.group, |s| s.visible())
            })
            .child(self.child)
    }
}
