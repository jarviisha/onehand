//! What every view of rendered Markdown shares.

use gpui::{Empty, StyleRefinement, Styled as _};
use gpui_component::text::{TextView, TextViewStyle};

/// `view` with no language named over its code blocks.
///
/// The renderer prints the language in a block's corner and makes room for
/// it with padding of its own, which a view setting its own padding takes
/// away, so the word sat on the block's first line. A view that puts its own
/// controls in that corner has none; this is the same, with nothing in it.
pub fn no_code_label(view: TextView) -> TextView {
    view.code_block_actions(|_, _, _| Empty)
}

/// The renderer's own styling, but for its code blocks' edge: a block is a
/// well, and a well is a fill with no edge. For a view with no styling of its
/// own; one that has its own puts `border_0` in its code block's.
pub fn unedged_code() -> TextViewStyle {
    TextViewStyle::default().code_block(StyleRefinement::default().border_0())
}
