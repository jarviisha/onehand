//! The Board group of the Projects page: the workflow a project's board runs
//! and how many tasks work at once.

use super::section;
use crate::shell::Shell;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::StyledExt;
use gpui_component::button::ButtonVariants as _;
use onehand_ui::{ControlSize, Tall as _};
use std::path::Path;

const NO_BOARD: &str = "No board";

pub(super) fn board_section(handle: &Entity<Shell>, root: &Path, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let project = crate::projects::of(root, cx);
    let bound = project.as_ref().and_then(|project| {
        crate::state::Shared::global(cx)
            .boards
            .board(&project.id)
            .cloned()
    });
    let templates = crate::workflow::templates(cx);
    let name_of = |id: &str| {
        crate::workflow::find(&templates, id)
            .map_or_else(|| format!("{id} (gone)"), |t| t.name.clone())
    };
    let label = bound
        .as_ref()
        .map_or_else(|| NO_BOARD.to_string(), |board| name_of(board.workflow()));
    let offered: Vec<(String, String, bool)> = templates
        .iter()
        .filter_map(|entry| entry.template.as_ref().ok())
        .map(|t| {
            let on = bound.as_ref().is_some_and(|b| b.workflow() == t.id);
            (t.id.clone(), t.name.clone(), on)
        })
        .collect();
    let more = offered
        .len()
        .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let none_on = bound.is_none();
    let picker = {
        let (handle, root) = (handle.clone(), root.to_path_buf());
        crate::controls::menu_below(
            "board-workflow",
            onehand_ui::picker(
                "board-workflow-trigger",
                label,
                bound.is_some(),
                ControlSize::Tall,
                cx,
            ),
            move |mut menu, _, _| {
                let pick = {
                    let (handle, root) = (handle.clone(), root.clone());
                    move |workflow: Option<String>| {
                        let (handle, root) = (handle.clone(), root.clone());
                        move |_: &_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| {
                                shell.bind_board(root.clone(), workflow.clone(), window, cx);
                            });
                        }
                    }
                };
                menu = menu.item(
                    crate::controls::menu_item(NO_BOARD)
                        .checked(none_on)
                        .on_click(pick(None)),
                );
                for (id, name, on) in offered.iter().take(crate::workflow::TEMPLATES_SHOWN) {
                    menu = menu.item(
                        crate::controls::menu_item(name.clone())
                            .checked(*on)
                            .on_click(pick(Some(id.clone()))),
                    );
                }
                if more > 0 {
                    menu = menu.label(SharedString::from(format!("{more} more not shown")));
                }
                menu
            },
        )
    };
    // A workflow that is gone has nothing to open.
    let edit = bound
        .as_ref()
        .filter(|board| crate::workflow::find(&templates, board.workflow()).is_some())
        .zip(project.as_ref())
        .map(|(board, project)| {
            let (handle, workflow, id) = (
                handle.clone(),
                board.workflow().to_string(),
                project.id.clone(),
            );
            crate::controls::action("board-edit-workflow")
                .tall()
                .secondary()
                .label("Edit workflow")
                .on_click(move |_, window, cx| {
                    handle.update(cx, |shell, cx| {
                        shell.show_workflow(&workflow, None, Some(id.clone()), window, cx)
                    });
                })
        });
    // A board this build cannot read is left as it is; why stands under the
    // menu until a bind is refused for a reason of its own.
    let unreadable = (project.as_ref())
        .and_then(|project| {
            crate::state::Shared::global(cx)
                .boards
                .unreadable(&project.id)
        })
        .map(|why| format!("Its board cannot be read, and is left as it is: {why}"));
    let refusal = (shell.board_refusal(root).map(str::to_string).or(unreadable)).map(|why| {
        div()
            .text_xs()
            .text_color(crate::theme::status_ink(cx).danger)
            .child(why)
    });
    let workflow = onehand_ui::field(
        "Workflow",
        Some("What each task of this project's board runs".into_any_element()),
        div()
            .v_flex()
            .gap_2()
            .items_start()
            .child(div().h_flex().gap_2().child(picker).children(edit))
            .children(refusal),
        cx,
    );
    let team = bound.map(|board| team_field(handle, root, board.team, cx));
    section(Some("Board"), None, cx)
        .child(workflow)
        .children(team)
        .into_any_element()
}

/// The team size, 1 to the most a board holds.
fn team_field(handle: &Entity<Shell>, root: &Path, team: u8, cx: &App) -> gpui::Div {
    let segments = (1..=onehand_core::board::model::MOST_TEAM).map(|n| {
        let (handle, root) = (handle.clone(), root.to_path_buf());
        onehand_ui::segment(
            ("board-team", usize::from(n)),
            n.to_string(),
            n == team,
            ControlSize::Tall,
            cx,
        )
        .on_click(move |_, _, cx| {
            handle.update(cx, |shell, cx| shell.set_board_team(&root, n, cx));
        })
    });
    onehand_ui::field(
        "Team",
        Some(
            format!("Tasks working at once. Lowering it stops none: no more start until fewer than {team} run")
                .into_any_element(),
        ),
        onehand_ui::choice_track(segments, cx),
        cx,
    )
}
