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
//!
//! **Side by side only while there is room for both.** Narrower, the tree and
//! the file show one at a time: opening a file shows it, and the strip over it
//! leads with the way back to the tree.

use crate::view::EditorView;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, rems,
};
use gpui_component::{ActiveTheme, ResizableState, StyledExt, h_resizable, resizable_panel};
use onehand_plugin_host::{ListWidths, list_detail, measure_width};
use std::cell::Cell;
use std::rc::Rc;

/// Where the divider starts, and how far it can be dragged.
///
/// The floor is what a file name needs to be told apart at; under it, the
/// pair is too narrow to share and shows one half at a time instead. The
/// ceiling is a preference: the half being squeezed by a wide tree is the one
/// with the long lines in it, and a drag never takes the file under its own
/// least useful width. In rems, so they follow the panel's zoom.
const TREE: ListWidths = ListWidths::new(12.5, 8.75, 26.25);

pub(crate) struct CodeView {
    /// Held as the view rather than the mode: it is the same entity on every
    /// frame, and everything else the file tree is asked is asked through its
    /// own `WorkbenchMode`.
    files: AnyView,
    editor: Entity<EditorView>,
    divider: Entity<ResizableState>,
    /// The pair's width in rems as last laid out. Infinite until measured, so
    /// the first frame draws the two side by side.
    width: Rc<Cell<f32>>,
    /// Redraws the pair when the buffers' view flips the tree, since what that
    /// view renders is its own and this one reads the flag.
    _tree: Subscription,
}

impl CodeView {
    pub(crate) fn new(files: AnyView, editor: Entity<EditorView>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            files,
            _tree: cx.observe(&editor, |_, _, cx| cx.notify()),
            editor,
            divider: cx.new(|_| ResizableState::default()),
            width: Rc::new(Cell::new(f32::INFINITY)),
        })
    }
}

impl Render for CodeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = list_detail(
            self.divider.read(cx),
            &TREE,
            rems(self.width.get()),
            window.rem_size(),
        );
        let alone = !layout.side_by_side;
        self.editor
            .update(cx, |editor, cx| editor.set_alone(alone, cx));
        let (tree_shown, showing_file) = {
            let editor = self.editor.read(cx);
            (editor.tree_shown(), editor.showing_file())
        };
        let frame = div()
            .size_full()
            .relative()
            .child(measure_width(self.width.clone(), cx.entity().downgrade()));
        // `flex` and not `h_flex`: that one also centres its children across
        // the row, which drew a half at its content's height in the middle of
        // the panel instead of stretched down it.
        if alone {
            return frame
                .flex()
                .map(|frame| match showing_file {
                    true => frame.child(self.editor.clone()),
                    false => frame.child(div().size_full().v_flex().child(self.files.clone())),
                })
                .into_any_element();
        }
        // Hidden, the buffers are drawn alone rather than beside a panel marked
        // invisible: the group still draws the divider's grip on the second
        // panel, which would leave a drag handle along the left edge moving
        // nothing. The divider's state is untouched, so the tree comes back at
        // the width it was dragged to.
        if !tree_shown {
            return frame.flex().child(self.editor.clone()).into_any_element();
        }
        frame
            .child(
                h_resizable("workbench-code")
                    .with_state(&self.divider)
                    .child(
                        // `flex_none`: the panel sets `flex_grow: 1` on itself,
                        // and a tree that grows is a tree taking whatever the
                        // editor is not using, which is most of the panel.
                        resizable_panel()
                            .size(layout.start)
                            .size_range(layout.range)
                            .flex_none()
                            .child(
                                div()
                                    .size_full()
                                    .v_flex()
                                    // The one hairline between the halves, on
                                    // the divider: its grip draws a line only
                                    // while it is dragged.
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .child(self.files.clone()),
                            ),
                    )
                    .child(resizable_panel().child(self.editor.clone())),
            )
            .into_any_element()
    }
}
