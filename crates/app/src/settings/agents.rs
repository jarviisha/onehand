use super::{APP, list_row, list_rows, page_head, section};
use crate::controls::Named as _;
use crate::controls::Short as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::config::{AgentSpec, Preset, Runs};
use onehand_ui::Tall as _;

/// What a row's code line says an agent runs: a built-in agent's adapter, or
/// its own program when it speaks the protocol itself, or a declared agent's
/// command line, with the variables it sets in front, as a shell would read
/// them.
pub(super) fn runs_line(spec: &AgentSpec) -> String {
    // The entry as the file names it, which is all this build can say of it.
    if let Some(id) = spec.unknown_preset() {
        return format!("preset = \"{id}\"");
    }
    let env = spec
        .env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    let env = onehand_core::config::join_args(&env);
    let built_in = spec.preset().map(|preset| {
        let info = preset.info();
        match info.runs {
            Runs::Adapter { .. } => spec.adapter().unwrap_or_default(),
            Runs::Native { args, .. } => std::iter::once(info.binary)
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
        }
    });
    match built_in {
        Some(runs) if env.is_empty() => format!("Built in · {runs}"),
        Some(runs) => format!("Built in · {runs} · {env}"),
        None => [env, spec.command.clone(), spec.args_line()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// What the *Kind* field says of the choice made: how a built-in agent
/// runs, or what any other agent has to be.
pub(crate) fn kind_about(kind: Option<Preset>) -> SharedString {
    let Some(preset) = kind else {
        return "Any other agent that speaks the Agent Client Protocol".into();
    };
    let info = preset.info();
    match info.runs {
        Runs::Adapter { .. } => format!(
            "Runs the {} on this machine through the adapter onehand was tried with",
            info.binary
        ),
        Runs::Native { .. } => format!(
            "Runs the {} on this machine, which speaks the protocol itself",
            info.binary
        ),
    }
    .into()
}

/// One row in the agent list: name + what it runs, with edit and delete actions.
fn agent_row(shell: &Entity<Shell>, idx: usize, spec: &AgentSpec, cx: &App) -> impl IntoElement {
    let line = SharedString::from(runs_line(spec));
    // A version the person chose over the one this release was tried with is
    // theirs to keep, and said once under the line rather than refused.
    let outside = spec.outside_pin().then(|| {
        format!(
            "Runs adapter {}, outside the version this release was tried with",
            spec.version.as_deref().unwrap_or_default()
        )
    });
    // One a newer build wrote: kept as it is, and said under the line, since
    // it cannot start until onehand is updated or it is removed.
    let unknown = spec.unknown_preset().is_some();
    let outside =
        outside.or_else(|| unknown.then(|| "Not supported by this build of onehand".to_string()));
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
            AgentCheck::Found(at, version) => {
                let what = match (spec.preset(), version) {
                    (Some(_), Some(version)) => version.clone(),
                    (Some(preset), None) => preset.info().binary.to_string(),
                    (None, _) => spec.command.clone(),
                };
                (format!("{what} found at {}", at.display()), muted)
            }
            // A built-in agent is missing in the words a start would use.
            AgentCheck::Missing => (
                match spec.launch(Some(std::ffi::OsStr::new(""))) {
                    Err(why) if spec.preset().is_some() => why,
                    _ => format!(
                        "{} was not found — check the command, or give its full path",
                        spec.command
                    ),
                },
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
        .children(outside.map(|line| {
            div()
                .text_xs()
                .text_color(crate::theme::status_ink(cx).warning)
                .child(line)
        }))
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
                    onehand_ui::badge(
                        "Default",
                        onehand_ui::Tint {
                            fill: cx.theme().secondary,
                            ink: cx.theme().secondary_foreground,
                        },
                    )
                    .flex_none(),
                )
            })
            .when(!is_default && !unknown, |row| {
                row.child(
                    crate::controls::action(("default-agent", idx))
                        .secondary()
                        .short()
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
            // looking through the disk for every agent in the list. An entry
            // this build does not know has nothing to test or edit: only its
            // removal is offered.
            .when(!unknown, |row| {
                row.child(
                    crate::controls::action(("check-agent", idx))
                        .secondary()
                        .short()
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
                        // rather than as opening it in the form.
                        .icon(Icon::new(crate::icons::Icon::SquarePen))
                        .named("Edit")
                        .on_click({
                            let shell = shell.clone();
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                shell.update(cx, |shell, cx| {
                                    shell.open_agent_form(Some(idx), window, cx)
                                });
                            }
                        }),
                )
            })
            // A word in the danger tint rather than a trash glyph, like every
            // other control here that removes something for good.
            .child(
                crate::controls::action(("delete-agent", idx))
                    .secondary()
                    .short()
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

/// What a *Test* result is filed under: everything that decides what starts,
/// so two agents that share a launcher and differ in their arguments are two
/// entries, and an edit to any of it drops the answer about the old one.
pub fn check_key(spec: &AgentSpec) -> String {
    format!(
        "{:?}\0{:?}\0{}\0{}\0{:?}",
        spec.preset,
        spec.version,
        spec.command,
        spec.args_line(),
        spec.env
    )
}

/// What the last *Test* of an agent's command found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentCheck {
    Running,
    /// Where it was found, and the version a built-in agent's binary gave.
    Found(std::path::PathBuf, Option<String>),
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

/// The agent page: the global agent menu a new session spawns from, and
/// the way into the form that adds to it.
pub(super) fn agents_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let specs = shell.agents(cx).to_vec();

    let more = specs.len().saturating_sub(super::pages::LIST_SHOWN);
    let rows = specs
        .iter()
        .take(super::pages::LIST_SHOWN)
        .enumerate()
        .map(|(i, spec)| agent_row(handle, i, spec, cx).into_any_element())
        .collect::<Vec<_>>();
    let add = handle.clone();

    // A built-in agent installed here with no entry of its own, offered rather
    // than added behind the person's back: a list someone set up is theirs.
    let offered = crate::state::Shared::global(cx)
        .on_machine
        .iter()
        .copied()
        .filter(|preset| specs.iter().all(|spec| spec.preset() != Some(*preset)))
        .map(|preset| {
            let shell = handle.clone();
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .text_size(crate::tokens::CHROME_TEXT)
                .child(format!("{} is on this machine", preset.info().shown_name))
                .child(
                    crate::controls::action(("add-found-agent", preset as usize))
                        .secondary()
                        .short()
                        .label("Add")
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| shell.add_found_agent(preset, window, cx));
                        }),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();

    let list = section(None, None, cx)
        .gap_3()
        .when(!rows.is_empty(), |list| list.child(list_rows(rows, cx)))
        .children(offered)
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
                    .text_size(crate::tokens::CHROME_TEXT)
                    .text_color(cx.theme().muted_foreground)
                    .child("No agents yet"),
            )
        })
        .child(
            div().h_flex().child(
                crate::controls::action("add-agent")
                    .tall()
                    .secondary()
                    .label("Add agent…")
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        add.update(cx, |shell, cx| shell.open_agent_form(None, window, cx));
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
        .into_any_element()
}
