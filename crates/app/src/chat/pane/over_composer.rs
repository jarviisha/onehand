//! The surface a line over the composer stands on: a lead's strip, a
//! worker's band.

use gpui::{App, Div, InteractiveElement as _, Stateful, Styled, div};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// The surface a line over the composer stands on: one row, narrower than
/// the composer, edged and lifted as the card it stands on, open at its foot
/// where the card's own edge closes it.
pub(super) fn over_composer(id: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .h(onehand_ui::tokens::ROW_H)
        .px_3()
        .bg(crate::theme::raised(cx))
        .border_t_1()
        .border_x_1()
        .border_color(cx.theme().input)
        .shadow(crate::theme::composer_lift(cx))
        .rounded_t(cx.theme().radius)
        .text_size(onehand_ui::tokens::CHROME_TEXT)
}
