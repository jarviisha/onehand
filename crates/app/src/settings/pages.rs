use super::{APP, about, field, list_row, page_head, section};
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::config::Appearance;
use onehand_core::unattended::Slots;

/// How many entries a Settings list draws before it counts the rest.
pub(super) const LIST_SHOWN: usize = 50;

/// The field a new workflow label is typed into: a label is a word or two.
const LABEL_FIELD_W: gpui::Rems = gpui::rems(10.);

/// The light/dark/system picker.
///
/// One value with three answers, drawn as the app draws every pick-one row: a
/// [`choice`](onehand_ui::choice) per answer, the one in force filled.
///
/// App-wide, unlike everything else on the page it sits in, because the theme
/// it selects is a global: two windows cannot be drawn in two modes.
fn appearance_picker(
    shell: &Entity<Shell>,
    current: Appearance,
    cx: &App,
) -> impl IntoElement + use<> {
    div()
        .h_flex()
        .gap_1()
        .children(Appearance::ALL.into_iter().enumerate().map(|(i, choice)| {
            let shell = shell.clone();
            onehand_ui::choice(
                ("appearance", i),
                choice.label(),
                choice == current,
                onehand_ui::ControlSize::Short,
                cx,
            )
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.set_appearance(choice, window, cx));
            })
        }))
}

/// The appearance page. One control, and it is app-wide -- said on the page,
/// since every other page here is about one workspace and nothing else marks
/// the difference.
pub(super) fn appearance_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let current = handle.read(cx).appearance(cx);
    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "How onehand looks, in every window at once",
            APP,
            cx,
        ))
        .child(section(None, None, cx).child(field(
            "Theme",
            about("Following the system keeps up with it as it changes"),
            appearance_picker(handle, current, cx),
            cx,
        )))
        .into_any_element()
}

/// The workspace page: this workspace's name and storage binding, then the two
/// ways to reach another one, then the unattended runs it may start.
///
/// No workspace is ever *replaced* in place -- one window hosts exactly one
/// workspace, so both of those buttons open another window, or focus the one
/// already showing that folder.
///
/// **The list of known workspaces is deliberately not here.** The rail's
/// identity row is the switcher, and it draws the same directories better: a
/// row named by its folder with the parent beside it, the one on screen checked
/// and unpickable. A second copy here printed each absolute path whole into a
/// button, uncapped, so the longer the app was used the further it pushed
/// everything else off the bottom. What stays is the binding, which has nowhere
/// else to live.
pub(super) fn workspace_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let name = shell.workspace_name_input().clone();
    let storage = shell
        .storage_dir()
        .map(|dir| SharedString::from(dir.display().to_string()));
    let bound = storage.is_some();
    let (bind, unbind, new, open) = (
        handle.clone(),
        handle.clone(),
        handle.clone(),
        handle.clone(),
    );
    // An unbound workspace persists nothing; say so rather than showing a
    // blank where the folder would be.
    let storage = match storage {
        Some(dir) => div()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .child(dir)
            .into_any_element(),
        None => "Not bound — nothing about this workspace is saved".into_any_element(),
    };

    let general = section(None, None, cx)
        .child(field(
            "Name",
            about("Written at the top of the rail"),
            text_input(&name, cx).short(),
            cx,
        ))
        .child(field(
            "Storage folder",
            Some(storage),
            div()
                .h_flex()
                .gap_2()
                .child(
                    crate::controls::action("bind-storage")
                        .short()
                        .secondary()
                        .label("Choose folder…")
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            bind.update(cx, |shell, cx| shell.pick_storage_dir(cx));
                        }),
                )
                .child(
                    crate::controls::action("unbind-storage")
                        .short()
                        .secondary()
                        .refuses((!bound).then_some("No storage folder is bound"))
                        .label("Unbind")
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            unbind.update(cx, |shell, cx| shell.unbind_storage(window, cx));
                        }),
                ),
            cx,
        ));
    let others = section(
        Some("Other workspaces"),
        Some(
            "Each opens in a window of its own; one already on screen is brought forward \
             instead, and this window's workspace is left as it is"
                .into(),
        ),
        cx,
    )
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                crate::controls::action("new-workspace")
                    .short()
                    .secondary()
                    .label("New workspace…")
                    .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        new.update(cx, |shell, cx| shell.new_workspace(cx));
                    }),
            )
            .child(
                crate::controls::action("open-workspace")
                    .short()
                    .secondary()
                    .label("Open workspace…")
                    .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        open.update(cx, |shell, cx| shell.open_workspace(cx));
                    }),
            ),
    );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        // The scope is the workspace, not its name: the field below says that.
        .child(page_head(
            "This window's workspace: its name, where it is kept, and the runs it may start",
            "Workspace",
            cx,
        ))
        .child(general)
        .child(others)
        .child(unattended_section(handle, cx))
        .into_any_element()
}

