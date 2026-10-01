use super::{APP, about, field, list_row, page_head, section};
use crate::controls::Refuses as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::Input;
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
            AgentCheck::Found(at) => (
                format!("{} found at {}", spec.command, at.display()),
                ink.success,
            ),
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

    list_row(
        spec.name.clone(),
        Some(about),
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .when(is_default, |row| {
                row.child(div().px_2().text_xs().text_color(muted).child("Default"))
            })
            .when(!is_default, |row| {
                row.child(
                    crate::controls::action(("default-agent", idx))
                        .ghost()
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
                    .ghost()
                    .small()
                    .label("Test")
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
                    .ghost()
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
            .child(
                crate::controls::action(("delete-agent", idx))
                    .ghost()
                    // Not the bundled `delete`, which is the backspace *key* --
                    // "erase the character behind the caret", drawn beside a
                    // button that removes a saved agent for good.
                    .icon(Icon::new(crate::icons::Icon::Trash))
                    .tooltip("Delete")
                    .on_click({
                        let shell = shell.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| shell.delete_agent(idx, window, cx));
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

    let rows = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| agent_row(handle, i, spec, cx).into_any_element())
        .collect::<Vec<_>>();
    let (clear, save) = (handle.clone(), handle.clone());

    let list = section(None, None, cx)
        .gap_3()
        .children(rows)
        .when(specs.is_empty(), |list| {
            list.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No agents yet — add one below."),
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
        about("What the New session menu calls it."),
        Input::new(&name),
        cx,
    ))
    .child(field(
        "Command",
        about(
            "The program to run. It has to speak the Agent Client Protocol over standard \
             input and output.",
        ),
        Input::new(&command),
        cx,
    ))
    .child(field(
        "Arguments",
        about("Separated by spaces; quote one that holds a space."),
        Input::new(&args),
        cx,
    ))
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                crate::controls::action("save-agent")
                    .primary()
                    // Disabled until name and command are both non-blank: an
                    // agent missing either cannot be launched.
                    .refuses(!saveable)
                    .label(if editing { "Save" } else { "Add" })
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        save.update(cx, |shell, cx| shell.save_agent_draft(window, cx));
                    }),
            )
            .child(
                crate::controls::action("clear-agent")
                    .ghost()
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
            "Agents",
            "The menu every new session starts from, shared by every workspace.",
            APP,
            cx,
        ))
        .child(list)
        .child(form)
        .into_any_element()
}
