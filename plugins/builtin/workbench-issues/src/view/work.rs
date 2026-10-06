//! What is being done with an issue: the tasks working it, as the app tells
//! the view of them.

use super::{IssuesView, RUNS_SHOWN};
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_plugin_host::{IssueRun, action, status_ink};

/// The tasks working the issue, working ones first: each its workflow and the
/// step it is at or how it ended, the ones waiting on a person in the warning
/// ink, with a way to the task. Not drawn when there are none.
pub(super) fn runs_view(runs: Vec<IssueRun>, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    if runs.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let warning = status_ink(cx).warning;
    let left_out = runs.len().saturating_sub(RUNS_SHOWN);
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(div().text_xs().text_color(muted).child("Runs"))
            .children(
                runs.into_iter()
                    .take(RUNS_SHOWN)
                    .enumerate()
                    .map(|(i, run)| {
                        let ink = match (run.waiting, run.working) {
                            (true, _) => warning,
                            (false, true) => cx.theme().foreground,
                            (false, false) => muted,
                        };
                        let task = run.task;
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(ink)
                                    .child(format!("{} · {}", run.workflow, run.at)),
                            )
                            .child(
                                action(("issue-run-task", i))
                                    .xsmall()
                                    .ghost()
                                    .label("Show task")
                                    .tooltip("Open the task on the Tasks page")
                                    .on_click(cx.listener(
                                        move |view, _: &ClickEvent, window, cx| {
                                            view.open_task(task.clone(), window, cx)
                                        },
                                    )),
                            )
                    }),
            )
            .when(left_out > 0, |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("… {left_out} more on the Tasks page")),
                )
            })
            .into_any_element(),
    )
}
