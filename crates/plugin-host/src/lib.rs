//! What the app hands its built-in plugins, and what they hand back.
//!
//! The things a plugin needs that it cannot reach into the binary for — a
//! button that answers the pointer, the derivation of status ink, and the
//! surface a dock is drawn on — plus the Workbench mode contract itself.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

mod list_detail;
mod menu;
mod tabs;
mod workbench;
pub use list_detail::{DETAIL_MIN, back_link, side_by_side};
pub use menu::{menu_below, menu_item, menu_row};
pub use onehand_core::worktree::removal::Process;
pub use tabs::{TAB_MAX_W, TabStrip, measure_width, tab_menu_rows, tab_select, tab_strip};
pub use workbench::{Ask, Request, WorkbenchMode};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Colorize as _, Size, StyledExt as _};
use std::rc::Rc;

/// How a remote channel is opened, once its credential has been read.
pub type RemoteChannelFactory = fn(String) -> Box<dyn onehand_core::remote::types::RemoteChannel>;

/// A button that answers the pointer, which is every button Onehand draws.
///
/// The component library draws every button variant except `link` and `text`
/// with the **arrow** cursor. That is the platform convention this app is not
/// following: a session row, a completion candidate, a selector chip and an ask
/// choice are all hand-made `div`s that show a pointer, because that is the one
/// feedback a control gets *before* it is pressed. Half the actions on screen
/// answering the pointer and half not is worse than either rule applied whole —
/// the cursor stops meaning anything, and the only way left to find out whether
/// something is clickable is to click it.
///
/// The library re-applies the caller's own style refinement last, after its
/// `cursor_default`, so setting the cursor here wins — which is the whole reason
/// this can be a wrapper rather than a fork of the control.
///
/// **It lives here rather than in the app** because a built-in plugin draws
/// buttons too and cannot reach into the binary that hosts it. A second copy in
/// each half is two places for the library's default to be let through, and the
/// bypass is one line and looks exactly like ordinary code — which is why a
/// guard counts the call sites and why there must be only one of these to count
/// against.
pub fn action(id: impl Into<ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

/// The surface a dock draws on: the palette's panel step, one off the reading
/// surface the conversation sits on, so a dock reads as a surface of its own
/// beside the conversation rather than as a card inset in it. The app
/// writes that step into the theme slot read here, with its contrast tested
/// beside the rest of its ramp.
///
/// Here rather than in the app for the reason [`action`] is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn dock_surface(cx: &App) -> Hsla {
    cx.theme().tiles
}

/// Status colours used as ink on the app's normal surfaces.
///
/// Here rather than in the app for the reason [`action`] is: a built-in plugin
/// draws change badges and cannot reach into the binary hosting it, and a
/// second copy of the derivation is a second place for the raw status fill to
/// be used as ink — which is the mistake this exists to prevent.
#[derive(Clone, Copy)]
pub struct StatusInk {
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
}

/// Resolve status ink from the active palette.
///
/// Base hues already switch between darker 600-level colours in light mode and
/// brighter 400-level colours in dark mode. Pulling them part of the way toward
/// the theme foreground gives small labels and thin icons enough contrast
/// without inventing a second set of hues beside the ramp.
///
/// **How far is set by the well, not by the reading surface.** A tool's status
/// word, a diff's added and removed lines and a terminal's exit code are all
/// drawn on the sunk fill rather than on the surface, and light amber is the
/// one that runs out of margin there first.
pub fn status_ink(cx: &App) -> StatusInk {
    let theme = cx.theme();
    StatusInk {
        danger: status_hue(theme.red, theme.foreground),
        warning: status_hue(theme.yellow, theme.foreground),
        success: status_hue(theme.green, theme.foreground),
    }
}

/// One hue, pulled toward the foreground.
///
/// `pub` because the app's contrast test asserts this derivation directly: it
/// works from a resolved ramp rather than from the theme in force, so it cannot
/// go through [`status_ink`], which reads a global. Not a way in for anything
/// else — a call site wanting status ink wants all three at once.
pub fn status_hue(base: Hsla, foreground: Hsla) -> Hsla {
    base.mix_oklab(foreground, 0.70)
}

