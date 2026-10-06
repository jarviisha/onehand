//! The head of the page's list: the open/closed switch, the progress,
//! project and label filters, how old the pull request reading is, and what
//! the list cannot vouch for.

use super::IssuesView;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, Size, StyledExt};
use onehand_core::issues;
use onehand_core::task::work::list::{Filters, Incomplete, Listed, Progress};
use onehand_plugin_host::{action, menu_below, menu_item, status_ink, switch};
use std::path::PathBuf;

/// How many labels the label filter offers.
const LABELS_SHOWN: usize = 100;

impl IssuesView {
    /// The filters: open or closed, progress, project and label, and how
    /// old the pull request reading is.
    pub(super) fn page_head(
        &self,
        listed: &Listed<'_>,
        filters: &Filters,
        projects: &[(PathBuf, SharedString)],
        labels: Vec<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity();
        let showing = switch(
            "issues-page-showing",
            &[
                SharedString::from(format!("Open {}", listed.open)),
                SharedString::from(format!("Closed {}", listed.closed)),
            ],
            usize::from(filters.closed),
            Size::XSmall,
            move |picked, _, cx| {
                let closed = *picked == 1;
                view.update(cx, |view, cx| view.filter(|f| f.closed = closed, cx))
            },
            cx,
        );
        let progress = {
            let picked = filters.progress;
            let view = cx.entity();
            menu_below(
                "issues-page-progress",
                trigger("issues-page-progress-trigger", picked.label()),
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
                trigger("issues-page-project-trigger", &name),
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
                trigger(
                    "issues-page-label-trigger",
                    picked.as_deref().unwrap_or("Label"),
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
        let read = self.prs_read_at().map(|at| {
            div()
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "read {}",
                    onehand_core::rel_time(issues::now(), at)
                ))
        });
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(showing)
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(progress)
                    .child(project)
                    .children(label)
                    .child(div().flex_1())
                    .children(read)
                    .child(
                        action("issues-page-refresh")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Redo))
                            .tooltip("Read the pull requests again")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.read_prs(cx);
                                cx.notify();
                            })),
                    ),
            )
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
                        .ghost()
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
                .px_2()
                .py_1()
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

/// A filter's trigger.
fn trigger(id: &'static str, label: &str) -> gpui_component::button::Button {
    action(id)
        .xsmall()
        .ghost()
        .label(label.to_string())
        .icon(Icon::new(IconName::ChevronDown))
}
