use super::{APP, about, field, list_row, page_head, section};
use crate::controls::Refuses as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Entity, IntoElement, ParentElement, SharedString,
    Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::switch::Switch;
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::pipeline::{self as core, GateKind, Place, StepKind, StepSpec, Template};
use std::path::PathBuf;

/// The template form's fields.
///
/// `file` is the template being changed, or `None` for a new one or a
/// duplicate; `original` is what it was when the form opened or was last
/// saved, which is what "not saved" is measured against.
pub struct PipelineDraft {
    pub file: Option<PathBuf>,
    pub original: Template,
    pub name: Entity<InputState>,
    pub description: Entity<InputState>,
    pub misses: Entity<InputState>,
    pub timeout: Entity<InputState>,
    pub place: Place,
    pub steps: Vec<StepDraft>,
}

/// A step's kind as the form holds it: the kind alone, its fields kept apart
/// on [`StepDraft`] so switching kinds and back loses nothing typed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Agent,
    Command,
    Approval,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Agent, Self::Command, Self::Approval];

    /// What a person calls it.
    fn label(self) -> &'static str {
        match self {
            Self::Agent => "Agent",
            Self::Command => "Command",
            Self::Approval => "Approval",
        }
    }
}

/// One step's fields. The prompt, gates, command and the step another kind
/// points at are all kept whatever the kind, so switching kinds and back
/// loses nothing typed.
pub struct StepDraft {
    pub id: Entity<InputState>,
    pub label: Entity<InputState>,
    pub kind: Kind,
    pub prompt: Entity<TextareaState>,
    pub gates: Vec<GateKind>,
    pub keep_answer: bool,
    pub command: Entity<InputState>,
    /// The step a command goes back to, or an approval approves, by id.
    pub target: String,
}

impl StepDraft {
    fn new(spec: &StepSpec, window: &mut Window, cx: &mut App) -> Self {
        let input = |text: &str, window: &mut Window, cx: &mut App| {
            let text = text.to_string();
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        let (kind, prompt, gates, keep_answer, command, target) = match &spec.kind {
            StepKind::Agent {
                prompt,
                gates,
                keep_answer,
            } => (
                Kind::Agent,
                prompt.as_str(),
                gates.clone(),
                *keep_answer,
                "",
                "",
            ),
            StepKind::Command { command, on_fail } => (
                Kind::Command,
                "",
                Vec::new(),
                false,
                command.as_deref().unwrap_or_default(),
                on_fail.as_str(),
            ),
            StepKind::Approval { of } => (Kind::Approval, "", Vec::new(), false, "", of.as_str()),
        };
        let prompt = prompt.to_string();
        Self {
            id: input(&spec.id, window, cx),
            label: input(&spec.label, window, cx),
            kind,
            prompt: cx.new(|cx| TextareaState::new(window, cx).default_value(prompt)),
            gates,
            keep_answer,
            command: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Empty: the project's check command")
                    .default_value(command.to_string())
            }),
            target: target.to_string(),
        }
    }

    fn to_spec(&self, cx: &App) -> StepSpec {
        let command = self.command.read(cx).value().trim().to_string();
        StepSpec {
            id: self.id.read(cx).value().trim().to_string(),
            label: self.label.read(cx).value().trim().to_string(),
            kind: match self.kind {
                Kind::Agent => StepKind::Agent {
                    prompt: self.prompt.read(cx).value().to_string(),
                    gates: self.gates.clone(),
                    keep_answer: self.keep_answer,
                },
                Kind::Command => StepKind::Command {
                    command: (!command.is_empty()).then_some(command),
                    on_fail: self.target.clone(),
                },
                Kind::Approval => StepKind::Approval {
                    of: self.target.clone(),
                },
            },
        }
    }
}

impl PipelineDraft {
    /// A form on `template`, kept in `file`.
    pub fn load(
        template: &Template,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let input = |text: String, window: &mut Window, cx: &mut App| {
            cx.new(|cx| InputState::new(window, cx).default_value(text))
        };
        Self {
            file,
            original: template.clone(),
            name: input(template.name.clone(), window, cx),
            description: input(template.description.clone(), window, cx),
            misses: input(template.misses.to_string(), window, cx),
            timeout: input(template.timeout.clone(), window, cx),
            place: template.place,
            steps: template
                .steps
                .iter()
                .map(|step| StepDraft::new(step, window, cx))
                .collect(),
        }
    }

