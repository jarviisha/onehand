//! The surfaces a dock, a page and a scrolling region are drawn on.

use crate::tokens;
use gpui::{App, Div, Hsla, Styled as _, div};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// A sunk box of machine text: full width, the muted fill, the large corner,
/// the mono family at [`tokens::CODE_TEXT`], clipped so a long line cannot push
/// the column wider. The caller sets the padding, which differs by what is in it.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: the Issues mode
/// draws the same well as the conversation's, and a second copy is a second
/// place for its fill to drift.
pub fn well(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(theme.radius_lg)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
        .text_size(tokens::CODE_TEXT)
}

/// The surface the Workbench draws on: the reading surface, as the
/// conversation's box is, so the two read as boxes of the same kind, parted
/// by the window's fill around them. The terminal's box stands a step deeper,
/// on a surface the app derives for it alone.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn dock_surface(cx: &App) -> Hsla {
    cx.theme().background
}

/// A region's box: the agent pane, the Workbench, the terminal. Filling what
/// it is given, on the reading surface (the terminal's box sets its own), at the large corner, wrapped in the
/// theme's hairline (`border`) so its edge holds where the two fills sit
/// close: it stands on the window's fill and is parted from its
/// neighbours by the gap around it ([`tokens::REGION_GAP`]). Clipped, so nothing inside paints
/// past it; the clip is square, so what touches a corner leaves its own fill
/// off.
pub fn region(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .size_full()
        .bg(theme.background)
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
}

/// A vertical scrollbar at the app's weight: every scrolling region draws its
/// thumb through this, so none of them is heavier than the next.
pub fn scrollbar<H: gpui_component::scroll::ScrollbarHandle + Clone>(
    handle: &H,
) -> gpui_component::scroll::Scrollbar {
    gpui_component::scroll::Scrollbar::vertical(handle).styles(|styles| {
        styles
            .thumb(|thumb| thumb.width(tokens::SCROLLBAR_THUMB))
            .thumb_hover(|thumb| thumb.width(tokens::SCROLLBAR_THUMB_ACTIVE))
            .thumb_active(|thumb| thumb.width(tokens::SCROLLBAR_THUMB_ACTIVE))
    })
}

/// The box a page groups one thing's facts in: a card on the overview or
/// Tasks, a project tile, a task's row, an issue's conflict or review.
/// Padded `p_3`, its parts `gap_2` apart, edged in a control's edge: the
/// hairline is too near the page's fill in the dark palette to group
/// anything.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: the Issues page
/// draws the same box as the overview's.
pub fn card_box(cx: &App) -> Div {
    div()
        .v_flex()
        .gap_2()
        .min_w_0()
        .p_3()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().input)
}

/// The band over a field's trailing edge: nothing fading to the box's fill
/// ([`dock_surface`]), [`tokens::BAR_FADE`] wide, drawn over the field and
/// taking no room of its own, so text running past the field melts into what
/// follows instead of being cut. The field's parent must be `relative`.
pub fn end_fade(cx: &App) -> gpui::Div {
    let surface = dock_surface(cx);
    gpui::div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(tokens::BAR_FADE)
        .bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(surface.alpha(0.), 0.),
            gpui::linear_color_stop(surface, 1.),
        ))
}
