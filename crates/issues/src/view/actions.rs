//! What can be done with an issue, at the end of its trail: a session on
//! it, and in ⋯ what is done to the issue itself.
//!
//! What to offer is core's (`onehand_core::task::work::next_action`); what is
//! here draws it, and turns a press into the request it names.

use super::IssuesView;
use super::detail::Doing;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, ParentElement, SharedString, Styled,
    Window, div,
};
use gpui_component::button::Button;
use gpui_component::menu::PopupMenu;
use gpui_component::{Icon, IconName, StyledExt};
use onehand_core::issues::LocalIssue;
use onehand_core::task::work::{Act, Around, Next, PrSeen, next_action};
use onehand_ui::Named as _;
use onehand_ui::Quiet as _;
use onehand_ui::Short as _;
use onehand_ui::ShortIcon as _;
use onehand_ui::{action, menu_below, menu_item};
use std::path::Path;

/// How many actions get a button of their own; the rest are in the ⋯ menu,
/// with what is done to the issue itself. One, because the actions share the
/// title's row.
const SHOWN: usize = 1;

/// What to offer about `issue`, as core decides it: no task works an issue,
/// so only a session on it and the issue itself.
pub(super) fn next_for(issue: &LocalIssue, doing: &Doing) -> Next {
    next_action(
        None,
        Around {
            open: issue.open(),
            pr: PrSeen::Unread,
            session: doing.session.is_some(),
            now: onehand_core::issues::now(),
        },
    )
}

/// The actions, at the end of the title's row: the first that has a button,
/// then ⋯ for the rest, *Edit* among them, and for the issue itself.
pub(super) fn actions(
    root: &Path,
    issue: &LocalIssue,
    doing: &Doing,
    next: &Next,
    publish_to: Option<&'static str>,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number();
    let mut rest: Vec<Act> = next
        .primary
        .into_iter()
        .chain(next.secondary.clone())
        .collect();
    // *Edit* lives in ⋯, beside what is done to the issue.
    let shown: Vec<Act> = rest
        .iter()
        .copied()
        .filter(|act| *act != Act::Edit)
        .take(SHOWN)
        .collect();
    rest.retain(|act| !shown.contains(act));
    div()
        .h_flex()
        .flex_none()
        .items_center()
        .gap_2()
        .children(
            shown
                .into_iter()
                .enumerate()
                .map(|(i, act)| act_button(("issue-action", i), act, number, doing, cx).quiet(cx)),
        )
        .child(more_menu(root, issue, doing, rest, publish_to, cx))
        .into_any_element()
}

fn tooltip(act: Act) -> Option<&'static str> {
    match act {
        Act::OpenWorkingSession => Some("A session is working this issue; show it"),
        Act::WorkHere => Some(WORK_HERE),
        Act::Edit | Act::ReopenIssue => None,
        // Never offered here: no task works an issue.
        Act::OpenSession
        | Act::AnswerInSession
        | Act::Review
        | Act::Extend
        | Act::Stop
        | Act::ShowTask
        | Act::Resume
        | Act::Retry
        | Act::RetryCurrent
        | Act::RemoveWorktree
        | Act::OpenPullRequest
        | Act::OpenBranch
        | Act::Refresh => None,
    }
}

/// What *Work here* does, said wherever it is offered.
const WORK_HERE: &str = "Open a session on this checkout as it is, with the issue as its prompt";

fn act_button(
    id: (&'static str, usize),
    act: Act,
    number: u64,
    doing: &Doing,
    cx: &mut Context<IssuesView>,
) -> Button {
    let pressed = Pressed::of(act, number, doing);
    let button = action(id).short().label(act.label());
    let button = match tooltip(act) {
        Some(tip) => button.tooltip(tip),
        None => button,
    };
    button.on_click(
        cx.listener(move |view, _: &ClickEvent, window, cx| pressed.clone().run(view, window, cx)),
    )
}

/// A press of an action, holding the session it was drawn for, so it acts
/// on that one even if another took its place meanwhile.
#[derive(Clone)]
struct Pressed {
    act: Act,
    number: u64,
    session: Option<String>,
}

impl Pressed {
    fn of(act: Act, number: u64, doing: &Doing) -> Self {
        Self {
            act,
            number,
            session: doing.session.clone(),
        }
    }

    fn run(self, view: &mut IssuesView, window: &mut Window, cx: &mut Context<IssuesView>) {
        let number = self.number;
        match self.act {
            Act::WorkHere => view.work_here(number, window, cx),
            Act::OpenWorkingSession => {
                if let Some(session) = self.session {
                    view.open_session(session, window, cx);
                }
            }
            Act::Edit => view.open_form(Some(number), window, cx),
            Act::ReopenIssue => view.set_open(number, true, cx),
            // Never offered here: no task works an issue.
            Act::OpenSession
            | Act::AnswerInSession
            | Act::Review
            | Act::Extend
            | Act::Stop
            | Act::ShowTask
            | Act::Resume
            | Act::Retry
            | Act::RetryCurrent
            | Act::RemoveWorktree
            | Act::OpenPullRequest
            | Act::OpenBranch
            | Act::Refresh => {}
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
    let number = issue.number();
    let open = issue.open();
    let forge = issue.link().map(|link| link.connector.clone());
    let view = cx.entity();
    let overflow: Vec<(&'static str, Pressed)> = overflow
        .into_iter()
        .map(|act| {
            // A menu row has no tooltip: *Work here* says what it does in
            // its own words.
            let said = match act {
                Act::WorkHere => "Work here: a session on this checkout",
                act => act.label(),
            };
            (said, Pressed::of(act, number, doing))
        })
        .collect();
    // The tab leads to the page, where there is room to read the issue.
    let in_tab = !doing.page;
    let trigger = action("issue-more")
        .short_icon()
        .quiet_icon(false, cx)
        .loading(doing.publishing)
        .icon(Icon::new(IconName::Ellipsis))
        .named("More actions");
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
    // Reopening is an action of a closed issue instead. A plain word:
    // closing can be taken back, so it neither warns nor asks.
    if open {
        menu = menu.item(
            menu_item("Close issue").on_click(move |_, _, cx: &mut App| {
                view.update(cx, |view, cx| view.set_open(number, false, cx))
            }),
        );
    }
    menu
}
