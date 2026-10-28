//! What a mode says in words of its own: its empty state, and the line
//! under its body for a standing condition.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// A mode's empty state: one line of meta text, centred in the body it would
/// fill.
///
/// Here rather than copied into each mode for the reason [`action`](crate::action) is. Four
/// copies of a centred muted line is four places for one of them to drift into
/// a different silence from its neighbours, in a panel where the user switches
/// between them with one click.
pub fn hint(text: &'static str, cx: &App) -> AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .text_xs()
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
///
/// In the ink of the state it reports, one of [`status_ink`](crate::status_ink)'s, and on the
/// strip's inset, so its words start where the strip's above them do.
pub fn status_line(message: impl Into<SharedString>, ink: Hsla) -> AnyElement {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .text_color(ink)
        .child(message.into())
        .into_any_element()
}
