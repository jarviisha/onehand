//! The page's head — the open count, *Refresh* and *New issue* — the filter
//! bar under it, and what the list cannot vouch for. The page's name is the
//! pane's header's, as every page's is.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Rems, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::issues;
use onehand_core::task::work::list::{Filters, Incomplete, Listed, Progress};
use onehand_plugin_host::Quiet as _;
use onehand_plugin_host::Tall as _;
use onehand_plugin_host::search_input;
use onehand_plugin_host::{
    AppIcon, ControlSize, action, menu_below, menu_item, picker, status_ink,
};
use std::path::PathBuf;

/// How many labels the label filter offers.
const LABELS_SHOWN: usize = 100;

/// The narrowest the search gets before the toolbar wraps: room for a
/// title's few words or a `#number`.
const SEARCH_W: Rems = rems(18.);

impl IssuesView {
    /// The page's head over the list: how many issues are open, *Refresh*
    /// and *New issue* on one row; under it the search as wide as
    /// the row allows and the open/closed, progress, project and label
    /// filters, wrapping when narrow.
    pub(super) fn page_head(
        &self,
        listed: &Listed<'_>,
        filters: &Filters,
        projects: &[(PathBuf, SharedString)],
        labels: Vec<String>,
        query: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reading = listed.open + listed.closed == 0 && self.reading_issues(projects, cx);
        let showing = {
            let closed = filters.closed;
            // The count of open issues heads the page; the menu keeps both.
            let count = |closed: bool| match closed {
                true => format!("Closed {}", listed.closed),
                false => format!("Open {}", listed.open),
            };
            let choices = [count(false), count(true)];
            let view = cx.entity();
            menu_below(
                "issues-page-showing",
                picker(
                    "issues-page-showing-trigger",
                    if closed { "Closed" } else { "Open" },
                    ControlSize::Tall,
                ),
                move |mut menu, _, _| {
                    for (i, choice) in choices.iter().enumerate() {
                        let pick = i == 1;
                        let view = view.clone();
                        menu =
                            menu.item(menu_item(choice.clone()).checked(pick == closed).on_click(
                                move |_, _, cx: &mut App| {
                                    view.update(cx, |view, cx| view.filter(|f| f.closed = pick, cx))
                                },
                            ));
                    }
                    menu
                },
            )
        };
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
                    ControlSize::Tall,
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
                picker("issues-page-project-trigger", name, ControlSize::Tall),
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
                    ControlSize::Tall,
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
        // What waits on the person holds the primary while it shows: an
        // issue or a form floating over the list.
        let floating = self.page.as_ref().is_some_and(|page| page.open) && self.has_page_issue();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(muted)
                    // Nothing until the first read lands: *0 open* over an
                    // empty list would say there are none.
                    .when(!reading, |count| {
                        count.child(format!("{} open", listed.open))
                    }),
            )
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
                            .quiet(cx)
                            .icon(Icon::new(AppIcon::Refresh))
                            .loading(pr_reading)
                            .tooltip(read)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.read_prs(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                action("issues-page-new")
                    .tall()
                    .map(|button| match floating {
                        true => button.secondary(),
                        false => button.primary(),
                    })
                    .label("New issue")
                    .tooltip("In the project filtered to, or the one picked")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.new_issue_on_page(window, cx)
                    })),
            );
        let filters = div()
            .h_flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            // The search takes what the row leaves, down to its floor, where
            // the row wraps.
            .child(
                div()
                    .flex_1()
                    .min_w(SEARCH_W)
                    .child(search_input(query, cx).tall()),
            )
            .child(showing)
            .child(progress)
            .child(project)
            .children(label);
        div()
            .v_flex()
            .gap_4()
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
                        .xsmall()
                        .quiet(cx)
                        .label("Read them")
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
