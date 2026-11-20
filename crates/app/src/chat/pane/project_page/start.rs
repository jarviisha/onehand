//! The project page's *New session*: the default agent in the project's own
//! folder, and from its caret another agent or one of its worktrees.

use super::{ChatPane, ChatPaneEvent};
use crate::chat::pane::WorktreeAct;
use crate::controls::Named as _;
use crate::controls::Short as _;
use gpui::{App, Context, IntoElement, SharedString};
use gpui_component::ActiveTheme as _;
use gpui_component::button::ButtonVariants as _;
use onehand_ui::ShortIcon as _;
use std::path::PathBuf;

/// *New session*, the page's one primary, joined to a caret starting one with
/// another agent while more than one is configured, or in one of the
/// project's worktrees, `in_branch`, each by its folder and branch.
pub(super) fn new_session(
    in_branch: Vec<(PathBuf, String)>,
    cx: &mut Context<ChatPane>,
) -> gpui::AnyElement {
    let agents: Vec<SharedString> = crate::state::Shared::global(cx)
        .settings
        .agents
        .iter()
        .map(|spec| SharedString::from(spec.name.clone()))
        .collect();
    // At the head's size, as *Run check* beside it is: one row of actions
    // stands at one height.
    let start = crate::controls::action("project-new-session")
        .short()
        .primary()
        .label("New session")
        .tooltip_with_action(
            "Start a session in this project",
            &crate::shell::NewSession,
            None,
        )
        .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
            cx.emit(ChatPaneEvent::StartSession {
                agent: None,
                resume: None,
            });
        }));
    if agents.len() < 2 && in_branch.is_empty() {
        return start.into_any_element();
    }
    let pane = cx.entity();
    crate::controls::split_action(
        "project-new-session-menu",
        start,
        // Square at the head's height: an icon alone under a worded size's
        // padding was left a couple of pixels and drew as a dot.
        crate::controls::action("project-new-session-agent")
            .primary()
            .short_icon()
            .named("Start a session with another agent, or in a worktree"),
        cx.theme().background,
        gpui::Anchor::TopRight,
        move |mut menu, _, _| {
            if !in_branch.is_empty() {
                menu = menu.item(crate::controls::menu_label("Start in"));
                for (folder, branch) in &in_branch {
                    let (folder, pane) = (folder.clone(), pane.clone());
                    menu = menu.item(crate::controls::menu_item(format!("In {branch}")).on_click(
                        move |_, _, cx: &mut App| {
                            pane.update(cx, |_: &mut ChatPane, cx| {
                                cx.emit(ChatPaneEvent::Worktree(WorktreeAct::NewSession(
                                    folder.clone(),
                                )));
                            });
                        },
                    ));
                }
            }
            if agents.len() < 2 {
                return menu;
            }
            menu = menu.item(crate::controls::menu_label("With agent"));
            for name in &agents {
                let (name, pane) = (name.clone(), pane.clone());
                menu = menu.item(crate::controls::menu_item(name.clone()).on_click(
                    move |_, _, cx: &mut App| {
                        pane.update(cx, |_: &mut ChatPane, cx| {
                            cx.emit(ChatPaneEvent::StartSession {
                                agent: Some(name.clone()),
                                resume: None,
                            });
                        });
                    },
                ));
            }
            menu
        },
    )
    .into_any_element()
}
