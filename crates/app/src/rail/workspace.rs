use super::row::{
    MAX_LABEL, ellipsize, faded, hover_fill, lead_row, rail_row_filled, row_surfaces,
};
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, App, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};

/// The workspace identity line, and the switcher behind it.
///
/// A workspace name is free text and users write sentences into it -- the one in
/// the screenshot wrapped onto two lines and pushed the primary action down the
/// rail. It is an *identity*, so it gets exactly one line: truncated, on the
/// rail's icon column like everything else.
///
/// **The whole row is the switcher.** The workspace the rail is drawing was the
/// one thing on screen with no way to be changed from the rail at all -- the
/// switch was reachable only by opening Settings, two surfaces away from the
/// name it changes. Behind a chevron at the row's end the target would have been
/// a few pixels wide while the thing being pointed at is the name beside it, so
/// the row carries the menu itself.
///
/// **Nothing marks it but the pointer.** No chevron, no second icon: this row
/// sits above the rail's quietest chrome and a caret on it competed with the
/// primary action right below. What says it is a control is the hover and the
/// cursor, which this row deliberately did not have while it was a label, plus
/// the tooltip that names what a press does.
pub(super) fn workspace_identity(
    name: SharedString,
    current: Option<std::path::PathBuf>,
    recents: Vec<std::path::PathBuf>,
    shell: WeakEntity<Shell>,
    cx: &App,
) -> impl IntoElement + use<> {
    let radius = cx.theme().radius;
    // Resolved up front: the hover closure outlives this borrow of `cx`.
    let accent = cx.theme().sidebar_accent;
    let (hover, accent_fg) = (hover_fill(cx), cx.theme().sidebar_accent_foreground);
    let (rest, hovered) = row_surfaces(false, cx);
    let hover_name = name.clone();
    let row = lead_row(
        div()
            .id("workspace-identity")
            .group("workspace-identity")
            .h_flex()
            .items_center()
            .w_full()
            .min_w_0()
            .px_2()
            .gap_x_2()
            .rounded(radius)
            .cursor_pointer()
            .hover(move |row| row.bg(hover).text_color(accent_fg))
            // The name leads it, because this row is the one place a workspace
            // name is written and it is written on one line: users put
            // sentences in that field, and truncated there it could be read
            // nowhere at all. The row is its own hover target, so the tooltip
            // that says what a press does is also the only one that can carry
            // the whole name.
            .tooltip(move |window, cx| {
                let name = hover_name.clone();
                Tooltip::element(move |_, _| {
                    div()
                        .v_flex()
                        .gap_0p5()
                        .child(name.clone())
                        .child("Workspaces, and the projects in this one")
                })
                .build(window, cx)
            }),
    )
    // Full ink, not muted. It was the dimmest thing in the header while
    // standing for the thing the header is about, so the eye read the row
    // beginning at the name and the mark before it as decoration.
    .child(Icon::new(IconName::LayoutDashboard).size_4())
    // Semibold rather than the header block's medium: this is the one name in
    // the window that says which workspace all of it belongs to.
    //
    // Faded like every list row's name, and one corner is taken knowingly:
    // while this row's *menu* is open the trigger fills it with the accent,
    // and a fade painted for the resting fill is then a step off -- visible
    // only on a name long enough to fade, while its menu is open, with the
    // pointer somewhere else. The fade cannot follow that fill because the
    // open flag is applied by the menu host after this row is already built.
    .child(faded(name, "workspace-identity".into(), rest, hovered).font_semibold());

    crate::controls::MenuTrigger::new(row, accent)
        .dropdown_menu_with_anchor(Anchor::TopLeft, workspace_menu(current, recents, shell))
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

/// The rail's primary action, and the chooser beside it.
///
/// On the same icon column as every other row -- the `+` used to sit mid-rail
/// while the workspace icon above it and the folder icons below it were at the
/// left edge.
///
/// **The row itself is unchanged and stays one click**: the selected project,
/// the default agent. What the caret adds is the two things that click has to
/// pick silently, and the project is the one worth reaching first — a session is
/// bound to one project root for its whole life, so starting one somewhere else
/// used to mean selecting that project, tearing down whatever was on screen, and
/// only then pressing `+`. The agents follow underneath, and only where there is
/// more than one configured: with one there is nothing to choose.
///
/// A popup and not the list that used to expand in the rail. That list pushed
/// the whole tree down while it was open, which is affordable for two agents and
/// not for a workspace's worth of projects — and the state saying whether it was
/// open had to be carried on the shell and cleared on every path that started a
/// session.
///
/// **The row names the project it would start in**, in its tooltip. A session
/// belongs to exactly one project root and this button silently picks the
/// selected one -- which is only obvious to someone who already knows that, and
/// invisible to someone reading a rail with ten projects in it.
pub(super) fn new_session_block(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let agents: Vec<SharedString> = shell
        .agents(cx)
        .iter()
        .map(|spec| ellipsize(&spec.name, MAX_LABEL))
        .collect();
    // Read off the workspace here rather than handed in: the caller was
    // deriving it from the very tree it was already passing, so the name and
    // the tree it came from travelled together and could disagree.
    let active_root = window_state.workspace.active_root().map(|root| &root.label);
    // The default agent is the one this row starts, which is what the tooltip
    // names.
    let hint = new_session_hint(
        active_root.map(String::as_str),
        agents.first().map(SharedString::as_ref),
    );
    // `display_order` so the menu lists projects the way the rail draws them:
    // two orders for one list is two lists as far as the reader is concerned.
    let projects: Vec<(usize, SharedString)> = window_state
        .workspace
        .display_order()
        .into_iter()
        .filter_map(|idx| {
            let root = window_state.workspace.roots.get(idx)?;
            Some((idx, ellipsize(&root.label, MAX_LABEL)))
        })
        .collect();
    let active_idx = window_state.workspace.active_root;
    // **More than one of either, or nothing to choose.** One project and one
    // agent leaves a caret whose whole menu is a single row doing exactly what
    // the button beside it does -- the same "control that exists to disappoint"
    // the agent list is gated on, and it has to be gated the same way or the
    // rule is one the rail applies in one place and not the other.
    let choosable = projects.len() > 1 || agents.len() > 1;
    let target = cx.entity().downgrade();

    let radius = cx.theme().radius;
    let (fill, fill_fg, fill_hover, open_fill) = (
        cx.theme().secondary,
        cx.theme().secondary_foreground,
        cx.theme().secondary_hover,
        cx.theme().secondary_active,
    );
    let primary = lead_row(rail_row_filled(
        "new-session",
        IconName::Plus,
        "New session",
        cx,
    ))
    // The two halves of one control, so the seam between them is square
    // and the outer edges keep the radius. Only while there is a caret to
    // join: a lone row squared off on one side reads as clipped.
    .when(choosable, |row| row.rounded_r(px(0.)))
    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
    .on_click(
        cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
            shell.new_session(window, cx);
        }),
    );

    div()
        .h_flex()
        .items_center()
        .w_full()
        .min_w_0()
        // The seam between the halves is a sliver of the well showing
        // through, not a border: a hairline in the border token sits on the
        // secondary fill with next to no contrast and disappeared there. The
        // well against that fill is the exact contrast that makes the button
        // itself visible, so the seam it draws can never be fainter than the
        // control it splits. With no caret there is one child and the gap
        // draws nothing.
        .gap(px(1.))
        .child(div().flex_1().min_w_0().child(primary))
        .when(choosable, |bar| {
            // The sentence names whichever section the menu will actually
            // carry. With one project and several agents there is no *Start
            // in* to open, and a caret promising another project over a menu
            // that has none is a control lying about itself before it is even
            // pressed.
            let says = match projects.len() > 1 {
                true => "Start a session in another project",
                false => "Start a session with a different agent",
            };
            // The other half of one filled control, drawn as a div rather
            // than a library button so the two halves share one fill and one
            // hover rule exactly. The seam between the halves is the bar's
            // 1px gap of well; hovering either half lights that half alone,
            // which is what says the control is split. `MenuTrigger` because
            // a div is not `Selectable` on its own, and the open state takes
            // the triple's third step so a held-open caret reads as pressed.
            //
            // Sized to the header row beside it rather than to the ••• the
            // menus share a look with: this is the right half of that
            // control, and a control half the height of its own other half
            // is two controls that happen to touch.
            let caret = div()
                .id("new-session-target")
                .h_flex()
                .items_center()
                .justify_center()
                .flex_none()
                .h_8()
                .w_7()
                .bg(fill)
                .text_color(fill_fg)
                .cursor_pointer()
                .hover(move |half| half.bg(fill_hover))
                .rounded_r(radius)
                .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
                .child(Icon::new(IconName::ChevronDown).size_4());
            // No `occlude` here, unlike the ••• menus: those sit *inside* a
            // row whose own click means something, while this caret is the
            // primary half's sibling with nothing behind it to protect.
            // The wrap re-borrows `Context<PopupMenu>` down to the `&mut App`
            // the builder is written against, as the ••• host does.
            let build = new_session_menu(projects, active_idx, agents, target);
            bar.child(
                crate::controls::MenuTrigger::new(caret, open_fill)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                        build(menu, window, cx)
                    }),
            )
        })
}

