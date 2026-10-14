//! A row's actions, shown only while they are wanted, in a slot that holds
//! its width so nothing beside it moves when they appear.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, Styled as _, Window, div,
};

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
