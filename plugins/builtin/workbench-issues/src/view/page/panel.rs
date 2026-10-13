//! The issue picked, floating over the list's right side: its reference,
//! where it stands in the list and `×` over the issue (or the form), its left
//! edge dragged to size.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, DragMoveEvent, Empty,
    InteractiveElement as _, IntoElement, ParentElement, Rems, Render,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme, IconName, Sizable as _, StyledExt};
use onehand_plugin_host::Quiet as _;
use onehand_plugin_host::tokens::BAR_H;
use onehand_plugin_host::{action, hint};

// The widths below are plain rems rather than `Rems`: the drag works them out
// against the pointer in that unit.

/// The panel's width before anybody drags it, in rems: a description's
/// reading measure, narrow enough that the titles stay readable beside it on
/// a common window.
const PANEL_W: f32 = 40.;

/// The narrowest a drag takes the panel, in rems: the issue's title row and
/// its actions still on one line.
const PANEL_MIN: f32 = 24.;

/// How much of the list a drag leaves showing beside the panel, in rems, so
/// the row the issue belongs to can still be told; an area narrower than the
/// panel and this is covered whole.
const LIST_MIN: f32 = 16.;

/// The gap the panel keeps from the area's edges, in rems: the list shows
/// round it, so it reads as lifted over the list rather than set in it.
const INSET: Rems = rems(1.);

/// What a drag of the panel's edge carries: nothing, the width is read off
/// the pointer.
pub(super) struct PanelDrag;

impl Render for PanelDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl IssuesView {
    /// The panel, over the list's area.
    pub(super) fn page_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let area = self.width.get().0;
        let wanted = self.page.as_ref().and_then(|page| page.panel_w);
        let width = clamp(wanted.unwrap_or(PANEL_W), area);
        let whole = area < width + LIST_MIN;
        let (reference, place) = self.panel_head(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        // Where the issue stands in the list, and the way to close the panel.
        let nav = div()
            .h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .children(place.map(|place| div().flex_none().child(place)))
            .child(
                action("issues-page-close")
                    .small()
                    .quiet(cx)
                    .icon(IconName::Close)
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        if let Some(page) = view.page.as_mut() {
                            page.open = false;
                        }
                        cx.notify();
                    })),
            )
            .into_any_element();
        // An issue heads itself with its trail, the way to close at its end;
        // the form has no trail, so the panel draws one over it.
        let (bar, body) = match self.root.clone() {
            Some(root) if !self.drafting() && self.selected_number().is_some() => {
                match self.issues_of(&root, cx).cloned() {
                    Some(issues) => (None, self.detail(&root, &issues, Some(nav), window, cx)),
                    None => (None, hint("Reading issues…", cx)),
                }
            }
            Some(root) => {
                let body = match self.issues_of(&root, cx).cloned() {
                    Some(issues) => self.detail(&root, &issues, None, window, cx),
                    None => hint("Reading issues…", cx),
                };
                let bar = div()
                    .flex_none()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .h(BAR_H)
                    .pl_4()
                    .pr_2()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(mono)
                            .child(reference),
                    )
                    .child(nav);
                (Some(bar), body)
            }
            None => (None, hint("Pick an issue", cx)),
        };
        let theme = cx.theme();
        div()
            .id("issues-page-panel")
            .occlude()
            .absolute()
            // Set down into the first row rather than on the columns' names'
            // foot, so the panel's top edge never runs along a line of the
            // table and reads as a lid on it.
            .top(INSET)
            .right(INSET)
            .bottom(INSET)
            .map(|panel| {
                if whole {
                    panel.left(INSET)
                } else {
                    panel.w(rems(width))
                }
            })
            .v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow(onehand_plugin_host::tokens::lift(cx))
            .overflow_hidden()
            .children(bar)
            .child(div().flex_1().min_h_0().v_flex().child(body))
            // The edge a drag sizes the panel by; covering the area, the
            // panel has no edge to drag.
            .when(!whole, |panel| {
                panel.child(
                    div()
                        .id("issues-page-panel-edge")
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w_1()
                        .cursor_col_resize()
                        .on_drag(PanelDrag, |_, _, _, cx| cx.new(|_| PanelDrag)),
                )
            })
            .into_any_element()
    }

    /// The panel's head: the project and reference of the issue, or what the
    /// form is for, and *N of M* while the issue is a row on screen.
    fn panel_head(&self, cx: &App) -> (String, Option<String>) {
        let Some(page) = self.page.as_ref() else {
            return (String::new(), None);
        };
        let Some(root) = self.root.as_deref() else {
            return (String::new(), None);
        };
        let project = page.label_of(root).unwrap_or_default();
        if self.drafting() {
            return (format!("{project} · new issue"), None);
        }
        let Some(number) = self.selected_number() else {
            return (project.to_string(), None);
        };
        let reference = self
            .issues_of(root, cx)
            .and_then(|issues| issues.get(number))
            .and_then(|issue| issue.reference().map(str::to_string))
            .unwrap_or_else(|| "Draft".to_string());
        let place = page
            .shown
            .iter()
            .position(|(r, n)| r == root && *n == number)
            .map(|at| format!("{} of {}", at + 1, page.shown.len()));
        (format!("{project} {reference}"), place)
    }

    /// The panel's edge dragged: its width follows the pointer, kept within
    /// what leaves the list showing beside it.
    pub(super) fn drag_panel(
        &mut self,
        event: &DragMoveEvent<PanelDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rem = window.rem_size();
        let right = event.bounds.right() - rem * INSET.0;
        let wanted = (right - event.event.position.x) / rem;
        let area = event.bounds.size.width / rem;
        if let Some(page) = self.page.as_mut() {
            page.panel_w = Some(clamp(wanted, area));
            cx.notify();
        }
    }
}

/// `width` kept between the narrowest the panel is drawn and what leaves
/// [`LIST_MIN`] of an `area` that wide showing beside it.
fn clamp(width: f32, area: f32) -> f32 {
    width.min(area - LIST_MIN - INSET.0).max(PANEL_MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_panel_keeps_to_its_range() {
        assert_eq!(clamp(PANEL_W, 100.), PANEL_W);
        // Wider than leaves the list showing: stopped there.
        assert_eq!(clamp(90., 100.), 100. - LIST_MIN - INSET.0);
        // Narrower than the floor, or an area too small for both: the floor.
        assert_eq!(clamp(4., 100.), PANEL_MIN);
        assert_eq!(clamp(PANEL_W, 20.), PANEL_MIN);
    }
}