/// The connections page: each connector, whether it is signed in and as whom.
///
/// A list of rows and not one line per connector under the runs that use
/// them, since the list is meant to grow and a connection is a fact about the
/// machine rather than about any one feature.
pub(super) fn connections_page(cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = crate::theme::status_ink(cx);
    let accounts = crate::unattended::accounts(cx);
    let checking = crate::unattended::accounts_checking(cx);
    // ponytail: read when the page is drawn, so it goes stale while nothing
    // redraws it; every check that lands redraws, which is when it matters.
    let checked = crate::unattended::accounts_checked_at(cx).map(|at| {
        let secs = |t: std::time::SystemTime| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        };
        crate::chat::pane::rel_time(secs(std::time::SystemTime::now()), secs(at))
    });
    let rows = crate::features::connectors()
        .iter()
        .map(|connector| {
            // Each connector's own answer, or *checking* until the first look
            // lands; what to do about a failure is the failure's own words.
            let (status, ink) = match accounts.as_ref().and_then(|all| {
                all.iter()
                    .find(|(c, _)| c.name() == connector.name())
                    .map(|(_, account)| account.clone())
            }) {
                None => ("Checking…".to_string(), muted),
                // Signed in is the state at rest: muted, as a fact is. Only a
                // failure, which needs the person, takes an ink.
                Some(Ok(who)) => (who, muted),
                Some(Err(why)) => (why, ink.warning),
            };
            list_row(
                connector.name(),
                Some(div().text_color(ink).child(status).into_any_element()),
                div(),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "The services onehand reaches outside the checkout, and who it acts as on each",
            APP,
            cx,
        ))
        .child(
            section(None, None, cx).gap_3().children(rows).child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .child(
                        crate::controls::action("connections-recheck")
                            .short()
                            .secondary()
                            .loading(checking)
                            .label("Check again")
                            .on_click(|_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                crate::unattended::recheck(cx);
                            }),
                    )
                    .children(checked.map(|when| {
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Last checked {when}"))
                    })),
            ),
        )
        .child(outside_section(cx))
        .into_any_element()
}

/// Whether a program outside onehand may use its tools over the socket.
///
/// On the connections page because it is one, the other way round: something
/// on this machine reaching onehand rather than onehand reaching a service.
/// What it may do is said beside the switch, and what it may never do, since
/// turning it on is the moment that is decided.
fn outside_section(cx: &App) -> AnyElement {
    let socket = onehand_core::instance::socket_path(&onehand_core::config::config_dir());
    section(
        Some("Outside control"),
        Some(SharedString::from(
            "A program you run — a script, a scheduled job — may use onehand's tools over its \
             socket: list projects, agents, workflows and tasks, list and write a project's \
             issues, open a session, start a workflow or an issue, stop a task and open a \
             file. It never answers a card, a permission or an approval; those wait for you",
        )),
        cx,
    )
    // The socket's path in the face a path is set in, as the config file's is.
    .child(
        div()
            .h_flex()
            .flex_wrap()
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Socket")
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(socket.display().to_string()),
            ),
    )
    // Held to its own width, as the unattended switches are, so the space
    // beside the label is not a target, and given the pointer the switch does
    // not set itself.
    .child(
        div().h_flex().child(
            div().flex_none().cursor_pointer().child(
                Switch::new("control-outside")
                    .checked(crate::control::outside())
                    .small()
                    .label("Let programs on this machine control onehand")
                    .on_click(|on: &bool, window: &mut Window, cx: &mut App| {
                        crate::control::set_outside(*on, window, cx);
                    }),
            ),
        ),
    )
    .into_any_element()
}

