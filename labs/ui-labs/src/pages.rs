//! The pages that take the agent pane: overview, Tasks (with a task's
//! detail in the same column), Workflows, and Issues, in a module of its own.
use super::*;
use gpui::{AnyElement, FontWeight};

mod issues;

/// One row of a sectioned list: a title over its facts, a state dot in a
/// stable column, and whatever the row offers at the end.
struct Row {
    id: &'static str,
    title: &'static str,
    meta: &'static str,
    dot: Option<Hsla>,
    end: Option<AnyElement>,
}

impl Labs {
    pub(super) fn page_view(
        &self,
        p: &Palette,
        avail: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (title, body): (&str, AnyElement) = match self.page {
            Page::Overview => (
                "Workspace overview",
                self.overview(p, cx).into_any_element(),
            ),
            Page::Tasks if self.task_open => ("Tasks", self.task_detail(p, cx).into_any_element()),
            Page::Tasks => ("Tasks", self.tasks(p, cx).into_any_element()),
            Page::Issues => ("Issues", self.issues(p, avail, cx).into_any_element()),
            Page::Workflows => ("Workflows", self.workflows(p, cx).into_any_element()),
            Page::Composer => (
                "Composer cards",
                self.composer_gallery(p, cx).into_any_element(),
            ),
            Page::Settings => (
                "Settings",
                self.settings_page(p, avail, cx).into_any_element(),
            ),
            Page::Chat => unreachable!("the chat is not a page"),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .h(rems(BAR_H))
                    .flex_none()
                    .px_4()
                    .border_b_1()
                    .border_color(p.hairline)
                    .child(div().flex_1().font_medium().text_color(p.text).child(title))
                    .child(
                        action("theme-page")
                            .ghost()
                            .small()
                            .icon(IconName::Palette)
                            .tooltip("Switch light and dark")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dark = !this.dark;
                                set_mode(this.dark, window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(body)
    }

    /// A scrolling page column: one inset, one section rhythm.
    pub(super) fn column(id: &'static str) -> gpui::Stateful<gpui::Div> {
        div().id(id).flex_1().overflow_y_scroll()
    }

    pub(super) fn inner(p: &Palette) -> gpui::Div {
        v_flex()
            .w_full()
            .max_w(rems(PAGE_MAX))
            .mx_auto()
            .px_4()
            .py_6()
            .gap_6()
            .text_color(p.text)
    }

    fn section(
        p: &Palette,
        title: &'static str,
        count: Option<usize>,
        extra: Option<AnyElement>,
        body: impl IntoElement,
    ) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().font_medium().child(title))
                    .children(
                        count.map(|n| div().text_xs().text_color(p.muted).child(n.to_string())),
                    )
                    .child(div().flex_1())
                    .children(extra),
            )
            .child(body)
    }

