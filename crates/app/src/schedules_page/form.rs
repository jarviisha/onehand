//! The schedule form: a page form whose submit stands in its head, as the
//! workflow editor's does, its fields in sections down one column.

use super::SchedulesPage;
use super::draft::ScheduleDraft;
use crate::controls::{Quiet as _, Refuses as _, SettingPick, setting_row, text_input};
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, StyledExt as _};
use onehand_ui::{ControlSize, Tall as _};
use std::path::PathBuf;
use std::rc::Rc;

/// What picking one of a picker's choices does.
type Press = Rc<dyn Fn(&mut Window, &mut App)>;

/// A picker's choice: its label, whether it is the one picked, and its press.
type Choice = (SharedString, bool, Press);

/// The most of an agent's settings drawn as rows of choices.
const OPTIONS_SHOWN: usize = 12;

/// Change the open draft with `edit`, and draw again.
pub(super) fn edit(
    handle: &Entity<SchedulesPage>,
    edit: impl Fn(&mut ScheduleDraft) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let handle = handle.clone();
    move |_, cx| {
        handle.update(cx, |page, cx| {
            if let Some(draft) = page.draft.as_mut() {
                edit(draft);
            }
            cx.notify();
        })
    }
}

/// The form's head: its name, what it is, then ⋯ (*Delete…*) for one
/// saved, *Cancel* and *Save*, the page's one primary.
pub(super) fn head(
    handle: &Entity<SchedulesPage>,
    draft: &ScheduleDraft,
    projects: &[(PathBuf, SharedString)],
    writing: bool,
    cx: &App,
) -> AnyElement {
    let name = draft.name.read(cx).value().trim().to_string();
    let title = match (name.is_empty(), draft.original.is_some()) {
        (false, _) => name,
        (true, false) => "New schedule".to_string(),
        (true, true) => "Untitled".to_string(),
    };
    let project = draft
        .project
        .as_ref()
        .and_then(|root| projects.iter().find(|(r, _)| r == root))
        .map(|(_, label)| label.to_string());
    let when = draft
        .when_expr(cx)
        .map(|expr| onehand_core::schedule::when_said(&expr))
        .unwrap_or_default();
    let facts: Vec<String> = [Some(when), project]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    let mut actions = Vec::new();
    if draft.original.is_some() {
        let handle = handle.clone();
        actions.push(
            crate::controls::menu_below(
                "schedule-more",
                crate::controls::action("schedule-more-trigger")
                    .tall()
                    .quiet_icon(false, cx)
                    .icon(IconName::Ellipsis)
                    .tooltip("More"),
                move |menu, _, _| {
                    let handle = handle.clone();
                    menu.item(crate::controls::menu_item("Delete…").on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |page, cx| page.confirm_delete(window, cx))
                        },
                    ))
                },
            )
            .into_any_element(),
        );
    }
    let problems = draft.problems(cx);
    actions.push({
        let handle = handle.clone();
        crate::controls::action("schedule-cancel")
            .tall()
            .secondary()
            .label("Cancel")
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                handle.update(cx, |page, cx| page.close_form(window, cx))
            })
            .into_any_element()
    });
    actions.push({
        let handle = handle.clone();
        crate::controls::action("schedule-save")
            .tall()
            .primary()
            .loading(writing)
            .label("Save")
            .refuses(problems.first().cloned())
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                handle.update(cx, |page, cx| page.save(window, cx))
            })
            .into_any_element()
    });
    crate::chat::pane::page_parts::page_head(
        title,
        (!facts.is_empty()).then(|| facts.join(" · ").into_any_element()),
        actions,
        cx,
    )
    .into_any_element()
}

/// The form's sections, down one column.
pub(super) fn body(
    handle: &Entity<SchedulesPage>,
    draft: &ScheduleDraft,
    projects: &[(PathBuf, SharedString)],
    cx: &App,
) -> Vec<AnyElement> {
    vec![
        about_section(handle, draft, projects, cx),
        starts_section(handle, draft, cx),
        agent_section(handle, draft, cx),
        tools_section(handle, draft, cx),
        super::when::when_section(handle, draft, cx),
    ]
}

/// A picker under its menu: `label` naming what is picked, `choices` each a
/// label and what picking it does.
fn picker(
    id: &'static str,
    label: impl Into<SharedString>,
    set: bool,
    choices: Vec<Choice>,
    cx: &App,
) -> AnyElement {
    crate::controls::menu_below(
        id,
        onehand_ui::picker(
            SharedString::from(format!("{id}-trigger")),
            label,
            set,
            ControlSize::Tall,
            cx,
        ),
        move |mut menu, _, _| {
            for (label, checked, pick) in &choices {
                let pick = pick.clone();
                menu = menu.item(
                    crate::controls::menu_item(label.clone())
                        .checked(*checked)
                        .on_click(move |_, window: &mut Window, cx: &mut App| pick(window, cx)),
                );
            }
            menu
        },
    )
    .into_any_element()
}