/// The per-project switch for unattended runs, as a list.
///
/// The same switch the project's own menu carries, gathered in one place so
/// every project's answer can be read at once — the menu shows one project's,
/// and only after it is opened. The label a run looks for is named, since it is
/// the one thing a user has to put on an issue and it lives in the config file.
fn unattended_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let label = crate::unattended::label(cx);
    let choices = handle.read(cx).unattended_choices();
    let ink = crate::theme::status_ink(cx);
    let empty = choices.is_empty();
    let config = crate::state::Shared::global(cx)
        .config_path
        .display()
        .to_string();

    section(
        Some("Unattended runs"),
        Some(SharedString::from(format!(
            "An issue you opened, labelled “{label}” — on the project's forge or in its \
             Issues tab — in a project switched on here is picked up by an agent, worked in \
             a worktree of its own, and answered with a pull request, or with commits on its \
             branch where the project has no forge. The switches are this workspace's; the \
             label and how often to look are the app's, set in its config file, and so is \
             the agent a run uses — the default agent unless that file names another"
        ))),
        cx,
    )
    // The config file's path in the face a path is set in.
    .child(
        div()
            .h_flex()
            .flex_wrap()
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Config file")
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(config),
            ),
    )
    // The search on demand, rather than at the next tick half an hour away. It
    // looks where the tick would, and does nothing the tick would not.
    .child(field(
        "Search",
        about(
            "Look for a labelled issue now rather than at the next scheduled look — in every \
             open workspace, as the scheduled look does; runs reach the forge through the \
             Connections page",
        ),
        div().h_flex().child(
            crate::controls::action("unattended-look-now")
                .short()
                .secondary()
                .label("Look for an issue now")
                .loading(crate::unattended::claiming(cx))
                .on_click(|_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    crate::unattended::look_now(window.window_handle(), cx);
                }),
        ),
        cx,
    ))
    .children(crate::unattended::slots(cx).map(|slots| slots_field(handle, slots, cx)))
    .child(field(
        "Workflow",
        about(
            "What an issue is worked with, unless one of its workflow labels below names \
             another; only workflows that work on a worktree of their own are offered",
        ),
        div().h_flex().child({
            let shell = handle.clone();
            let (default, _) = crate::unattended::workflows(cx);
            crate::dialogs::issue_workflow_menu(
                "unattended-workflow",
                Some(&default),
                None,
                move |id, window, cx| {
                    if let Some(id) = id {
                        shell.update(cx, |shell, cx| {
                            shell.set_default_issue_workflow(id, window, cx)
                        });
                    }
                },
                cx,
            )
        }),
        cx,
    ))
    .child(field(
        "Workflow labels",
        about(
            "A label on an issue, beside the trigger label, that chooses its workflow; an \
             issue carrying several takes the first in this list",
        ),
        workflow_labels(handle, cx),
        cx,
    ))
    // Whatever stops every run, said above the switches in the warning ink:
    // with it unsaid they would look as though they did something.
    .when_some(crate::unattended::blocked(cx), |group, why| {
        group.child(
            div()
                .text_xs()
                .text_color(ink.warning)
                .child(format!("Nothing will be picked up: {why}")),
        )
    })
    .child(field(
        "Projects",
        empty.then(|| "No project in this workspace can be switched on".into_any_element()),
        div()
            .v_flex()
            .gap_3()
            .children(choices.into_iter().take(LIST_SHOWN).map(|(idx, name, on)| {
                let shell = handle.clone();
                // The switch sets no cursor of its own, and an arrow over a
                // control that acts reads as one that does not. Held to its own
                // width, so the empty space to the right of the name is not a
                // target.
                div().h_flex().child(
                    div().flex_none().cursor_pointer().child(
                        Switch::new(("unattended", idx))
                            .checked(on)
                            .small()
                            .label(name)
                            .on_click(move |_: &bool, window: &mut Window, cx: &mut App| {
                                shell.update(cx, |shell, cx| {
                                    shell.toggle_unattended(idx, window, cx)
                                });
                            }),
                    ),
                )
            })),
        cx,
    ))
    .into_any_element()
}

