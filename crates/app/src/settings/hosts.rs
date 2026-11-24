//! The Hosts page: the machines a person declared, what each is and has, how
//! it stands, and which projects use it.

use super::{list_row, list_rows, page_head, section};
use crate::controls::text_input;
use crate::dialogs::{host_choices, host_menu};
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement,
    SharedString, Styled, Subscription, Task, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::config::drive::HostProbe;
use onehand_core::config::{HostSpec, add_refusal, listed, removal_refusal};
use onehand_core::host::drive::HostState;
use onehand_core::host::probe::Tool;
use onehand_core::host::sshconf;
use onehand_core::workspace::HostName;
use onehand_ui::{Refuses as _, Short as _, Tall as _};
use std::rc::Rc;

/// The field a host is added by.
pub struct HostDraft {
    pub name: Entity<InputState>,
    /// The aliases of the SSH config, read once as the window opens, for the
    /// menu beside the field.
    pub aliases: Vec<String>,
    /// The host *Add* last asked after, whose probe is said under the field
    /// until it lands: there is no row to say it on before it is declared.
    pub adding: Option<HostName>,
    _aliases: Task<()>,
    _typing: Subscription,
}

impl HostDraft {
    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        // The page is the shell's to draw, and *Add* refuses by what the
        // field holds, so each key draws it again.
        let typing = cx.subscribe_in(
            &name,
            window,
            |_: &mut Shell, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );
        // A file read, so off the UI thread; the menu waits for it.
        let reading = cx.background_executor().spawn(async {
            sshconf::ssh_dir().map_or_else(Vec::new, |dir| sshconf::aliases(&dir))
        });
        let aliases = cx.spawn(async move |shell, cx| {
            let aliases = reading.await;
            let _ = shell.update(cx, |shell: &mut Shell, cx| {
                shell.host_draft_mut().aliases = aliases;
                cx.notify();
            });
        });
        Self {
            name,
            aliases: Vec::new(),
            adding: None,
            _aliases: aliases,
            _typing: typing,
        }
    }
}

/// What a host's row says under its name.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HostLines {
    /// The system, then what the last probe found.
    pub about: String,
    /// A fact about the host that asks nothing of the person.
    pub note: Option<String>,
    /// What is wrong with it.
    pub warning: Option<String>,
    pub users: Option<String>,
}

/// The lines of `spec`'s row, from its last `probe`, how its attach stands
/// (`state`, with whether it is being tried again) and the projects naming it.
pub(crate) fn host_lines(
    spec: &HostSpec,
    probe: Option<&HostProbe>,
    state: &HostState,
    retrying: bool,
    users: &[String],
) -> HostLines {
    let system = spec.system();
    let about = match probe {
        Some(HostProbe::Running) => format!("{system} · Checking…"),
        Some(HostProbe::Found(found)) if !found.tools.is_empty() => {
            let tools: Vec<String> = found
                .tools
                .iter()
                .map(|tool| match version(tool) {
                    Some(version) => format!("{} {version}", tool.name),
                    None => tool.name.clone(),
                })
                .collect();
            format!("{system} · {}", tools.join(", "))
        }
        Some(HostProbe::Found(_) | HostProbe::Failed(_)) | None => system,
    };
    let unmet = match probe {
        Some(HostProbe::Failed(why)) => Some(why.clone()),
        Some(HostProbe::Found(found)) => match found.missing().as_slice() {
            [] => None,
            [one] => Some(format!("{one} is not installed")),
            many => Some(format!("{} are not installed", many.join(" and "))),
        },
        Some(HostProbe::Running) | None => None,
    };
    // How its attach stands matters only for a host sessions run on.
    let attach = match state {
        _ if !spec.runs_sessions() => None,
        HostState::Lost(_) => Some(crate::hosts::not_connected(&spec.name, retrying)),
        HostState::Taken => Some(format!("{} was attached from another machine", spec.name)),
        HostState::Unknown | HostState::Attaching | HostState::Live => None,
    };
    HostLines {
        about,
        // A host no session runs on builds, unless its system is one this
        // build does not know.
        note: (!spec.runs_sessions()).then(|| match spec.builds() {
            true => "Builds only".into(),
            false => "Builds only; nothing builds on it yet".into(),
        }),
        warning: unmet.or(attach),
        users: (!users.is_empty()).then(|| format!("Used by {}", users.join(", "))),
    }
}

/// The version to say of a program a host has: the first word of what it
/// said that starts with a digit. The two every build needs are said by name
/// alone, since which version of them a host has decides nothing.
fn version(tool: &Tool) -> Option<&str> {
    if tool.needed() {
        return None;
    }
    tool.said
        .split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
}

/// Every host the open windows' projects use, to run on or to build on, with
/// those projects' names.
pub(crate) fn host_users(cx: &App) -> Vec<(HostName, Vec<String>)> {
    let mut all: Vec<(HostName, Vec<String>)> = Vec::new();
    for (_, shell) in crate::windows::live(cx) {
        for (host, project) in shell.read(cx).host_users() {
            match all.iter_mut().find(|(seen, _)| *seen == host) {
                Some((_, users)) if !users.contains(&project) => users.push(project),
                Some(_) => {}
                None => all.push((host, vec![project])),
            }
        }
    }
    all
}

