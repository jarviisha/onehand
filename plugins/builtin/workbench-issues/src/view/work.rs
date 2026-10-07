//! Where an issue's work stands, as the app tells the view of it: the
//! progress and the next action above the body, what the work left below
//! it, and what came before.
//!
//! What to say and what to offer is core's (`onehand_core::task::work`);
//! what is here draws it, and turns a press into the request it names.

use super::detail::Doing;
use super::{EARLIER_SHOWN, IssuesView};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, ParentElement, SharedString, Styled,
    Window, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::PopupMenu;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::LocalIssue;
use onehand_core::task::work::{Act, Around, IssueWork, Next, PrSeen, next_action, pr_named};
use onehand_plugin_host::{Request, action, menu_below, menu_item, status_ink};
use std::path::Path;

/// How many secondary actions get a button of their own after the primary
/// one; the rest are in the ⋯ menu, with what is done to the issue itself.
const SECONDARY_SHOWN: usize = 2;

/// The primary action's place, in rems, kept with nothing in it so the
/// secondary actions do not move into it.
const PRIMARY_MIN_W: f32 = 6.;

/// The width of a fact's name below the body, in rems, so the values line up.
const FACT_NAME_W: f32 = 6.;

/// What `doing` says to do next about `issue`, as core decides it.
pub(super) fn next_for(issue: &LocalIssue, doing: &Doing) -> Next {
    next_action(
        doing.work.as_ref().map(|work| &work.work),
        Around {
            open: issue.open,
            can_start: doing.offered,
            pr: doing.pr,
            session: doing.session.is_some(),
            now: onehand_core::issues::now(),
        },
    )
}

/// Above the body: the latest run's progress in one line, the next action in one
/// more, and the actions. Always these three lines, whatever the state, so
/// the body under it never moves when a step ends.
pub(super) fn progress_view(
    root: &Path,
    issue: &LocalIssue,
    doing: &Doing,
    next: &Next,
    publish_to: Option<&'static str>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let warning = status_ink(cx).warning;
    let work = doing.work.as_ref().map(|work| &work.work);
    let ink = match work {
        _ if next.muted => muted,
        Some(work) if work.group.needs_attention() => warning,
        _ => cx.theme().foreground,
    };
    // The step is drawn whole and the words before it give way: on a narrow
    // dock it is the step that says how far the work has come.
    let progress = div()
        .h_flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_sm()
        .text_color(ink)
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_semibold()
                .child(work.map_or_else(|| "No run recorded".to_string(), |w| w.said.clone())),
        )
        .children(work.and_then(|w| w.step.as_ref()).map(|step| {
            div().flex_none().child(format!(
                "· {} · step {} of {}",
                step.label, step.at, step.of
            ))
        }));
    let said = next.said.clone().unwrap_or_default();
    let ink = if next.muted {
        muted
    } else {
        cx.theme().foreground
    };
    // The run's own sentence keeps its ink on a closed issue; only the note
    // that the issue is closed is quieter.
    let sentence = div()
        .h_flex()
        .gap_1()
        .min_w_0()
        .text_xs()
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(ink)
                // A space keeps the line's height when there is nothing to say.
                .child(if said.is_empty() {
                    "\u{a0}".to_string()
                } else {
                    said
                }),
        )
        .when(next.still_active, |line| {
            line.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child("Issue is closed; this run is still active."),
            )
        });
    div()
        .flex_none()
        .v_flex()
        .gap_1()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(progress)
        // The page names the steps still to come, always on a line of its
        // own so the body under it does not move when one is done.
        .when(doing.full.is_some(), |region| {
            region.child(super::full::rest_line(work, cx))
        })
        .child(sentence)
        .child(actions_row(root, issue, doing, next, publish_to, cx))
        .into_any_element()
}

/// The primary action, then the secondary ones in a place that does not
/// move, then ⋯ for the rest and for the issue itself. With no primary
/// action its place stays empty rather than taken by a secondary one.
fn actions_row(
    root: &Path,
    issue: &LocalIssue,
    doing: &Doing,
    next: &Next,
    publish_to: Option<&'static str>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let mut secondary = next.secondary.clone();
    let shown: Vec<Act> = secondary
        .drain(..secondary.len().min(SECONDARY_SHOWN))
        .collect();
    let primary = next
        .primary
        .map(|act| act_button(("issue-primary", 0), act, number, doing, cx).primary());
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .min_w(gpui::rems(PRIMARY_MIN_W))
                .children(primary),
        )
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .gap_1()
                .children(shown.into_iter().enumerate().map(|(i, act)| {
                    act_button(("issue-secondary", i), act, number, doing, cx).ghost()
                })),
        )
        .child(more_menu(root, issue, doing, secondary, publish_to, cx))
        .into_any_element()
}

