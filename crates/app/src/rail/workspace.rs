use super::row::{MAX_LABEL, ellipsize, faded};
use crate::controls::Refuses as _;
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::{
    Anchor, App, ClickEvent, Context, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// The workspace bar: a letter tile and the workspace's name, the switcher
/// that opens the workspace menu, and *Hide the rail*.
///
/// The name gets exactly one line, faded, and whole on hover: people write
/// sentences into it.
pub(super) fn workspace_bar(
    name: SharedString,
    current: Option<std::path::PathBuf>,
    recents: Vec<std::path::PathBuf>,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let initial: String = name.chars().take(1).collect();
    let well = cx.theme().muted;
    let tip = name.clone();
    div()
        .h_flex()
        .items_center()
        .h(crate::controls::BAR_H)
        .flex_none()
        .px_3()
        .gap_2()
        .child(
            div()
                .size_5()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius)
                .bg(cx.theme().accent)
                .text_xs()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(initial),
        )
        .child(
            faded("workspace-name", name, "workspace".into(), well, well)
                .font_medium()
                .text_color(cx.theme().foreground)
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
        )
        .child(
            crate::controls::action("workspace")
                .ghost()
                .small()
                .icon(Icon::new(IconName::ChevronsUpDown).text_color(cx.theme().muted_foreground))
                .tooltip("Workspaces, and the projects in this one")
                .dropdown_menu_with_anchor(
                    Anchor::TopLeft,
                    workspace_menu(current, recents, cx.entity().downgrade()),
                ),
        )
        .child(
            crate::controls::action("hide-rail")
                .ghost()
                .small()
                .icon(Icon::new(IconName::PanelLeftClose).text_color(cx.theme().muted_foreground))
                .tooltip("Hide the rail")
                .on_click(
                    cx.listener(|shell: &mut Shell, _, window, cx| shell.toggle_rail(window, cx)),
                ),
        )
}

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

/// How much of a workspace's parent path a switcher row carries.
const MAX_PARENT: usize = 30;

/// Every workspace this launch knows about.
///
/// **Switching still means another window** -- one window hosts exactly one
/// workspace, so a row here opens the workspace it names, or focuses the window
/// already showing it. That is what makes the list safe to offer from the rail:
/// nothing on screen is torn down by a click in it, and the workspace the window
/// is drawing is marked and cannot be picked, because "switch to where you
/// already are" is a click that does nothing and reads as a failure.
///
/// A row is named by its folder, with the parent path beside it, because the
/// list is directories and a workspace's own name lives inside a file that
/// reading here would put disk I/O in the middle of a frame.
///
/// The builder, not the trigger: the menu is rebuilt every time it opens, so the
/// row it hangs off is free to be any element the rail wants.
fn workspace_menu(
    current: Option<std::path::PathBuf>,
    recents: Vec<std::path::PathBuf>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let muted = cx.theme().muted_foreground;
        let mut menu = menu;
        // No heading over nothing: a workspace that has never been bound
        // has no recents, and the two actions below stand on their own.
        if !recents.is_empty() {
            menu = menu.label("Workspaces");
        }
        for dir in &recents {
            let is_current = current.as_deref() == Some(dir.as_path());
            let label = ellipsize(
                &dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string()),
                MAX_LABEL,
            );
            let parent = dir
                .parent()
                .map(|parent| ellipsize_front(&parent.display().to_string(), MAX_PARENT));
            let target = shell.clone();
            let dir = dir.clone();
            // The workspace already on screen keeps the library's own row, and so
            // its cursor: it is checked and unpickable, and a pointer over it
            // would promise a press that is refused.
            let row =
                move |_: &mut Window, _: &mut App| {
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(div().flex_none().child(label.clone()))
                        .children(parent.clone().map(|parent| {
                            div().flex_none().text_xs().text_color(muted).child(parent)
                        }))
                };
            menu = menu.item(
                match is_current {
                    true => PopupMenuItem::element(row),
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
        let (open, new) = (shell.clone(), shell.clone());
        menu.separator()
            .item(
                crate::controls::menu_item("Open workspace…")
                    .icon(Icon::new(IconName::FolderOpen))
                    .on_click(move |_, _, cx: &mut App| {
                        open.update(cx, |shell: &mut Shell, cx| shell.open_workspace(cx))
                            .ok();
                    }),
            )
            .item(
                crate::controls::menu_item("New workspace…")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, _, cx: &mut App| {
                        new.update(cx, |shell: &mut Shell, cx| shell.new_workspace(cx))
                            .ok();
                    }),
            )
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

/// *New*: a session in `target` with the default agent, and a caret choosing
/// another project or agent. With no project only *Add project…* helps, and
/// the hint says so.
///
/// Ghost, like the rail's other controls: no edge, a fill under the pointer.
pub(super) fn new_button(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    target: Option<usize>,
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
    let build = new_session_menu(projects, target, agents, cx.entity().downgrade());
    let new = crate::controls::action("new-session")
        .icon(IconName::Plus)
        .label("New")
        .tooltip(hint)
        .refuses(target.is_none())
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                if let Some(root) = target {
                    shell.new_session_in(root, window, cx);
                }
            }),
        );
    // Two ghost halves rather than the library's split button, whose caret
    // half is built past the app's wrapper and so takes the arrow cursor.
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .child(new.ghost().small())
        .child(
            crate::controls::action("new-session-target")
                .ghost()
                .small()
                .dropdown_caret(true)
                .tooltip("Start a session in another project, or with another agent")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    build(menu, window, cx)
                }),
        )
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
        let mut menu = menu.label("Start in");
        for (idx, label) in &projects {
            let (idx, label, to) = (*idx, label.clone(), shell.clone());
            menu = menu.item(
                crate::controls::menu_item(label)
                    .icon(Icon::new(IconName::Folder))
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
        menu = menu.separator().label("With agent");
        for (i, name) in agents.iter().enumerate() {
            let (name, to) = (name.clone(), shell.clone());
            menu = menu.item(
                crate::controls::menu_item(name)
                    .icon(Icon::new(IconName::Bot))
                    .on_click(move |_, window, cx: &mut App| {
                        to.update(cx, |shell: &mut Shell, cx| {
                            shell.new_session_at(root, i, window, cx);
                        })
                        .ok();
                    }),
            );
        }
        menu
    }
}