    /// The template this form describes, or why the allowance of misses does
    /// not read as a number.
    pub fn to_template(&self, cx: &App) -> Result<Template, String> {
        let misses = self.misses.read(cx).value().trim().to_string();
        let misses = misses
            .parse()
            .map_err(|_| format!("the misses allowed, `{misses}`, is not a whole number"))?;
        Ok(Template {
            schema_version: core::SCHEMA_VERSION,
            name: self.name.read(cx).value().trim().to_string(),
            description: self.description.read(cx).value().trim().to_string(),
            place: self.place,
            misses,
            timeout: self.timeout.read(cx).value().trim().to_string(),
            steps: self.steps.iter().map(|step| step.to_spec(cx)).collect(),
        })
    }

    /// Everything that keeps the form from being saved, as sentences.
    pub fn problems(&self, cx: &App) -> Vec<String> {
        match self.to_template(cx) {
            Ok(template) => core::validate(&template)
                .iter()
                .map(ToString::to_string)
                .collect(),
            Err(why) => vec![why],
        }
    }

    /// Whether the form holds something not yet saved.
    pub fn dirty(&self, cx: &App) -> bool {
        self.to_template(cx).ok().as_ref() != Some(&self.original)
    }

    /// Add a step of `kind` at the end, with an id no other step has.
    pub fn add_step(&mut self, kind: Kind, window: &mut Window, cx: &mut App) {
        let taken: Vec<String> = self
            .steps
            .iter()
            .map(|step| step.id.read(cx).value().to_string())
            .collect();
        let id = (1..)
            .map(|n| format!("step-{n}"))
            .find(|id| !taken.contains(id))
            .unwrap_or_default();
        let spec = StepSpec {
            id,
            label: kind.label().to_string(),
            kind: match kind {
                Kind::Agent => StepKind::Agent {
                    prompt: String::new(),
                    gates: Vec::new(),
                    keep_answer: false,
                },
                Kind::Command => StepKind::Command {
                    command: None,
                    on_fail: String::new(),
                },
                Kind::Approval => StepKind::Approval { of: String::new() },
            },
        };
        self.steps.push(StepDraft::new(&spec, window, cx));
    }

    /// Move the step at `at` one place up, or down.
    pub fn move_step(&mut self, at: usize, up: bool) {
        let to = match up {
            true => at.checked_sub(1),
            false => Some(at + 1).filter(|to| *to < self.steps.len()),
        };
        if let Some(to) = to {
            self.steps.swap(at, to);
        }
    }
}

/// The pipelines page: the templates on offer, the form that edits one, and
/// each project's check command.
pub(super) fn pipelines_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let entries = crate::pipeline::templates(cx);
    let muted = cx.theme().muted_foreground;
    let warning = crate::theme::status_ink(cx).warning;

    let left_out = entries
        .len()
        .saturating_sub(crate::pipeline::TEMPLATES_SHOWN);
    let rows = entries
        .iter()
        .take(crate::pipeline::TEMPLATES_SHOWN)
        .enumerate()
        .map(|(i, entry)| {
            let shipped = entry.file.is_none();
            let about = match &entry.template {
                Ok(template) => div()
                    .text_xs()
                    .child(template.description.clone())
                    .into_any_element(),
                Err(why) => div()
                    .text_xs()
                    .text_color(warning)
                    .child(format!("Cannot be read: {why}"))
                    .into_any_element(),
            };
            let readable = entry.template.is_ok();
            list_row(
                entry.name(),
                Some(about),
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(shipped, |row| {
                        row.child(Tag::secondary().small().child("Built in"))
                    })
                    .when(readable, |row| {
                        row.child(row_action(
                            handle,
                            ("duplicate-pipeline", i),
                            "Duplicate",
                            move |shell, window, cx| shell.duplicate_pipeline(i, window, cx),
                        ))
                    })
                    .when(readable && !shipped, |row| {
                        row.child(
                            row_icon(("edit-pipeline", i), crate::icons::Icon::SquarePen, "Edit")
                                .on_click(click(handle, move |shell, window, cx| {
                                    shell.edit_pipeline(i, window, cx)
                                })),
                        )
                    })
                    .when(!shipped, |row| {
                        row.child(
                            row_icon(("delete-pipeline", i), crate::icons::Icon::Trash, "Delete")
                                .on_click(click(handle, move |shell, window, cx| {
                                    shell.confirm_delete_pipeline(i, window, cx)
                                })),
                        )
                    }),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

    let list = section(None, None, cx)
        .gap_3()
        .children(rows)
        .when(left_out > 0, |list| {
            list.child(div().text_sm().text_color(muted).child(format!(
                "{left_out} more templates not shown; remove some from {}",
                core::store::dir().display()
            )))
        })
        .when(entries.is_empty(), |list| {
            list.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Reading the templates…"),
            )
        })
        .child(
            div().h_flex().child(
                crate::controls::action("new-pipeline")
                    .ghost()
                    .icon(Icon::new(IconName::Plus))
                    .label("New template")
                    .on_click(click(handle, |shell, window, cx| {
                        shell.new_pipeline(window, cx)
                    })),
            ),
        );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "Pipelines",
            "The templates a pipeline run starts from, shared by every workspace. The ones \
             onehand ships are read-only: duplicate one to change it.",
            APP,
            cx,
        ))
        .child(list)
        .children(shell.pipeline_draft().map(|draft| form(handle, draft, cx)))
        .child(checks_section(handle, cx))
        .into_any_element()
}

