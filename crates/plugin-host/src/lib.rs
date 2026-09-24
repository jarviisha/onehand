//! What the app hands its built-in plugins, and what they hand back.
//!
//! The things a plugin needs that it cannot reach into the binary for — a
//! button that answers the pointer, the derivation of status ink, and the chrome
//! surface a panel is drawn on — plus the Workbench mode contract itself.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

mod workbench;
pub use workbench::{Ask, Request, WorkbenchMode};

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement as _, ParentElement as _, Styled as _, div,
};
use gpui_component::button::Button;
use gpui_component::{ActiveTheme as _, Colorize as _, StyledExt as _};

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

/// The surface a dock panel's card draws on.
///
/// **It is the reading surface, the same one the conversation is on**, and the
/// card's border is the whole of what says where the panel begins. That is a
/// reversal: the two cards used to be filled a step off the conversation, so in
/// the dark palette they were the *lighter* regions on screen with the
/// conversation as the dark gap between them. Lighter reads as nearer, so two
/// panels that are about the work were drawn in front of the work — and with
/// both docks open the conversation was the one thing on screen not being
/// lifted toward the reader.
///
/// **A step in the other direction is not available**, which is why it is no
/// step at all rather than a smaller one. The fill was the ramp's well step,
/// one notch up from the reading surface; going *down* instead would need a
/// value below a near-black surface, and there is none. Halving the step was
/// tried in that same spot and measured 1.07 against the reading surface in
/// both palettes, under the 1.14 floor the ramp's own tests hold every surface
/// pair to — and the light palette has only 1.15 between white and the well to
/// divide in the first place, so no value between them can clear that floor
/// twice.
///
/// **The rail keeps the well and is now the only thing in the window that has
/// it.** It is the one panel that is not about the work at all — a workspace,
/// its projects, its sessions — so it stays lifted while the docks lie flat,
/// and what used to be a code shared by three panels is now a code carried by
/// one.
///
/// What flipping this costs is the well *inside* a panel. While the card was
/// the well, anything sunk into it had to borrow the reading surface to be
/// seen; with the card on the reading surface those go back to being the well —
/// the two strips' hover fills and the Markdown mode's code blocks. `accent` is
/// still what a *selected* thing takes.
///
/// Here rather than in the app for the reason [`action`] is: the Neovim mode
/// draws a terminal grid and has to hand it the surface it is sitting on, and
/// it cannot reach into the binary hosting it. A grid fills every cell it has
/// not been told otherwise about with its palette's default background, so a
/// second copy of this answer is visible as a rectangle of the wrong shade
/// behind a running program.
pub fn chrome(cx: &App) -> Hsla {
    cx.theme().background
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
