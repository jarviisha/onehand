//! Writing workflows: the form a workflow is edited in, one box per step,
//! and the small controls its rows share.

mod draft;
mod form;
mod step;

pub(crate) use draft::WorkflowDraft;
pub(crate) use form::form;

use crate::shell::Shell;
use gpui::{App, ClickEvent, Entity, IntoElement, Styled as _, Window};
use gpui_component::button::ButtonVariants;
use gpui_component::{Icon, Sizable as _};

/// A click handler that hands `act` the shell.
pub(crate) fn click(
    handle: &Entity<Shell>,
    act: impl Fn(&mut Shell, &mut Window, &mut gpui::Context<Shell>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handle = handle.clone();
    move |_, window, cx| handle.update(cx, |shell, cx| act(shell, window, cx))
}

pub(crate) fn row_action(
    handle: &Entity<Shell>,
    id: (&'static str, usize),
    label: &'static str,
    act: impl Fn(&mut Shell, &mut Window, &mut gpui::Context<Shell>) + 'static,
) -> impl IntoElement {
    crate::controls::action(id)
        .ghost()
        .small()
        .label(label)
        .on_click(click(handle, act))
}

/// A row's *Delete*: a word in the danger tint, like every control that
/// removes something.
pub(crate) fn row_delete(
    handle: &Entity<Shell>,
    id: (&'static str, usize),
    act: impl Fn(&mut Shell, &mut Window, &mut gpui::Context<Shell>) + 'static,
    cx: &App,
) -> impl IntoElement {
    crate::controls::action(id)
        .ghost()
        .small()
        .text_color(crate::theme::status_ink(cx).danger)
        .label("Delete")
        .on_click(click(handle, act))
}

pub(crate) fn row_icon(
    id: (&'static str, usize),
    icon: impl Into<Icon>,
    tip: &'static str,
) -> gpui_component::button::Button {
    crate::controls::action(id)
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .tooltip(tip)
}
