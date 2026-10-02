use super::{APP, about, field, list_row, page_head, section};
use crate::controls::Refuses as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::{ButtonGroup, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Selectable, StyledExt};
use onehand_core::config::Appearance;

/// The light/dark/system picker.
///
/// A button group rather than three loose buttons: the choice is one value with
/// three answers, and a segmented control is the shape that says so -- one
/// pressed, the others available, no state where none or two are chosen.
///
/// App-wide, unlike everything else on the page it sits in, because the theme
/// it selects is a global: two windows cannot be drawn in two modes.
fn appearance_picker(shell: &Entity<Shell>, current: Appearance) -> impl IntoElement + use<> {
    let shell = shell.clone();
    ButtonGroup::new("appearance")
        .outline()
        .children(Appearance::ALL.into_iter().enumerate().map(|(i, choice)| {
            crate::controls::action(("appearance", i))
                .label(choice.label())
                .selected(choice == current)
        }))
        .on_click(
            move |clicked: &Vec<usize>, window: &mut Window, cx: &mut App| {
                let Some(choice) = clicked
                    .first()
                    .and_then(|i| Appearance::ALL.get(*i))
                    .copied()
                else {
                    return;
                };
                shell.update(cx, |shell, cx| shell.set_appearance(choice, window, cx));
            },
        )
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
            "Appearance",
            "How onehand looks. This applies to every window at once.",
            APP,
            cx,
        ))
        .child(section(None, None, cx).child(field(
            "Theme",
            about("Following the system keeps up with it as it changes."),
            appearance_picker(handle, current),
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
        None => "Not bound — nothing about this workspace is saved.".into_any_element(),
    };

    let general = section(None, None, cx)
        .child(field(
            "Name",
            about("Written at the top of the rail."),
            Input::new(&name),
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
                        .outline()
                        .label("Choose folder…")
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            bind.update(cx, |shell, cx| shell.pick_storage_dir(cx));
                        }),
                )
                .child(
                    crate::controls::action("unbind-storage")
                        .ghost()
                        .refuses(!bound)
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
             instead. This window's workspace is left as it is."
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
                    .outline()
                    .label("New workspace…")
                    .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        new.update(cx, |shell, cx| shell.new_workspace(cx));
                    }),
            )
            .child(
                crate::controls::action("open-workspace")
                    .outline()
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
        .child(page_head(
            "Workspace",
            "This window's workspace: its name, where it is kept, and the runs it may start.",
            format!("Workspace: {}", name.read(cx).value()),
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
    let rows = crate::plugins::connectors()
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
                Some(Ok(who)) => (format!("Signed in as {who}"), ink.success),
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
            "Connections",
            "The services onehand reaches outside the checkout, and who it acts as on each.",
            APP,
            cx,
        ))
        .child(
            section(None, None, cx).gap_3().children(rows).child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .pt_2()
                    .child(
                        crate::controls::action("connections-recheck")
                            .outline()
                            .refuses(checking)
                            .label(if checking {
                                "Checking…"
                            } else {
                                "Check again"
                            })
                            .on_click(|_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                crate::unattended::recheck(cx);
                            }),
                    )
                    .children(checked.map(|when| {
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("Last checked {when}"))
                    })),
            ),
        )
        .into_any_element()
}

/// The mode an issue's sessions start in, as one value with a few answers.
///
/// A button group for the reason the theme picker is one: one choice, one
/// pressed. An id put into the config by hand is shown as a choice of its own
/// rather than as none pressed.
fn mode_picker(shell: &Entity<Shell>, current: &str) -> impl IntoElement + use<> {
    let shell = shell.clone();
    let choices = onehand_core::unattended::mode_choices(current);
    let ids: Vec<String> = choices.iter().map(|(id, _)| id.clone()).collect();
    ButtonGroup::new("issue-mode")
        .outline()
        .children(choices.into_iter().enumerate().map(|(i, (id, name))| {
            crate::controls::action(("issue-mode", i))
                .label(name)
                .selected(id == current)
        }))
        .on_click(
            move |clicked: &Vec<usize>, window: &mut Window, cx: &mut App| {
                let Some(mode) = clicked.first().and_then(|i| ids.get(*i)).cloned() else {
                    return;
                };
                shell.update(cx, |shell, cx| shell.set_issue_mode(mode, window, cx));
            },
        )
}

