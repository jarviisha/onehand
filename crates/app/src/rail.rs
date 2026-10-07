//! The navigation rail.
//!
//! gpui-component's `Sidebar`. The rail is
//! **session-first**: every folder row lists its root's sessions underneath, and
//! clicking a session row selects root *and* session in one touch.
//!
//! It draws one of two lists at a time ([`RailTab`]): the project tree, which
//! answers "what is in this workspace", and every session flat, which answers
//! "what is running" in the order the sessions were started.
//!
//! **The tree's order is the user's.** A project row and a session row are each
//! dragged to another place in it ([`ProjectDrag`](row::ProjectDrag), [`SessionDrag`](row::SessionDrag)) — projects
//! within their pin group, sessions within their project. The flat list is not
//! draggable: it is in creation order across every project, which is not an
//! order this app stores anywhere, so there would be nothing for a drop to
//! write into.
//!
//! What each row *shows* is decided in `onehand-core`, not here:
//!
//! - folder row — label, plus the root's branch and change count when it is a
//!   git repo (`onehand_core::gitstat`). A `SidebarMenuItem` is a fixed-height
//!   single row, so these ride in its suffix rather than on a second line,
//!   under a width cap that keeps the label first;
//! - session row — the conversation's own name once it has one, and a trailing
//!   mark **only** while the session carries a signal. Each of the four signals
//!   has a shape of its own, not a tint of one shared dot, and names itself in
//!   a tooltip. A calmly-ready session is a clean text row.

use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::{
    ClickEvent, Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div,
};
use gpui_component::sidebar::{Sidebar, SidebarCollapsible};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, IconName, Side, StyledExt};

mod project;
mod row;
mod session;
mod workspace;
use project::folder_row;
pub use project::{pick_item, unattended_item};
use row::KeyedMenu;
use row::{rail_row, rail_row_marked};
use session::session_rows;
pub(crate) use session::{signal_mark, signal_word};
pub(crate) use workspace::ellipsize_front;
use workspace::{new_session_block, workspace_identity};

/// Which of the rail's two lists is showing.
///
/// The tree answers "what is in this workspace"; the flat list answers "what
/// wants me". They are two questions about one set of sessions, and the reason
/// they are tabs rather than two stacked groups is that the second one
/// **reorders itself**: a section that rearranges under the eye cannot sit above
/// a tree the user navigates by position, because every glance at the busy list
/// moves the quiet one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailTab {
    Projects,
    Sessions,
}

impl RailTab {
    /// Both of them, in the order they are drawn.
    const ALL: [Self; 2] = [Self::Projects, Self::Sessions];

    fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Sessions => "All sessions",
        }
    }
}

