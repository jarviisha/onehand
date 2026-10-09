//! The rail's rows: a session's status mark, a session, and a project's
//! header. Their actions show under the pointer, and on the selected row.
use super::model::{PROJECTS, Status, badge};
use super::tree::{project_menu, session_menu};
use super::*;
use crate::controls::faded;
use gpui::AnyElement;
use gpui::accesskit::Role;
use gpui_component::menu::ContextMenuExt as _;

/// A status in its stable column, so titles never shift beside it. Each has
/// a shape of its own as well as a colour, so none is told by colour alone.
/// `label` is what assistive technology reads and the tooltip says.
pub(super) fn status_icon(
    p: &Palette,
    id: impl Into<gpui::ElementId>,
    status: Status,
    label: impl Into<SharedString>,
) -> impl IntoElement {
    let mark = match status {
        Status::NeedsInput => Icon::empty()
            .path(crate::assets::HAND)
            .xsmall()
            .text_color(p.warning)
            .into_any_element(),
        // Still, like every other mark: a list of turning glyphs pulls the
        // eye from what waits on the person.
        Status::Running => Icon::new(IconName::LoaderCircle)
            .xsmall()
            .text_color(p.accent)
            .into_any_element(),
        Status::Failed => Icon::new(IconName::TriangleAlert)
            .xsmall()
            .text_color(p.danger)
            .into_any_element(),
        Status::DoneUnread => Icon::new(IconName::CircleCheck)
            .xsmall()
            .text_color(p.success)
            .into_any_element(),
        Status::Idle => Icon::empty()
            .path(crate::assets::CIRCLE)
            .xsmall()
            .text_color(p.muted)
            .into_any_element(),
    };
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex_none()
        .w(rems(DOT_COLUMN))
        .flex()
        .justify_center()
        .role(Role::Image)
        .aria_label(label.clone())
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .child(mark)
}

/// An icon-only button's name for assistive technology. The library's button
/// takes its accessible name only from a text label, so the name sits on a
/// wrapper around it.
pub(super) fn labelled(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    button: impl IntoElement,
) -> impl IntoElement {
    div().id(id).flex_none().aria_label(label).child(button)
}

/// One part of a project's git state: a mark and its count, named in full
/// on hover and to assistive technology.
fn git_part(
    id: impl Into<gpui::ElementId>,
    mark: impl IntoElement,
    n: usize,
    label: String,
) -> impl IntoElement {
    let label = SharedString::from(label);
    h_flex()
        .id(id)
        .flex_none()
        .gap_0p5()
        .role(Role::Image)
        .aria_label(label.clone())
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .child(mark)
        .child(n.to_string())
}

/// `1 commit`, `2 commits`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Hidden until the pointer is on the row, unless `shown`; the space stays,
/// so nothing beside it moves when it appears.
fn on_hover(group: &SharedString, shown: bool, child: impl IntoElement) -> impl IntoElement {
    h_flex()
        .flex_none()
        .when(!shown, |d| {
            d.invisible().group_hover(group.clone(), |s| s.visible())
        })
        .child(child)
}

