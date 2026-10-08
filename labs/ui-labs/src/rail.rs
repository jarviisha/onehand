//! The rail: the workspace, the pages, New session, the project tree and
//! Settings at its foot. It hides completely, never to an icon column.
use super::controls::full;
use super::*;

/// The projects in the tree: a short name, and one long enough to be cut.
const PROJECTS: [(&str, &str); 2] = [
    ("atlas-api", "main · 3 changes"),
    ("atlas-api-internal-billing-reconciliation-service", "main"),
];

impl Labs {
    pub(super) fn rail(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.page;
        let nav = |cx: &mut Context<Self>,
                   page: Page,
                   label: &'static str,
                   icon: IconName,
                   count: Option<&'static str>| {
            h_flex()
                .id(label)
                .h(rems(ROW_H))
                .px_2()
                .gap_2()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .hover(|d| d.bg(p.selected))
                .when(current == page, |d| d.bg(p.selected).text_color(p.text))
                .when(current != page, |d| d.text_color(p.text2))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    this.task_open = false;
                    cx.notify();
                }))
                .child(Icon::new(icon).small())
                .child(div().flex_1().child(label))
                .children(count.map(|n| div().text_xs().text_color(p.muted).child(n)))
        };
        let session = |cx: &mut Context<Self>,
                       title: &'static str,
                       foot: &'static str,
                       dot: Option<Hsla>,
                       sel: bool| {
            let sel = sel && current == Page::Chat;
            h_flex()
                .id(title)
                .py_1()
                .pl(rems(RAIL_INDENT))
                .pr_2()
                .gap_2()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .hover(|d| d.bg(p.selected))
                .when(sel, |d| d.bg(p.selected))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Chat;
                    cx.notify();
                }))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(full(format!("title-{title}"), title).text_color(p.text))
                        .child(div().truncate().text_xs().text_color(p.muted).child(foot)),
                )
                .child(
                    div()
                        .w(rems(DOT_COLUMN))
                        .flex()
                        .justify_center()
                        .children(dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                )
        };
        let project = |this: &Self, cx: &mut Context<Self>, i: usize, open: bool| {
            let (name, git) = PROJECTS[i];
            let menu_id = if i == 0 {
                "project-menu-0"
            } else {
                "project-menu-1"
            };
            v_flex()
                .child(
                    h_flex()
                        .id(("project", i))
                        .h(rems(ROW_H))
                        .pl_2()
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .hover(|d| d.bg(p.selected))
                        .text_color(p.text)
                        .font_medium()
                        .child(
                            Icon::new(if open {
                                IconName::FolderOpen
                            } else {
                                IconName::FolderClosed
                            })
                            .small(),
                        )
                        .child(full(format!("project-name-{i}"), name).flex_1())
                        .child(
                            this.icon_button(menu_id, IconName::Ellipsis, "Project actions", cx)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_delete(PROJECTS[i].0, window, cx);
                                })),
                        ),
                )
                // The branch takes a line of its own, so the name keeps the row.
                .when(open, |d| {
                    d.child(
                        div()
                            .pl(rems(RAIL_INDENT))
                            .text_xs()
                            .text_color(p.muted)
                            .child(git),
                    )
                })
        };

        // A project and its sessions leave together when it is deleted.
        let first = v_flex()
            .gap_0p5()
            .child(project(self, cx, 0, true))
            .child(session(
                cx,
                "Inspect dashboard",
                "claude",
                Some(p.warning),
                true,
            ))
            .child(session(
                cx,
                "Fix flaky retry test",
                "claude",
                Some(p.accent),
                false,
            ))
            .child(session(
                cx,
                "Audit interface fast",
                "codex",
                Some(p.danger),
                false,
            ))
            .child(session(
                cx,
                "Rename the retry configuration keys across both services",
                "claude",
                None,
                false,
            ));
        let kept = |i: usize| !self.removed.contains(&PROJECTS[i].0);
        v_flex()
            .w(rems(self.rail_w))
            .h_full()
            .flex_none()
            .bg(p.sunken)
            .child(
                h_flex()
                    .h(rems(BAR_H))
                    .pl_3()
                    .pr_2()
                    .child(
                        div()
                            .flex_1()
                            .font_medium()
                            .text_color(p.text)
                            .child("Audit workspace"),
                    )
                    .child(
                        self.icon_button(
                            "hide-rail",
                            IconName::PanelLeftClose,
                            "Hide the rail",
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rail_hidden = true;
                            this.hovered = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                v_flex()
                    .px_2()
                    .gap_0p5()
                    .child(nav(
                        cx,
                        Page::Overview,
                        "Workspace overview",
                        IconName::LayoutDashboard,
                        None,
                    ))
                    .child(nav(cx, Page::Tasks, "Tasks", IconName::Inbox, Some("2")))
                    .child(nav(cx, Page::Issues, "Issues", IconName::CircleCheck, None))
                    .child(nav(
                        cx,
                        Page::Composer,
                        "Composer cards",
                        IconName::Frame,
                        None,
                    ))
                    .child(
                        div().py_2().child(
                            action("new-session")
                                .primary()
                                .small()
                                .w_full()
                                .icon(IconName::Plus)
                                .label("New session"),
                        ),
                    ),
            )
            .child(div().h(gpui::px(HAIRLINE_PX)).bg(p.hairline))
            .child(
                v_flex()
                    .id("tree")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_0p5()
                    .when(kept(0), |d| d.child(first))
                    .when(kept(1), |d| d.child(project(self, cx, 1, false))),
            )
            .child(div().p_2().border_t_1().border_color(p.hairline).child(nav(
                cx,
                Page::Settings,
                "Settings",
                IconName::Settings,
                None,
            )))
    }
}
