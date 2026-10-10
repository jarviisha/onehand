//! The Workbench's Issues tab at a dock's width: the search over both halves,
//! then the list beside the issue, or one of the two at a time where there is
//! no room for both.

use super::IssuesView;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{ActiveTheme, StyledExt, h_resizable, resizable_panel};
use onehand_core::issues::Issues;
use onehand_plugin_host::{DETAIL_MIN, back_link, side_by_side};
use std::path::Path;

/// The list's width before anybody drags it, and the range a drag may take it
/// through — pixels, because that is the only thing the split accepts. A
/// row's title wraps to two lines, so the floor is what the filters above the
/// rows need.
const LIST_W: f32 = 240.;
const LIST_MIN: f32 = 160.;
const LIST_MAX: f32 = 420.;

impl IssuesView {
    pub(super) fn tab_body(
        &mut self,
        root: &Path,
        issues: &Issues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rem = f32::from(window.rem_size());
        // The list's width as dragged, read off the split; a size outside the
        // range is the split seeding its panels before anything was measured.
        let list_w = self
            .split
            .read(cx)
            .sizes()
            .first()
            .map(|w| f32::from(*w))
            .filter(|w| (LIST_MIN..=LIST_MAX).contains(w))
            .unwrap_or(LIST_W);
        let width = self.width.get();
        let search = self.search_bar(window, cx);
        // `flex` and not `v_flex` around a half: each half is a column that
        // takes the height it is given, which a row hands down whole.
        let half = |element: AnyElement| div().flex_1().min_h_0().flex().child(element);

        let halves = if side_by_side(width, list_w / rem) {
            let list = self.list(issues, window, cx);
            let detail = self.detail(root, issues, window, cx);
            // No drag takes the issue under its least readable width, so
            // dragging the list never flips the two into one at a time.
            let list_max = ((width - DETAIL_MIN) * rem).clamp(LIST_MIN, LIST_MAX);
            div().flex_1().min_h_0().child(
                h_resizable("issues-split")
                    .with_state(&self.split)
                    .child(
                        // `flex_none`, as the Markdown mode's list is: a
                        // panel in the group grows by default, and a list
                        // that grows takes the room the issue was opened
                        // to be read in.
                        resizable_panel()
                            .size(px(LIST_W))
                            .size_range(px(LIST_MIN)..px(list_max))
                            .flex_none()
                            .child(
                                div()
                                    .size_full()
                                    // The one hairline between the halves,
                                    // on the divider: its grip draws a line
                                    // only while it is dragged.
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .child(list),
                            ),
                    )
                    .child(resizable_panel().child(detail)),
            )
        } else if self.detail && self.has_detail(root, issues) {
            let back = back_link("issues-back", "Issues").on_click(cx.listener(
                |view: &mut Self, _, _, cx| {
                    view.detail = false;
                    cx.notify();
                },
            ));
            let detail = self.detail(root, issues, window, cx);
            div()
                .flex_1()
                .min_h_0()
                .v_flex()
                .child(
                    div()
                        .h_flex()
                        .flex_none()
                        .items_center()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(back),
                )
                .child(half(detail))
        } else {
            half(self.list(issues, window, cx))
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(search)
            .child(halves)
            .into_any_element()
    }

    /// Whether there is an issue or a form to show in place of the list.
    fn has_detail(&self, root: &Path, issues: &Issues) -> bool {
        self.roots.get(root).is_some_and(|state| {
            state.form.is_some() || state.selected.is_some_and(|n| issues.get(n).is_some())
        })
    }
}
