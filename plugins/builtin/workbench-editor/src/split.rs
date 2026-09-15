//! The mode's two halves side by side: the project's file tree, then the
//! buffers open out of it.
//!
//! They were two entries on the mode strip, and the strip is where the cost
//! showed: picking a file meant Files, reading it meant Editor, and going back
//! for the next file meant the strip again — a switch per file in the one place
//! a user moves between files all day. Neither half is big enough to want the
//! whole panel, either: a tree is a narrow column of names and an editor wants
//! whatever is left.
//!
//! **The divider is draggable and its position is not persisted.** It is one
//! number per window, restored to the same starting width on every launch, and
//! the alternative is another key in the workspace file for something a drag
//! re-answers in a second.

use crate::view::EditorView;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::{ActiveTheme, ResizableState, StyledExt, h_resizable, resizable_panel};

/// Where the divider starts, and how far it can be dragged.
///
/// The floor is what a nested path still reads at rather than a round number:
/// below it the tree is a column of ellipses. The ceiling is there because the
/// half being squeezed is the one with the long lines in it.
const TREE_W: f32 = 200.;
const TREE_MIN: f32 = 140.;
const TREE_MAX: f32 = 420.;

pub(crate) struct CodeView {
    /// Held as the view rather than the mode: it is the same entity on every
    /// frame, and everything else the file tree is asked is asked through its
    /// own `WorkbenchMode`.
    files: AnyView,
    editor: Entity<EditorView>,
    divider: Entity<ResizableState>,
}

impl CodeView {
    pub(crate) fn new(files: AnyView, editor: Entity<EditorView>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            files,
            editor,
            divider: cx.new(|_| ResizableState::default()),
        })
    }
}

impl Render for CodeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_resizable("workbench-code")
            .with_state(&self.divider)
            .child(
                // `flex_none`: the panel sets `flex_grow: 1` on itself, and a
                // tree that grows is a tree taking whatever the editor is not
                // using, which is most of the panel.
                resizable_panel()
                    .size(px(TREE_W))
                    .size_range(px(TREE_MIN)..px(TREE_MAX))
                    .flex_none()
                    .child(
                        div()
                            .size_full()
                            .v_flex()
                            // The handle draws a line only under the pointer, so
                            // without this the tree and the code it opened run
                            // into each other whenever nobody is dragging.
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(self.files.clone()),
                    ),
            )
            .child(resizable_panel().child(self.editor.clone()))
    }
}