/// What the chosen mode lets an agent do. Bypass is said in the warning ink:
/// it is the one choice that hands the agent everything, with nobody asked.
fn mode_about(current: &str, cx: &App) -> AnyElement {
    match current {
        "bypassPermissions" => div()
            .text_color(crate::theme::status_ink(cx).warning)
            .child(
                "Bypass runs every command without asking, with your credentials. Nothing \
                 stops a mistake, or an instruction written into an issue.",
            )
            .into_any_element(),
        _ => "For unattended runs and for a session started on an issue from its Issues tab. \
              Auto lets Claude judge each command itself; Accept edits asks before every \
              command; Ask asks before every edit as well. A question about what the issue \
              wants is asked whatever the mode."
            .into_any_element(),
    }
}

/// The per-project switch for unattended runs, as a list.
///
/// The same switch the project's own menu carries, gathered in one place so
/// every project's answer can be read at once — the menu shows one project's,
/// and only after it is opened. The label a run looks for is named, since it is
/// the one thing a user has to put on an issue and it lives in the config file.
fn unattended_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let label = crate::unattended::label(cx);
    let mode = crate::unattended::mode(cx);
    let choices = handle.read(cx).unattended_choices();
    let ink = crate::theme::status_ink(cx);
    let empty = choices.is_empty();

    section(
        Some("Unattended runs"),
        Some(SharedString::from(format!(
            "An issue you opened, labelled “{label}” — on the project's forge or in its \
             Issues tab — in a project switched on here is picked up by an agent, worked in \
             a worktree of its own, and answered with a pull request, or with commits on its \
             branch where the project has no forge. The switches are this workspace's; the \
             mode below is the app's, and so are the label, how often to look and the agent \
             a run uses, set in onehand.toml — the default agent unless it names another."
        ))),
        cx,
    )
    // The search on demand, rather than at the next tick half an hour away. It
    // looks where the tick would, and does nothing the tick would not.
    .child(field(
        "Search",
        about(
            "Look for a labelled issue now rather than at the next scheduled look — in every \
             open workspace, as the scheduled look does. Runs reach the forge through the \
             Connections page.",
        ),
        div().h_flex().child(
            crate::controls::action("unattended-look-now")
                .outline()
                .label("Look for an issue now")
                .on_click(|_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    crate::unattended::look_now(window.window_handle(), cx);
                }),
        ),
        cx,
    ))
    .child(field(
        "Mode",
        Some(mode_about(&mode, cx)),
        mode_picker(handle, &mode),
        cx,
    ))
    // Whatever stops every run, said above the switches in the warning ink:
    // with it unsaid they would look as though they did something.
    .when_some(crate::unattended::blocked(cx), |group, why| {
        group.child(
            div()
                .text_sm()
                .text_color(ink.warning)
                .child(format!("Nothing will be picked up: {why}")),
        )
    })
    .child(field(
        "Projects",
        empty.then(|| "No project in this workspace can be switched on.".into_any_element()),
        div()
            .v_flex()
            .gap_3()
            .children(choices.into_iter().map(|(idx, name, on)| {
                let shell = handle.clone();
                // The switch sets no cursor of its own, and an arrow over a
                // control that acts reads as one that does not. Held to its own
                // width, so the empty space to the right of the name is not a
                // target.
                div().h_flex().child(
                    div().flex_none().cursor_pointer().child(
                        Switch::new(("unattended", idx))
                            .checked(on)
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
