//! The Workbench's row over a mode with no bar of its own to draw it in: the
//! row's start, a filler, and its end.

use super::Workbench;
use gpui::{Context, IntoElement, ParentElement, Styled, div};

impl Workbench {
    /// The dock's tab bar, as tall as the conversation's header beside it so
    /// the two line up across the gap.
    pub(super) fn strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        onehand_ui::tab_bar(cx)
            .child(self.ends.lead.clone())
            .child(div().flex_1().min_w_0())
            .child(self.ends.end.clone())
    }
}
