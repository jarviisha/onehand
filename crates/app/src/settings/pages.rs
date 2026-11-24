use super::{APP, about, field, list_row, list_rows, page_head, section};
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::config::Appearance;
use onehand_ui::Tall as _;

/// How many entries a Settings list draws before it counts the rest.
pub(super) const LIST_SHOWN: usize = 50;

/// The light/dark/system picker.
///
/// One value with three answers, drawn as the app draws every pick-one row: a
/// [`segment`](onehand_ui::segment) per answer, the one in force filled.
///
/// App-wide, unlike everything else on the page it sits in, because the theme
/// it selects is a global: two windows cannot be drawn in two modes.
fn appearance_picker(
    shell: &Entity<Shell>,
    current: Appearance,
    cx: &App,
) -> impl IntoElement + use<> {
    onehand_ui::choice_track(
        Appearance::ALL.into_iter().enumerate().map(|(i, choice)| {
            let shell = shell.clone();
            onehand_ui::segment(
                ("appearance", i),
                choice.label(),
                choice == current,
                onehand_ui::ControlSize::Tall,
                cx,
            )
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                shell.update(cx, |shell, cx| shell.set_appearance(choice, window, cx));
            })
        }),
        cx,
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

/// The workspace page: this workspace's name and the folder onehand keeps it
/// in, shown so it can be found for a backup and never chosen, then a way to
/// make another one.
///
/// No workspace is ever *replaced* in place -- one window hosts exactly one
/// workspace, so *New workspace…* opens another window, or focuses the one
/// already showing that workspace.
///
/// **The list of known workspaces is deliberately not here.** The rail's
/// identity row is the switcher, and it draws the same workspaces better: a
/// row named by its workspace, the one on screen checked and unpickable. A second copy here printed each absolute path whole into a
/// button, uncapped, so the longer the app was used the further it pushed
/// everything else off the bottom.
pub(super) fn workspace_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let name = shell.workspace_name_input().clone();
    let storage = SharedString::from(shell.storage_dir().display().to_string());
    let new = handle.clone();

    let general = section(None, None, cx)
        .child(field(
            "Name",
            about("Written at the top of the rail"),
            text_input(&name, cx).tall(),
            cx,
        ))
        .child(field(
            "Storage folder",
            about("Where onehand keeps this workspace"),
            div()
                .min_w_0()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .child(storage),
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
        div().h_flex().gap_2().child(
            crate::controls::action("new-workspace")
                .tall()
                .secondary()
                .label("New workspace…")
                .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    new.update(cx, |shell, cx| shell.new_workspace(cx));
                }),
        ),
    );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        // The scope is the workspace, not its name: the field below says that.
        .child(page_head(
            "This window's workspace: its name and where it is kept",
            "Workspace",
            cx,
        ))
        .child(general)
        .child(others)
        .child(delete_section(handle, cx))
        .into_any_element()
}

/// The connections page: each connector, whether it is signed in and as whom.
///
/// A list of rows, since the list is meant to grow and a connection is a fact
/// about the machine rather than about any one feature.
pub(super) fn connections_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let ink = crate::theme::status_ink(cx);
    let checking = crate::accounts::checking(cx);
    // ponytail: read when the page is drawn, so it goes stale while nothing
    // redraws it; every check that lands redraws, which is when it matters.
    let checked = crate::accounts::checked_at(cx)
        .map(|at| crate::chat::pane::rel_time(onehand_core::Now::read().secs, at));
    let rows = crate::features::connectors()
        .iter()
        .map(|connector| {
            // Each connector's own answer, or *checking* until the first look
            // lands; what to do about a failure is the failure's own words.
            let (status, ink) = match crate::accounts::of(connector.name(), cx) {
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
            section(None, None, cx)
                .gap_3()
                .child(list_rows(rows, cx))
                .child(
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
                                    crate::accounts::check(cx);
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
        .child(super::telegram::telegram_section(handle, cx))
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
    // Held to its own width, so the space beside the label is not a target,
    // and given the pointer the switch does not set itself.
    .child(
        div().h_flex().child(
            div().flex_none().cursor_pointer().child(
                onehand_ui::toggle("control-outside", crate::control::outside())
                    .label("Let programs on this machine control onehand")
                    .on_click(|on: &bool, window: &mut Window, cx: &mut App| {
                        crate::control::set_outside(*on, window, cx);
                    }),
            ),
        ),
    )
    .into_any_element()
}

/// Deleting this window's workspace: the window closes and its folder goes;
/// its projects, and their issues, stay kept.
fn delete_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let ink = crate::theme::status_ink(cx);
    let delete = handle.clone();
    section(
        Some("Delete workspace"),
        Some("The window closes and onehand forgets this workspace".into()),
        cx,
    )
    .child(
        div().h_flex().child(
            crate::controls::action("delete-workspace")
                .tall()
                .secondary()
                .text_color(ink.danger)
                .label("Delete workspace…")
                .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    delete.update(cx, |shell, cx| shell.confirm_delete_workspace(window, cx));
                }),
        ),
    )
    .into_any_element()
}