/// Who holds each slot of unattended runs, across every window: the slots
/// line, then each holder as a link to its task, and why nothing more is
/// taken up when the caps say so. Read as it stands; nothing is stored.
fn slots_field(handle: &Entity<Shell>, slots: Slots, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let shown = onehand_core::unattended::HOLDERS_SAID;
    let left_out = slots.holders.len().saturating_sub(shown);
    let holders = slots
        .holders
        .iter()
        .take(shown)
        .enumerate()
        .map(|(at, holder)| {
            let (shell, task) = (handle.clone(), holder.task.clone());
            crate::controls::action(("unattended-slot", at))
                .secondary()
                .short()
                .label(holder.shown.clone())
                .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    shell.update(cx, |shell, cx| shell.show_task(&task, window, cx));
                })
        });
    let waiting = match slots.waiting_cap {
        Some(cap) => format!("{} of {cap} may wait on a person", slots.waiting),
        None => format!("{} waiting on a person, no cap", slots.waiting),
    };
    let starting = (slots.starting > 0).then(|| format!("{} starting", slots.starting));
    field(
        "Slots",
        about(
            "Issue runs working or queued, across every window, against unattended.at_once; \
             a run waiting on a person holds none, and is counted against \
             unattended.waiting when that is set",
        ),
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .child(slots.count_said()),
            )
            .child(div().h_flex().flex_wrap().gap_2().children(holders))
            .children((left_out > 0).then(|| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("and {left_out} more"))
            }))
            .children(starting.map(|line| div().text_xs().text_color(muted).child(line)))
            .child(div().text_xs().text_color(muted).child(waiting))
            .children(slots.full().is_some().then(|| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("No other issue is taken up until one of these makes room")
            })),
        cx,
    )
    .into_any_element()
}

/// The workflow labels, each with the workflow it chooses and *Remove*, then
/// a row to add one: the label, its workflow, *Add*, and why the last *Add*
/// was refused. A label naming a workflow that is gone shows its id in the
/// warning ink, since its issues will not start.
fn workflow_labels(handle: &Entity<Shell>, cx: &App) -> impl IntoElement {
    let (_, by_label) = crate::unattended::workflows(cx);
    let offered = crate::unattended::issue_workflows(cx);
    let (input, picked, refused) = {
        let (input, picked, refused) = handle.read(cx).label_workflow();
        (
            input,
            picked.map(str::to_string),
            refused.map(str::to_string),
        )
    };
    let ink = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    let more = by_label.len().saturating_sub(LIST_SHOWN);
    let rows = by_label
        .into_iter()
        .take(LIST_SHOWN)
        .enumerate()
        .map(|(i, (label, id))| {
            let named = offered.iter().find(|(offered, _, _)| *offered == id);
            let shell = handle.clone();
            let remove = label.clone();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .text_size(crate::tokens::CHROME_TEXT)
                .child(div().flex_none().child(label))
                .child(Icon::new(IconName::ArrowRight).small().text_color(muted))
                .child(match named {
                    Some((_, name, _)) => div().flex_1().min_w_0().truncate().child(name.clone()),
                    None => div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(ink.warning)
                        .child(format!("{id} (no such workflow on a worktree)")),
                })
                .child(
                    crate::controls::action(("workflow-label-remove", i))
                        .secondary()
                        .short()
                        .label("Remove")
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            shell.update(cx, |shell, cx| {
                                shell.remove_workflow_label(&remove, window, cx)
                            });
                        }),
                )
        });
    let pick = {
        let shell = handle.clone();
        crate::dialogs::issue_workflow_menu(
            "workflow-label-pick",
            picked.as_deref(),
            Some("Choose a workflow"),
            move |id, _, cx| shell.update(cx, |shell, cx| shell.pick_label_workflow(id, cx)),
            cx,
        )
    };
    let add = {
        let shell = handle.clone();
        crate::controls::action("workflow-label-add")
            .secondary()
            .short()
            .label("Add")
            .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.add_workflow_label(window, cx));
            })
    };
    div()
        .v_flex()
        .gap_2()
        .children(rows)
        .children((more > 0).then(|| {
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("{more} more not shown"))
        }))
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().w(LABEL_FIELD_W).child(text_input(&input, cx).short()))
                .child(pick)
                .child(add),
        )
        .when_some(refused, |list, why| {
            list.child(div().text_xs().text_color(ink.warning).child(why))
        })
}
