//! The form that adds or changes one of a board's columns.

use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, ParentElement, SharedString, Styled, Window, div};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_core::board::{MovesOn, Starts};
use onehand_ui::{ControlSize, Tall as _};

/// One choice of a track: its words, whether it is the one picked, and what
/// picking it sets.
type Choice = (&'static str, bool, fn(&mut Shell, &mut Context<Shell>));

/// The column form. **No trigger**, for the other forms' reason: the shell
/// decides whether it exists.
pub(crate) fn column(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.column_draft() else {
        return Dialog::new(cx);
    };
    let (name, limit) = (draft.name.clone(), draft.limit.clone());
    let (title, act) = match draft.editing {
        Some(_) => (format!("A column of {}", draft.label), "Save"),
        None => (format!("Add a column to {}", draft.label), "Add"),
    };
    let (person, starts, moves_on) = (draft.person, draft.starts, draft.moves_on);
    let error = draft.error.clone();
    let danger = crate::theme::status_ink(cx).danger;
    let templates = crate::workflow::templates(cx);
    let offered: Vec<(String, String)> = templates
        .iter()
        .filter_map(|entry| entry.template.as_ref().ok())
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    let picked = draft.workflow.clone();
    let picked_name = picked.as_ref().map(|id| {
        crate::workflow::find(&templates, id)
            .map_or_else(|| format!("{id} (gone)"), |t| t.name.clone())
    });
    let weak = cx.entity().downgrade();

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        // What is wrong with the entry is said in the form, which stays.
        .on_ok(super::enter_presses(
            weak.clone(),
            true,
            vec![name.focus_handle(cx), limit.focus_handle(cx)],
            |shell, window, cx| shell.save_column(window, cx),
        ))
        .content(move |content, _, cx: &mut App| {
            let pick = |set: fn(&mut Shell, &mut Context<Shell>)| {
                let weak = weak.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, set);
                }
            };
            let track = |id: &'static str, choices: [Choice; 2], cx: &App| {
                onehand_ui::choice_track(
                    choices
                        .into_iter()
                        .enumerate()
                        .map(|(at, (label, on, set))| {
                            onehand_ui::segment((id, at), label, on, ControlSize::Tall, cx)
                                .on_click(pick(set))
                        }),
                    cx,
                )
            };
            let work = track(
                "column-work",
                [
                    ("A person", person, |s, cx| s.set_column_work(true, cx)),
                    ("A workflow", !person, |s, cx| s.set_column_work(false, cx)),
                ],
                cx,
            );
            let workflow = (!person).then(|| {
                let more = offered
                    .len()
                    .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
                let (offered, picked, weak) = (offered.clone(), picked.clone(), weak.clone());
                let label = picked_name
                    .clone()
                    .unwrap_or_else(|| "Pick a workflow".into());
                field(
                    "Workflow",
                    crate::settings::about("What each issue entering the column runs"),
                    crate::controls::menu_below(
                        "column-workflow",
                        onehand_ui::picker(
                            "column-workflow-trigger",
                            label,
                            picked.is_some(),
                            ControlSize::Tall,
                            cx,
                        ),
                        move |mut menu, _, _| {
                            for (id, name) in offered.iter().take(crate::workflow::TEMPLATES_SHOWN)
                            {
                                let (weak, id) = (weak.clone(), id.clone());
                                let on = picked.as_deref() == Some(id.as_str());
                                menu = menu.item(
                                    crate::controls::menu_item(name.clone())
                                        .checked(on)
                                        .on_click(move |_, _, cx| {
                                            let _ = weak.update(cx, |shell, cx| {
                                                shell.set_column_workflow(id.clone(), cx)
                                            });
                                        }),
                                );
                            }
                            if more > 0 {
                                menu = menu
                                    .label(SharedString::from(format!("{more} more not shown")));
                            }
                            menu
                        },
                    ),
                    cx,
                )
            });
            let starts = (!person).then(|| {
                field(
                    "Starts",
                    crate::settings::about("When an issue's run begins in it"),
                    track(
                        "column-starts",
                        [
                            ("When it enters", starts == Starts::OnEntry, |s, cx| {
                                s.set_column_starts(Starts::OnEntry, cx)
                            }),
                            (
                                "When Start is pressed",
                                starts == Starts::OnPress,
                                |s, cx| s.set_column_starts(Starts::OnPress, cx),
                            ),
                        ],
                        cx,
                    ),
                    cx,
                )
            });
            let moves = field(
                "Moves on",
                crate::settings::about("How an issue ready to leave it goes to the next column"),
                track(
                    "column-moves",
                    [
                        ("By itself", moves_on == MovesOn::ByItself, |s, cx| {
                            s.set_column_moves_on(MovesOn::ByItself, cx)
                        }),
                        (
                            "A person moves it",
                            moves_on == MovesOn::ByPerson,
                            |s, cx| s.set_column_moves_on(MovesOn::ByPerson, cx),
                        ),
                    ],
                    cx,
                ),
                cx,
            );
            content
                .child(title_row(title.clone(), cx))
                .child(field(
                    "Name",
                    crate::settings::about("What its head says on the board"),
                    text_input(&name, cx).tall(),
                    cx,
                ))
                .child(field(
                    "Work",
                    crate::settings::about("Who works the issues in it"),
                    work,
                    cx,
                ))
                .children(workflow)
                .children(starts)
                .child(moves)
                .child(field(
                    "Limit",
                    crate::settings::about("How many issues it holds at most; empty for no limit"),
                    text_input(&limit, cx).tall(),
                    cx,
                ))
                .when_some(error.clone(), |form, why| {
                    form.child(div().text_xs().text_color(danger).child(why))
                })
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-column")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_column(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-column")
                        .tall()
                        .primary()
                        .label(act)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.save_column(window, cx);
                            }),
                        ),
                ),
        )
        // Esc clears the state putting this on screen, or the next frame
        // renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_column(cx);
        }))
}
