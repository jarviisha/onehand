use super::row::{MAX_LABEL, ellipsize, labelled, menu_button, on_hover, row_control};
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use crate::tokens::ROW_H;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Context, FocusHandle, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_ui::mark_slot;

/// The parent path of a workspace folder, shortened from the *front*.
///
/// A path is read from its tail: the last few components are what tell two
/// checkouts of the same project apart, and they are exactly what trimming the
/// end throws away. Keeping the last characters and marking the cut with a
/// leading `…` is what makes the line identify a folder rather than name the
/// drive it happens to sit on.
pub(crate) fn ellipsize_front(s: &str, max: usize) -> SharedString {
    let count = s.chars().count();
    if count <= max {
        return SharedString::from(s.to_string());
    }
    let kept: String = s.chars().skip(count - max.saturating_sub(1)).collect();
    SharedString::from(format!("…{kept}"))
}

/// How many workspaces the menu lists; past it, a line says how many it left
/// out.
const MENU_CAP: usize = 20;

/// Every workspace onehand keeps, most recently opened first.
///
/// **Switching still means another window** -- one window hosts exactly one
/// workspace, so a row here opens the workspace it names, or focuses the window
/// already showing it. That is what makes the list safe to offer from the title bar:
/// nothing on screen is torn down by a click in it, and the workspace the window
/// is drawing is marked and cannot be picked, because "switch to where you
/// already are" is a click that does nothing and reads as a failure.
///
/// A row is named as the title bar names its workspace, from the list's copy of
/// its name. One whose file cannot be read says so beside its folder's name, and
/// pressing it says why. Never read from the workspace's own file here, which
/// would put disk I/O in the middle of a frame.
///
/// The builder, not the trigger: the menu is rebuilt every time it opens, so the
/// control it hangs off, the title bar's workspace control, is free to be any element.
pub(super) fn workspace_menu(
    current: Option<std::path::PathBuf>,
    recents: Vec<onehand_core::config::recents::Listed>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let detail = crate::theme::meta_ink(cx);
        let mut menu = menu.item(crate::controls::menu_label("Workspaces"));
        for listed in recents.iter().take(MENU_CAP) {
            let is_current = current.as_deref() == Some(listed.dir.as_path());
            let label = ellipsize(&listed.name, MAX_LABEL);
            let unreadable = listed.unreadable.is_some();
            let target = shell.clone();
            let dir = listed.dir.clone();
            // The workspace already on screen keeps the library's own row, and so
            // its cursor: it is checked and unpickable, and a pointer over it
            // would promise a press that is refused.
            let row = move |_: &mut Window, _: &mut App| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(div().flex_none().child(label.clone()))
                    .when(unreadable, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(detail)
                                .child("Cannot be read"),
                        )
                    })
            };
            menu = menu.item(
                match is_current {
                    true => crate::controls::menu_still(row),
                    false => crate::controls::menu_row(row),
                }
                .checked(is_current)
                .disabled(is_current)
                .on_click(move |_, _, cx: &mut App| {
                    let dir = dir.clone();
                    target
                        .update(cx, |shell: &mut Shell, cx| shell.open_recent(dir, cx))
                        .ok();
                }),
            );
        }
        let left_out = recents.len().saturating_sub(MENU_CAP);
        if left_out > 0 {
            menu = menu.item(crate::controls::menu_still(move |_, _| {
                div()
                    .text_xs()
                    .text_color(detail)
                    .child(format!("{left_out} more not shown"))
            }));
        }
        let new = shell.clone();
        menu.separator()
            .item(crate::controls::menu_item("New workspace…").on_click(
                move |_, _, cx: &mut App| {
                    new.update(cx, |shell: &mut Shell, cx| shell.new_workspace(cx))
                        .ok();
                },
            ))
    }
}

/// What the primary action promises, in words.
///
/// Pure and separate because it is the rule rather than the rendering: the
/// button starts a session on the *selected* project, and with ten projects in
/// the rail that is not something a `+` can say on its own.
pub(super) fn new_session_hint(root: Option<&str>, agent: Option<&str>) -> SharedString {
    match (root, agent) {
        (Some(root), Some(agent)) => format!("Start a new session in {root} with {agent}").into(),
        (Some(root), None) => format!("Start a new session in {root}").into(),
        // Nothing to start one *in*. Saying so beats naming a project that is
        // not there, and beats a tooltip that promises what the click cannot do.
        (None, _) => "Add a project first — every session belongs to one".into(),
    }
}