/// A click handler that hands `act` the shell.
fn click(
    handle: &Entity<Shell>,
    act: impl Fn(&mut Shell, &mut Window, &mut gpui::Context<Shell>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let handle = handle.clone();
    move |_, window, cx| handle.update(cx, |shell, cx| act(shell, window, cx))
}

fn row_action(
    handle: &Entity<Shell>,
    id: (&'static str, usize),
    label: &'static str,
    act: impl Fn(&mut Shell, &mut Window, &mut gpui::Context<Shell>) + 'static,
) -> impl IntoElement {
    crate::controls::action(id)
        .ghost()
        .small()
        .label(label)
        .on_click(click(handle, act))
}

fn row_icon(
    id: (&'static str, usize),
    icon: impl Into<Icon>,
    tip: &'static str,
) -> gpui_component::button::Button {
    crate::controls::action(id)
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .tooltip(tip)
}

/// The form: the template's own fields, its steps, what is wrong with it,
/// and Save.
fn form(handle: &Entity<Shell>, draft: &crate::settings::PipelineDraft, cx: &App) -> AnyElement {
    let problems = draft.problems(cx);
    let danger = crate::theme::status_ink(cx).danger;
    let muted = cx.theme().muted_foreground;
    let editing = draft.file.is_some();
    let place = draft.place;
    let places: Vec<SharedString> = Place::ALL.iter().map(|p| p.label().into()).collect();
    let place_at = Place::ALL.iter().position(|p| *p == place).unwrap_or(0);
    let pick_place = {
        let handle = handle.clone();
        move |at: &usize, window: &mut Window, cx: &mut App| {
            let place = Place::ALL[*at];
            handle.update(cx, |shell, cx| {
                shell.edit_pipeline_draft(window, cx, |d, _, _| d.place = place)
            });
        }
    };
    // Every earlier agent step by id, for what a command goes back to and an
    // approval approves.
    let ids: Vec<Earlier> = draft
        .steps
        .iter()
        .map(|step| Earlier {
            id: step.id.read(cx).value().trim().to_string(),
            agent: step.kind == Kind::Agent,
            keeps: step.keep_answer,
        })
        .collect();

    let steps = draft
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| step_box(handle, i, step, &ids[..i], place, cx))
        .collect::<Vec<_>>();

    section(
        Some(if editing {
            "Edit template"
        } else {
            "New template"
        }),
        None,
        cx,
    )
    .child(field(
        "Name",
        about("What the launcher lists it as."),
        Input::new(&draft.name),
        cx,
    ))
    .child(field(
        "Description",
        None,
        Input::new(&draft.description),
        cx,
    ))
    .child(field(
        "Where it works",
        about(
            "In the checkout leaves the change uncommitted for you; a new worktree gets a \
             branch of its own, and the work is committed there.",
        ),
        onehand_plugin_host::switch(
            "pipeline-place",
            &places,
            place_at,
            gpui_component::Size::Small,
            pick_place,
            cx,
        ),
        cx,
    ))
    .child(
        div()
            .h_flex()
            .gap_4()
            .child(div().flex_1().child(field(
                "Misses allowed",
                about("Turns that fail their gates before the run stops."),
                Input::new(&draft.misses),
                cx,
            )))
            .child(div().flex_1().child(field(
                "Timeout",
                about("Working time, such as 45m or 2h."),
                Input::new(&draft.timeout),
                cx,
            ))),
    )
    .child(div().text_sm().text_color(muted).child(
        "A prompt may use {brief}, {instructions}, {check_output}, {revise} and \
                 {output.<step id>} for what an earlier step kept.",
    ))
    .children(steps)
    .child(
        div()
            .h_flex()
            .gap_2()
            .children(Kind::ALL.into_iter().enumerate().map(|(at, kind)| {
                crate::controls::action(("add-step", at))
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .label(format!("{} step", kind.label()))
                    .on_click(click(handle, move |shell, window, cx| {
                        shell.edit_pipeline_draft(window, cx, |d, window, cx| {
                            d.add_step(kind, window, cx)
                        });
                    }))
            })),
    )
    .children(
        problems
            .iter()
            .map(|problem| div().text_sm().text_color(danger).child(problem.clone())),
    )
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                crate::controls::action("save-pipeline")
                    .primary()
                    .refuses(!problems.is_empty())
                    .label("Save")
                    .on_click(click(handle, |shell, window, cx| {
                        shell.save_pipeline_draft(window, cx)
                    })),
            )
            .child(
                crate::controls::action("clear-pipeline")
                    .ghost()
                    .label("Cancel")
                    .on_click(click(handle, |shell, _, cx| shell.clear_pipeline_draft(cx))),
            ),
    )
    .into_any_element()
}