/// Where a new session can go: a project, or — where there is a choice — an
/// agent.
///
/// Two lists in one menu because they answer the same question from two sides.
/// A project row starts the default agent there and **selects that project on
/// the way**, which is the same thing the project row's own *New session* entry
/// does: a session bound to a root the rail is not showing is an agent nobody is
/// watching. An agent row starts in the project already selected, since the
/// caret's whole promise is that the row above it is unchanged.
///
/// The project already selected is checked and still pickable, unlike the
/// workspace switcher's current row: picking it is not a no-op, it starts a
/// session exactly as the button above would.
fn new_session_menu(
    projects: Vec<(usize, SharedString)>,
    active: usize,
    agents: Vec<SharedString>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, _cx: &mut App| {
        let mut menu = menu;
        // One project is not a choice, and a heading over a single row that
        // repeats the button above it says there was one.
        if projects.len() > 1 {
            menu = menu.label("Start in");
            for (idx, label) in &projects {
                let (idx, label, target) = (*idx, label.clone(), shell.clone());
                menu = menu.item(
                    crate::controls::menu_item(label)
                        .icon(Icon::new(IconName::Folder))
                        .checked(idx == active)
                        .on_click(move |_, window, cx: &mut App| {
                            target
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.new_session_in(idx, window, cx);
                                })
                                .ok();
                        }),
                );
            }
        }
        // Only where there is a choice. A list of one agent is a control that
        // exists to disappoint, and it would sit under a heading naming a
        // decision nobody has.
        if agents.len() > 1 {
            if projects.len() > 1 {
                menu = menu.separator();
            }
            menu = menu.label("With agent");
            for (i, name) in agents.iter().enumerate() {
                let (name, target) = (name.clone(), shell.clone());
                menu = menu.item(
                    crate::controls::menu_item(name)
                        .icon(Icon::new(IconName::Bot))
                        .on_click(move |_, window, cx: &mut App| {
                            target
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.new_session_with(i, window, cx);
                                })
                                .ok();
                        }),
                );
            }
        }
        menu
    }
}