/// *New session*: a session in `target` with the default agent, drawn as the
/// first of the rail's rows, a page's row set apart by its weight and the
/// full ink, the key's hint at its end. Its menu, choosing another project or
/// agent, opens from a chevron at the row's end, shown under the pointer and
/// while the keys are on the row, and from a right-click anywhere on it. A tab
/// stop: Enter on it starts the session as a press does, the window turning
/// Enter on a focused row into its click. With
/// no project only *Add project…* helps: the row refuses, and its hint says so.
pub(super) fn new_session_row(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    target: Option<usize>,
    focus: &FocusHandle,
    window: &Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let agents: Vec<SharedString> = shell
        .agents(cx)
        .iter()
        .map(|spec| ellipsize(&spec.name, MAX_LABEL))
        .collect();
    let roots = &window_state.workspace.roots;
    let hint = new_session_hint(
        target
            .and_then(|idx| roots.get(idx))
            .map(|root| root.label.as_str()),
        agents.first().map(SharedString::as_ref),
    );
    // In the order the rail draws them: two orders for one list is two lists
    // as far as the reader is concerned.
    let projects: Vec<(usize, SharedString)> = window_state
        .workspace
        .display_order()
        .into_iter()
        .filter_map(|idx| Some((idx, ellipsize(&roots.get(idx)?.label, MAX_LABEL))))
        .collect();
    let build = std::rc::Rc::new(new_session_menu(
        projects,
        target,
        agents,
        cx.entity().downgrade(),
    ));
    let (caret_build, context_build) = (build.clone(), build);
    let group = SharedString::from("new-session");
    let ring = (focus.is_focused(window) && window.last_input_was_keyboard())
        .then(|| super::row::key_ring(cx));
    let hover = super::row::hover_fill(cx);
    let key = crate::keymap::first_key(crate::keymap::NEW_SESSION, cx);
    div()
        .id("new-session")
        .group(group.clone())
        .track_focus(focus)
        .relative()
        .h_flex()
        .items_center()
        .h(ROW_H)
        .px_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .text_color(cx.theme().foreground)
        .font_semibold()
        .tooltip(onehand_ui::tip(move |window, cx| {
            Tooltip::new(hint.clone())
                .action(&crate::shell::NewSession, None)
                .build(window, cx)
        }))
        .when_some(target, |row, root| {
            row.cursor_pointer()
                .hover(move |row| row.bg(hover))
                .on_click(
                    cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                        shell.new_session_in(root, window, cx);
                    }),
                )
        })
        .child(mark_slot(Icon::new(IconName::Plus).small()))
        .child(div().flex_1().min_w_0().truncate().child("New session"))
        .children(key.map(|key| {
            div()
                .flex_none()
                .font_normal()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(gpui_component::kbd::Kbd::new(key).appearance(false))
        }))
        .child(
            on_hover(
                &group,
                false,
                labelled(
                    "new-session-target-name",
                    "Start a session in another project, or with another agent",
                    menu_button(
                        row_control("new-session-target", IconName::ChevronDown, cx),
                        "Start a session in another project, or with another agent",
                        move |menu, window, cx| caret_build(menu, window, cx),
                    ),
                ),
            )
            .focus(focus),
        )
        .children(ring)
        .context_menu(move |menu, window, cx| context_build(menu, window, cx))
}

/// Where a new session can go: which project, then which agent. A project
/// row starts the default agent there; an agent row starts in the project
/// *New* would, the first one drawn when there is none.
fn new_session_menu(
    projects: Vec<(usize, SharedString)>,
    target: Option<usize>,
    agents: Vec<SharedString>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, _cx: &mut App| {
        let mut menu = menu.item(crate::controls::menu_label("Start in"));
        for (idx, label) in &projects {
            let (idx, label, to) = (*idx, label.clone(), shell.clone());
            menu = menu.item(
                crate::controls::menu_item(label)
                    .checked(Some(idx) == target)
                    .on_click(move |_, window, cx: &mut App| {
                        to.update(cx, |shell: &mut Shell, cx| {
                            shell.new_session_in(idx, window, cx);
                        })
                        .ok();
                    }),
            );
        }
        let Some(root) = target.or(projects.first().map(|(idx, _)| *idx)) else {
            return menu;
        };
        menu = menu
            .separator()
            .item(crate::controls::menu_label("With agent"));
        for (i, name) in agents.iter().enumerate() {
            let (name, to) = (name.clone(), shell.clone());
            menu = menu.item(crate::controls::menu_item(name).on_click(
                move |_, window, cx: &mut App| {
                    to.update(cx, |shell: &mut Shell, cx| {
                        shell.new_session_at(root, i, window, cx);
                    })
                    .ok();
                },
            ));
        }
        menu
    }
}