fn tooltip(act: Act) -> Option<&'static str> {
    match act {
        Act::OpenWorkingSession => Some("A session is working this issue; show it"),
        Act::WorkHere => Some(WORK_HERE),
        Act::RunWorkflow => Some(
            "Choose a workflow and work the issue with it as a task, on a branch and worktree \
             of its own",
        ),
        Act::Review => Some("Read what waits for approval, and approve it or send it back"),
        Act::OpenBranch => Some("Show the folder the branch is checked out in"),
        Act::Retry => Some("Run it again in a new run, as the last run was configured"),
        Act::AnswerReview => Some(
            "Run the task again from its repair step with the review as its note, as putting \
             the label back does",
        ),
        Act::RetryCurrent => Some(
            "Run it again with what Settings say now, saying first what changes and where it \
             starts",
        ),
        Act::Resume => Some("Carry on from that step in a new session"),
        Act::RemoveWorktree => {
            Some("Remove the worktree and its branch, once nothing in them is past the merge")
        }
        Act::Refresh => Some("Read the pull request again"),
        Act::Edit
        | Act::OpenSession
        | Act::AnswerInSession
        | Act::Stop
        | Act::ShowTask
        | Act::OpenPullRequest
        | Act::ReopenIssue => None,
    }
}

/// What *Work here* does, said wherever it is offered so it is not taken for
/// a second way to run a workflow.
const WORK_HERE: &str = "Open a session on this checkout as it is, with the issue as its prompt \
                         and no workflow";

fn act_button(
    id: (&'static str, usize),
    act: Act,
    number: u64,
    doing: &Doing,
    cx: &mut Context<IssuesView>,
) -> Button {
    let pressed = Pressed::of(act, number, doing);
    let button = action(id).xsmall().label(act.label());
    let button = match tooltip(act) {
        Some(tip) => button.tooltip(tip),
        None => button,
    };
    button.on_click(
        cx.listener(move |view, _: &ClickEvent, window, cx| pressed.clone().run(view, window, cx)),
    )
}

/// A press of an action, holding what it needs from the work it was drawn
/// for, so it acts on that work even if another took its place meanwhile.
#[derive(Clone)]
struct Pressed {
    act: Act,
    number: u64,
    task: Option<String>,
    session: Option<String>,
    pr_url: Option<String>,
    dir: Option<std::path::PathBuf>,
}

impl Pressed {
    fn of(act: Act, number: u64, doing: &Doing) -> Self {
        let work = doing.work.as_ref().map(|work| &work.work);
        Self {
            act,
            number,
            task: work.map(|work| work.task.clone()),
            session: doing.session.clone(),
            pr_url: match doing.pr {
                PrSeen::Read(Some(pr)) => Some(pr.url.clone()),
                PrSeen::Read(None) | PrSeen::Unread | PrSeen::Failed(_) => None,
            },
            dir: work.map(|work| work.dir.clone()),
        }
    }

    fn run(self, view: &mut IssuesView, window: &mut Window, cx: &mut Context<IssuesView>) {
        let number = self.number;
        let task = self.task;
        match self.act {
            Act::RunWorkflow => view.run_workflow(number, window, cx),
            Act::WorkHere => view.work_here(number, window, cx),
            Act::OpenWorkingSession => {
                if let Some(session) = self.session {
                    view.open_session(session, window, cx);
                }
            }
            Act::Edit => view.open_form(Some(number), window, cx),
            Act::OpenSession | Act::AnswerInSession => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::OpenTaskSession(id), window, cx);
                }
            }
            Act::Review => view.review(number, window, cx),
            Act::Stop => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::StopTask(id), window, cx);
                }
            }
            Act::ShowTask => {
                if let Some(id) = task {
                    view.open_task(id, window, cx);
                }
            }
            Act::Resume => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::ResumeTask(id), window, cx);
                }
            }
            Act::Retry => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::RetryTask(id), window, cx);
                }
            }
            Act::RetryCurrent => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::RetryTaskCurrent(id), window, cx);
                }
            }
            Act::AnswerReview => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::AnswerReview(id), window, cx);
                }
            }
            Act::RemoveWorktree => {
                if let Some(id) = task {
                    view.ask_task(id, |id| Request::RemoveWorktree(id), window, cx);
                }
            }
            Act::OpenPullRequest => match self.pr_url {
                Some(url) => cx.open_url(&url),
                None => view.refresh(cx),
            },
            Act::OpenBranch => {
                if let Some(dir) = self.dir {
                    cx.reveal_path(&dir);
                }
            }
            Act::Refresh => view.refresh(cx),
            Act::ReopenIssue => view.set_open(number, true, cx),
        }
    }
}

