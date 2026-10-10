//! The Workbench's Issues tab at a dock's width: the search over both halves,
//! then the list beside the issue, or one of the two at a time where there is
//! no room for both.

use super::IssuesView;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::{ActiveTheme, StyledExt, h_resizable, resizable_panel};
use onehand_core::issues::Issues;
use onehand_plugin_host::{ListWidths, back_link, list_detail};
use std::path::Path;

/// The list's width before anybody drags it, and the range a drag may take it
/// through, in rems so they follow the panel's zoom. A row's title wraps to two
/// lines, so the floor is what the filters above the rows need.
const LIST: ListWidths = ListWidths::new(15., 10., 26.25);

impl IssuesView {
    pub(super) fn tab_body(
        &mut self,
        root: &Path,
        issues: &Issues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = list_detail(
            self.split.read(cx),
            &LIST,
            self.width.get(),
            window.rem_size(),
        );
        let search = self.search_bar(window, cx);
        // `flex` and not `v_flex` around a half: each half is a column that
        // takes the height it is given, which a row hands down whole.
        let half = |element: AnyElement| div().flex_1().min_h_0().flex().child(element);

        let halves = if layout.side_by_side {
            let list = self.list(issues, window, cx);
            let detail = self.detail(root, issues, window, cx);
            div().flex_1().min_h_0().child(
                h_resizable("issues-split")
                    .with_state(&self.split)
                    .child(
                        // `flex_none`, as the Markdown mode's list is: a
                        // panel in the group grows by default, and a list
                        // that grows takes the room the issue was opened
                        // to be read in.
                        resizable_panel()
                            .size(layout.start)
                            .size_range(layout.range)
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
                        .px_3()
                        .py_1p5()
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
