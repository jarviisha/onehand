//! The bar along the top of a region, and the control it carries.
//!
//! Every strip is one shape, so the conversation's header, the docks' strips
//! and the bars inside a mode line up across a seam and read as one kind of
//! thing: as tall as the header, its words at the chrome size, what it names
//! at the start and its controls at the end.

use crate::Quiet as _;
use gpui::{App, Div, ElementId, Styled as _, div};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _};

/// A strip: [`crate::tokens::BAR_H`] tall, inset `px_3`, its parts `gap_2`
/// apart, lettered at the chrome size in the muted ink. What it names is
/// written in full ink by the caller, first.
///
/// **No line under it.** A strip and what it heads are one surface; the space
/// and the change from a row of controls to its content are what part them.
pub fn strip(cx: &App) -> Div {
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .w_full()
        .h(crate::tokens::BAR_H)
        .px_3()
        .gap_2()
        .text_size(crate::tokens::CHROME_TEXT)
        .text_color(cx.theme().muted_foreground)
}

/// A strip's control: an icon with no words, a quiet ghost at a strip's size,
/// muted at rest and inked when hovered or `on`. The caller names it in a
/// tooltip, which an icon alone needs.
pub fn strip_control(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    on: bool,
    cx: &App,
) -> Button {
    crate::action(id)
        .quiet_icon(on, cx)
        .small()
        .flex_none()
        .icon(icon.into())
}