/// The ⋯ menu: the secondary actions that did not get a button, publishing,
/// where the issue lives on its forge, and closing it. Closing is here
/// rather than a button of its own because a bare *Close* beside an issue
/// reads as closing the view.
fn more_menu(
    root: &Path,
    issue: &LocalIssue,
    doing: &Doing,
    overflow: Vec<Act>,
    publish_to: Option<&'static str>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let forge = issue.link.as_ref().map(|link| link.connector.clone());
    let view = cx.entity();
    let overflow: Vec<(&'static str, Pressed)> = overflow
        .into_iter()
        .map(|act| {
            // A menu row has no tooltip: *Work here* says what it does in
            // its own words.
            let said = match act {
                Act::WorkHere => "Work here: a session on this checkout, no workflow",
                act => act.label(),
            };
            (said, Pressed::of(act, number, doing))
        })
        .collect();
    // The tab leads to the page, where there is room to work the issue.
    let in_tab = doing.full.is_none();
    let trigger = action("issue-more")
        .xsmall()
        .ghost()
        .icon(Icon::new(IconName::Ellipsis))
        .tooltip("More actions");
    // Named by the project and the issue, so a menu held open across a switch
    // is one for this issue and never for whichever took its place.
    let id = SharedString::from(format!("issue-more-menu-{}-{number}", root.display()));
    div()
        .flex_none()
        .child(menu_below(id, trigger, move |menu, _, _| {
            let mut menu = menu;
            for (said, pressed) in &overflow {
                let (view, pressed) = (view.clone(), pressed.clone());
                menu = menu.item(menu_item(*said).on_click(move |_, window, cx: &mut App| {
                    let pressed = pressed.clone();
                    view.update(cx, |view, cx| pressed.run(view, window, cx))
                }));
            }
            if let Some(forge) = publish_to {
                let view = view.clone();
                menu = menu.item(
                    menu_item(format!(
                        "Publish to {forge}: open it there and keep the two in step"
                    ))
                    .on_click(move |_, _, cx: &mut App| {
                        view.update(cx, |view, cx| view.publish(number, cx))
                    }),
                );
            }
            if in_tab {
                let view = view.clone();
                menu = menu.item(
                    menu_item("Open in Issues")
                        .icon(Icon::new(IconName::Maximize))
                        .on_click(move |_, window, cx: &mut App| {
                            view.update(cx, |view, cx| view.open_in_issues(number, window, cx))
                        }),
                );
            }
            if !overflow.is_empty() || publish_to.is_some() || in_tab {
                menu = menu.separator();
            }
            issue_items(menu, &view, number, open, forge.as_deref())
        }))
        .into_any_element()
}

/// What is done to the issue itself, at the end of the ⋯ menu.
fn issue_items(
    mut menu: PopupMenu,
    view: &Entity<IssuesView>,
    number: u64,
    open: bool,
    forge: Option<&str>,
) -> PopupMenu {
    if let Some(forge) = forge {
        let (open_view, copy_view) = (view.clone(), view.clone());
        menu = menu
            .item(
                menu_item(format!("Open on {forge}"))
                    .icon(Icon::new(IconName::ExternalLink))
                    .on_click(move |_, _, cx: &mut App| {
                        open_view.update(cx, |view, cx| {
                            view.with_url(number, |url, cx| cx.open_url(&url), cx)
                        })
                    }),
            )
            .item(
                menu_item("Copy link")
                    .icon(Icon::new(IconName::Copy))
                    .on_click(move |_, _, cx: &mut App| {
                        copy_view.update(cx, |view, cx| {
                            view.with_url(
                                number,
                                |url, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(url))
                                },
                                cx,
                            )
                        })
                    }),
            )
            .separator();
    }
    let view = view.clone();
    // Reopening is a secondary action of a closed issue's work instead.
    if open {
        menu = menu.item(
            menu_item("Close issue")
                .icon(Icon::new(IconName::CircleX))
                .on_click(move |_, window, cx: &mut App| {
                    view.update(cx, |view, cx| view.confirm_close(number, window, cx))
                }),
        );
    }
    menu
}

