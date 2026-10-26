//! The Workbench's Issues tab at a dock's width: the filter bar over both halves,
//! then the list beside the issue, or one of the two at a time where there is
//! no room for both. ↑ and ↓ step the list as on the Issues page, and Esc
//! closes the form or, one at a time, goes back to the list.

use super::IssuesView;
use crate::{IssuesClose, IssuesDown, IssuesUp};
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Styled, Window,
    div,
};
use gpui_component::{ActiveTheme, StyledExt, resizable_panel};
use onehand_core::issues::Issues;
use onehand_ui::h_resizable;
use onehand_ui::{ListWidths, back_link, list_detail};
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
        let search = self.search_bar(issues, window, cx);
        // `flex` and not `v_flex` around a half: each half is a column that
        // takes the height it is given, which a row hands down whole.
        let half = |element: AnyElement| div().flex_1().min_h_0().flex().child(element);

        let halves = if layout.side_by_side {
            let list = self.list(issues, window, cx);
            let detail = self.detail(root, issues, None, window, cx);
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
            let back = back_link("issues-back", "Issues", cx)
                .tooltip_with_action("Back to the list", &IssuesClose, Some("Issues"))
                .on_click(cx.listener(|view: &mut Self, _, _, cx| {
                    view.detail = false;
                    cx.notify();
                }));
            let detail = self.detail(root, issues, None, window, cx);
            div()
                .flex_1()
                .min_h_0()
                .v_flex()
                .child(onehand_ui::workbench_strip(cx).child(back))
                .child(half(detail))
        } else {
            half(self.list(issues, window, cx))
        };
        div()
            .key_context("Issues")
            .track_focus(&self.tab_focus)
            .on_action(cx.listener(|view, _: &IssuesUp, window, cx| view.tab_step(-1, window, cx)))
            .on_action(cx.listener(|view, _: &IssuesDown, window, cx| view.tab_step(1, window, cx)))
            .on_action(cx.listener(|view, _: &IssuesClose, window, cx| view.tab_close(window, cx)))
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(search)
            .child(halves)
            .into_any_element()
    }

    /// Whether the tab is wide enough for the list beside the issue.
    fn side_by_side(&self, window: &Window, cx: &App) -> bool {
        list_detail(
            self.split.read(cx),
            &LIST,
            self.width.get(),
            window.rem_size(),
        )
        .side_by_side
    }

    /// Pick the row `step` away from the one picked. Where the list shows
    /// alone, the first press opens the row picked rather than moving, as a
    /// closed issue on the page does.
    fn tab_step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(issues) = self.issues_of(&root, cx) else {
            return;
        };
        let rows = self.rows_shown(issues, cx);
        let at = self
            .roots
            .get(&root)
            .and_then(|state| state.selected)
            .and_then(|number| rows.iter().position(|row| *row == number));
        let open = self.side_by_side(window, cx) || self.issue_alone(window, cx);
        let next = match at {
            Some(at) if !open => Some(at),
            Some(at) => at.checked_add_signed(step),
            None => Some(0),
        };
        if let Some(&number) = next.and_then(|next| rows.get(next)) {
            self.select(number, window, cx);
        }
    }

    /// Close the form, else, where the issue shows in place of the list, go
    /// back to the list. With neither, Esc is left to whatever holds the tab.
    fn tab_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state_mut().is_some_and(|state| state.form.is_some()) {
            self.unless_drafting(window, cx, |view, _, cx| view.cancel_form(cx));
        } else if self.issue_alone(window, cx) {
            self.detail = false;
            cx.notify();
        } else {
            cx.propagate();
            return;
        }
        // What held the keyboard may be gone with what closed.
        window.focus(&self.tab_focus, cx);
    }

    /// Whether an issue shows in place of the list, as the tab draws it: the
    /// flag alone can outlive the issue or form it was set for.
    fn issue_alone(&self, window: &Window, cx: &App) -> bool {
        let Some(root) = self.root.as_deref() else {
            return false;
        };
        self.detail
            && !self.side_by_side(window, cx)
            && self
                .issues_of(root, cx)
                .is_some_and(|issues| self.has_detail(root, issues))
    }

    /// Whether there is an issue or a form to show in place of the list.
    fn has_detail(&self, root: &Path, issues: &Issues) -> bool {
        self.roots.get(root).is_some_and(|state| {
            state.form.is_some() || state.selected.is_some_and(|n| issues.get(n).is_some())
        })
    }
}
