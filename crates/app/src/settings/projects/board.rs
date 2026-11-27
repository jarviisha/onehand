//! The Board group of the Projects page: a board made by picking the
//! workflow it runs first, then its columns, how many tasks work at once,
//! and the way to take it away.

use super::section;
use crate::controls::Named as _;
use crate::settings::{list_row, list_rows, more_not_shown};
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    Styled, div,
};
use gpui_component::StyledExt;
use gpui_component::button::ButtonVariants as _;
use gpui_component::{Icon, IconName};
use onehand_core::board::model::Column;
use onehand_core::board::{Board, MovesOn, Starts, Work};
use onehand_core::project::Toward;
use onehand_ui::{ControlSize, Tall as _};
use onehand_ui::{Refuses as _, Short as _, ShortIcon as _};
use std::path::Path;

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
    let refusal = refusal.map(|line| line.debug_selector(|| "board-refusal".into()));
    let remove = || {
        let (handle, root) = (handle.clone(), root.to_path_buf());
        div()
            .h_flex()
            .child(
                crate::controls::action("board-remove")
                    .tall()
                    .secondary()
                    .text_color(crate::theme::status_ink(cx).danger)
                    .label("Remove the board…")
                    .on_click(move |_, window, cx| {
                        handle.update(cx, |shell, cx| shell.remove_board(root.clone(), window, cx));
                    }),
            )
            .debug_selector(|| "board-remove".into())
    };
    let section = section(Some("Board"), None, cx);
    // A board this build cannot read is never written, nor taken away:
    // why, alone.
    if bound.is_none() && unreadable_board(project.as_ref(), cx) {
        return section
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .children(refusal)
                    .debug_selector(|| "board-unreadable".into()),
            )
            .into_any_element();
    }
    let Some(board) = bound else {
        let workflow = onehand_ui::field(
            "Workflow",
            Some("Pick the workflow its tasks run to make the project a board".into_any_element()),
            div()
                .v_flex()
                .gap_2()
                .items_start()
                .child(workflow_menu(handle, root, &templates, cx))
                .children(refusal),
            cx,
        )
        .debug_selector(|| "board-workflow-field".into());
        return section.child(workflow).into_any_element();
    };
    let team = team_field(handle, root, board.team, cx);
    section
        .child(columns_field(handle, root, &board, &templates, refusal, cx))
        .child(team)
        .child(remove())
        .into_any_element()
}

/// The library's workflows, picking one making the project at `root` a board
/// that runs it.
fn workflow_menu(
    handle: &Entity<Shell>,
    root: &Path,
    templates: &[crate::workflow::Entry],
    cx: &App,
) -> AnyElement {
    let offered: Vec<(String, String)> = templates
        .iter()
        .filter_map(|entry| entry.template.as_ref().ok())
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    let more = offered
        .len()
        .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let (handle, root) = (handle.clone(), root.to_path_buf());
    crate::controls::menu_below(
        "board-workflow",
        onehand_ui::picker(
            "board-workflow-trigger",
            "No board",
            false,
            ControlSize::Tall,
            cx,
        ),
        move |mut menu, _, _| {
            for (id, name) in offered.iter().take(crate::workflow::TEMPLATES_SHOWN) {
                let (handle, root, id) = (handle.clone(), root.clone(), id.clone());
                menu = menu.item(crate::controls::menu_item(name.clone()).on_click(
                    move |_, window, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.bind_board(root.clone(), Some(id.clone()), window, cx);
                        });
                    },
                ));
            }
            if more > 0 {
                menu = menu.label(SharedString::from(format!("{more} more not shown")));
            }
            menu
        },
    )
    .into_any_element()
}

/// How many columns the Board box lists before it says how many it left out.
const COLUMNS_SHOWN: usize = 20;

/// Whether the board of `project` is one this build cannot read.
fn unreadable_board(project: Option<&onehand_core::project::Project>, cx: &App) -> bool {
    project.is_some_and(|project| {
        (crate::state::Shared::global(cx).boards)
            .unreadable(&project.id)
            .is_some()
    })
}

/// What a column's row says under its name: who works it, then how it starts,
/// moves on and holds, where that differs from the first choice.
fn column_line(column: &Column, templates: &[crate::workflow::Entry]) -> String {
    let mut said = vec![match &column.work {
        Work::Person => "A person".to_string(),
        Work::Workflow { id } => match crate::workflow::find(templates, id) {
            Some(t) => format!("Runs {}", t.name),
            None => format!("Runs {id} (gone)"),
        },
    }];
    if matches_workflow(&column.work) && column.starts == Starts::OnPress {
        said.push("starts on a press".into());
    }
    if column.moves_on == MovesOn::ByPerson {
        said.push("moves on by a person".into());
    }
    if let Some(n) = column.limit {
        said.push(format!("holds {n}"));
    }
    said.join(" · ")
}