impl Labs {
    /// A session: its status, its title, and under it `status · who · time`
    /// (who is the agent, or the project in a flat list) with the diff at the
    /// line's end.
    pub(super) fn session_row(
        &self,
        p: &Palette,
        i: usize,
        flat: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = &self.rail.sessions[i];
        let sel = self.rail.session == Some(i) && self.page == Page::Chat;
        let at = focused && self.rail.cursor == Some(Cursor::Session(i));
        let who = if flat {
            PROJECTS[s.project].name
        } else {
            s.agent
        };
        let facts = s.meta(who);
        let status = s.status;
        let title = s.title;
        // The fades paint in the row's fill: the rail at rest, its hover fill
        // under the pointer or while the row is chosen.
        let hovered = p.sunken.blend(p.selected);
        let rest = if sel || at { hovered } else { p.sunken };
        let group = SharedString::from(format!("session-{i}"));
        let this = cx.entity().downgrade();
        let pal = *p;
        // The status sits on the title's line, and the facts start under the
        // title, past the status's column.
        v_flex()
            .id(("session", i))
            .group(group.clone())
            .py_1()
            .when(flat, |d| d.pl_1p5())
            .when(!flat, |d| d.pl(rems(RAIL_INDENT)))
            .pr_1()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|d| d.bg(p.selected))
            .when(sel || at, |d| d.bg(p.selected))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_session(i, cx);
                this.rail.focus.focus(window, cx);
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(status_icon(p, ("status", i), s.status, s.status.label()))
                    .child(
                        faded(("title", i), s.title, group.clone(), rest, hovered)
                            .tooltip(move |window, cx| Tooltip::new(title).build(window, cx))
                            .text_color(p.text)
                            .when(sel || s.status == Status::DoneUnread, |d| d.font_medium()),
                    )
                    .child(on_hover(
                        &group,
                        sel,
                        labelled(
                            ("session-menu-name", i),
                            "Session actions",
                            action(("session-menu", i))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Ellipsis).text_color(p.muted))
                                .tooltip("What can be done with this session")
                                .dropdown_menu({
                                    let this = this.clone();
                                    move |menu, _, _| session_menu(menu, i, this.clone(), pal)
                                }),
                        ),
                    )),
            )
            .child(
                h_flex()
                    .relative()
                    .pl(rems(DOT_COLUMN))
                    .ml_2()
                    .gap_2()
                    .text_xs()
                    .child({
                        let tip = SharedString::from(facts.clone());
                        faded(("meta", i), facts, group.clone(), rest, hovered)
                            .text_color(p.muted)
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    })
                    .children(s.diff.map(|(added, removed)| {
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(div().text_color(p.success).child(format!("+{added}")))
                            .child(div().text_color(p.danger).child(format!("−{removed}")))
                    }))
                    .child(self.session_actions(p, i, status, &group, cx)),
            )
            .context_menu(move |menu, _, _| session_menu(menu, i, this.clone(), pal))
            .into_any_element()
    }

    /// A session's quick actions, over the end of its metadata line while
    /// the pointer is on the row: Stop while it runs, Retry once it failed,
    /// Archive always. Backed by the row's own fill, so what is under them
    /// does not show through.
    fn session_actions(
        &self,
        p: &Palette,
        i: usize,
        status: Status,
        group: &SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let button = |id: &'static str, icon: Icon, tip: &'static str| {
            action((id, i))
                .ghost()
                .xsmall()
                .icon(icon.text_color(p.muted))
                .tooltip(tip)
        };
        h_flex()
            .absolute()
            .right_0()
            .top_0()
            .bottom_0()
            .bg(p.sunken)
            .invisible()
            .group_hover(group.clone(), |s| s.visible())
            .child(
                h_flex()
                    .h_full()
                    .bg(p.selected)
                    .when(status == Status::Running, |d| {
                        d.child(labelled(
                            ("session-stop-name", i),
                            "Stop the agent",
                            button(
                                "session-stop",
                                Icon::empty().path(crate::assets::SQUARE),
                                "Stop the agent",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.set_status(i, Status::Idle, cx),
                            )),
                        ))
                    })
                    .when(status == Status::Failed, |d| {
                        d.child(labelled(
                            ("session-retry-name", i),
                            "Retry the last turn",
                            button(
                                "session-retry",
                                Icon::new(IconName::Redo),
                                "Retry the last turn",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.set_status(i, Status::Running, cx),
                            )),
                        ))
                    })
                    .child(labelled(
                        ("session-archive-name", i),
                        "Archive the session",
                        button(
                            "session-archive",
                            Icon::empty().path(crate::assets::ARCHIVE),
                            "Archive the session",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.drop_sessions(|at, _| at == i);
                            cx.notify();
                        })),
                    )),
            )
    }

    /// A project: the fold chevron, its folder, its name fading where its
    /// room ends, its branch with what differs from the
    /// remote, and, while folded, its badge.
    pub(super) fn project_header(
        &self,
        p: &Palette,
        i: usize,
        open: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let project = &PROJECTS[i];
        let pinned = self.rail.pinned.contains(&i);
        let at = focused && self.rail.cursor == Some(Cursor::Project(i));
        let badge = (!open).then(|| badge(&self.rail.sessions, i)).flatten();
        let hovered = p.sunken.blend(p.selected);
        let rest = if at { hovered } else { p.sunken };
        let group = SharedString::from(format!("project-{i}"));
        let this = cx.entity().downgrade();
        let pal = *p;
        h_flex()
            .id(("project", i))
            .group(group.clone())
            .h(rems(ROW_H))
            .pl_1p5()
            .pr_1()
            .gap_1p5()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|d| d.bg(p.selected))
            .when(at, |d| d.bg(p.selected))
            .text_color(p.text)
            .font_medium()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.rail.cursor = Some(Cursor::Project(i));
                this.set_folded(i, open);
                this.rail.focus.focus(window, cx);
                cx.notify();
            }))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall()
                .text_color(p.muted),
            )
            .child(
                Icon::new(if open {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .small(),
            )
            .child(
                faded(
                    ("project-name", i),
                    project.name,
                    group.clone(),
                    rest,
                    hovered,
                )
                .tooltip(move |window, cx| {
                    let mut more = vec![format!("Branch: {}", project.branch)];
                    if project.changes > 0 {
                        more.push(format!("{} changed files", project.changes));
                    }
                    if pinned {
                        more.push("Pinned to the top".into());
                    }
                    if let Some(auto) = project.auto {
                        more.push(auto.to_string());
                    }
                    more.push(format!("~/work/{}", project.name));
                    two_lines(project.name, more.join("\n"), window, cx)
                }),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1p5()
                    .text_xs()
                    .font_normal()
                    .text_color(p.muted)
                    .child(project.branch)
                    .when(project.changes > 0, |d| {
                        d.child(git_part(
                            ("git-changes", i),
                            div().size(rems(DOT)).rounded_full().bg(p.muted),
                            project.changes,
                            count(project.changes, "uncommitted change", "uncommitted changes"),
                        ))
                    })
                    .when(project.ahead > 0, |d| {
                        d.child(git_part(
                            ("git-ahead", i),
                            Icon::new(IconName::ArrowUp).xsmall(),
                            project.ahead,
                            count(project.ahead, "commit", "commits") + " ahead of the remote",
                        ))
                    })
                    .when(project.behind > 0, |d| {
                        d.child(git_part(
                            ("git-behind", i),
                            Icon::new(IconName::ArrowDown).xsmall(),
                            project.behind,
                            count(project.behind, "commit", "commits") + " behind the remote",
                        ))
                    }),
            )
            .children(badge.map(|s| {
                status_icon(
                    p,
                    ("badge", i),
                    s,
                    format!("{}: {}", project.name, s.label()),
                )
            }))
            .child(on_hover(
                &group,
                false,
                h_flex()
                    .child(labelled(
                        ("project-new-name", i),
                        "New session in this project",
                        action(("project-new", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Plus).text_color(p.muted))
                            .tooltip("New session in this project")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.new_session(i, AGENTS[0], cx)
                            })),
                    ))
                    .child(labelled(
                        ("project-menu-name", i),
                        "Project actions",
                        action(("project-menu", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Ellipsis).text_color(p.muted))
                            .tooltip("What can be done with this project")
                            .dropdown_menu({
                                let this = this.clone();
                                move |menu, _, _| project_menu(menu, i, pinned, this.clone(), pal)
                            }),
                    )),
            ))
            .context_menu(move |menu, _, _| project_menu(menu, i, pinned, this.clone(), pal))
            .into_any_element()
    }
}