/// Below the body, short: the branch the work is on and, on a project a forge
/// serves once the run has reached its pull request step, the pull
/// request's state, how old that reading is and *Refresh*. `None` with no
/// branch to say.
pub(super) fn left_view(doing: &Doing, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    let work = &doing.work.as_ref()?.work;
    let branch = work.branch.clone()?;
    let muted = cx.theme().muted_foreground;
    let warning = status_ink(cx).warning;
    let has_pr = work.has_pull_request();
    let now = onehand_core::issues::now();
    let line = move |name: &'static str, value: AnyElement| {
        div()
            .h_flex()
            .items_start()
            .gap_2()
            .min_w_0()
            .text_xs()
            .child(
                div()
                    .flex_none()
                    .w(gpui::rems(FACT_NAME_W))
                    .text_color(muted)
                    .child(name),
            )
            .child(div().flex_1().min_w_0().child(value))
            .into_any_element()
    };
    let full = doing
        .full
        .as_ref()
        .map(|full| super::full::left_lines(full, work, &line, cx))
        .unwrap_or_default();
    let on_page = doing.full.is_some();
    let pr_value = has_pr.then(|| match doing.pr {
        PrSeen::Unread => div().text_color(muted).child("reading…").into_any_element(),
        PrSeen::Read(None) => div()
            .text_color(muted)
            .child("none for this branch")
            .into_any_element(),
        PrSeen::Read(Some(pr)) => div().truncate().child(pr_named(pr)).into_any_element(),
        // The last value stays, marked stale, with what failed beside it:
        // silence is never taken for an answer.
        PrSeen::Failed(why) => div()
            .truncate()
            .text_color(warning)
            .child(match doing.last.and_then(|(pr, _)| pr.as_ref()) {
                Some(stale) => format!("{} (stale) · could not be read: {why}", pr_named(stale)),
                None => format!("could not be read: {why}"),
            })
            .into_any_element(),
    });
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(div().flex_1().text_color(muted).child("What the work left"))
                    .when(has_pr || on_page, |head| {
                        head.children(doing.last.map(|(_, at)| {
                            div()
                                .text_color(muted)
                                .child(format!("read {}", onehand_core::rel_time(now, *at)))
                        }))
                        .child(
                            action("issue-pr-refresh")
                                .xsmall()
                                .ghost()
                                .label("Refresh")
                                .tooltip(match on_page {
                                    true => "Read what the work left again",
                                    false => "Read the pull request again",
                                })
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.refresh(cx);
                                    let work = view.shown_work();
                                    view.refresh_left(work.as_ref(), cx);
                                })),
                        )
                    }),
            )
            .child(line(
                "Branch",
                div()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(branch)
                    .into_any_element(),
            ))
            .children(pr_value.map(|value| line("Pull request", value)))
            .children(full)
            .into_any_element(),
    )
}

/// Last before the history: one line counting the earlier runs of the issue's task, and
/// one line per earlier task, newest first, any needing attention in the
/// warning ink. Each leads to its task. `None` when nothing came before.
pub(super) fn before_view(work: &IssueWork, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    if work.earlier_runs == 0 && work.earlier.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let warning = status_ink(cx).warning;
    let left_out = work.earlier.len().saturating_sub(EARLIER_SHOWN);
    let row = |key: (&'static str, usize),
               text: String,
               ink,
               task: String,
               cx: &mut Context<IssuesView>| {
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
                    .child(text),
            )
            .child(
                action(key)
                    .xsmall()
                    .ghost()
                    .label("Show task")
                    .tooltip("Open the task on the Tasks page")
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.open_task(task.clone(), window, cx)
                    })),
            )
    };
    let runs = (work.earlier_runs > 0).then(|| {
        let text = match work.earlier_runs {
            1 => "1 earlier run of this task".to_string(),
            n => format!("{n} earlier runs of this task"),
        };
        row(
            ("issue-earlier-runs", 0),
            text,
            muted,
            work.work.task.clone(),
            cx,
        )
    });
    let tasks: Vec<_> = work
        .earlier
        .iter()
        .take(EARLIER_SHOWN)
        .enumerate()
        .map(|(i, earlier)| {
            let ink = if earlier.attention { warning } else { muted };
            let text = match earlier.attention {
                true => format!("Earlier task needs attention: {}", earlier.line),
                false => format!("Earlier task: {}", earlier.line),
            };
            row(
                ("issue-earlier-task", i),
                text,
                ink,
                earlier.task.clone(),
                cx,
            )
        })
        .collect();
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(div().text_xs().text_color(muted).child("Before"))
            .children(runs)
            .children(tasks)
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