fn host_row(
    shell: &Entity<Shell>,
    idx: usize,
    spec: &HostSpec,
    declared: bool,
    users: &[String],
    cx: &App,
) -> AnyElement {
    let settings = &crate::state::Shared::global(cx).settings;
    let probe = settings.host_probe(&spec.name);
    let lines = host_lines(
        spec,
        probe,
        &crate::hosts::state(cx, &spec.name),
        crate::hosts::retrying(cx, &spec.name),
        users,
    );
    let ink = crate::theme::status_ink(cx);
    let about = div()
        .v_flex()
        .gap_0p5()
        .child(lines.about)
        .children(lines.note)
        .children(
            lines
                .warning
                .map(|line| div().text_color(ink.warning).child(line)),
        )
        .children(lines.users)
        .into_any_element();
    let running = probe == Some(&HostProbe::Running);
    let refusal = removal_refusal(&spec.name, users);
    let (check, remove) = (spec.name.clone(), spec.name.clone());
    list_row(
        spec.name.to_string(),
        Some(about),
        div()
            .h_flex()
            .items_center()
            .gap_2()
            // A fact about the row, as *Default* is on an agent: a project
            // names this host and nobody declared it.
            .when(!declared, |row| {
                row.child(
                    onehand_ui::badge(
                        "Not declared",
                        onehand_ui::Tint {
                            fill: cx.theme().secondary,
                            ink: cx.theme().secondary_foreground,
                        },
                    )
                    .flex_none(),
                )
            })
            // Asked for, never run on its own: opening Settings must not
            // reach out to every host in the list.
            .child(
                crate::controls::action(("check-host", idx))
                    .secondary()
                    .short()
                    .label("Check")
                    .loading(running)
                    .tooltip(match declared {
                        true => "Ask the host what it is and what it has",
                        false => "Ask the host what it is, and declare it",
                    })
                    .on_click({
                        let shell = shell.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            let host = check.clone();
                            shell.update(cx, |shell, cx| shell.check_host(host, window, cx));
                        }
                    }),
            )
            .when(declared, |row| {
                row.child(
                    crate::controls::action(("remove-host", idx))
                        .secondary()
                        .short()
                        .text_color(ink.danger)
                        .label("Remove…")
                        .refuses(refusal)
                        .on_click({
                            let shell = shell.clone();
                            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                let host = remove.clone();
                                shell.update(cx, |shell, cx| {
                                    shell.confirm_remove_host(host, window, cx)
                                });
                            }
                        }),
                )
            }),
        cx,
    )
    .into_any_element()
}

/// The field a host is added by, its menu of SSH aliases, and what the probe
/// of the host being added says until it lands.
fn add_section(handle: &Entity<Shell>, declared: &[HostSpec], cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let draft = shell.host_draft();
    let settings = &crate::state::Shared::global(cx).settings;
    let adding = draft
        .adding
        .as_ref()
        .and_then(|host| Some((host, settings.host_probe(host)?)));
    let running = match adding {
        Some((_, HostProbe::Running)) => true,
        Some((_, HostProbe::Found(_) | HostProbe::Failed(_))) | None => false,
    };
    let said = adding.and_then(|(host, probe)| match probe {
        HostProbe::Running => Some((format!("Checking {host}…"), cx.theme().muted_foreground)),
        HostProbe::Failed(why) => Some((
            format!("{host} was not added: {why}"),
            crate::theme::status_ink(cx).danger,
        )),
        HostProbe::Found(_) => None,
    });
    // What is declared already is not offered again.
    let fresh = draft
        .aliases
        .iter()
        .filter(|alias| declared.iter().all(|host| host.name.0 != **alias))
        .cloned()
        .collect();
    let (choices, more) = host_choices(fresh, Vec::new(), &[]);
    let refusal = add_refusal(&draft.name.read(cx).value(), declared);
    let (pick, add) = (handle.clone(), handle.clone());
    section(
        Some("Add a host"),
        Some(SharedString::from(
            "An alias from your SSH config, or user@machine",
        )),
        cx,
    )
    .gap_2()
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text_input(&draft.name, cx).tall()),
            )
            .when(!choices.is_empty(), |row| {
                row.child(host_menu(
                    choices,
                    more,
                    Rc::new(move |host, window, cx| {
                        pick.update(cx, |shell, cx| shell.pick_declared_host(host, window, cx));
                    }),
                    cx,
                ))
            })
            .child(
                crate::controls::action("add-host")
                    .tall()
                    .secondary()
                    .label("Add")
                    .loading(running)
                    .refuses(refusal)
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        add.update(cx, |shell, cx| shell.add_host(window, cx));
                    }),
            ),
    )
    .children(said.map(|(line, ink)| div().text_xs().text_color(ink).child(line)))
    .into_any_element()
}

/// The Hosts page.
pub(super) fn hosts_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let declared = crate::state::Shared::global(cx).settings.hosts.clone();
    let users = host_users(cx);
    let named: Vec<HostName> = users.iter().map(|(host, _)| host.clone()).collect();
    let all = listed(&declared, &named);
    let more = all.len().saturating_sub(super::pages::LIST_SHOWN);
    let rows = all
        .iter()
        .take(super::pages::LIST_SHOWN)
        .enumerate()
        .map(|(idx, (spec, is_declared))| {
            let users = users
                .iter()
                .find(|(host, _)| *host == spec.name)
                .map_or(&[][..], |(_, users)| users.as_slice());
            host_row(handle, idx, spec, *is_declared, users, cx)
        })
        .collect::<Vec<_>>();
    let muted = cx.theme().muted_foreground;
    let list = section(None, None, cx)
        .gap_3()
        .when(!rows.is_empty(), |list| list.child(list_rows(rows, cx)))
        .children(super::more_not_shown(more, cx))
        .when(all.is_empty(), |list| {
            list.child(
                div()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .text_color(muted)
                    .child("No hosts yet"),
            )
        });
    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "The machines projects run on and build on, shared by every workspace",
            super::APP,
            cx,
        ))
        .child(list)
        .child(add_section(handle, &declared, cx))
        .into_any_element()
}