fn matches_workflow(work: &Work) -> bool {
    match work {
        Work::Workflow { .. } => true,
        Work::Person => false,
    }
}

/// The board's columns, a row each with its controls, then the way to add one.
fn columns_field(
    handle: &Entity<Shell>,
    root: &Path,
    board: &Board,
    templates: &[crate::workflow::Entry],
    refusal: Option<gpui::Div>,
    cx: &App,
) -> gpui::Div {
    let last = board.columns.len().saturating_sub(1);
    let more = board.columns.len().saturating_sub(COLUMNS_SHOWN);
    let project = crate::projects::of(root, cx).map(|p| p.id);
    let rows: Vec<AnyElement> = (board.columns.iter().enumerate())
        .take(COLUMNS_SHOWN)
        .map(|(at, column)| {
            let id = column.id.clone();
            let shift = |toward: Toward, name: &'static str, icon: IconName, refuse: bool| {
                let (handle, root, id) = (handle.clone(), root.to_path_buf(), id.clone());
                crate::controls::action((name, at))
                    .short_icon()
                    .secondary()
                    .icon(icon)
                    .named(name)
                    .refuses(refuse.then_some("It is at that end already"))
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.move_board_column(root.clone(), id.clone(), toward, cx)
                        });
                    })
            };
            let edit = {
                let (handle, root, id) = (handle.clone(), root.to_path_buf(), id.clone());
                crate::controls::action(("edit-column", at))
                    .short_icon()
                    .secondary()
                    .icon(Icon::new(crate::icons::Icon::SquarePen))
                    .named("Edit")
                    .on_click(move |_, window, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.open_column_form(root.clone(), Some(id.clone()), window, cx)
                        });
                    })
            };
            // A workflow on offer opens where it is edited.
            let workflow = match &column.work {
                Work::Workflow { id } => crate::workflow::find(templates, id).map(|_| id.clone()),
                Work::Person => None,
            };
            let open = workflow.zip(project.clone()).map(|(workflow, project)| {
                let handle = handle.clone();
                crate::controls::action(("edit-column-workflow", at))
                    .short_icon()
                    .secondary()
                    .icon(IconName::Settings)
                    .named("Edit workflow")
                    .on_click(move |_, window, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.show_workflow(&workflow, None, Some(project.clone()), window, cx)
                        });
                    })
            });
            let held = column.entries.len();
            let remove = {
                let (handle, root, id) = (handle.clone(), root.to_path_buf(), id.clone());
                let why = match held {
                    0 => None,
                    1 => Some("Move its issue out first".to_string()),
                    n => Some(format!("Move its {n} issues out first")),
                };
                // A refused removal is not drawn in the danger ink.
                let ink = why.is_none().then(|| crate::theme::status_ink(cx).danger);
                crate::controls::action(("remove-column", at))
                    .short()
                    .secondary()
                    .when_some(ink, |button, ink| button.text_color(ink))
                    .label("Remove")
                    .refuses(why)
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.remove_board_column(root.clone(), id.clone(), cx)
                        });
                    })
            };
            let selector = format!("board-column-row-{}", column.id);
            list_row(
                column.name.clone(),
                Some(column_line(column, templates).into_any_element()),
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(shift(Toward::Up, "Move up", IconName::ChevronUp, at == 0))
                    .child(shift(
                        Toward::Down,
                        "Move down",
                        IconName::ChevronDown,
                        at == last,
                    ))
                    .child(edit)
                    .children(open)
                    .child(remove),
                cx,
            )
            .debug_selector(move || selector.clone())
            .into_any_element()
        })
        .collect();
    let (add, add_root) = (handle.clone(), root.to_path_buf());
    onehand_ui::field(
        "Columns",
        Some("Backlog comes first and Done last; issues move left to right".into_any_element()),
        div()
            .v_flex()
            .gap_2()
            .children((!rows.is_empty()).then(|| list_rows(rows, cx)))
            .children(more_not_shown(more, cx))
            .children(refusal)
            .child(
                div()
                    .h_flex()
                    .child(
                        crate::controls::action("board-add-column")
                            .short()
                            .secondary()
                            .label("Add column…")
                            .on_click(move |_, window, cx| {
                                add.update(cx, |shell, cx| {
                                    shell.open_column_form(add_root.clone(), None, window, cx)
                                });
                            }),
                    )
                    .debug_selector(|| "board-add-column".into()),
            ),
        cx,
    )
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
