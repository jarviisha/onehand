use super::row::{MAX_LABEL, ellipsize, faded};
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::{
    Anchor, App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::PopupMenu;
use gpui_component::sidebar::SidebarHeader;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_ui::{ShortIcon as _, Tall as _, TallIcon as _};

/// The workspace bar: the switcher, a letter tile, the workspace's name and a
/// chevron in one strip that opens the workspace menu wherever it is pressed,
/// and *Hide the rail* beside it.
///
/// The strip is the library's sidebar header, so it takes the hover fill a
/// rail row takes and holds it while its menu is open. It is a tab stop that
/// Enter opens, named for what it opens, and ringed as a rail row is while the
/// keys are on it. The header takes no role or name of its own (the library
/// makes it stateful only inside its render), so assistive technology reads
/// it by the workspace's name in it. The name gets exactly one line, faded, and whole on hover:
/// people write sentences into it; the fade ends on the fill under it, open
/// or not. The tile stands on the list's marks' line, and the name a gap
/// after it starts on the edge their labels do.
pub(super) fn workspace_bar(
    name: SharedString,
    current: Option<std::path::PathBuf>,
    recents: Vec<(std::path::PathBuf, String)>,
    state: &super::RailState,
    window: &Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let initial: String = name.chars().take(1).collect();
    let hover = super::row::hover_fill(cx);
    let rest = match state.workspace_open {
        true => hover,
        false => cx.theme().sidebar,
    };
    let focus = state.workspace_focus.clone();
    let ring = focus.is_focused(window) && window.last_input_was_keyboard();
    let ring = ring.then(|| super::row::key_ring(cx));
    let shell = cx.entity().downgrade();
    let tip = name.clone();
    div()
        .h_flex()
        .items_center()
        .h(crate::tokens::BAR_H)
        .flex_none()
        .pl_1p5()
        .pr_3()
        .gap_1()
        .child(
            // The popover wraps the strip in a box of its own, which the row
            // lays out: it takes the room left, or a long name pushes the
            // row past the rail's edge instead of fading.
            onehand_ui::menu_below(
                "workspace-menu",
                SidebarHeader::new()
                    .group("workspace")
                    .relative()
                    .track_focus(&focus)
                    .flex_1()
                    .min_w_0()
                    .px_1p5()
                    .py_1()
                    .gap_1p5()
                    .cursor_pointer()
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
                            .text_color(cx.theme().accent_foreground)
                            .child(initial),
                    )
                    .child(
                        faded("workspace-name", name, "workspace".into(), rest, hover)
                            .font_medium()
                            .text_color(cx.theme().foreground)
                            .tooltip(onehand_ui::tip(move |window, cx| {
                                Tooltip::new(tip.clone()).build(window, cx)
                            })),
                    )
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .small()
                            .flex_none()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .children(ring),
                workspace_menu(current, recents, cx.entity().downgrade()),
            )
            .trigger_style(gpui::StyleRefinement::default().flex_1().min_w_0())
            // Deferred: the popover is mid-update when it announces, and the
            // shell is the rail's owner, not the popover's.
            .on_open_change(move |open, _, cx| {
                let (open, shell) = (*open, shell.clone());
                cx.defer(move |cx| {
                    shell
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.rail_state_mut().workspace_open = open;
                            cx.notify();
                        })
                        .ok();
                });
            }),
        )
        .child(
            crate::controls::action("hide-rail")
                .quiet_icon(false, cx)
                .short_icon()
                .icon(Icon::new(IconName::PanelLeftClose))
                .named_with_action("Hide the rail", &crate::shell::ToggleRail, None)
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
/// A row is named as the rail names its workspace, from the name the recents
/// remember, with the parent path beside it; a workspace remembered before
/// names were reads as its folder until it is next opened. Never read from the
/// workspace's own file here, which would put disk I/O in the middle of a frame.
///
/// The builder, not the trigger: the menu is rebuilt every time it opens, so the
/// row it hangs off is free to be any element the rail wants.
fn workspace_menu(
    current: Option<std::path::PathBuf>,
    recents: Vec<(std::path::PathBuf, String)>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let (detail, mono) = (
            crate::theme::meta_ink(cx),
            cx.theme().mono_font_family.clone(),
        );
        let mut menu = menu;
        // No heading over nothing: a workspace that has never been bound
        // has no recents, and the two actions below stand on their own.
        if !recents.is_empty() {
            menu = menu.item(crate::controls::menu_label("Workspaces"));
        }
        for (dir, name) in &recents {
            let is_current = current.as_deref() == Some(dir.as_path());
            let label = ellipsize(name, MAX_LABEL);
            let parent = dir
                .parent()
                .map(|parent| ellipsize_front(&parent.display().to_string(), MAX_PARENT));
            let (target, mono) = (shell.clone(), mono.clone());
            let dir = dir.clone();
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
                    .children(parent.clone().map(|parent| {
                        div()
                            .flex_none()
                            .text_xs()
                            .font_family(mono.clone())
                            .text_color(detail)
                            .child(parent)
                    }))
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
        let (open, new) = (shell.clone(), shell.clone());
        menu.separator()
            .item(crate::controls::menu_item("Open workspace…").on_click(
                move |_, _, cx: &mut App| {
                    open.update(cx, |shell: &mut Shell, cx| shell.open_workspace(cx))
                        .ok();
                },
            ))
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

/// *New*: a session in `target` with the default agent, and a caret choosing
/// another project or agent. With no project only *Add project…* helps, and
/// the hint says so.
///
/// Filled, unlike the rail's ghost controls: starting a session is what the
/// rail is most often opened for, so it reads as a button at rest, yet stays
/// quieter than the page's one primary.
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
    let refusal = target.is_none().then(|| hint.clone());
    let new = crate::controls::action("new-session")
        .label("New")
        .tooltip_with_action(hint, &crate::shell::NewSession, None)
        .refuses(refusal)
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                if let Some(root) = target {
                    shell.new_session_in(root, window, cx);
                }
            }),
        );
    // The whole row is *New*'s: it is what the rail is most often opened for,
    // and a button its label's width read as a footnote under the workspace.
    crate::controls::split_action(
        "new-session-menu",
        new.secondary().tall().flex_1(),
        crate::controls::action("new-session-target")
            .secondary()
            .tall_icon()
            .named("Start a session in another project, or with another agent"),
        cx.theme().sidebar,
        Anchor::TopRight,
        move |menu, window, cx| build(menu, window, cx),
    )
    .w_full()
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
