//! The surfaces a dock, a page and a scrolling region are drawn on.

use crate::tokens;
use gpui::{App, Div, Hsla, Styled as _, div};
use gpui_component::{ActiveTheme as _, Colorize as _, StyledExt as _};

/// A sunk box of machine text: full width, the muted fill, the theme's corner,
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
        .rounded(theme.radius)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
        .text_size(tokens::CODE_TEXT)
}

/// The surface a dock draws on: the palette's panel step, one off the reading
/// surface the conversation sits on, so a dock reads as a surface of its own
/// beside the conversation rather than as a card inset in it. The rail stands
/// on the same step, so the app writes it into the sidebar slot read here,
/// with its contrast tested beside the rest of its ramp.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn dock_surface(cx: &App) -> Hsla {
    cx.theme().sidebar
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
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().input)
}

/// A box in the library's alert look: an edge and a wash of `tone` mixed into
/// the page's fill, opaque, for a box that waits on the person (a run's
/// approval).
pub fn alert_tone(boxed: Div, tone: Hsla, cx: &App) -> Div {
    let fill = cx.theme().background;
    boxed
        .border_color(tone.mix_oklab(fill, 0.3))
        .bg(tone.mix_oklab(fill, 0.04))
}