/// Build the rail for a window.
pub fn rail(
    window_state_shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
    // `use<>`: the returned sidebar is fully owned (every string is cloned and
    // every handler is an Rc), so it must not capture the borrows of `self` and
    // `cx` that edition 2024 would otherwise infer -- the caller needs `cx` back
    // to attach its action handlers.
) -> impl IntoElement + use<> {
    let name = window_state.workspace.name.clone();
    let workspace_dir = window_state.workspace.storage_dir.clone();
    let workspace_recents = window_state_shell.recents(cx);
    let workspace_target = cx.entity().downgrade();
    let tab = window_state_shell.rail_tab();
    // Only the list being drawn is built. The other tab's rows cost a lookup per
    // session and hang a handler on each, and none of that reaches the screen.
    let rows = match tab {
        // `display_order`, not `0..len`: pinned projects are drawn first while
        // the roots themselves stay put, so every index a row hands back still
        // means the project the user clicked.
        RailTab::Projects => window_state
            .workspace
            .display_order()
            .into_iter()
            .enumerate()
            .map(|(at, idx)| folder_row(window_state_shell, window_state, idx, at, cx))
            .collect::<Vec<_>>(),
        RailTab::Sessions => session_rows(window_state_shell, window_state, cx),
    };

    // Every `Sidebar` child must be the same type, so the primary action rides
    // in the header next to the workspace identity and the tab bar, and the
    // content is one keyed menu -- which is where it belongs anyway: starting a
    // session is about the workspace, not about the project list.
    Sidebar::new("rail")
        .side(Side::Left)
        // The rail is a panel in a draggable split now, so its width is the
        // split's to decide: `w_full` is what hands it over. Left at its own
        // fixed default the drag would move the boundary and the rail would
        // stay the width it always was, with the gap behind it.
        //
        // `SidebarCollapsible::None` for the same reason. The library's
        // collapse animates the sidebar between a stored width and 48px, which
        // is a second thing driving the same number -- and this app does not
        // collapse the rail at all, it hides it.
        .collapsible(SidebarCollapsible::None)
        .w_full()
        // **The well, asked for by name rather than through the sidebar
        // token.** The rail sat on the reading surface for a while, separated
        // from the conversation by the hairline down its edge alone -- the one
        // piece of chrome in the window still dressed as a place text is read.
        //
        // The well, asked for here rather than left to the library's own
        // `sidebar` token: that token ships with a value of its own and the ramp
        // writes the reading surface into it, so the panel would come up level
        // with the conversation beside it. The library applies the caller's
        // refinement after its own `bg`, which is what lets this win.
        //
        // **The rail is the only panel in the window still lifted off the
        // reading surface**, and it is the only one that is not about the work:
        // a workspace, its projects, its sessions. The two dock cards used to
        // take the same step, which made lifted mean nothing more precise than
        // "not the conversation", and with both docks open left the conversation
        // as the one region on screen that nothing had raised. They are flat on
        // the reading surface now, marked by their borders, so this step says
        // what it always meant to.
        //
        // Nothing else about the rail moves with it: `sidebar_accent` and the
        // selected fill are both well clear of this step in either palette, so a
        // hovered row and a marked one still read.
        .bg(cx.theme().muted)
        // **No line down the rail's edge, because the fill is the edge.**
        // `Sidebar` draws a 1px right border of its own and this turns it off:
        // the rail is on the well and the conversation beside it is on the
        // reading surface, and a surface that changes at a seam already says
        // where the seam is. Ruled as well, it was a line drawn along a
        // boundary that was not in doubt.
        //
        // **What makes that safe to say is a number rather than a taste**: those
        // two surfaces are the ramp's own reading surface and its well, a pair
        // the ramp's tests hold at 1.14 or better in either palette. This edge
        // and the transparent resize handle beside it were changed together and
        // each could otherwise be read as leaning on the other; neither does --
        // both lean on that step.
        //
        // This has been both ways. While the rail was on the reading surface
        // there was nothing else marking that seam, so the border had to stay --
        // and before *that* the library's drag handle drew a rule hard against
        // it in the same colour, which read as a 2px edge no single declaration
        // accounted for. The handle is drawn in nothing at rest now; it is still
        // the affordance and still brightens under a drag.
        .border_r_0()
        // Not `SidebarHeader`: it carries a hover highlight of its own, so the
        // workspace identity lit up on hover as though it were a control. Its
        // children carry their own `px_2` instead, which is the inset
        // `SidebarMenuItem` gives the project rows -- that is what puts every
        // icon in the rail on one column.
        .header(
            div()
                .v_flex()
                // A step wider than the list's own row gap. Three rows of
                // roughly one height, stacked at the spacing a list uses, read
                // as the first three entries of that list -- which is the one
                // thing the header is not.
                .gap_2p5()
                .w_full()
                .min_w_0()
                .child(workspace_identity(
                    name.into(),
                    workspace_dir,
                    workspace_recents,
                    workspace_target,
                    cx,
                ))
                // Above *New session*, because it is what a workspace with no
                // project needs first and because both of them are about the
                // workspace rather than about the list underneath. Quieter than
                // the primary action right below it: adding a project is done
                // once per project, starting a session is done all day.
                .child(
                    rail_row("rail-add-project", IconName::FolderOpen, "Add project…", cx)
                        .text_color(cx.theme().muted_foreground)
                        .tooltip(|window, cx| {
                            Tooltip::new("Add a project root to this workspace").build(window, cx)
                        })
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.add_root(cx);
                        })),
                )
                // The one way to the workspace page, which answers across every
                // project what the rows below answer one project at a time.
                // Quiet like *Add project…*, and marked while the page shows,
                // as a project row is while it is the one on screen.
                .child(
                    {
                        let (id, icon, label) = (
                            "rail-workspace",
                            IconName::LayoutDashboard,
                            "Workspace overview",
                        );
                        match window_state_shell.workspace_shown(cx) {
                            true => rail_row_marked(id, icon, label, cx),
                            false => rail_row(id, icon, label, cx)
                                .text_color(cx.theme().muted_foreground),
                        }
                    }
                    .tooltip(|window, cx| {
                        Tooltip::new("What is waiting, working and open across every project")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(
                        |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.show_workspace(window, cx);
                        },
                    )),
                )
                // Right under the overview: every task of the window's
                // projects, with a count of those that need a person.
                .child({
                    let (id, icon, label) = ("rail-tasks", IconName::Inbox, "Tasks");
                    let count = attention_pill(crate::task::attention(
                        &window_state_shell.page_roots(),
                        cx,
                    ));
                    match window_state_shell.tasks_shown(cx) {
                        true => rail_row_marked(id, icon, label, cx),
                        false => {
                            rail_row(id, icon, label, cx).text_color(cx.theme().muted_foreground)
                        }
                    }
                    .children(count.map(|count| {
                        div()
                            .flex_none()
                            .px_1()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().secondary)
                            .text_color(cx.theme().secondary_foreground)
                            .child(count)
                    }))
                    .tooltip(|window, cx| {
                        Tooltip::new("Every task, and what each needs").build(window, cx)
                    })
                    .on_click(cx.listener(
                        |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.show_tasks(None, window, cx);
                        },
                    ))
                })
                // Below Tasks: every issue of the window's projects, on a
                // page with room to work through them. No count: Tasks
                // already counts what needs a person.
                .child(
                    {
                        let (id, icon, label) =
                            ("rail-issues", crate::icons::Icon::CircleDot, "Issues");
                        match window_state_shell.issues_shown(cx) {
                            true => rail_row_marked(id, icon, label, cx),
                            false => rail_row(id, icon, label, cx)
                                .text_color(cx.theme().muted_foreground),
                        }
                    }
                    .tooltip(|window, cx| {
                        Tooltip::new("Every issue of every project, and where its work stands")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(
                        |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.show_issues(window, cx);
                        },
                    )),
                )
                // Below Issues: the workflows a run starts from, on a page
                // with room to write them.
                .child(
                    {
                        let (id, icon, label) = ("rail-workflows", IconName::Play, "Workflows");
                        match window_state_shell.workflows_shown(cx) {
                            true => rail_row_marked(id, icon, label, cx),
                            false => rail_row(id, icon, label, cx)
                                .text_color(cx.theme().muted_foreground),
                        }
                    }
                    .tooltip(|window, cx| {
                        Tooltip::new("Every workflow, to run one or write one").build(window, cx)
                    })
                    .on_click(cx.listener(
                        |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.show_workflows(window, cx);
                        },
                    )),
                )
                .child(new_session_block(window_state_shell, window_state, cx))
                // The hairline is where the header stops being about the
                // workspace and starts being about the list: everything above
                // it acts on the whole window, everything from the tabs down is
                // what is in it. Space alone said it too quietly, since the
                // rows above are already spaced.
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .pt_3()
                        .border_t_1()
                        .border_color(cx.theme().sidebar_border)
                        .child(tab_bar(tab, cx)),
                ),
        )
        .child(KeyedMenu::new(rows))
        // Not `SidebarFooter`: that is an `h_flex justify_between` with its own
        // hover highlight, meant for one row of controls. Stacked triggers
        // inside it made hovering any one of them light up the whole block.
        //
        // One row, where there were three. Agents and the keyboard table are
        // pages of Settings now, and a rail footer listing every page of one
        // dialog is a table of contents for a dialog nobody has opened yet.
        .footer(
            div().v_flex().gap_0p5().w_full().min_w_0().child(
                rail_row("open-settings", IconName::Settings, "Settings", cx)
                    .on_click(cx.listener(|shell, _, window, cx| shell.open_settings(window, cx))),
            ),
        )
}

/// The two lists, as a segmented control.
///
/// In the header rather than in the scrolling content: it is the thing that says
/// what is underneath it, and a control that scrolls away from what it labels
/// leaves the reader with a list and no name for it.
///
/// The space and the hairline above it are the header's, not this control's:
/// they separate two halves of the header rather than decorating one element,
/// and the half below the line is this and the list under it. The control is
/// [`onehand_plugin_host::switch`], shared with the Workbench's Plugins mode.
fn tab_bar(active: RailTab, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    onehand_plugin_host::switch(
        "rail-tab",
        &RailTab::ALL.map(|tab| tab.label().into()),
        RailTab::ALL
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0),
        gpui_component::Size::XSmall,
        cx.listener(|shell: &mut Shell, i: &usize, _, cx| {
            shell.set_rail_tab(RailTab::ALL[*i], cx);
        }),
        cx,
    )
}

/// What the Tasks row's pill reads for `n` tasks needing a person: nothing
/// at zero, so a pill is always news.
fn attention_pill(n: usize) -> Option<String> {
    (n > 0).then(|| n.to_string())
}

#[cfg(test)]
mod tests;