/// A mode's empty state: one muted line, centred in the body it would fill.
///
/// Here rather than copied into each mode for the reason [`action`] is. Four
/// copies of a centred muted line is four places for one of them to drift into
/// a different silence from its neighbours, in a panel where the user switches
/// between them with one click.
pub fn hint(text: &'static str, cx: &App) -> AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// The line a mode draws under its body for a standing condition.
///
/// **Deliberately not a notification**, unlike the rest of the app's transient
/// status. A save refused because the file changed on disk, a document that has
/// outgrown the read's size bound, an editor that would not start: none of them
/// is news that may be missed, and a toast that fades leaves the user believing
/// the thing went through. Each is cleared by whatever answers it.
///
/// Drawn by the mode and not by the panel, because the panel no longer knows
/// what any of them mean — which is what makes one definition worth having.
pub fn status_line(message: String, cx: &App) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(status_ink(cx).warning)
        .child(message)
        .into_any_element()
}

/// A choice between a few views of one panel: a track with a filled plate in
/// the half that is showing, every half the same width.
///
/// **Drawn here, after both of the library's answers were tried.** The shape
/// needs a fill, an inset and halves of equal width, and neither component
/// gives all three. A `ButtonGroup` splits evenly but has no track, so it reads
/// as two outlined controls that happen to disagree. A segmented `TabBar` is
/// the track and the plate exactly, but sizes every tab to its own label inside
/// a `flex_shrink_0` nothing outside the library can stretch — so one label
/// came out two thirds the width of the other, both against the left edge of a
/// bar as wide as its panel. So the track is the library's own segmented fill
/// and the halves are `flex_1`; the fills and the radius are the theme's.
///
/// **The selected half is `accent`**, which is the app's own "this one, among
/// several" — what the terminal's tabs and the Workbench's modes take, so
/// one condition keeps one spelling. It is not the reading surface: on a panel
/// drawn in the well, a plate in the reading surface is a step *below* what it
/// sits on, a hole rather than a plate. And there is no shadow under it: a fill
/// that differs lifts by itself, and a drop shadow over a near-black surface is
/// invisible anyway.
///
/// **Here rather than in the app** for the reason [`action`] is: the rail and a
/// built-in plugin both draw one, and a plugin cannot reach into the binary
/// hosting it.
///
/// `size` is the library's own scale, so a switch sits level with the buttons
/// and inputs of that size beside it: the rail's is the smallest, a row of
/// chips in a narrow column, and a panel's tabs take a little room to breathe.
pub fn switch(
    id: &'static str,
    labels: &[SharedString],
    active: usize,
    size: Size,
    pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (track, plate, radius) = (theme.tokens.tab_bar_segmented, theme.accent, theme.radius);
    let (ink, ink_on) = (theme.muted_foreground, theme.accent_foreground);
    let pick = Rc::new(pick);

    div()
        .h_flex()
        .w_full()
        .gap_0p5()
        .p_0p5()
        .rounded(radius)
        .bg(track)
        .children(labels.iter().enumerate().map(|(i, label)| {
            let on = i == active;
            let pick = pick.clone();
            div()
                .id((id, i))
                .h_flex()
                .justify_center()
                .flex_1()
                .min_w_0()
                .rounded(radius)
                .cursor_pointer()
                .map(|half| match size {
                    Size::XSmall => half.text_xs(),
                    Size::Small => half.py_0p5().text_xs(),
                    Size::Medium | Size::Size(_) => half.py_1().text_sm(),
                    Size::Large => half.py_1p5().text_base(),
                })
                .text_color(if on { ink_on } else { ink })
                .when(on, |half| half.bg(plate))
                .on_click(move |_, window, cx| pick(&i, window, cx))
                .child(label.clone())
        }))
        .into_any_element()
}
