//! What follows the pointer through a drag of a row, the rail's and the
//! board's alike, so every row dragged in the window looks the same.

use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div};
use gpui_component::ActiveTheme;

/// The dragged row's own name, on the sidebar's accent. `gpui` wants an
/// entity here, so this is the smallest one that can carry a string.
pub(crate) struct DragGhost(pub(crate) SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .rounded(cx.theme().radius)
            // Solid, as the ghost floats over whatever it is dragged across.
            .bg(cx.theme().sidebar_accent)
            .text_size(crate::tokens::CHROME_TEXT)
            .text_color(cx.theme().sidebar_accent_foreground)
            .child(self.0.clone())
    }
}