/// What a later step's menus need to know of an earlier one.
struct Earlier {
    id: String,
    agent: bool,
    keeps: bool,
}

/// One step of the form, in a box of its own.
fn step_box(
    handle: &Entity<Shell>,
    i: usize,
    step: &StepDraft,
    earlier: &[Earlier],
    place: Place,
    cx: &App,
) -> AnyElement {
    let kind_menu = {
        let handle = handle.clone();
        crate::controls::menu_below(
            ("step-kind", i),
            crate::controls::action(("step-kind-trigger", i))
                .outline()
                .xsmall()
                .label(step.kind.label())
                .icon(Icon::new(IconName::ChevronDown)),
            move |mut menu, _, _| {
                for kind in Kind::ALL {
                    let handle = handle.clone();
                    menu = menu.item(crate::controls::menu_item(kind.label()).on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| {
                                shell.edit_pipeline_draft(window, cx, |d, _, _| {
                                    d.steps[i].kind = kind
                                })
                            });
                        },
                    ));
                }
                menu
            },
        )
    };
    let head = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_medium()
                .child(format!("Step {}", i + 1)),
        )
        .child(kind_menu)
        .child(div().flex_1())
        .child(
            row_icon(("step-up", i), IconName::ArrowUp, "Move up").on_click(click(
                handle,
                move |shell, window, cx| {
                    shell.edit_pipeline_draft(window, cx, |d, _, _| d.move_step(i, true))
                },
            )),
        )
        .child(
            row_icon(("step-down", i), IconName::ArrowDown, "Move down").on_click(click(
                handle,
                move |shell, window, cx| {
                    shell.edit_pipeline_draft(window, cx, |d, _, _| d.move_step(i, false))
                },
            )),
        )
        .child(
            row_icon(("step-delete", i), crate::icons::Icon::Trash, "Remove step").on_click(click(
                handle,
                move |shell, window, cx| {
                    shell.edit_pipeline_draft(window, cx, |d, _, _| {
                        d.steps.remove(i);
                    })
                },
            )),
        );
    let names = div()
        .h_flex()
        .gap_4()
        .child(
            div()
                .flex_1()
                .child(field("Id", None, Input::new(&step.id).small(), cx)),
        )
        .child(
            div()
                .flex_1()
                .child(field("Label", None, Input::new(&step.label).small(), cx)),
        );

    let body = match step.kind {
        Kind::Agent => {
            let gates = GateKind::ALL
                .iter()
                .filter(|gate| gate.fits(place) || step.gates.contains(gate))
                .map(|gate| {
                    let gate = *gate;
                    let on = step.gates.contains(&gate);
                    let handle = handle.clone();
                    div().flex_none().cursor_pointer().child(
                        Switch::new(SharedString::from(format!("gate-{i}-{gate:?}")))
                            .checked(on)
                            .small()
                            .label(gate.label())
                            .on_click(move |_: &bool, window: &mut Window, cx: &mut App| {
                                handle.update(cx, |shell, cx| {
                                    shell.edit_pipeline_draft(window, cx, |d, _, _| {
                                        let gates = &mut d.steps[i].gates;
                                        match gates.iter().position(|g| *g == gate) {
                                            Some(at) => {
                                                gates.remove(at);
                                            }
                                            None => gates.push(gate),
                                        }
                                    })
                                });
                            }),
                    )
                })
                .collect::<Vec<_>>();
            let keep = {
                let handle = handle.clone();
                div().flex_none().cursor_pointer().child(
                    Switch::new(("keep-answer", i))
                        .checked(step.keep_answer)
                        .small()
                        .label("Keep its answer")
                        .on_click(move |_: &bool, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| {
                                shell.edit_pipeline_draft(window, cx, |d, _, _| {
                                    d.steps[i].keep_answer = !d.steps[i].keep_answer
                                })
                            });
                        }),
                )
            };
            div()
                .v_flex()
                .gap_3()
                .child(field(
                    "Prompt",
                    None,
                    Textarea::new(&step.prompt).h(rems(8.)),
                    cx,
                ))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(div().text_sm().child("Gates, checked when its turn ends"))
                        .child(div().h_flex().flex_wrap().gap_3().children(gates)),
                )
                .child(div().h_flex().child(keep))
                .into_any_element()
        }
        kind @ (Kind::Command | Kind::Approval) => {
            let needs_keep = kind == Kind::Approval;
            let title = match needs_keep {
                true => "Approves the answer of",
                false => "On failure, back to",
            };
            let choices: Vec<String> = earlier
                .iter()
                .filter(|step| step.agent && (!needs_keep || step.keeps))
                .map(|step| step.id.clone())
                .collect();
            let current = step.target.clone();
            let target = {
                let handle = handle.clone();
                crate::controls::menu_below(
                    ("step-target", i),
                    crate::controls::action(("step-target-trigger", i))
                        .outline()
                        .small()
                        .label(if current.is_empty() {
                            "Pick a step".to_string()
                        } else {
                            current.clone()
                        })
                        .icon(Icon::new(IconName::ChevronDown)),
                    move |mut menu, _, _| {
                        for id in &choices {
                            let (handle, id) = (handle.clone(), id.clone());
                            menu = menu.item(
                                crate::controls::menu_item(id.clone())
                                    .checked(id == current)
                                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                                        let id = id.clone();
                                        handle.update(cx, |shell, cx| {
                                            shell.edit_pipeline_draft(window, cx, move |d, _, _| {
                                                d.steps[i].target = id
                                            })
                                        });
                                    }),
                            );
                        }
                        if choices.is_empty() {
                            menu = menu.label("No earlier step fits");
                        }
                        menu
                    },
                )
            };
            div()
                .v_flex()
                .gap_3()
                .when(kind == Kind::Command, |col| {
                    col.child(field(
                        "Command",
                        about("Run by onehand in the work, through sh."),
                        Input::new(&step.command),
                        cx,
                    ))
                })
                .child(field(title, None, div().h_flex().child(target), cx))
                .into_any_element()
        }
    };

    div()
        .v_flex()
        .gap_3()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(head)
        .child(names)
        .child(body)
        .into_any_element()
}

/// How many projects' check command fields the page draws before it says how
/// many more there are.
const CHECKS_SHOWN: usize = 30;

/// Each project's check command: what a command step that names none runs.
fn checks_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let rows = handle.read(cx).check_inputs();
    let empty = rows.is_empty();
    let left_out = rows.len().saturating_sub(CHECKS_SHOWN);
    section(
        Some("Check commands"),
        Some(SharedString::from(
            "What a command step that names no command runs, per project of this workspace. \
             onehand runs it itself, so passing is never the agent's word.",
        )),
        cx,
    )
    .children(
        rows.into_iter()
            .take(CHECKS_SHOWN)
            .map(|(name, input)| field(name, None, Input::new(&input), cx).into_any_element()),
    )
    .when(left_out > 0, |group| {
        group.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{left_out} more projects not shown")),
        )
    })
    .when(empty, |group| {
        group.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No projects in this workspace."),
        )
    })
    .into_any_element()
}