fn about_section(
    handle: &Entity<SchedulesPage>,
    draft: &ScheduleDraft,
    projects: &[(PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    let shown = draft
        .project
        .as_ref()
        .and_then(|root| projects.iter().find(|(r, _)| r == root))
        .map_or_else(|| "Pick a project".into(), |(_, label)| label.clone());
    let choices = projects
        .iter()
        .map(|(root, label)| {
            let to = root.clone();
            let pick: Press = Rc::new(edit(handle, move |d| d.project = Some(to.clone())));
            (label.clone(), draft.project.as_ref() == Some(root), pick)
        })
        .collect();
    section(None, None, cx)
        .child(field(
            "Name",
            about("What the page and its tasks call it"),
            text_input(&draft.name, cx).tall(),
            cx,
        ))
        .child(field(
            "Project",
            about("Where it runs; it waits while no window holds the project"),
            div().h_flex().child(picker(
                "schedule-project",
                shown,
                draft.project.is_some(),
                choices,
                cx,
            )),
            cx,
        ))
        .child(
            div().h_flex().child(
                div().flex_none().cursor_pointer().child(
                    Switch::new("schedule-form-on")
                        .checked(draft.on)
                        .small()
                        .label("On")
                        .on_click({
                            let handle = handle.clone();
                            move |on: &bool, window: &mut Window, cx: &mut App| {
                                let on = *on;
                                edit(&handle, move |d| d.on = on)(window, cx)
                            }
                        }),
                ),
            ),
        )
        .into_any_element()
}

fn starts_section(handle: &Entity<SchedulesPage>, draft: &ScheduleDraft, cx: &App) -> AnyElement {
    let kind = onehand_ui::switch(
        "schedule-starts",
        &[
            "A session with a prompt".into(),
            "A workflow on a brief".into(),
        ],
        usize::from(!draft.prompt_kind),
        ControlSize::Tall,
        {
            let handle = handle.clone();
            move |at: &usize, window, cx| {
                let prompt = *at == 0;
                edit(&handle, move |d| d.prompt_kind = prompt)(window, cx)
            }
        },
        cx,
    );
    let mut group = section(Some("What it starts"), None, cx).child(kind);
    if draft.prompt_kind {
        return group
            .child(field(
                "Prompt",
                about("Sent as the session's first message; name a skill here to use one"),
                crate::controls::text_area(&draft.prompt, cx),
                cx,
            ))
            .into_any_element();
    }
    let entries = crate::workflow::templates(cx);
    let shown = draft.workflow.as_ref().map_or_else(
        || "Pick a workflow".to_string(),
        |id| {
            entries
                .iter()
                .find_map(|e| e.template.as_ref().ok().filter(|t| &t.id == id))
                .map_or_else(|| format!("{id} · not in the library"), |t| t.name.clone())
        },
    );
    let choices = entries
        .iter()
        .filter_map(|entry| entry.template.as_ref().ok())
        .take(crate::workflow::TEMPLATES_SHOWN)
        .map(|template| {
            let id = template.id.clone();
            let checked = draft.workflow.as_ref() == Some(&id);
            let pick: Press = Rc::new(edit(handle, move |d| d.workflow = Some(id.clone())));
            (SharedString::from(template.name.clone()), checked, pick)
        })
        .collect();
    group = group
        .child(field(
            "Workflow",
            None,
            div().h_flex().child(picker(
                "schedule-workflow",
                shown,
                draft.workflow.is_some(),
                choices,
                cx,
            )),
            cx,
        ))
        .child(field(
            "Title",
            None,
            text_input(&draft.title, cx).tall(),
            cx,
        ))
        .child(field(
            "Brief",
            about("The details: what, why, and how to tell it is done"),
            crate::controls::text_area(&draft.body, cx),
            cx,
        ));
    group.into_any_element()
}

/// The agent, then the settings it offers as rows of choices, *Check the
/// agent* while they are not known.
fn agent_section(handle: &Entity<SchedulesPage>, draft: &ScheduleDraft, cx: &App) -> AnyElement {
    let agents = &crate::state::Shared::global(cx).agents;
    let spec = onehand_core::config::agent_or_first(agents, draft.agent.as_deref());
    let first = agents.first().map(|spec| spec.name.clone());
    let shown = match (&draft.agent, &first) {
        (Some(name), _) if spec.is_none() => format!("{name} · not configured"),
        (Some(name), _) => name.clone(),
        (None, Some(first)) => format!("The first agent ({first})"),
        (None, None) => "No agent is configured".to_string(),
    };
    let mut choices: Vec<Choice> = vec![(
        "The first agent".into(),
        draft.agent.is_none(),
        Rc::new(edit(handle, |d| d.agent = None)),
    )];
    choices.extend(agents.iter().map(|spec| {
        let name = spec.name.clone();
        let checked = draft.agent.as_ref() == Some(&name);
        let pick: Press = Rc::new(edit(handle, move |d| d.agent = Some(name.clone())));
        (SharedString::from(spec.name.clone()), checked, pick)
    }));
    let offer = spec.and_then(|spec| crate::unattended::offered(spec, cx));
    let said: SharedString = match (spec, &offer) {
        (Some(spec), Some(_)) => format!("Choices {} offers", spec.name).into(),
        (Some(spec), None) => format!("What {} offers is not known yet", spec.name).into(),
        (None, _) => "Nothing to pick from without an agent".into(),
    };
    let check = spec
        .filter(|_| offer.is_none())
        .zip(draft.project.clone().or_else(std::env::home_dir))
        .and_then(|(spec, cwd)| crate::agent_check::Ask::new(Some(&spec.name), &cwd, None, cx))
        .map(|ask| crate::agent_check::button(0, &ask, cx));

    let mut group = section(Some("The agent"), Some(said), cx)
        .child(field(
            "Agent",
            None,
            div().h_flex().child(picker(
                "schedule-agent",
                shown,
                draft.agent.is_some(),
                choices,
                cx,
            )),
            cx,
        ))
        .children(check.map(|check| div().h_flex().child(check)));

    let setting =
        |id: String, offered: Option<&[String]>, current: Option<&str>, pick: SettingPick| {
            setting_row(id, "Agent's own", offered, current, true, None, pick, cx)
        };
    let mode_pick: SettingPick = {
        let handle = handle.clone();
        Rc::new(move |to, window, cx| edit(&handle, move |d| d.mode = to.clone())(window, cx))
    };
    group = group.child(field(
        "Mode",
        about("The agent's own session mode; one that asks less lets it run through"),
        setting(
            "schedule-mode".into(),
            offer.as_ref().map(|o| o.modes.as_slice()),
            draft.mode.as_deref(),
            mode_pick,
        ),
        cx,
    ));
    // What it offers, in its order, then any picked it does not offer.
    let shown: Vec<(String, Option<Vec<String>>)> = offer
        .iter()
        .flat_map(|o| o.options.iter().take(OPTIONS_SHOWN))
        .map(|(id, values)| (id.clone(), Some(values.clone())))
        .chain(
            draft
                .options
                .keys()
                .filter(|id| {
                    !offer
                        .iter()
                        .any(|o| o.options.iter().any(|(i, _)| i == *id))
                })
                .map(|id| (id.clone(), None)),
        )
        .collect();
    for (n, (id, values)) in shown.into_iter().enumerate() {
        let pick: SettingPick = {
            let (handle, id) = (handle.clone(), id.clone());
            Rc::new(move |to, window, cx| {
                let id = id.clone();
                edit(&handle, move |d| match &to {
                    Some(value) => {
                        d.options.insert(id.clone(), value.clone());
                    }
                    None => {
                        d.options.remove(&id);
                    }
                })(window, cx)
            })
        };
        let row = setting(
            format!("schedule-option-{n}"),
            values.as_deref(),
            draft.options.get(&id).map(String::as_str),
            pick,
        );
        group = group.child(field(id, None, row, cx));
    }
    group.into_any_element()
}

/// The tools its agent may use, a switch each; and the labels its issues get.
fn tools_section(handle: &Entity<SchedulesPage>, draft: &ScheduleDraft, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let switches = onehand_core::control::tool_names().iter().map(|name| {
        let on = draft.tools.iter().any(|t| t == name);
        let handle = handle.clone();
        let name = *name;
        div().h_flex().child(
            div().flex_none().cursor_pointer().child(
                Switch::new(SharedString::from(format!("schedule-tool-{name}")))
                    .checked(on)
                    .small()
                    .label(name)
                    .on_click(move |on: &bool, window: &mut Window, cx: &mut App| {
                        let on = *on;
                        edit(&handle, move |d| {
                            d.tools.retain(|t| t != name);
                            if on {
                                d.tools.push(name.to_string());
                            }
                        })(window, cx)
                    }),
            ),
        )
    });
    section(
        Some("Tools"),
        Some(
            "What its agent may use of onehand's tools; nothing by default. It never answers a \
             card, a permission or an approval"
                .into(),
        ),
        cx,
    )
    .child(
        div()
            .v_flex()
            .gap_2()
            .children(switches)
            .when(draft.tools.is_empty(), |col| {
                col.child(div().text_xs().text_color(muted).child("None given"))
            }),
    )
    .child(field(
        "Labels",
        about("Given to every issue its agent writes, comma-separated; auto hands each one to unattended runs"),
        text_input(&draft.labels, cx).tall(),
        cx,
    ))
    .into_any_element()
}