    /// Rows in one hairline box, divided by hairlines. An empty list says so.
    fn rows(p: &Palette, cx: &App, rows: Vec<Row>, empty: &'static str) -> impl IntoElement {
        let boxed = v_flex()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel);
        if rows.is_empty() {
            return boxed.child(div().px_3().py_2().text_color(p.muted).child(empty));
        }
        let last = rows.len() - 1;
        boxed.children(rows.into_iter().enumerate().map(|(i, r)| {
            h_flex()
                .id(r.id)
                .hover(|d| d.bg(p.selected))
                .px_3()
                .py_2()
                .gap_3()
                .when(i != last, |d| d.border_b_1().border_color(p.hairline))
                .child(
                    div()
                        .w(rems(DOT_COLUMN))
                        .flex_none()
                        .children(r.dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(r.title))
                        .child(div().truncate().text_xs().text_color(p.muted).child(r.meta)),
                )
                .children(r.end)
        }))
    }

    fn badge(p: &Palette, label: &'static str, ink: Hsla, fill: Option<Hsla>) -> impl IntoElement {
        div()
            .flex_none()
            .px_2()
            .rounded_full()
            .border_1()
            .border_color(if fill.is_some() {
                gpui::transparent_black()
            } else {
                p.control
            })
            .when_some(fill, |d, f| d.bg(f))
            .text_xs()
            .text_color(ink)
            .child(label)
    }

    fn action(id: &'static str, label: &'static str) -> AnyElement {
        action(id).ghost().small().label(label).into_any_element()
    }

    fn overview(&self, p: &Palette, cx: &App) -> impl IntoElement {
        let tile =
            |name: &'static str, git: &'static str, sessions: &'static str, dot: Option<Hsla>| {
                v_flex()
                    .w(rems(TILE_W))
                    .p_3()
                    .gap_1()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.panel)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Icon::new(IconName::Folder).small().text_color(p.muted))
                            .child(div().flex_1().truncate().font_medium().child(name))
                            .children(dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                    )
                    .child(div().text_xs().text_color(p.muted).child(git))
                    .child(div().text_xs().text_color(p.text2).child(sessions))
            };
        Self::column("overview").child(
            Self::inner(p)
                .child(Self::section(
                    p,
                    "Waiting on you",
                    Some(2),
                    None,
                    Self::rows(
                        p,
                        cx,
                        vec![
                            Row {
                                id: "w1",
                                title: "Inspect dashboard",
                                meta: "atlas-api · claude · waiting for permission",
                                dot: Some(p.warning),
                                end: Some(Self::action("w1-open", "Open session")),
                            },
                            Row {
                                id: "w2",
                                title: "Retry with backoff — step 2 of 3",
                                meta: "atlas-api · issue #42 · waiting for approval",
                                dot: Some(p.warning),
                                end: Some(Self::action("w2-review", "Review…")),
                            },
                        ],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Working",
                    Some(1),
                    None,
                    Self::rows(
                        p,
                        cx,
                        vec![Row {
                            id: "k1",
                            title: "Fix flaky retry test",
                            meta: "atlas-api · claude · running 4 min",
                            dot: Some(p.accent),
                            end: None,
                        }],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Projects",
                    None,
                    None,
                    h_flex()
                        .flex_wrap()
                        .gap_3()
                        .child(tile(
                            "atlas-api",
                            "main · 3 changes",
                            "4 sessions · 1 waiting",
                            Some(p.warning),
                        ))
                        .child(tile(
                            "dashboard-web",
                            "feat/charts · clean",
                            "2 sessions",
                            Some(p.accent),
                        ))
                        .child(tile("infra", "main · clean", "No sessions", None)),
                ))
                .child(Self::section(
                    p,
                    "Recent conversations",
                    None,
                    None,
                    Self::rows(
                        p,
                        cx,
                        vec![
                            Row {
                                id: "r1",
                                title: "Rename config keys",
                                meta: "atlas-api · 2 h ago",
                                dot: None,
                                end: None,
                            },
                            Row {
                                id: "r2",
                                title: "Chart legend overlaps",
                                meta: "dashboard-web · yesterday",
                                dot: None,
                                end: None,
                            },
                        ],
                        "",
                    ),
                )),
        )
    }

    /// The workflows the workspace can run, and where a new one starts.
    fn workflows(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let new = action("new-workflow")
            .ghost()
            .small()
            .icon(IconName::Plus)
            .label("New workflow")
            .into_any_element();
        let row = |id, title, meta| Row {
            id,
            title,
            meta,
            dot: None,
            end: Some(Self::action(id, "Run")),
        };
        Self::column("workflows").child(Self::inner(p).child(Self::section(
            p,
            "Workflows",
            None,
            Some(new),
            Self::rows(
                p,
                cx,
                vec![
                    row("w1", "Fix an issue", "plan · implement · check · 3 steps"),
                    row("w2", "Review a pull request", "read · comment · 2 steps"),
                    row("w3", "Release notes", "collect · write · 2 steps"),
                ],
                "No workflows yet. Write one to run the same steps on any project.",
            ),
        )))
    }

    fn tasks(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = action("filter")
            .ghost()
            .small()
            .label("All projects")
            .icon(IconName::ChevronDown)
            .into_any_element();
        Self::column("tasks").child(
            Self::inner(p)
                .child(Self::section(
                    p,
                    "Needs attention",
                    Some(2),
                    Some(filter),
                    Self::rows(
                        p, cx,
                        vec![
                            Row {
                                id: "t1",
                                title: "Retry with backoff",
                                meta: "Fix an issue · step 2 of 3 waits for approval · atlas-api",
                                dot: Some(p.warning),
                                end: Some(
                                    action("t1-open")
                                        .ghost()
                                        .small()
                                        .label("Open")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.task_open = true;
                                            cx.notify();
                                        }))
                                        .into_any_element(),
                                ),
                            },
                            Row {
                                id: "t2",
                                title: "Migrate settings schema",
                                meta: "Check failed · cargo test · infra",
                                dot: Some(p.danger),
                                end: Some(Self::action("t2-retry", "Retry")),
                            },
                        ],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Running",
                    Some(1),
                    None,
                    Self::rows(
                        p, cx,
                        vec![Row {
                            id: "t3",
                            title: "Chart legend overlaps",
                            meta: "Fix an issue · step 1 of 3 · dashboard-web",
                            dot: Some(p.accent),
                            end: Some(Self::action("t3-stop", "Stop")),
                        }],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Queued",
                    Some(0),
                    None,
                    Self::rows(
                        p, cx,
                        vec![],
                        "Nothing queued. A run waits here when its project is already busy with another.",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Finished",
                    Some(1),
                    None,
                    Self::rows(
                        p, cx,
                        vec![Row {
                            id: "t4",
                            title: "Run check",
                            meta: "Passed · true · atlas-api · 10:42",
                            dot: Some(p.success),
                            end: Some(Self::action("t4-dismiss", "Dismiss")),
                        }],
                        "",
                    ),
                )),
        )
    }

    fn task_detail(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let step = |label: &'static str, state: Option<bool>| {
            h_flex()
                .gap_2()
                .py_1()
                .child(match state {
                    Some(true) => Icon::new(IconName::Check).small().text_color(p.muted),
                    Some(false) => Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(p.warning),
                    None => Icon::new(IconName::Minus).small().text_color(p.muted),
                })
                .child(
                    div()
                        .when(state.is_none(), |d| d.text_color(p.muted))
                        .child(label),
                )
        };
        Self::column("task").child(
            Self::inner(p)
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div().child(
                                action("back-tasks")
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowLeft)
                                    .label("Tasks")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.task_open = false;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_xl()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("Retry with backoff"),
                                )
                                .child(action("open-session").outline().small().label("Open session")),
                        )
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .text_xs()
                                .text_color(p.muted)
                                .child(div().flex_1().child("Fix an issue · atlas-api · issue #42 · started 14:02 · 12 min"))
                                .child(Self::badge(p, "Awaiting approval", p.warning, Some(p.warning_bg))),
                        ),
                )
                .child(Self::section(
                    p,
                    "Steps",
                    None,
                    None,
                    v_flex()
                        .child(step("Plan", Some(true)))
                        .child(step("Implement", Some(false)))
                        .child(step("Open pull request", None)),
                ))
                .child(Self::section(
                    p,
                    "Awaiting approval",
                    None,
                    None,
                    v_flex()
                        .gap_3()
                        .child(
                            div()
                                .p_3()
                                .rounded(cx.theme().radius_lg)
                                .bg(p.sunken)
                                .font_family(mono)
                                .text_size(self.read(TEXT_READ_SM))
                                .text_color(p.text2)
                                .child("Backoff now reads an injected clock. tests/flaky.rs passes 200 runs in a row.\n2 files changed, +38 −11"),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(action("revise").ghost().small().label("Revise…"))
                                .child(action("continue").primary().small().label("Continue")),
                        ),
                )),
        )
    }
}
