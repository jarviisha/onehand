//! The page's head — its name, *Refresh* and *New issue* — and the row of
//! search, open or closed and filters under it.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::task::work::list::{Filters, Listed};
use onehand_ui::Named as _;
use onehand_ui::Quiet as _;
use onehand_ui::Short as _;
use onehand_ui::ShortIcon as _;
use onehand_ui::page_search;
use onehand_ui::{
    AppIcon, ControlSize, action, badge, choice, menu_below, menu_item, neutral_tint, picker, tint,
};
use std::path::PathBuf;

/// How many labels the label filter offers.
const LABELS_SHOWN: usize = 100;

impl IssuesView {
    /// The page's head over the list, its name with *Refresh* and *New
    /// issue* at its end; and the row under it:
    /// the search, open or closed, each with its count after its word, and at
    /// the row's end the project and label filters, wrapping when narrow.
    pub(super) fn page_head(
        &self,
        listed: &Listed<'_>,
        filters: &Filters,
        projects: &[(PathBuf, SharedString)],
        labels: Vec<String>,
        query: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let project = {
            let picked = filters.project.clone();
            let name = picked
                .as_ref()
                .and_then(|only| projects.iter().find(|(root, _)| root == only))
                .map_or_else(
                    || "All projects".to_string(),
                    |(_, label)| label.to_string(),
                );
            let projects = projects.to_vec();
            let view = cx.entity();
            menu_below(
                "issues-page-project",
                picker(
                    "issues-page-project-trigger",
                    name,
                    picked.is_some(),
                    ControlSize::Short,
                    cx,
                ),
                move |menu, _, _| {
                    let pick = |only: Option<PathBuf>| {
                        let view = view.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let only = only.clone();
                            view.update(cx, |view, cx| view.filter(|f| f.project = only, cx))
                        }
                    };
                    let mut menu = menu.item(
                        menu_item("All projects")
                            .checked(picked.is_none())
                            .on_click(pick(None)),
                    );
                    for (root, label) in &projects {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(root))
                                .on_click(pick(Some(root.clone()))),
                        );
                    }
                    menu
                },
            )
        };
        let label = (!labels.is_empty()).then(|| {
            let picked = filters.label.clone();
            let left_out = labels.len().saturating_sub(LABELS_SHOWN);
            let view = cx.entity();
            menu_below(
                "issues-page-label",
                picker(
                    "issues-page-label-trigger",
                    picked.clone().unwrap_or_else(|| "Label".to_string()),
                    picked.is_some(),
                    ControlSize::Short,
                    cx,
                ),
                move |menu, window, _| {
                    let pick = |label: Option<String>| {
                        let view = view.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let label = label.clone();
                            view.update(cx, |view, cx| view.filter(|f| f.label = label, cx))
                        }
                    };
                    let tall = window.rem_size() * 20.;
                    let mut menu = menu.scrollable(true).max_h(tall).item(
                        menu_item("All labels")
                            .checked(picked.is_none())
                            .on_click(pick(None)),
                    );
                    for label in labels.iter().take(LABELS_SHOWN) {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(label))
                                .on_click(pick(Some(label.clone()))),
                        );
                    }
                    if left_out > 0 {
                        menu = menu.item(onehand_ui::menu_label(format!(
                            "… {left_out} more labels not shown"
                        )));
                    }
                    menu
                },
            )
        });
        let reading = listed.open + listed.closed == 0 && self.reading_issues(projects, cx);
        // The page's one primary at rest; what floats over the list (an issue,
        // a form) holds it while it shows, and this steps down under it.
        let covered = self.page.as_ref().is_some_and(|page| page.open);
        // Open or closed, a choice with its count after its word, in a badge
        // in the hue of that state, the one its rows' marks take: open green,
        // closed magenta. The word reads alone; no count before the first read
        // lands, which would say there are none.
        let (open_tint, closed_tint) = (tint(cx.theme().green, cx), tint(cx.theme().magenta, cx));
        let choose =
            |pick: bool, name: &'static str, count: usize, cx: &mut Context<Self>| {
                choice(
                    ("issues-page-showing", usize::from(pick)),
                    name,
                    pick == filters.closed,
                    ControlSize::Short,
                    cx,
                )
                .when(!reading, |button| {
                    // None at all takes no hue: a count of nothing has nothing to
                    // draw the eye to.
                    button.child(badge(
                        count.to_string(),
                        match (count, pick) {
                            (0, _) => neutral_tint(cx),
                            (_, true) => closed_tint,
                            (_, false) => open_tint,
                        },
                    ))
                })
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.filter(|f| f.closed = pick, cx)
                }))
            };
        // The head names the page, its counts being on the open and closed
        // choices under it; the way to read again and *New issue* end it.
        let actions = div()
            .h_flex()
            .items_center()
            .gap_2()
            // Named for assistive technology in a box of its own: the
            // library's button takes a name only from a text label.
            .child(
                div()
                    .id("issues-page-refresh-named")
                    .flex_none()
                    .aria_label("Refresh")
                    .child(
                        action("issues-page-refresh")
                            .short_icon()
                            .quiet_icon(false, cx)
                            .icon(Icon::new(AppIcon::Refresh))
                            .named("Read the issues again")
                            .on_click(
                                cx.listener(|view, _: &ClickEvent, _, cx| view.mark_stale(cx)),
                            ),
                    ),
            )
            .child(
                action("issues-page-new")
                    .short()
                    .when(covered, |button| button.secondary())
                    .when(!covered, |button| button.primary())
                    .icon(Icon::new(IconName::Plus))
                    .label("New issue")
                    .tooltip("In the project filtered to, or the one picked")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.new_issue_on_page(window, cx)
                    })),
            );
        // Anything narrowing the list but the tab, which is what is listed
        // rather than a filter on it.
        let narrowed =
            !filters.query.is_empty() || filters.project.is_some() || filters.label.is_some();
        // Only while a filter is set, after the pickers, so nothing moves as
        // it comes and goes. A plain ghost: undoing a narrowing destroys
        // nothing, so it takes no danger ink.
        let clear =
            narrowed.then(|| {
                action("issues-page-clear-filters")
                    .short()
                    .quiet(cx)
                    .label("Clear filters")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.clear_narrowing(window, cx)
                    }))
            });
        // The search leads the row under the head, then open or closed; the
        // filters keep to the row's end, and the row wraps when narrow.
        let filters = div()
            .h_flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(page_search(query, cx))
            .child(div().flex_1())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(choose(false, "Open", listed.open, cx))
                    .child(choose(true, "Closed", listed.closed, cx)),
            )
            .child(
                // Its own wrap, so a pane too narrow for the pickers in one
                // row stacks them rather than clipping the last.
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(project)
                    .children(label)
                    .children(clear),
            );
        (
            onehand_ui::page_head(
                "Issues",
                Some("The issues of every project in this workspace".into_any_element()),
                vec![actions.into_any_element()],
                cx,
            )
            .into_any_element(),
            filters.into_any_element(),
        )
    }
}
