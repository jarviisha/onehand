//! The page's head — the open and closed choices, *Refresh* and *New issue*
//! — the filter bar under it, and what the list cannot vouch for. The page's
//! name is the pane's header's, as every page's is.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Rems, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::issues;
use onehand_core::task::work::list::{Filters, Incomplete, Listed, Progress};
use onehand_ui::Named as _;
use onehand_ui::Quiet as _;
use onehand_ui::Short as _;
use onehand_ui::Tall as _;
use onehand_ui::search_input;
use onehand_ui::{
    AppIcon, ControlSize, action, badge, choice, menu_below, menu_item, picker, status_ink, tint,
};
use std::path::PathBuf;

/// How many labels the label filter offers.
const LABELS_SHOWN: usize = 100;

/// The narrowest the search gets before the toolbar wraps: room for a
/// title's few words or a `#number`.
const SEARCH_W: Rems = rems(18.);

/// The widest the search gets: past it the row's free width opens between the
/// search and the filters, which keep to the row's end.
const SEARCH_MAX: Rems = rems(28.);

impl IssuesView {
    /// The page's head over the list: the search, then *Refresh* and *New
    /// issue* at the row's end; under it, at a form's height, open or closed,
    /// each with its count on its corner, and at the row's end the progress,
    /// project and label filters, wrapping when narrow.
    pub(super) fn page_head(
        &self,
        listed: &Listed<'_>,
        filters: &Filters,
        projects: &[(PathBuf, SharedString)],
        labels: Vec<String>,
        query: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let progress = {
            let picked = filters.progress;
            let view = cx.entity();
            menu_below(
                "issues-page-progress",
                // Unset, the trigger names what it filters by.
                picker(
                    "issues-page-progress-trigger",
                    match picked {
                        Progress::All => "Progress",
                        picked => picked.label(),
                    },
                    picked != Progress::All,
                    ControlSize::Short,
                    cx,
                ),
                move |mut menu, _, _| {
                    for choice in Progress::ALL {
                        let view = view.clone();
                        menu = menu.item(
                            menu_item(choice.label())
                                .checked(choice == picked)
                                .on_click(move |_, _, cx: &mut App| {
                                    view.update(cx, |view, cx| {
                                        view.filter(|f| f.progress = choice, cx)
                                    })
                                }),
                        );
                    }
                    menu
                },
            )
        };
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
                        menu = menu.label(format!("… {left_out} more labels not shown"));
                    }
                    menu
                },
            )
        });
        // The age of the pull request reading rides in the tooltip, so the
        // button says what it does and when it last did it.
        let read = self.prs_read_at().map_or_else(
            || "Refresh".to_string(),
            |at| {
                format!(
                    "Refresh (pull requests read {})",
                    onehand_core::rel_time(issues::now(), at)
                )
            },
        );
        let pr_reading = self.page.as_ref().is_some_and(|page| page.pr_reading > 0);
        let reading = listed.open + listed.closed == 0 && self.reading_issues(projects, cx);
        // The page's one primary at rest; what floats over the list (an issue,
        // a form) holds it while it shows, and this steps down under it.
        let covered = self.page.as_ref().is_some_and(|page| page.open);
        // Open or closed, a choice with its count pinned to its corner in a
        // badge in the hue of that state, the one its rows' marks take: open
        // green, closed magenta. The word reads alone; no count before the
        // first read lands, which would say there are none.
        let (open_tint, closed_tint) = (tint(cx.theme().green, cx), tint(cx.theme().magenta, cx));
        let choose = |pick: bool, name: &'static str, count: usize, cx: &mut Context<Self>| {
            let button =
                choice(
                    ("issues-page-showing", usize::from(pick)),
                    name,
                    pick == filters.closed,
                    ControlSize::Short,
                    cx,
                )
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.filter(|f| f.closed = pick, cx)
                }));
            div()
                .relative()
                .flex_none()
                .child(button)
                .when(!reading, |slot| {
                    // Hung from a point on the button's top-right corner,
                    // its lower-left corner `2` in from that point each way:
                    // over the button's inset and above its words, never on
                    // them. Insets from a point rather than margins, so where
                    // it lands does not hang on how margins meet a box taken
                    // out of the flow.
                    slot.child(
                        div().absolute().top_0().right_0().size_0().child(
                            div().absolute().left_neg_2().bottom_neg_2().child(
                                // A notch slimmer than a badge in a row, as a
                                // count on a corner is.
                                badge(
                                    count.to_string(),
                                    if pick { closed_tint } else { open_tint },
                                )
                                .py_0p5(),
                            ),
                        ),
                    )
                })
        };
        let well = cx.theme().muted;
        // The search leads the head at its height, growing up to its widest;
        // the way to read again and *New issue* on the secondary fill end the
        // row. The page holds no primary, so what floats over the list never
        // has one to outweigh.
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_basis(SEARCH_MAX)
                    .min_w(SEARCH_W)
                    .max_w(SEARCH_MAX)
                    // No edge: the well's fill marks it off the page, and an
                    // edge in that fill is left for focus to turn the ring ink.
                    .child(search_input(query, cx).tall().bg(well).border_color(well)),
            )
            // The search keeps its widest and gives way only when the row
            // runs short; what it leaves falls here, before the actions.
            .child(div().flex_1())
            // Named for assistive technology in a box of its own: the
            // library's button takes a name only from a text label.
            .child(
                div()
                    .id("issues-page-refresh-named")
                    .flex_none()
                    .aria_label("Refresh")
                    .child(
                        action("issues-page-refresh")
                            .tall()
                            .quiet_icon(false, cx)
                            .icon(Icon::new(AppIcon::Refresh))
                            .loading(pr_reading)
                            .named(read)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.read_prs(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                action("issues-page-new")
                    .tall()
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
        let narrowed = !filters.query.is_empty()
            || filters.progress != Progress::All
            || filters.project.is_some()
            || filters.label.is_some();
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
        // Open or closed leads the row under it, the choices `gap_6` apart so
        // a count off one's corner keeps clear of the next; the filters keep
        // to the row's end, and wrap under the choices when narrow.
        let filters = div()
            .h_flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_6()
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
                    .child(progress)
                    .child(project)
                    .children(label)
                    .children(clear),
            );
        // Apart by a page section's gap rather than a row's: the search and
        // the way to start an issue are one group, what narrows the list
        // another, and a count over a choice's corner needs the room.
        div()
            .v_flex()
            .gap_6()
            .child(header)
            .child(filters)
            .into_any_element()
    }

    /// What the list cannot vouch for: the projects whose pull requests
    /// could not be read, and under *Pull request open* the issues not read.
    pub(super) fn page_notices(
        &self,
        listed: &Listed<'_>,
        projects: &[(PathBuf, SharedString)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let warning = status_ink(cx).warning;
        let reading = self.page.as_ref().is_some_and(|page| page.pr_reading > 0);
        let failed: Vec<String> = listed
            .failed
            .iter()
            .filter_map(|root| projects.iter().find(|(r, _)| r == root))
            .map(|(_, label)| label.to_string())
            .collect();
        if failed.is_empty() && listed.incomplete.is_none() {
            return None;
        }
        // A stale reading or a project whose read failed is read again
        // whole; otherwise the branches a capped read missed are looked up.
        let again = !listed.failed.is_empty();
        let incomplete = listed.incomplete.map(|Incomplete { not_read, stale }| {
            let said = match not_read {
                0 => "The reading is old; the list may be incomplete".to_string(),
                1 => "1 issue not read; the list may be incomplete".to_string(),
                n => format!("{n} issues not read; the list may be incomplete"),
            };
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .child(div().flex_1().min_w_0().text_color(warning).child(said))
                .child(
                    action("issues-page-read-them")
                        .short()
                        .quiet(cx)
                        .label("Read them")
                        .loading(reading)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            if stale || again {
                                view.read_prs(cx);
                            } else {
                                view.read_missing(cx);
                            }
                            cx.notify();
                        })),
                )
        });
        Some(
            div()
                .flex_none()
                .v_flex()
                .gap_0p5()
                .text_xs()
                .when(!failed.is_empty(), |notes| {
                    notes.child(div().text_color(warning).child(format!(
                        "Pull requests could not be read for {}",
                        failed.join(", ")
                    )))
                })
                .children(incomplete)
                .into_any_element(),
        )
    }
}
