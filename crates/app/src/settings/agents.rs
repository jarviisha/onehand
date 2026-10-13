use super::{APP, about, field, list_row, page_head, section};
use crate::controls::Refuses as _;
use crate::controls::Tall as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::config::AgentSpec;

/// One row in the agent list: name + command, with edit and delete actions.
fn agent_row(shell: &Entity<Shell>, idx: usize, spec: &AgentSpec, cx: &App) -> impl IntoElement {
    let line = SharedString::from(if spec.args.is_empty() {
        spec.command.clone()
    } else {
        format!("{} {}", spec.command, spec.args.join(" "))
    });
    let ink = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    // What the last check of this agent found, said under the command it is
    // about. Keyed by the command line rather than the row, so it follows the
    // agent through a reorder and is dropped the moment the line is edited. It
    // names the program because the program is all it looked for.
    let check = shell
        .read(cx)
        .agent_check(&check_key(spec))
        .map(|check| match check {
            AgentCheck::Running => ("Checking…".to_string(), muted),
            // Found is the state at rest: muted, as a fact is.
            AgentCheck::Found(at) => (format!("{} found at {}", spec.command, at.display()), muted),
            AgentCheck::Missing => (
                format!(
                    "{} was not found — check the command, or give its full path",
                    spec.command
                ),
                ink.warning,
            ),
        });
    // A command line is code, and set in the face code is set in so a flag and
    // a path read as what they are.
    let about = div()
        .v_flex()
        .gap_0p5()
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .child(line),
        )
        .children(check.map(|(line, ink)| div().text_xs().text_color(ink).child(line)))
        .into_any_element();
    // The first agent is the one *New session* starts, so being first is what
    // being the default means; there is no second setting to fall out of step
    // with the order.
    let is_default = idx == 0;
    let running = matches!(
        shell.read(cx).agent_check(&check_key(spec)),
        Some(AgentCheck::Running)
    );

    list_row(
        spec.name.clone(),
        Some(about),
        div()
            .h_flex()
            .items_center()
            .gap_2()
            // A badge, as *Built in* is on a workflow: a fact about the row.
            .when(is_default, |row| {
                row.child(
                    onehand_plugin_host::badge(
                        "Default",
                        onehand_plugin_host::Tint {
                            fill: cx.theme().secondary,
                            ink: cx.theme().secondary_foreground,
                        },
                    )
                    .flex_none(),
                )
            })
            .when(!is_default, |row| {
                row.child(
                    crate::controls::action(("default-agent", idx))
                        .secondary()
                        .small()
                        .label("Make default")
                        .on_click({
                            let shell = shell.clone();
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                shell.update(cx, |shell, cx| {
                                    shell.make_default_agent(idx, window, cx)
                                });
                            }
                        }),
                )
            })
            // Asked for, never run on its own: opening Settings must not go
            // looking through the disk for every agent in the list.
            .child(
                crate::controls::action(("check-agent", idx))
                    .secondary()
                    .small()
                    .label("Test")
                    .loading(running)
                    .tooltip("Look for the command without starting it")
                    .on_click({
                        let shell = shell.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| shell.check_agent(idx, cx));
                        }
                    }),
            )
            .child(
                crate::controls::action(("edit-agent", idx))
                    .small()
                    .secondary()
                    // Not the bundled `replace`, which is a find-and-replace
                    // mark: it reads as swapping this agent for another one
                    // rather than as opening it in the form below.
                    .icon(Icon::new(crate::icons::Icon::SquarePen))
                    .tooltip("Edit")
                    .on_click({
                        let shell = shell.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| shell.edit_agent(idx, window, cx));
                        }
                    }),
            )
            // A word in the danger tint rather than a trash glyph, like every
            // other control here that removes something for good.
            .child(
                crate::controls::action(("delete-agent", idx))
                    .secondary()
                    .small()
                    .text_color(ink.danger)
                    .label("Delete…")
                    .on_click({
                        let shell = shell.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| {
                                shell.confirm_delete_agent(idx, window, cx)
                            });
                        }
                    }),
            ),
        cx,
    )
}

/// What a *Test* result is filed under: the whole command line, so two agents
/// that share a launcher and differ in their arguments are two entries, and an
/// edit to either half drops the answer about the old one.
pub fn check_key(spec: &AgentSpec) -> String {
    format!("{}\0{}", spec.command, spec.args_line())
}

/// What the last *Test* of an agent's command found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentCheck {
    Running,
    Found(std::path::PathBuf),
    Missing,
}

/// Where a form open on `editing` belongs once the agent at `promoted` has been
/// moved to the front of the list.
///
/// Same reason as [`draft_shift`](super::draft_shift): the form holds a *position*, and moving one
/// agent to the front shifts every agent that was ahead of it down by one.
pub fn draft_after_promote(editing: Option<usize>, promoted: usize) -> Option<usize> {
    editing.map(|at| match at {
        at if at == promoted => 0,
        at if at < promoted => at + 1,
        at => at,
    })
}

/// The agent page: the global agent menu a new session spawns from, and the
/// form that adds to it.
pub(super) fn agents_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let specs = shell.agents(cx).to_vec();
    let draft = shell.agent_draft();
    let (name, command, args) = (
        draft.name.clone(),
        draft.command.clone(),
        draft.args.clone(),
    );
    let saveable = draft.to_spec(cx).is_some();
    let editing = draft.editing.is_some();

    let more = specs.len().saturating_sub(super::pages::LIST_SHOWN);
    let rows = specs
        .iter()
        .take(super::pages::LIST_SHOWN)
        .enumerate()
        .map(|(i, spec)| agent_row(handle, i, spec, cx).into_any_element())
        .collect::<Vec<_>>();
    let (clear, save) = (handle.clone(), handle.clone());

    let list = section(None, None, cx)
        .gap_3()
        .children(rows)
        .when(more > 0, |list| {
            list.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{more} more not shown")),
            )
        })
        .when(specs.is_empty(), |list| {
            list.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No agents yet"),
            )
        });
    let form = section(
        Some(if editing {
            "Edit agent"
        } else {
            "Add an agent"
        }),
        None,
        cx,
    )
    .child(field(
        "Name",
        about("What the New session menu calls it"),
        text_input(&name, cx).tall(),
        cx,
    ))
    .child(field(
        "Command",
        about(
            "The program to run; it has to speak the Agent Client Protocol over standard \
             input and output",
        ),
        text_input(&command, cx).tall(),
        cx,
    ))
    .child(field(
        "Arguments",
        about("Separated by spaces; quote one that holds a space"),
        text_input(&args, cx).tall(),
        cx,
    ))
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                crate::controls::action("save-agent")
                    .tall()
                    .secondary()
                    // Refused until name and command are both non-blank: an
                    // agent missing either cannot be launched.
                    .refuses((!saveable).then_some("Name it and give its command first"))
                    .label(if editing { "Save" } else { "Add" })
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        save.update(cx, |shell, cx| shell.save_agent_draft(window, cx));
                    }),
            )
            .child(
                crate::controls::action("clear-agent")
                    .tall()
                    .secondary()
                    .label(if editing { "Cancel edit" } else { "Clear" })
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        clear.update(cx, |shell, cx| shell.clear_agent_draft(window, cx));
                    }),
            ),
    );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "The menu every new session starts from, shared by every workspace",
            APP,
            cx,
        ))
        .child(list)
        .child(form)
        .into_any_element()
}
