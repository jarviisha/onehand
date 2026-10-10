//! The rule every list beside its detail follows in a dock.
//!
//! Side by side only while the container holds the list at the width it is
//! dragged to and [`DETAIL_MIN`] beside it; otherwise one at a time, the detail
//! under a link back naming the list. The width is the container's, measured
//! each frame, never the window's: a dock is dragged and zoomed apart from the
//! window it sits in. Going back keeps what was picked.

use gpui::Styled as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName, Sizable as _};

/// The least room, in rems, a detail is still readable in beside its list.
pub const DETAIL_MIN: f32 = 24.;

/// Whether a list `list` rems wide and its detail fit side by side in
/// `container` rems.
pub fn side_by_side(container: f32, list: f32) -> bool {
    container >= list + DETAIL_MIN
}

/// The way back from a detail shown alone to the list it was picked from,
/// named for that list. The caller says what a press does.
pub fn back_link(id: &'static str, list: &'static str) -> Button {
    crate::action(id)
        .ghost()
        .small()
        .flex_none()
        .icon(Icon::new(IconName::ArrowLeft))
        .label(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_and_its_detail_share_a_container_only_when_both_fit() {
        assert!(side_by_side(40., 12.));
        assert!(side_by_side(12. + DETAIL_MIN, 12.));
        assert!(!side_by_side(12. + DETAIL_MIN - 0.1, 12.));
        // A list dragged wider needs a wider container.
        assert!(!side_by_side(40., 20.));
    }
}
